//! The P/L of each coin held, as on the Mac (Costs.swift): its average cost from what the wallet paid on-chain (the
//! explorer's or the node's history, priced on the day for what came in unpaid), from the app's own buys, or — when
//! nothing tells — from when the app first saw it. And SolanaMoves: each Solana transaction a wallet's token account took
//! part in, read once (`State::solana_moves`).

use std::collections::{HashMap, HashSet};

use chrono::{TimeDelta, Utc};
use execution::Kind;
use ihatecrypto_core::charts::Candle;
use ihatecrypto_core::foundation::{Date, Num};
use ihatecrypto_core::holdings;
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::network::Hyperliquid;
use ihatecrypto_core::pnl::{Change, CostBasis, SolanaMove};
use net::solana_rpc::SolanaRpc;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use solana::spl;

use crate::engine::{ChainCost, Engine};
use crate::portfolio::PortfolioView;

/// A coin whose cost is read: its key, network, contract, amount held and price now.
struct Wanted {
    key: String,
    chain: String,
    contract: String,
    held: Decimal,
    price: Option<Decimal>,
}

impl Engine {
    /// Native NEAR bought through NEAR Intents: each swap the journal signed, as 1Click settled it (the Mac's
    /// nearCoinChanges); None when the journal or the account can't be read.
    pub async fn near_coin_changes(&self) -> Option<Vec<Change>> {
        let account = self.st().near_address()?;
        let entries = self.journal().entries().ok()?;
        let mut swaps: Vec<(String, Date)> = vec![];
        for e in entries.iter().filter(|e| e.chain == "1click" && e.kind == Kind::Signed && e.note.contains("NEAR")) {
            if !swaps.iter().any(|s| s.0 == e.id) { swaps.push((e.id.clone(), e.at)) }
        }
        let mut asked = tokio::task::JoinSet::new();
        for (id, _) in &swaps {
            let id = id.clone();
            asked.spawn_local(async move { let s = protocols::one_click::settled(&id).await.ok().flatten(); (id, s) });
        }
        let mut settled = HashMap::new();
        while let Some(Ok((id, s))) = asked.join_next().await { settled.insert(id, s); }
        Some(swaps.iter().filter_map(|(id, at)| {
            let (tokens, dollars) = settled.get(id)?.as_ref()?.near(&account)?;
            Some(Change { at: *at, tokens, paid: (tokens > Decimal::ZERO).then_some(dollars) })
        }).collect())
    }

    /// The cost a coin's P/L is measured from: the chain's or the app's, whichever covers more of it; else «seen».
    pub fn cost(&self, key: &str) -> Option<CostBasis> {
        let st = self.st();
        // `max(by:)` keeps the first of equals: the chain's.
        [st.chain_cost.get(key).map(|c| &c.cost), st.app_costs.get(key)].into_iter().flatten().filter(|c| c.amount > Decimal::ZERO)
            .fold(None, |best: Option<&CostBasis>, c| if best.is_some_and(|b| b.amount >= c.amount) { best } else { Some(c) }).cloned()
            .or_else(|| st.seen_costs.get(key).filter(|c| c.amount > Decimal::ZERO).cloned())
    }

    /// The rows' P/L from the costs known, for the window.
    pub fn with_profit(&self, mut p: PortfolioView) -> PortfolioView {
        for r in p.networks.iter_mut().flat_map(|n| n.rows.iter_mut()) {
            let (Some(key), Some(usd)) = (r.key.clone(), r.usd) else { continue };
            if r.value <= Decimal::ZERO { continue }
            let Some(price) = (Num::of(usd) / r.value).0 else { continue };
            let Some((gain, pct)) = self.cost(&key).and_then(|c| c.pnl(r.value, price)) else { continue };
            r.pnl = Some(format!("{} ({}{}%)", self.signed_usd(gain), if pct >= Decimal::ZERO { "+" } else { "" }, Fmt::num(pct, 1)));
            r.gain = Some(gain);
        }
        p
    }

    /// Reads what each coin cost, for the coins whose amount changed since the last read; in the background after a
    /// refresh, one coin at a time (a token's history is up to 50 transactions).
    pub async fn load_costs(&self) {
        let Some(owner) = ({
            let st = self.st();
            if st.costs_loading { return }
            st.solana_address()
        }) else { return };
        self.st_mut().costs_loading = true;
        let read = self.read_costs(&owner).await;
        self.st_mut().costs_loading = false;
        if read {
            let p = self.st().portfolio.clone();
            let p = self.with_profit(p);
            self.st_mut().portfolio = p;
            self.changed();
        }
    }

    /// `load_costs`' reads; false when another wallet came on screen meanwhile (its own read follows).
    async fn read_costs(&self, owner: &str) -> bool {
        let (evm, wanted) = {
            let st = self.st();
            let wanted: Vec<Wanted> = st.portfolio.networks.iter().flat_map(|n| n.rows.iter().filter_map(|r| {
                let (key, contract) = (r.key.clone()?, r.contract.clone()?);
                if r.value <= Decimal::ZERO || st.chain_cost.get(&key).map(|c| c.held) == Some(r.value) { return None }
                Some(Wanted { key, chain: n.key.clone(), contract, held: r.value, price: r.usd.and_then(|u| (Num::of(u) / r.value).0) })
            })).collect();
            (st.evm_address(), wanted)
        };
        let still = || self.st().solana_address().as_deref() == Some(owner);
        let mut sol_prices: Option<Vec<Candle>> = None;
        let mut spot: Option<HashMap<String, CostBasis>> = None;
        let mut near_tokens: Option<HashMap<String, Vec<Change>>> = None;
        for w in wanted {
            if !still() { return false }
            let (mut changes, mut basis): (Option<Vec<Change>>, Option<CostBasis>) = (None, None);
            match w.chain.as_str() {
                "solana" => {
                    let moves = self.moves(owner, &w.contract).await;
                    if sol_prices.is_none() && w.contract != CostBasis::WSOL && moves.iter().any(|m| m.change.get(&w.contract).is_some_and(|c| *c > Decimal::ZERO)) {
                        let now = Utc::now();
                        sol_prices = Some(Hyperliquid::candles_between("SOL", "4h", now - TimeDelta::days(800), now).await.unwrap_or_default());
                    }
                    changes = (!moves.is_empty()).then(|| CostBasis::replay_solana(&w.contract, &moves, |at| {
                        sol_prices.as_ref()?.iter().rev().find(|c| c.time <= at).map(|c| c.close)
                    }));
                }
                "hyperliquid" => {
                    if let Some(evm) = &evm {
                        if spot.is_none() { spot = holdings::hyperliquid_entries(evm).await }
                        basis = spot.as_ref().map(|s| s.get(&w.contract).cloned().unwrap_or_default());
                    }
                }
                "near" => {
                    if w.contract == "native" {
                        changes = self.near_coin_changes().await;
                    } else {
                        let account = self.st().near_address();
                        if let Some(account) = account {
                            if near_tokens.is_none() { near_tokens = holdings::near_token_changes(&account).await }
                            changes = near_tokens.as_ref().map(|t| t.get(&w.contract).cloned().unwrap_or_default());
                        }
                    }
                }
                chain => {
                    if let Some(evm) = &evm {
                        changes = if w.contract == "native" { holdings::coin_changes(chain, evm).await } else { holdings::token_changes(chain, evm, &w.contract).await };
                    }
                }
            }
            if let Some(changes) = changes {
                let days = CostBasis::unpaid(&changes);
                let mut prices = Some(HashMap::new());
                if !days.is_empty() && let Some(id) = holdings::llama_id(&w.chain, &w.contract) { prices = holdings::past_prices(&id, &days).await }
                basis = prices.map(|p| CostBasis::fold(&changes, w.held, |d| p.get(&d).copied()));
            }
            // A switch during this coin's requests: its cost isn't the new wallet's.
            if !still() { return false }
            let unknown = basis.as_ref().is_none_or(|b| b.amount.is_zero());
            if let Some(basis) = basis { self.st_mut().chain_cost.insert(w.key.clone(), ChainCost { held: w.held, cost: basis }); }
            // Nothing tells what it cost: from when the app first saw it.
            let app_unknown = self.st().app_costs.get(&w.key).is_none_or(|c| c.amount.is_zero());
            if unknown && app_unknown && let Some(price) = w.price { self.seen(&w.key, w.held, price) }
        }
        true
    }

    pub fn seen(&self, key: &str, held: Decimal, price: Decimal) {
        let before = self.st().seen_costs.get(key).cloned();
        let mut s = before.clone().unwrap_or_default();
        if held > s.amount {
            let came = held - s.amount;
            if let Some(worth) = (Num::of(came) * price).0 { s.came(came, worth, Utc::now()) }
        } else if held < s.amount {
            s.sold(s.amount - held);
        }
        if Some(&s) != before.as_ref() { self.set_seen_costs(|c| { c.insert(key.into(), s); }) }
    }

    /// The owner's moves in `mint`, oldest first: the 50 newest transactions of its token account; for SOL, the wallet's.
    /// Each read once (a transaction never changes): SolanaMoves.
    // ponytail: 50 a token, as on the EVM networks; what was held before them counts at the price then.
    pub async fn moves(&self, owner: &str, mint: &str) -> Vec<SolanaMove> {
        let rpc = SolanaRpc::new();
        let accounts: Vec<String> = if mint == CostBasis::WSOL { vec![owner.into()] }
            else { [spl::TOKEN_PROGRAM, spl::TOKEN_2022_PROGRAM].iter().filter_map(|p| spl::associated_account(owner, mint, p).ok()).collect() };
        let mut signatures: Vec<String> = vec![];
        for account in accounts {
            let list = rpc.call("getSignaturesForAddress", json!([account, {"limit": 50}])).await.ok();
            signatures.extend(list.as_ref().and_then(Value::as_array).into_iter().flatten()
                .filter(|s| s.get("err").is_none_or(Value::is_null)).filter_map(|s| s.get("signature")?.as_str().map(str::to_string)));
        }
        let mut out = vec![];
        for sig in signatures.into_iter().collect::<HashSet<_>>() {
            let known = self.st().solana_moves.contains_key(&sig);
            if !known {
                let tx = rpc.call("getTransaction", json!([sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}])).await.ok();
                if let Some(m) = tx.filter(Value::is_object).and_then(|tx| SolanaMove::from_tx(&tx, owner)) { self.st_mut().solana_moves.insert(sig.clone(), m); }
            }
            if let Some(m) = self.st().solana_moves.get(&sig) { out.push(m.clone()) }
        }
        out.sort_by_key(|m| m.at);
        out
    }
}

#[cfg(test)]
#[path = "costs/tests.rs"]
mod tests;
