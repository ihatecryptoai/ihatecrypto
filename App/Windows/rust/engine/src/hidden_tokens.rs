//! Tokens the person hid and money between networks (HiddenTokens.swift): the tokens the person hid, and airdrops that
//! look like scams, tucked under «N hidden · show» on their network's card (the Mac's HiddenTokens and
//! Workspace.tokenChoices), out of the card's total and the portfolio's until shown; and the bridge orders signed and not
//! landed yet, read with the balances, with each bridge's own word on them.

use std::collections::HashMap;

use evm::protocols::cctp;
use execution::Entry;
use ihatecrypto_core::foundation::Date;
use protocols::{one_click, relay};
use rust_decimal::Decimal;
use serde::Serialize;
use trade::{EvmNetwork, Executor};

use crate::engine::Engine;
use crate::json;
use crate::portfolio::{Network, Row};

/// Money between networks, as the Mac's «In transit» card: each bridge order signed and not landed yet.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TransitView {
    /// The bridge's order: Relay's request, 1Click's deposit address, «domain:burn» for CCTP, a LayerZero transaction.
    pub id: String,
    pub chain: String,
    /// «Base USDC→Solana 5», without its transaction.
    pub what: String,
    pub bridge: String,
    pub logo: String,
    /// A deposit address opened for money coming in from elsewhere, not a move of the app's own.
    pub receiving: bool,
    #[serde(with = "json::date")]
    pub at: Date,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx: Option<String>,
    pub links: Vec<TransitLink>,
}

/// `TransitView.Link`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TransitLink {
    pub title: String,
    pub url: String,
}

/// CCTP's order id «domain:burn», split.
fn cctp_order(id: &str) -> Option<(u32, &str)> {
    let (source, burn) = id.split_once(':')?;
    Some((source.parse().ok()?, burn))
}

impl Engine {
    /// The person's say on one token, by Holding.id: shown or hidden. Not per wallet, as on the Mac: a scam token hidden
    /// once stays hidden on every wallet.
    pub fn show_token(&self, id: &str, shown: bool) {
        if id.is_empty() { return }
        let mut choices = self.st().settings.token_choices.clone().unwrap_or_default();
        choices.insert(id.into(), shown);
        let kept = choices.clone();
        self.set_settings(|s| s.token_choices = Some(kept));
        let mut p = self.st().portfolio.clone();
        p.networks = p.networks.into_iter().map(|n| self.noted(Self::tucked(n, &choices))).collect();
        p.total = p.networks.iter().fold(Decimal::ZERO, |s, n| s.saturating_add(n.usd));
        let shown = self.with_profit(p);
        self.st_mut().portfolio = shown;
    }

    /// A network's tokens split into those on show and those tucked away; its total counts the shown ones only.
    pub fn tucked(mut n: Network, choices: &HashMap<String, bool>) -> Network {
        let shown = |r: &Row| r.id.as_ref().is_none_or(|id| choices.get(id).copied().or(r.real).unwrap_or(true));
        let (rows, mut tucked): (Vec<Row>, Vec<Row>) = n.rows.drain(..).chain(n.hidden.drain(..)).partition(shown);
        n.rows = rows;
        // ponytail: the hundred worth the most; the rest only counted. A spammed address had 666 airdrops on Base,
        // and every state carries the list. Page through them if anyone ever needs the 101st.
        tucked.sort_by_key(|r| std::cmp::Reverse(r.usd.unwrap_or_default()));
        n.hidden_more += tucked.len().saturating_sub(100) as i64;
        tucked.truncate(100);
        n.hidden = tucked;
        n.usd = n.rows.iter().fold(Decimal::ZERO, |s, r| s.saturating_add(r.usd.unwrap_or_default()));
        n
    }

    /// The orders still on their way, each older than two minutes asked of its bridge first (Executor.resolveTransit).
    pub async fn read_transit(&self) {
        let Ok(executor) = self.executor() else {
            self.st_mut().in_transit = vec![];
            return;
        };
        let found = executor.resolve_transit(chrono::Utc::now()).await;
        self.st_mut().in_transit = found.iter().map(Self::transit).collect();
    }

    pub fn transit(e: &Entry) -> TransitView {
        let (bridge, logo) = match e.chain.as_str() {
            "1click" => ("NEAR Intents", "near"),
            "cctp" => ("Circle's CCTP", "usdc"),
            "layerzero" => ("LayerZero", "usdt"),
            _ => ("Relay", "relay"),
        };
        let tx = e.note.split(", tx ").nth(1).map(|t| t.chars().take_while(|c| !c.is_whitespace()).collect::<String>());
        let link = |title: String, url: String| TransitLink { title, url };
        let mut links = vec![];
        match e.chain.as_str() {
            "1click" => links.push(link("NEAR Intents explorer".into(), format!("https://explorer.near-intents.org/transactions/{}", e.id))),
            "cctp" => {
                if let Some((source, burn)) = cctp_order(&e.id) && let Some(net) = EvmNetwork::all().iter().find(|n| cctp::domain(n.rpc.chain_id) == Some(source)) {
                    links.push(link(format!("Burn on {}", net.name), format!("{}/tx/{burn}", net.explorer)));
                }
            }
            "layerzero" => links.push(link("LayerZero Scan".into(), format!("https://layerzeroscan.com/tx/{}", e.id))),
            _ => links.push(link("Relay".into(), format!("https://relay.link/transaction/{}", e.id))),
        }
        if e.chain != "cctp" && let Some(tx) = tx.as_ref().filter(|t| !t.starts_with("0x")) {
            links.push(link("Solscan".into(), format!("https://solscan.io/tx/{tx}")));
        }
        TransitView {
            id: e.id.clone(), chain: e.chain.clone(), what: e.note.split(", tx ").next().unwrap_or(&e.note).into(), bridge: bridge.into(), logo: logo.into(),
            receiving: e.note.starts_with("Receive:"), at: e.at, tx, links,
        }
    }

    /// The bridge's own word on an order now, for its row opened in the «In transit» card.
    pub async fn transit_state(&self, chain: &str, id: &str) -> String {
        match chain {
            "1click" => match one_click::status(id).await {
                Ok(s) => Executor::one_click_word(&s.status).map_or_else(|| s.status.to_lowercase(), str::to_string),
                Err(_) => self.say("NEAR Intents didn't answer", "NEAR Intents не ответил"),
            },
            "cctp" => {
                let Some((source, burn)) = cctp_order(id) else { return self.say("unknown", "неизвестно") };
                match Executor::cctp_forward(source, burn).await {
                    Some(mint) => format!("{} ({}…)", self.say("minted by Circle", "Circle выпустил"), mint.chars().take(10).collect::<String>()),
                    None => self.say("burned, waiting for Circle's mint", "сожжено, ждёт выпуска у Circle"),
                }
            }
            "layerzero" => {
                let Some((status, _)) = Executor::layer_zero_status(id).await else { return self.say("LayerZero hasn't seen it yet", "LayerZero его ещё не видит") };
                match status.as_str() {
                    "INFLIGHT" => self.say("on its way: waiting for the source network's finality", "в пути: ждёт подтверждения в исходной сети"),
                    "CONFIRMING" => self.say("verified, being delivered", "проверено, доставляется"),
                    "DELIVERED" => self.say("landed", "дошло"),
                    "FAILED" => self.say("failed", "не вышло"),
                    _ => status.to_lowercase(),
                }
            }
            _ => {
                let Ok((s, _)) = relay::status(id).await else { return self.say("Relay didn't answer", "Relay не ответил") };
                match s.as_str() {
                    "waiting" => self.say("waiting for the deposit", "ждёт депозита"),
                    "pending" => self.say("deposit seen, filling", "депозит виден, исполняется"),
                    "submitted" => self.say("fill sent", "исполнение отправлено"),
                    "success" => self.say("landed", "дошло"),
                    "delayed" => self.say("delayed — Relay is still on it", "задерживается — Relay ещё занят"),
                    "refund" => self.say("refunded", "возвращено"),
                    "failure" => self.say("failed", "не вышло"),
                    _ => s,
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "hidden_tokens/tests.rs"]
mod tests;
