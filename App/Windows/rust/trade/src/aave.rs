//! The EVM networks ihatecrypto moves USDC on, and Aave v3 on them (Aave.swift): USDC in and out; USDC missing there
//! comes from Solana over Relay, with a little of the network's gas coin; a withdrawal goes back to Solana. Also the
//! bridges between them (Relay, by signature, by BNB Chain's paymaster), approvals and the one EVM transaction.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use evm::abi::{address_in, u256_in};
use evm::call::Call;
use evm::protocols::batch7702::{self, BatchCall};
use evm::protocols::relay::{Call as ProxyCall, MulticallPermit, PROXY};
use evm::protocols::{Permit3009, SponsoredTransfer, aave};
use evm::{U256, erc20, tempo};
use execution::Kind;
use execution::trade_signer::EvmFees;
use ihatecrypto_core::foundation::{Num, Rounding, decimal};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::modules::ModuleNetwork;
use ihatecrypto_core::network::{Amount, Hyperliquid};
use ihatecrypto_core::top_up::TopUpOrder;
use net::evm_rpc::EvmRpc;
use protocols::{one_click, relay};
use rust_decimal::Decimal;
use serde_json::Value;
use wallet::evm::{EvmSignature, keccak};
use wallet::to_hex;

use crate::executor::uint64;
use crate::{BATCHES_ON, Boxed, Error, Executor, LANDINGS, Result, TOPPING_UP, fail, landings, modules, num, stop, token_balance, topping_up, word};

/// An EVM network ihatecrypto moves USDC on: where to read it, where Relay reaches it, how much gas to keep there.
#[derive(Debug, Clone)]
pub struct EvmNetwork {
    pub name: &'static str,
    /// The journal's chain name.
    pub key: &'static str,
    pub rpc: EvmRpc,
    pub usdc: &'static str,
    pub relay_chain: u64,
    pub explorer: &'static str,
    /// The coin to keep for several transactions, in wei; below it a bridge brings a top-up along.
    pub gas_floor: u64,
    /// What the top-up is worth in USD (it must land above the floor).
    pub topup_usd: Decimal,
    /// The coin gas is paid in.
    pub gas_coin: &'static str,
    /// A network with no coin of its own (Tempo): fees come out of this stablecoin, so there's no gas to bring.
    pub fee_token: Option<&'static str>,
    /// Its dollar's decimals: 6 everywhere but BNB Chain, whose Binance-Peg USDC has 18. The app counts dollars in
    /// millionths on every network; `raw`, `micro` and `usdc_balance` cross to the token's own units at the chain.
    pub usdc_decimals: u32,
}

static ALL: LazyLock<Vec<EvmNetwork>> = LazyLock::new(|| {
    use relay::chain;
    let net = |name, key, chain_id, usdc, relay_chain, explorer, gas_floor, topup: &str, gas_coin| EvmNetwork {
        name, key, rpc: EvmRpc::network(chain_id).expect("a network the app signs on"), usdc, relay_chain, explorer, gas_floor,
        topup_usd: topup.parse().expect("a decimal"), gas_coin, fee_token: None, usdc_decimals: 6,
    };
    vec![
        net("Base", "base", 8453, evm::BASE_USDC, chain::BASE, "https://basescan.org", 5_000_000_000_000, "0.03", "ETH"),
        // Gas on Arbitrum costs a few times Base's (its L1 data share): a larger floor and top-up.
        net("Arbitrum", "arbitrum", 42161, evm::ARBITRUM_USDC, chain::ARBITRUM, "https://arbiscan.io", 10_000_000_000_000, "0.05", "ETH"),
        // Robinhood Chain: its dollar is USDG (Relay delivers it; Stock Tokens trade against it on Uniswap).
        net("Robinhood Chain", "robinhood", 4663, evm::ROBINHOOD_USDG, chain::ROBINHOOD, "https://robinhoodchain.blockscout.com", 15_000_000_000_000,
            "0.08", "ETH"),
        // Polygon's gas is POL at ≈ $0.12: 0.3 POL covers several calls, and a $0.06 top-up lands above it.
        net("Polygon", "polygon", 137, evm::POLYGON_USDC, chain::POLYGON, "https://polygonscan.com", 300_000_000_000_000_000, "0.06", "POL"),
        // Ethereum: the floor covers a supply and a withdraw at twice today's fee, and a $2.50 top-up lands above it.
        // ponytail: fixed floor; derive it from eth_gasPrice if mainnet fees jump past ≈ 2 gwei.
        net("Ethereum", "ethereum", 1, evm::ETHEREUM_USDC, chain::ETHEREUM, "https://etherscan.io", 800_000_000_000_000, "2.5", "ETH"),
        // Tempo: fees in USDC itself; a transfer is the only thing ihatecrypto does there besides the bridge.
        EvmNetwork { fee_token: Some(evm::TEMPO_USDC), ..net("Tempo", "tempo", 4217, evm::TEMPO_USDC, chain::TEMPO, "https://explore.tempo.xyz", 0, "0", "USDC") },
        // HyperEVM: 0.0005 HYPE covers dozens of calls, and Relay's $0.15 top-up lands above it.
        net("HyperEVM", "hyperevm", 999, evm::HYPEREVM_USDC, chain::HYPEREVM, "https://hyperevmscan.io", 500_000_000_000_000, "0.15", "HYPE"),
        // Optimism: an OP Stack network like Base; the same floor and top-up.
        net("Optimism", "optimism", 10, evm::OPTIMISM_USDC, chain::OPTIMISM, "https://optimistic.etherscan.io", 5_000_000_000_000, "0.03", "ETH"),
        // BNB Chain: 0.0001 BNB covers several calls, and Relay's $0.15 top-up lands above it. Its USDC has 18 decimals.
        EvmNetwork { usdc_decimals: 18, ..net("BNB Chain", "bnb", 56, evm::BNB_USDC, chain::BNB, "https://bscscan.com", 100_000_000_000_000, "0.15", "BNB") },
    ]
});

impl EvmNetwork {
    pub fn all() -> &'static [EvmNetwork] { &ALL }
    pub fn base() -> &'static EvmNetwork { &ALL[0] }
    pub fn arbitrum() -> &'static EvmNetwork { &ALL[1] }
    pub fn robinhood() -> &'static EvmNetwork { &ALL[2] }
    pub fn polygon() -> &'static EvmNetwork { &ALL[3] }
    pub fn ethereum() -> &'static EvmNetwork { &ALL[4] }
    pub fn tempo() -> &'static EvmNetwork { &ALL[5] }
    pub fn hyperevm() -> &'static EvmNetwork { &ALL[6] }
    pub fn optimism() -> &'static EvmNetwork { &ALL[7] }
    pub fn bnb() -> &'static EvmNetwork { &ALL[8] }

    fn scale(&self) -> U256 { U256::from(10u64).pow(U256::from(self.usdc_decimals.saturating_sub(6))) }

    /// Its USDC's own units for millionths of a dollar: the same number, but a trillion times it on BNB Chain.
    pub fn raw(&self, micro: u64) -> U256 { U256::from(micro) * self.scale() }

    /// Millionths of a dollar for its USDC's own units, rounded down (the most there is past 64 bits).
    pub fn micro(&self, raw: U256) -> u64 { u64::try_from(raw / self.scale()).unwrap_or(u64::MAX) }

    /// Its USDC at `owner`, in millionths of a dollar (an 18-decimal balance doesn't fit the node's 64-bit read).
    pub async fn usdc_balance(&self, owner: &str) -> Result<u64> {
        if self.usdc_decimals == 6 { return token_balance(&self.rpc, self.usdc, owner).await }
        let answer = self.rpc.read(self.usdc, &erc20::balance_of(owner)?).await?;
        Ok(self.micro(u256_in(&answer, 0).unwrap_or(U256::ZERO)))
    }

    /// What to leave for fees when everything goes: a cent pays ten Tempo calls.
    // ponytail: fixed cent; read the base fee if Tempo's fees ever pass $0.001 a call.
    pub(crate) fn fee_reserve(&self) -> u64 { if self.fee_token.is_none() { 0 } else { 10_000 } }

    /// None for a network ihatecrypto doesn't move money on: never guessed.
    pub fn of(chain_id: u64) -> Option<&'static EvmNetwork> { ALL.iter().find(|n| n.rpc.chain_id == chain_id) }
    pub fn named(key: &str) -> Option<&'static EvmNetwork> { ALL.iter().find(|n| n.key == key) }

    /// Where a trade's money comes from when Solana is short: the first network holding `send` and a dime over for
    /// Relay's fee, straight to Hyperliquid. Ethereum is asked last (its gas costs dollars), never left out.
    pub fn funding(send: Decimal, held: impl Fn(&str) -> Option<Decimal>) -> Option<&'static EvmNetwork> {
        let need = (Num::of(send) + Decimal::new(1, 1)).0?;
        Self::searched().into_iter().find(|n| held(n.key).unwrap_or_default() >= need)
    }

    /// Where the USDC a move needs on `target` comes from — the one rule the executor signs and the card shows: Solana
    /// pays when it holds what must arrive plus 10 cents; else the EVM network holding it (Ethereum last) sends it
    /// straight, a dollar over for Relay's fee. None: Solana pays (or nobody holds it, and the move says so).
    pub fn source(arrive: Decimal, on_solana: Decimal, target: &str, held: impl Fn(&str) -> Option<Decimal>) -> Option<(&'static EvmNetwork, Decimal)> {
        if on_solana >= (Num::of(arrive) + Decimal::new(1, 1)).0? { return None }
        let send = (Num::of(arrive) + 1).rounded(2, Rounding::Up).0?;
        Self::funding(send, |k| if k == target { None } else { held(k) }).map(|n| (n, send))
    }

    /// Every network a search for money looks at: all but the ones switched off in Settings › Modules. Ethereum comes
    /// last, so a cheaper network pays first when both hold enough.
    pub fn searched() -> Vec<&'static EvmNetwork> {
        let modules = modules();
        let mut out: Vec<&'static EvmNetwork> = ALL.iter().filter(|n| ModuleNetwork::from_place(n.key).is_none_or(|m| modules.is_on(m))).collect();
        out.sort_by_key(|n| n.key == "ethereum");
        out
    }
}

/// The node of a network ihatecrypto moves money on.
pub(crate) fn rpc_of(chain_id: u64) -> Result<EvmRpc> {
    EvmNetwork::of(chain_id).map(|n| n.rpc.clone())
        .ok_or_else(|| Error::Execution(format!("ihatecrypto doesn't move money on network {chain_id}. Nothing was signed.")))
}

/// AaveV3.verify: checks the pool and the aToken against what the market's own network says, and returns the USDC supply
/// APY. Fails when either differs: then nothing is signed.
pub async fn aave_verify(m: &aave::Market) -> Result<Decimal> {
    let rpc = rpc_of(m.chain_id)?;
    let pool = address_in(&rpc.read(m.provider, &aave::GET_POOL).await?, 0);
    if pool.as_deref() != Some(m.pool) {
        return Err(Error::Wallet(format!("Aave's pool on {} isn't where ihatecrypto expects it ({}). Nothing was signed.", m.chain_name,
                                         pool.as_deref().unwrap_or("?"))));
    }
    let reserve = rpc.read(m.pool, &aave::get_reserve_data(m.usdc)?).await?;
    match (address_in(&reserve, 8), word(&reserve, 2)) {
        (Some(a), Some(rate)) if a == m.a_usdc => Ok(aave::apy(rate)),
        _ => Err(Error::Wallet(format!("Aave's USDC reserve on {} doesn't match. Nothing was signed.", m.chain_name))),
    }
}

/// «chainId:address» → the nonce after that wallet's last confirmed transaction there, across moves (next_nonce).
static EVM_NONCES: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(Default::default);

fn d(s: &str) -> Decimal { s.parse().expect("a decimal literal") }
fn millionths(units: u64) -> Decimal { Decimal::from(units) / Decimal::from(1_000_000) }
fn show(u: u64) -> String { Fmt::num(millionths(u), 6) }
fn sleep(secs: u64) -> tokio::time::Sleep { tokio::time::sleep(Duration::from_secs(secs)) }
/// A network's own coin in wei, as a coin.
pub(crate) fn coin(wei: u128) -> Decimal {
    i128::try_from(wei).ok().and_then(|w| Decimal::try_from_i128_with_scale(w, 18).ok()).unwrap_or(Decimal::MAX).normalize()
}
/// `"0x" + r + s + v`.
fn hex(s: &EvmSignature) -> String { format!("0x{}{}{:02x}", to_hex(&s.r), to_hex(&s.s), s.v) }
/// A bridge's cost cap: 3% of what arrives, at least $0.15, at most $1.
fn cap(worth: Decimal) -> Result<Decimal> { Ok(num(Num::of(worth) * d("0.03"))?.max(d("0.15")).min(Decimal::ONE)) }
fn units(d: Decimal) -> Result<u64> { Ok(uint64(num((Num::of(d) * 1_000_000).rounded(0, Rounding::Down))?)) }
fn pairs(p: &[(&str, &str)]) -> HashMap<String, String> { p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }

impl Executor {
    pub async fn aave(&self, deposit: bool, usdc: Option<Decimal>, m: &aave::Market) -> Result<String> {
        let Some(net) = EvmNetwork::named(m.chain) else { return fail(format!("ihatecrypto doesn't move money on {}. Nothing was signed.", m.chain_name)) };
        let rpc = &net.rpc;
        // Pool and aToken are re-read from the market's own network before anything is signed.
        let apy = aave_verify(m).await?;
        let (token, pool, me) = (m.usdc, m.pool, self.wallet.evm.as_str());

        if !deposit {
            let supplied = token_balance(rpc, m.a_usdc, me).await?;
            if supplied == 0 { return fail(format!("Nothing is supplied to Aave on {}.", m.chain_name)) }
            // None = everything, interest included.
            let amount = match usdc { Some(r) => Some(units(r)?).filter(|u| *u < supplied), None => None };
            self.ensure_evm(net, 0, None).await?;
            let before = token_balance(rpc, token, me).await?;
            self.send_evm(rpc, pool, &aave::withdraw(token, amount, me)?,
                          &format!("Aave withdraw {} USDC on {}", amount.map_or("all".into(), show), m.chain_name), &[], None, 0).await?;
            return self.bridge_back(net, before, amount.unwrap_or(supplied)).await;
        }

        let Some(requested) = usdc.filter(|r| *r > Decimal::ZERO) else { return fail("Say how much, e.g. “lend 2 USDC on Aave”.") };
        let want = units(requested)?;
        self.ensure_evm(net, want, None).await?;
        let before = token_balance(rpc, m.a_usdc, me).await?;
        let hash = self.approve_and_send(rpc, token, pool, U256::from(want), &format!("Approve exactly {} USDC to Aave on {}", show(want), m.chain_name),
                                         pool, &aave::supply(token, want, me)?, &format!("Aave supply {} USDC on {}", show(want), m.chain_name), &[], 0).await?;
        // A node can answer from a block before the receipt's for a moment: give it up to ten seconds.
        let mut after = token_balance(rpc, m.a_usdc, me).await?;
        for _ in 0..10 {
            if after > before { break }
            sleep(1).await;
            after = token_balance(rpc, m.a_usdc, me).await?;
        }
        if after <= before { return fail(format!("The supply went through but aUSDC didn't grow. {}/tx/{hash}", net.explorer)) }
        Ok(format!("Supplied {} USDC to Aave on {} at ≈ {}% a year: {} USDC there now. {}/tx/{hash}", show(want), m.chain_name,
                   Fmt::num(ihatecrypto_core::foundation::rounded(apy, 2, Rounding::Plain), 2), show(after), net.explorer))
    }

    /// After a withdrawal on `net`: back where the money lives, Solana — only the USDC the withdrawal brought, never what
    /// already lay there. `expected`: what it was for, while a node is a block behind.
    pub(crate) async fn bridge_back(&self, net: &EvmNetwork, before: u64, expected: u64) -> Result<String> {
        let mut there = net.usdc_balance(&self.wallet.evm).await?;
        for _ in 0..10 {
            if there > before { break }
            sleep(1).await;
            there = net.usdc_balance(&self.wallet.evm).await?;
        }
        let got = if there > before { there - before } else { expected };
        if got < 500_000 {
            return Ok(format!("{} USDC stays on {}: under the half-dollar a bridge to Solana is worth.", show(got), net.name));
        }
        self.evm_to_solana(net, Some(millionths(got))).await
    }

    /// At least `want` USDC and some of the coin for gas on the network: what is missing comes from Solana over Relay.
    /// `heavier`: a floor and a top-up for calls heavier than a supply; the top-up must land above the floor.
    pub(crate) async fn ensure_evm(&self, net: &EvmNetwork, want: u64, heavier: Option<(u128, Decimal)>) -> Result<()> {
        let me = self.wallet.evm.as_str();
        let floor = (net.gas_floor as u128).max(heavier.map_or(0, |h| h.0));
        let topup_usd = net.topup_usd.max(heavier.map_or(Decimal::ZERO, |h| h.1));
        // Where fees come out of the stablecoin, the fees ride on the USDC wanted and there's no coin to read.
        let Some(want) = want.checked_add(net.fee_reserve()) else { return fail(crate::OUT_OF_RANGE) };
        let gas_there = || async move { if net.fee_token.is_none() { net.rpc.balance(me).await } else { Ok(floor) } };
        let mut there = net.usdc_balance(me).await?;
        let mut gas = gas_there().await?;
        if there >= want && gas >= floor { return Ok(()) }
        let short = millionths(want.saturating_sub(there));
        let need_gas = gas < floor;
        // Relay has minimums: a gas top-up rides on at least half a dollar of USDC.
        let arrive = short.max(if need_gas { d("0.5") } else { Decimal::ZERO });
        // Solana pays when it holds enough. Else the EVM network that holds the USDC sends it straight (Ethereum asked last).
        let on_solana = millionths(self.rpc.token_balance(&self.wallet.solana, relay::currency::SOLANA_USDC).await?);
        if on_solana < arrive + d("0.1") {
            let idle: HashMap<&str, Decimal> = self.idle_usdc().await.into_iter().map(|(n, u)| (n.key, u)).collect();
            if let Some((from, send)) = EvmNetwork::source(arrive, on_solana, net.key, |k| idle.get(k).copied()) {
                self.report(&format!("{} needs {} USDC and Solana has {}: bringing {} straight from {}.", net.name, Fmt::num(arrive, 2),
                                     Fmt::num(on_solana, 2), Fmt::num(send, 2), from.name));
                self.bridge(from, Some(net), false, Some(send)).await?;
                return Ok(());
            }
        }
        let (step, changes) = self.prepare_bridge(Amount::Arrive(arrive), net.relay_chain, net.usdc, net.usdc_decimals, need_gas.then_some(topup_usd), false).await?;
        let stable = if net.key == "robinhood" { "USDG" } else { "USDC" };
        self.report(&format!("Bridge to {}: {} USDC from Solana → {} {stable}{}. {}", net.name, Fmt::num(step.spend, 2), Fmt::num(step.arriving, 2),
                             if need_gas { format!(" plus {} for gas", net.gas_coin) } else { String::new() }, changes.join(" ")));
        self.send_bridge(&step).await?;
        let started = Instant::now();
        while there < want || gas < floor {
            if started.elapsed() >= Duration::from_secs(240) {
                return fail(format!("The USDC didn't reach {} within 4 minutes. Relay order {} is in the journal; nothing else was signed.", net.name,
                                    step.request_id));
            }
            sleep(2).await;
            there = net.usdc_balance(me).await?;
            gas = gas_there().await?;
        }
        self.report(&format!("On {} in {} s: {} {stable} and gas.", net.name, started.elapsed().as_secs(), show(there)));
        Ok(())
    }

    /// USDC on Base → our Solana address over Relay. None = everything on Base.
    pub async fn base_to_solana(&self, usdc: Option<Decimal>) -> Result<String> { self.evm_to_solana(EvmNetwork::base(), usdc).await }

    /// USDC from Solana to our own address on an EVM network (USDG on Robinhood Chain) over Relay, with a little of its
    /// coin for gas when there is none. `usdc` is what arrives.
    pub async fn solana_to_evm(&self, net: &EvmNetwork, usdc: Decimal) -> Result<String> {
        let units = units(usdc)?;
        if units < 500_000 { return fail("At least 0.5 USDC: less isn't worth the bridge.") }
        let (me, sol) = (self.wallet.evm.as_str(), self.wallet.solana.as_str());
        let stable = if net.key == "robinhood" { "USDG" } else { "USDC" };
        let before = net.usdc_balance(me).await?;
        // With gas already there, NEAR Intents' 1Click may land more than Relay: both are quoted, the better one goes.
        let mut moved = false;
        if one_click::asset::usdc_on_evm(net.key).is_some() && net.rpc.balance(me).await? >= net.gas_floor as u128 {
            let ask = relay::Ask {
                user: sol.into(), recipient: me.into(), origin_chain: relay::chain::SOLANA, destination_chain: net.relay_chain,
                origin_currency: relay::currency::SOLANA_USDC.into(), destination_currency: net.usdc.into(), amount: units, ..Default::default()
            };
            if let Some(relay_out) = relay::quote(&ask).await.ok().and_then(|q| decimal(&q.amount_out_formatted)) {
                if let Some(better) = self.one_click_beats(None, Some(net.key), units, relay_out).await {
                    self.one_click_move(None, Some(net.key), units, decimal(&better.amount_out_formatted).unwrap_or(relay_out),
                                        &format!("{} USDC", Fmt::num(usdc, 2))).await?;
                    moved = true;
                }
            }
        }
        if !moved {
            let Some(want) = before.checked_add(units) else { return fail(crate::OUT_OF_RANGE) };
            self.ensure_evm(net, want, None).await?;
        }
        let after = net.usdc_balance(me).await?;
        Ok(format!("Moved {} {stable} to {}: {} {stable} there now. Say “move {stable} from {} to Solana” to bring it back.",
                   Fmt::num(millionths(after.saturating_sub(before)), 2), net.name, Fmt::num(millionths(after), 2), net.name))
    }

    /// The same from any EVM network ihatecrypto knows.
    pub async fn evm_to_solana(&self, net: &EvmNetwork, usdc: Option<Decimal>) -> Result<String> { self.bridge(net, None, false, usdc).await }

    /// USDC from one EVM network straight to another at our own address, to our Hyperliquid trading balance
    /// (`hyperliquid`), or to Solana when `dest` is None — over Relay, the same two checked transactions each way. A
    /// destination with a gas coin and none of it gets a little along. None `usdc` = everything on `net`.
    /// (Boxed: it's on a call cycle, ensure_evm → bridge → cctp_move → ensure_evm.)
    pub fn bridge<'a>(&'a self, net: &'a EvmNetwork, dest: Option<&'a EvmNetwork>, hyperliquid: bool, usdc: Option<Decimal>) -> Boxed<'a, String> {
        Box::pin(async move {
            let (rpc, token, me, chain) = (&net.rpc, net.usdc, self.wallet.evm.as_str(), net.relay_chain);
            if dest.is_some_and(|d| d.key == net.key) { return fail(format!("The money is already on {}.", net.name)) }
            let to_solana = dest.is_none() && !hyperliquid;
            let there: &'static str = if hyperliquid { "Hyperliquid" } else { dest.map_or("Solana", |d| d.name) };
            let stable = if dest.is_some_and(|d| d.key == "robinhood") { "USDG" } else { "USDC" };
            let have = net.usdc_balance(me).await?;
            // On Tempo the fees come out of this USDC: a cent of it stays.
            let spendable = have.saturating_sub(net.fee_reserve());
            let amount = match usdc { Some(r) => units(r)?.min(spendable), None => spendable };
            if amount < 500_000 { return fail(format!("{} USDC on {} — below the half-dollar a bridge is worth.", show(have), net.name)) }
            // Gas for what comes next there, when the destination has a gas coin and too little of it.
            let mut topup = None;
            if let Some(dest) = dest {
                if dest.fee_token.is_none() && dest.rpc.balance(me).await? < dest.gas_floor as u128 { topup = Some(dest.topup_usd) }
            }
            // No gas here for an approve and a deposit: where the dollar pays by signature, Relay takes it so; on BNB Chain a
            // plain transfer to Relay's deposit address goes with BNB Chain paying. CCTP and 1Click both start with a
            // transaction here, so they're not asked.
            if net.fee_token.is_none() && Self::leaves_without_gas(net) && rpc.balance(me).await? < net.gas_floor as u128 {
                return self.bridge_without_gas(net, dest, hyperliquid, amount, topup, false).await;
            }

            let recipient = if to_solana { self.wallet.solana.as_str() } else { me };
            let quote = self.relay_quote(&relay::Ask {
                user: me.into(), recipient: recipient.into(), origin_chain: chain,
                destination_chain: if hyperliquid { relay::chain::HYPERLIQUID } else { dest.map_or(relay::chain::SOLANA, |d| d.relay_chain) },
                origin_currency: token.into(),
                destination_currency: if hyperliquid { relay::currency::HYPERLIQUID_USDC } else { dest.map_or(relay::currency::SOLANA_USDC, |d| d.usdc) }.into(),
                amount, topup_gas_usd: topup, units: Some(net.raw(amount).to_string()), ..Default::default()
            }).await?;
            let bound_recipient = if to_solana { quote.recipient.clone() } else { quote.recipient.to_lowercase() };
            let expected_recipient = if to_solana { recipient.to_string() } else { recipient.to_lowercase() };
            let binding = guard::binding_check(&pairs(&[("sender", &quote.sender.to_lowercase()), ("recipient", &bound_recipient)]),
                                               &pairs(&[("sender", &me.to_lowercase()), ("recipient", &expected_recipient)]), "Relay quote");
            if !binding.ok() { return stop(binding.problems.join(" ")) }
            let depository = relay::depository(chain).await?.map(|d| d.to_lowercase());
            let deposit = quote.evm.iter().find(|s| s.id == "deposit");
            let (Some(depository), Some(deposit)) = (depository, deposit) else {
                return stop(format!("Relay's deposit isn't to its published depository on {}. Nothing was signed.", net.name));
            };
            if deposit.to.to_lowercase() != depository || deposit.chain_id != chain || !(deposit.value == "0" || deposit.value == "<null>") {
                return stop(format!("Relay's deposit isn't to its published depository on {}. Nothing was signed.", net.name));
            }
            // depositErc20(address depositor, address token, uint256 amount, bytes32 id)
            let args = &deposit.data[deposit.data.len().min(4)..];
            if deposit.data.get(..4) != Some(&[0xe8, 0x01, 0x79, 0x52][..]) || args.len() != 4 * 32
                || address_in(args, 0) != Some(me.to_lowercase()) || address_in(args, 1).as_deref() != Some(token) || u256_in(args, 2) != Some(net.raw(amount)) {
                return stop(format!("Relay's deposit asks for something else than {} USDC from this wallet. Nothing was signed.", show(amount)));
            }
            let arriving = decimal(&quote.amount_out_formatted).unwrap_or_default();
            let spend = millionths(amount);
            // Circle's CCTP between EVM networks, when no gas has to come along and it lands more than Relay by the margin
            // 1Click must beat too: one signature, Circle mints on the other side itself.
            if let (false, None, Some(dest)) = (hyperliquid, topup, dest) {
                if let Some((max_fee, arrives)) = self.cctp_quote(net, dest, amount).await {
                    let better = num(Num::of(arrives) - arriving)? >= Self::one_click_edge(spend);
                    self.report(&format!("Relay lands ≈ {} USDC, Circle's CCTP ≈ {}: {}", Fmt::num(arriving, 4), Fmt::num(arrives, 4), if better {
                        "taking CCTP.".to_string()
                    } else {
                        format!("Relay{}.", if arrives > arriving { format!(" (CCTP's {} more isn't worth the wait)", Fmt::usd(arrives - arriving)) } else { String::new() })
                    }));
                    if better {
                        let started = Instant::now();
                        let got = self.cctp_move(net, dest, amount, max_fee).await?;
                        return Ok(format!("Moved {} USDC from {} to {there} through Circle's CCTP in {} s: {} USDC arrived.", show(amount), net.name,
                                          started.elapsed().as_secs(), Fmt::num(got, 2)));
                    }
                }
            }
            // NEAR Intents' 1Click for the same USDC, when it serves both ends and no gas has to come along: the better lands.
            if !hyperliquid && topup.is_none() && one_click::asset::usdc_on_evm(net.key).is_some()
                && dest.is_none_or(|d| one_click::asset::usdc_on_evm(d.key).is_some()) {
                if let Some(better) = self.one_click_beats(Some(net.key), dest.map(|d| d.key), amount, arriving).await {
                    let started = Instant::now();
                    let got = self.one_click_move(Some(net.key), dest.map(|d| d.key), amount, decimal(&better.amount_out_formatted).unwrap_or(arriving),
                                                  &format!("{} USDC", show(amount))).await?;
                    return Ok(format!("Moved {} USDC from {} to {there} through NEAR Intents' 1Click in {} s: {} USDC arrived.", show(amount), net.name,
                                      started.elapsed().as_secs(), Fmt::num(got, 2)));
                }
            }
            Self::check_loss(num(Num::of(spend) - arriving - topup.unwrap_or_default())?, spend, cap(arriving)?, "Relay")?;
            let before = self.relay_landed(dest, hyperliquid).await?;
            let hash = self.approve_and_send(rpc, token, &depository, net.raw(amount), &format!("Approve exactly {} USDC to Relay", show(amount)), &deposit.to,
                                             &deposit.data, &format!("Relay deposit {} USDC on {}", show(amount), net.name), &[], 0).await?;
            self.record("relay", &quote.request_id, Kind::Signed, &format!("{} USDC→{there} {}, tx {hash}", net.name, show(amount)))?;
            let (link, sent, gas) = (format!("{}/tx/{hash}", net.explorer), show(amount), topup.is_some());
            // Inside a chain the next step is signed while this lands; a step spending where it lands waits for it.
            if let Some(landings) = landings() {
                let (me, request_id, from, to, said) = (self.arc(), quote.request_id.clone(), net.clone(), dest.cloned(), sent.clone());
                landings.add(if hyperliquid { "hyperliquid" } else { dest.map_or("solana", |d| d.key) }, async move {
                    me.relay_landing(&request_id, &link, &from, to.as_ref(), hyperliquid, before, arriving, &said, stable, there, gas).await
                });
                return Ok(format!("Sent {sent} USDC from {} to {there}: ≈ {} {stable} lands there in seconds, and what needs it waits for it.", net.name,
                                  Fmt::num(arriving, 2)));
            }
            self.relay_landing(&quote.request_id, &link, net, dest, hyperliquid, before, arriving, &sent, stable, there, gas).await
        })
    }

    /// The USDC at our address where a Relay order from an EVM network lands: Hyperliquid's withdrawable, an EVM network's
    /// USDC, or Solana's.
    async fn relay_landed(&self, dest: Option<&EvmNetwork>, hyperliquid: bool) -> Result<u64> {
        if hyperliquid { return Ok(uint64(num(Num::of(Hyperliquid::balance(&self.wallet.evm).await?.withdrawable) * 1_000_000)?)) }
        if let Some(dest) = dest { return dest.usdc_balance(&self.wallet.evm).await }
        Ok(self.rpc.token_balance(&self.wallet.solana, relay::currency::SOLANA_USDC).await?)
    }

    /// A Relay order from an EVM network, signed: Relay's word until it settles, then the USDC on the other side.
    #[allow(clippy::too_many_arguments)]
    async fn relay_landing(&self, request_id: &str, link: &str, net: &EvmNetwork, dest: Option<&EvmNetwork>, hyperliquid: bool, before: u64,
                           arriving: Decimal, sent: &str, stable: &str, there: &str, gas: bool) -> Result<String> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(180) {
            let status = relay::status(request_id).await?.0;
            if status == "success" {
                self.record("relay", request_id, Kind::Settled, &status)?;
                break;
            }
            if ["refund", "failure"].contains(&status.as_str()) {
                self.record("relay", request_id, Kind::Failed, &status)?;
                return fail(format!("Relay returned {status}. Check USDC on {}. {link}", net.name));
            }
            sleep(2).await;
        }
        let target = before.saturating_add(uint64(num(Num::of(arriving) * 990_000)?));
        while started.elapsed() < Duration::from_secs(240) {
            let now = self.relay_landed(dest, hyperliquid).await?;
            if now >= target {
                return Ok(format!("Moved {sent} USDC from {} to {there} in {} s: {} {stable} arrived, {} {stable} on {there} now.{}", net.name,
                                  started.elapsed().as_secs(), Fmt::num(arriving, 2), show(now), if gas { " A little gas came along." } else { "" }));
            }
            sleep(2).await;
        }
        fail(format!("Relay didn't credit {there} within 4 minutes. Order {request_id} is in the journal. {link}"))
    }

    /// `amount` of `net`'s USDC to Solana (as SOL when `sol`), to Hyperliquid or to another EVM network, paid by signature:
    /// Relay's approval proxy takes it and its solver pays the gas, so a wallet with no gas coin here still moves it. The
    /// quote is checked like a deposit: from and to this wallet, our token and amount, Relay's proxy.
    pub(crate) async fn bridge_by_permit(&self, net: &EvmNetwork, dest: Option<&EvmNetwork>, hyperliquid: bool, amount: u64, topup: Option<Decimal>,
                                         sol: bool) -> Result<String> {
        let (me, chain, to_solana) = (self.wallet.evm.as_str(), net.relay_chain, dest.is_none() && !hyperliquid);
        let there: &'static str = if hyperliquid { "Hyperliquid" } else { dest.map_or("Solana", |d| d.name) };
        let stable = if sol { "SOL" } else if dest.is_some_and(|d| d.key == "robinhood") { "USDG" } else { "USDC" };
        let recipient = if to_solana { self.wallet.solana.as_str() } else { me };
        let units = net.raw(amount).to_string();
        let quote = self.relay_quote(&relay::Ask {
            user: me.into(), recipient: recipient.into(), origin_chain: chain,
            destination_chain: if hyperliquid { relay::chain::HYPERLIQUID } else { dest.map_or(relay::chain::SOLANA, |d| d.relay_chain) },
            origin_currency: net.usdc.into(),
            destination_currency: if sol { relay::currency::SOLANA_SOL } else if hyperliquid { relay::currency::HYPERLIQUID_USDC }
                                  else { dest.map_or(relay::currency::SOLANA_USDC, |d| d.usdc) }.into(),
            amount, topup_gas_usd: topup, units: Some(units.clone()), use_permit: true, ..Default::default()
        }).await?;
        let bound_recipient = if to_solana { quote.recipient.clone() } else { quote.recipient.to_lowercase() };
        let expected_recipient = if to_solana { recipient.to_string() } else { recipient.to_lowercase() };
        let binding = guard::binding_check(&pairs(&[("sender", &quote.sender.to_lowercase()), ("recipient", &bound_recipient)]),
                                           &pairs(&[("sender", &me.to_lowercase()), ("recipient", &expected_recipient)]), "Relay quote");
        if !binding.ok() { return stop(binding.problems.join(" ")) }
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        let known = Permit3009::token_info(net.rpc.chain_id, net.usdc);
        // Relay's v3.2 proxy: the MulticallAuthorization the signer checks and signs with the EIP-3009 item it binds. Each is
        // held to what this wallet asked for before anything is signed.
        let v32 = quote.multicall.as_ref().and_then(|m| {
            let f = m.funding.first()?;
            let calls: Vec<ProxyCall> = m.calls.iter().filter_map(|c| c.value.parse::<u64>().ok().map(|value| ProxyCall {
                target: c.target.clone(), allow_failure: c.allow_failure, value, call_data: c.call_data.clone(),
            })).collect();
            let held = m.chain_id == net.rpc.chain_id && m.domain_name == "RelayApprovalProxy" && m.domain_version == "3.1"
                && m.verifying_contract.to_lowercase() == PROXY && m.from.to_lowercase() == me.to_lowercase() && m.msg_value == "0"
                && m.funding_index == 0 && m.funding.len() == 1 && f.token.to_lowercase() == net.usdc && f.from.to_lowercase() == me.to_lowercase()
                && f.value == units && f.valid_after == 0 && calls.len() == m.calls.len()
                && m.permit.to.to_lowercase() == PROXY && m.permit.value == units && m.permit.valid_before == f.valid_before
                && Some(m.permit.domain_name.as_str()) == known.map(|k| k.0) && Some(m.permit.domain_version.as_str()) == known.map(|k| k.1);
            if !held { return None }
            let mine = MulticallPermit {
                chain_id: m.chain_id, token: net.usdc.into(), from: me.into(), value: amount, valid_before: f.valid_before, relayer: m.relayer.clone(),
                refund_to: m.refund_to.clone(), nft_recipient: m.nft_recipient.clone(), metadata: m.metadata.clone(), calls,
            };
            // Relay's nonce must be the digest computed here: else the two signatures wouldn't pair.
            (mine.refusal(me, now).is_none() && mine.digest().ok().map(|d| d.to_vec()) == Some(m.permit.nonce.clone())).then_some(mine)
        });
        // Only v3.2: v3's single EIP-3009 item names no order, so whom Relay pays would be its quote's word alone.
        let Some(v32) = v32 else {
            return fail(format!("There's no {} on {} for gas, and Relay's payment by signature that ihatecrypto can check (v3.2) isn't offered here yet. \
                                 Add a little {} to {}; nothing was signed.", net.gas_coin, net.name, net.gas_coin, net.name));
        };
        let (arriving, spend) = (decimal(&quote.amount_out_formatted).unwrap_or_default(), millionths(amount));
        let worth = if sol { decimal(&quote.amount_out_usd).unwrap_or_default() } else { arriving };
        Self::check_loss(num(Num::of(spend) - worth - topup.unwrap_or_default())?, spend, cap(worth)?, "Relay")?;
        let before = if sol { self.rpc.balance(&self.wallet.solana).await? } else { self.relay_landed(dest, hyperliquid).await? };
        if let Some(m) = &quote.multicall {
            let signatures = self.signer.sign_relay_multicall_permit(v32).await?;
            let [authorization, permit, ..] = signatures.as_slice() else { return fail("The signer didn't sign both halves of Relay's payment. Nothing was sent.") };
            self.record("relay", &quote.request_id, Kind::Signed, &format!("{} USDC→{there} {} by signature (v3.2)", net.name, show(amount)))?;
            // Relay takes the MulticallAuthorization first; the EIP-3009 one before it is refused.
            relay::post_signature(&m.endpoint, &m.body, &hex(authorization)).await?;
            relay::post_permit(&m.permit, &hex(permit)).await?;
        }
        self.report(&format!("Signed: Relay takes {} USDC on {} by signature and pays the gas there; ≈ {} {stable} to {there}.", show(amount), net.name,
                             Fmt::num(arriving, 6)));
        self.relay_arrival(&quote.request_id, &format!("https://relay.link/transaction/{}", quote.request_id), net, dest, hyperliquid, before, arriving,
                           &show(amount), stable, there, topup.is_some(), sol).await
    }

    /// Where a network's dollars can leave with no gas coin there: by signature (Relay, Permit3009) or as a transfer BNB
    /// Chain pays the gas for (MegaFuel, SponsoredTransfer).
    pub(crate) fn leaves_without_gas(net: &EvmNetwork) -> bool {
        Permit3009::supports(net.rpc.chain_id, net.usdc) || SponsoredTransfer::supports(net.rpc.chain_id, net.usdc)
    }

    /// `amount` of `net`'s USDC out with no gas coin there, whichever way the network has.
    pub(crate) async fn bridge_without_gas(&self, net: &EvmNetwork, dest: Option<&EvmNetwork>, hyperliquid: bool, amount: u64, topup: Option<Decimal>,
                                           sol: bool) -> Result<String> {
        if Permit3009::supports(net.rpc.chain_id, net.usdc) { return self.bridge_by_permit(net, dest, hyperliquid, amount, topup, sol).await }
        self.bridge_by_sponsor(net, dest, hyperliquid, amount, topup, sol).await
    }

    /// `amount` of BNB Chain's USDC to Relay's deposit address for the order, as a zero-fee transfer BNB Chain's paymaster
    /// pays the gas for (MegaFuel). The quote is checked like a deposit: from and to this wallet, the transfer exactly this
    /// amount of our token to the deposit address the quote names; the signer then holds the address to the new-recipient
    /// rule.
    pub(crate) async fn bridge_by_sponsor(&self, net: &EvmNetwork, dest: Option<&EvmNetwork>, hyperliquid: bool, amount: u64, topup: Option<Decimal>,
                                          sol: bool) -> Result<String> {
        let (me, chain, to_solana) = (self.wallet.evm.as_str(), net.relay_chain, dest.is_none() && !hyperliquid);
        let there: &'static str = if hyperliquid { "Hyperliquid" } else { dest.map_or("Solana", |d| d.name) };
        let stable = if sol { "SOL" } else if dest.is_some_and(|d| d.key == "robinhood") { "USDG" } else { "USDC" };
        let recipient = if to_solana { self.wallet.solana.as_str() } else { me };
        let quote = self.relay_quote(&relay::Ask {
            user: me.into(), recipient: recipient.into(), origin_chain: chain,
            destination_chain: if hyperliquid { relay::chain::HYPERLIQUID } else { dest.map_or(relay::chain::SOLANA, |d| d.relay_chain) },
            origin_currency: net.usdc.into(),
            destination_currency: if sol { relay::currency::SOLANA_SOL } else if hyperliquid { relay::currency::HYPERLIQUID_USDC }
                                  else { dest.map_or(relay::currency::SOLANA_USDC, |d| d.usdc) }.into(),
            amount, topup_gas_usd: topup, units: Some(net.raw(amount).to_string()), use_deposit_address: true, refund_to: Some(me.into()), ..Default::default()
        }).await?;
        let bound_recipient = if to_solana { quote.recipient.clone() } else { quote.recipient.to_lowercase() };
        let expected_recipient = if to_solana { recipient.to_string() } else { recipient.to_lowercase() };
        let binding = guard::binding_check(&pairs(&[("recipient", &bound_recipient)]), &pairs(&[("recipient", &expected_recipient)]), "Relay quote");
        if !binding.ok() { return stop(binding.problems.join(" ")) }
        let refused = || stop(format!("Relay's deposit on {} isn't a plain transfer of {} USDC to the deposit address it names. Nothing was signed.", net.name,
                                      show(amount)));
        let (Some(deposit), [step]) = (quote.deposit_address.clone().filter(|a| evm::abi::address(a).is_ok()), quote.evm.as_slice()) else { return refused() };
        if step.to.to_lowercase() != net.usdc || step.chain_id != chain || !(step.value == "0" || step.value == "<null>")
            || step.data != erc20::transfer(&deposit, net.raw(amount))? {
            return refused();
        }
        let (arriving, spend) = (decimal(&quote.amount_out_formatted).unwrap_or_default(), millionths(amount));
        let worth = if sol { decimal(&quote.amount_out_usd).unwrap_or_default() } else { arriving };
        Self::check_loss(num(Num::of(spend) - worth - topup.unwrap_or_default())?, spend, cap(worth)?, "Relay")?;
        let transfer = SponsoredTransfer {
            chain_id: net.rpc.chain_id, nonce: mega_fuel::nonce(me).await?, gas_limit: 80_000, token: net.usdc.into(), to: deposit.clone(),
            amount: net.raw(amount).to_string(),
        };
        if !mega_fuel::sponsors(&transfer, me).await? {
            return fail(format!("There's no {} on {} for gas, and BNB Chain won't pay the gas for this transfer now. Add a little {}; nothing was signed.",
                                net.gas_coin, net.name, net.gas_coin));
        }
        let before = if sol { self.rpc.balance(&self.wallet.solana).await? } else { self.relay_landed(dest, hyperliquid).await? };
        let raw = self.signer.sign_sponsored_transfer(transfer.clone(), vec![deposit]).await?;
        let hash = format!("0x{}", to_hex(&keccak(&raw)));
        let note = format!("{} USDC→{there} {}, BNB Chain paying the gas", net.name, show(amount));
        self.record(net.key, &hash, Kind::Signed, &note)?;
        mega_fuel::send(&raw, transfer.nonce, me).await?;
        if !net.rpc.wait_for_receipt(&hash, Duration::from_secs(120)).await? {
            self.record(net.key, &hash, Kind::Failed, &note)?;
            return fail(format!("The transfer to Relay failed on {}: {}/tx/{hash}", net.name, net.explorer));
        }
        self.record(net.key, &hash, Kind::Confirmed, "")?;
        self.record("relay", &quote.request_id, Kind::Signed, &format!("{note}, tx {hash}"))?;
        self.report(&format!("Sent {} USDC on {} to Relay's deposit address, BNB Chain paying the gas; ≈ {} {stable} to {there}.", show(amount), net.name,
                             Fmt::num(arriving, 6)));
        self.relay_arrival(&quote.request_id, &format!("{}/tx/{hash}", net.explorer), net, dest, hyperliquid, before, arriving, &show(amount), stable, there,
                           topup.is_some(), sol).await
    }

    /// A Relay order sent with no gas of ours: its landing (SOL on Solana, or the dollar where it goes), waited for here
    /// or, inside a chain of moves, by the step that needs it.
    #[allow(clippy::too_many_arguments)]
    async fn relay_arrival(&self, request_id: &str, link: &str, net: &EvmNetwork, dest: Option<&EvmNetwork>, hyperliquid: bool, before: u64,
                           arriving: Decimal, sent: &str, stable: &'static str, there: &'static str, gas: bool, sol: bool) -> Result<String> {
        if let Some(landings) = landings() {
            let (me, request_id, link, from, to, said) = (self.arc(), request_id.to_string(), link.to_string(), net.clone(), dest.cloned(), sent.to_string());
            landings.add(if hyperliquid { "hyperliquid" } else { dest.map_or("solana", |d| d.key) }, async move {
                me.arrival(&request_id, &link, &from, to.as_ref(), hyperliquid, before, arriving, &said, stable, there, gas, sol).await
            });
            return Ok(format!("Sent {sent} USDC from {} to {there} with no gas of yours: ≈ {} {stable} lands there in seconds, and what needs it waits for it.",
                              net.name, Fmt::num(arriving, 2)));
        }
        self.arrival(request_id, link, net, dest, hyperliquid, before, arriving, sent, stable, there, gas, sol).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn arrival(&self, request_id: &str, link: &str, net: &EvmNetwork, dest: Option<&EvmNetwork>, hyperliquid: bool, before: u64, arriving: Decimal,
                     sent: &str, stable: &str, there: &str, gas: bool, sol: bool) -> Result<String> {
        if !sol { return self.relay_landing(request_id, link, net, dest, hyperliquid, before, arriving, sent, stable, there, gas).await }
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(180) {
            let status = relay::status(request_id).await?.0;
            if ["refund", "failure"].contains(&status.as_str()) {
                self.record("relay", request_id, Kind::Failed, &status)?;
                return fail(format!("Relay returned {status}. Check USDC on {}. {link}", net.name));
            }
            let now = self.rpc.balance(&self.wallet.solana).await?;
            if status == "success" && now > before {
                self.record("relay", request_id, Kind::Settled, &status)?;
                return Ok(format!("Moved {sent} USDC from {} to Solana as SOL in {} s: +{} SOL.", net.name, started.elapsed().as_secs(),
                                  Fmt::num(Decimal::from(now - before) / Decimal::from(1_000_000_000u64), 6)));
            }
            sleep(2).await;
        }
        fail(format!("Relay didn't credit Solana within 3 minutes. Order {request_id} is in the journal. {link}"))
    }

    /// An exact approve when the allowance is short, then a wait until the chain shows it: the next call's dry run on a
    /// node that hasn't seen the approve yet would revert. (Swift's two `approve`s: a 64-bit amount is `U256::from`.)
    pub(crate) async fn approve(&self, rpc: &EvmRpc, token: &str, spender: &str, units: U256, note: &str, recipients: &[String]) -> Result<()> {
        let allowance = || async {
            let answer = rpc.read(token, &erc20::allowance(&self.wallet.evm, spender)?).await?;
            Ok::<_, Error>(u256_in(&answer, 0).unwrap_or(U256::ZERO))
        };
        if allowance().await? >= units { return Ok(()) }
        self.send_evm(rpc, token, &erc20::approve(spender, units)?, note, recipients, None, 0).await?;
        for _ in 0..15 {
            if allowance().await? >= units { return Ok(()) }
            sleep(1).await;
        }
        fail("The approve went through but the network doesn't show it yet. Try again in a minute; nothing else was signed.")
    }

    /// Settings › Wallet › «One transaction for approve and swap»'s key on the Mac; here the engine sets `BATCHES_ON`.
    pub const BATCH_KEY: &str = "batch7702";
    pub fn batches_on() -> bool { BATCHES_ON.load(std::sync::atomic::Ordering::Relaxed) }

    /// Whether calls on this network can go as one EIP-7702 transaction: the user's switch on, a network in
    /// Batch7702.chains that takes fees in its own coin, and a signer that signs batches (not a Ledger).
    pub(crate) fn can_batch(&self, rpc: &EvmRpc) -> bool {
        Self::batches_on() && self.signer.signs_batches() && batch7702::CHAINS.contains(&rpc.chain_id)
            && EvmNetwork::of(rpc.chain_id).and_then(|n| n.fee_token).is_none()
    }

    /// An exact approve and the call that spends it. Where they can batch they go as one transaction, dry-run together
    /// first; elsewhere the approve, its wait, then the call. `value`: wei of the network's coin sent with the call; a
    /// batch carries none, so then the approve goes on its own first and the call with its coins after it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn approve_and_send(&self, rpc: &EvmRpc, token: &str, spender: &str, units: U256, approve_note: &str, to: &str, data: &[u8],
                                         note: &str, recipients: &[String], value: u64) -> Result<String> {
        if value == 0 && self.can_batch(rpc) {
            let answer = rpc.read(token, &erc20::allowance(&self.wallet.evm, spender)?).await?;
            if u256_in(&answer, 0).unwrap_or(U256::ZERO) < units {
                let calls = vec![BatchCall { to: token.into(), data: erc20::approve(spender, units)? }, BatchCall { to: to.into(), data: data.to_vec() }];
                return self.send_evm(rpc, &self.wallet.evm, &[], &format!("{approve_note} + {note}"), recipients, Some(calls), 0).await;
            }
        } else {
            // send_evm's own rule for coins, before the approve: on a network paying fees in a token it would sign for nothing.
            if value != 0 && EvmNetwork::of(rpc.chain_id).and_then(|n| n.fee_token).is_some() {
                return fail("Coins go only with a single call on a network that pays its fees in them. Nothing was signed.");
            }
            self.approve(rpc, token, spender, units, approve_note, recipients).await?;
        }
        self.send_evm(rpc, to, data, note, recipients, None, value).await
    }

    /// One EVM transaction: dry run by eth_estimateGas, the fee within the coin there, the journal written before sending.
    /// `batch`: these calls instead, as one EIP-7702 transaction from and to the wallet (`to` and `data` unused).
    /// `value`: wei of the network's coin sent with the call; never with `batch`. (Boxed: it's on call cycles, through
    /// choose_fee_token and the gas top-up.)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn send_evm<'a>(&'a self, rpc: &'a EvmRpc, to: &'a str, data: &'a [u8], note: &'a str, recipients: &'a [String],
                               batch: Option<Vec<BatchCall>>, value: u64) -> Boxed<'a, String> {
        Box::pin(async move {
            let me = self.wallet.evm.as_str();
            let Some(net) = EvmNetwork::of(rpc.chain_id) else {
                return fail(format!("ihatecrypto doesn't move money on network {}. Nothing was signed.", rpc.chain_id));
            };
            if !(value == 0 || (batch.is_none() && net.fee_token.is_none())) {
                return fail("Coins go only with a single call on a network that pays its fees in them. Nothing was signed.");
            }
            if let Some(token) = net.fee_token {
                if to.to_lowercase() != tempo::FEE_MANAGER {
                    // Only a plain transfer of the fee coin pays its fee in that coin by itself.
                    let pays_in_token = matches!(Call::decode(data), Call::Transfer { .. }) && to.to_lowercase() == token;
                    if !pays_in_token { self.choose_fee_token(net, token).await? }
                }
            }
            let estimate = match &batch {
                Some(calls) => rpc.estimate_gas(me, me, 0, &batch7702::execute_batch(calls)?, Some(batch7702::DELEGATE)).await?,
                None => rpc.estimate_gas(me, to, value, data, None).await?,
            };
            let (tip, max_fee) = rpc.fees().await?;
            let out_of_range = || Error::Execution(crate::OUT_OF_RANGE.into());
            let gas_limit = estimate.checked_add(estimate / 4).and_then(|g| g.checked_add(if batch.is_none() { 0 } else { batch7702::AUTHORIZATION_GAS }))
                .ok_or_else(out_of_range)?;
            let fee = gas_limit.checked_mul(max_fee).ok_or_else(out_of_range)?;
            if let Some(token) = net.fee_token {
                if tempo::fee_units(fee) > token_balance(rpc, token, me).await? {
                    return fail(format!("Not enough {} on {} for the fee: {note}.", net.gas_coin, net.name));
                }
            } else {
                // Short of gas is never a dead end: a top-up of three times what this call may cost comes over first. Coins
                // sent with the call are the user's amount: when they and the fee don't fit, it's said, not topped up.
                if value > 0 && fee as u128 + value as u128 > rpc.balance(me).await? {
                    return fail(format!("{} {} and the fee don't both fit in the {} on {}: {note} wasn't signed.",
                                        Fmt::num(Decimal::from(value) / Decimal::from(1_000_000_000_000_000_000u64), 8), net.gas_coin, net.gas_coin, net.name));
                }
                if fee as u128 > rpc.balance(me).await? {
                    // A top-up that itself runs short (Relay's swap for gas with none there) stops instead of looping.
                    if topping_up() {
                        return fail(format!("Not enough {} on {} even to buy gas there: {note}. Top up Solana's USDC and try again.", net.gas_coin, net.name));
                    }
                    let price = relay::native_price(net.rpc.chain_id).await?.unwrap_or_default();
                    let usd = if price > Decimal::ZERO {
                        num(Num::of(coin(fee as u128)) * price * 3)?.max(net.topup_usd).max(d("0.5")).min(Decimal::TEN)
                    } else {
                        net.topup_usd.max(Decimal::ONE)
                    };
                    self.report(&format!("Not enough {} on {} for «{note}»: bringing {} of it first.", net.gas_coin, net.name, Fmt::usd(usd)));
                    TOPPING_UP.scope(true, self.top_up_gas(net, usd)).await?;
                    if fee as u128 > rpc.balance(me).await? {
                        return fail(format!("Not enough {} on {} for gas even after a top-up: {note}.", net.gas_coin, net.name));
                    }
                }
            }
            let nonce = self.next_nonce(rpc).await?;
            let fees = EvmFees { chain_id: rpc.chain_id, nonce, max_priority_fee: tip, max_fee, gas_limit };
            let batched = batch.is_some();
            let raw = match batch {
                Some(calls) => self.signer.sign_evm_batch(fees, calls, recipients.to_vec()).await?,
                None if value > 0 => self.signer.sign_evm_payable(fees, to.into(), value, data.to_vec(), recipients.to_vec()).await?,
                None => self.signer.sign_evm(fees, to.into(), data.to_vec(), recipients.to_vec()).await?,
            };
            let hash = format!("0x{}", to_hex(&keccak(&raw)));
            self.record(net.key, &hash, Kind::Signed, note)?;
            rpc.send(&raw).await?;
            if !rpc.wait_for_receipt(&hash, Duration::from_secs(120)).await? {
                self.record(net.key, &hash, Kind::Failed, note)?;
                return fail(format!("{note} failed on {}: {}/tx/{hash}", net.name, net.explorer));
            }
            self.record(net.key, &hash, Kind::Confirmed, "")?;
            // A batch's own authorization takes the next nonce too.
            self.confirmed(nonce + if batched { 2 } else { 1 }, rpc);
            self.report(&format!("{note}: done. {}/tx/{hash}", net.explorer));
            Ok(hash)
        })
    }

    /// The nonce for this wallet's next transaction: never below what follows our last confirmed transaction there (a
    /// node can answer behind right after our own).
    pub(crate) async fn next_nonce(&self, rpc: &EvmRpc) -> Result<u64> {
        let node = rpc.nonce(&self.wallet.evm).await?;
        let key = format!("{}:{}", rpc.chain_id, self.wallet.evm.to_lowercase());
        Ok(node.max(EVM_NONCES.lock().unwrap_or_else(|e| e.into_inner()).get(&key).copied().unwrap_or(0)))
    }

    /// Only after a receipt: a transaction that never landed would leave a gap no later one could get past.
    pub(crate) fn confirmed(&self, next: u64, rpc: &EvmRpc) {
        let key = format!("{}:{}", rpc.chain_id, self.wallet.evm.to_lowercase());
        let mut nonces = EVM_NONCES.lock().unwrap_or_else(|e| e.into_inner());
        let at = nonces.entry(key).or_insert(0);
        *at = (*at).max(next);
    }

    /// `usd` of the network's own coin for gas, at our address there: over Relay from Solana's USDC; from Hyperliquid's
    /// balance; from this network's USDC by signature through Solana; or, short everywhere, Relay swaps the network's
    /// own USDC for it on the chain.
    pub async fn top_up_gas(&self, net: &EvmNetwork, usd: Decimal) -> Result<String> {
        if net.fee_token.is_some() { return Ok(format!("{} takes its fees in {}: there's no gas coin to top up.", net.name, net.gas_coin)) }
        if usd < d("0.1") || usd > Decimal::from(20) { return fail("A gas top-up is between $0.10 and $20.") }
        let (me, rpc) = (self.wallet.evm.as_str(), &net.rpc);
        let before = rpc.balance(me).await?;
        let price = relay::native_price(rpc.chain_id).await?.unwrap_or_default();
        let expected = Self::gas_expected(usd, price);
        let solana_usdc = millionths(self.rpc.token_balance(&self.wallet.solana, relay::currency::SOLANA_USDC).await?);
        let dollars = uint64(num(((Num::of(usd) + 1) * 1_000_000).rounded(0, Rounding::Up))?);
        let floor = Some((before.saturating_add(expected as u128), usd));
        if solana_usdc >= usd + Decimal::ONE {
            self.ensure_evm(net, 0, floor).await?;
        } else if Hyperliquid::balance(me).await?.withdrawable >= ihatecrypto_core::foundation::rounded(usd, 2, Rounding::Up) + Decimal::ONE {
            // Hyperliquid's balance brings it with no gas anywhere: the way for BNB Chain and HyperEVM.
            let line = self.hyperliquid_gas(net, usd).await?;
            self.report(&line);
        } else if before < net.gas_floor as u128 && Self::leaves_without_gas(net) && net.usdc_balance(me).await? >= dollars.saturating_add(1_000_000) {
            // No gas anywhere: this network's USDC goes to Solana by signature — a dollar of it as SOL when Solana has none
            // for its own fee — and the gas comes back from there.
            self.report(&format!("No {} on {} and too little USDC on Solana to bring it: {}'s USDC goes to Solana by signature first, then the gas comes back.",
                                 net.gas_coin, net.name, net.name));
            LANDINGS.scope(None, async {
                if self.rpc.balance(&self.wallet.solana).await? < TopUpOrder::SOLANA_FEE_FLOOR {
                    let line = self.bridge_without_gas(net, None, false, 1_000_000, None, true).await?;
                    self.report(&line);
                }
                let line = self.bridge_without_gas(net, None, false, dollars, None, false).await?;
                self.report(&line);
                Ok::<_, Error>(())
            }).await?;
            self.ensure_evm(net, 0, floor).await?;
        } else {
            self.swap_for_gas(net, usd).await?;
        }
        Ok(Self::gas_line(net, before, rpc.balance(me).await?, usd))
    }

    /// A top-up is said done only when the coin grew. Not grown, it's said, not thrown: the money went, and «Try again»
    /// would buy the gas a second time.
    pub(crate) fn gas_line(net: &EvmNetwork, before: u128, after: u128, usd: Decimal) -> String {
        let c = |wei: u128| Fmt::num(coin(wei), 8);
        if after > before { return format!("Topped up gas on {}: {} → {} {} (≈ {}).", net.name, c(before), c(after), net.gas_coin, Fmt::usd(usd)) }
        if after == before {
            return format!("The gas on {} hasn't shown up yet: still {} {}, though ≈ {} went to the top-up. Check the balance in a minute before topping up again.",
                           net.name, c(before), net.gas_coin, Fmt::usd(usd));
        }
        format!("The gas on {} didn't grow: {} → {} {}, though ≈ {} went to the top-up: {}'s fees for the swap took more than it brought, and another swap \
                 this size would too. With {} of USDC on Solana or Hyperliquid the gas comes from there, with no fee on {}.",
                net.name, c(before), c(after), net.gas_coin, Fmt::usd(usd), net.name, Fmt::usd(usd + Decimal::ONE), net.name)
    }

    /// Most of the wei `usd` buys at `price`: a top-up's wait ends when 80% of it is there.
    pub(crate) fn gas_expected(usd: Decimal, price: Decimal) -> u64 {
        if price <= Decimal::ZERO { return 1 }
        (Num::of(usd) / price * Decimal::from(1_000_000_000_000_000_000u64) * d("0.8")).rounded(0, Rounding::Down).0.map_or(u64::MAX, uint64)
    }

    /// Gas bought with the network's own USDC by Relay's swap on the chain: an exact approve to the approval proxy Relay
    /// publishes for it, then its call to that proxy, from and to this wallet, at most 5% (or $0.15) lost.
    async fn swap_for_gas(&self, net: &EvmNetwork, usd: Decimal) -> Result<()> {
        let (me, rpc, chain) = (self.wallet.evm.as_str(), &net.rpc, net.relay_chain);
        let units = uint64(num((Num::of(usd) * 1_000_000).rounded(0, Rounding::Up))?);
        if net.usdc_balance(me).await? < units {
            return fail(format!("No gas on {}, and neither Solana nor {} has {} of USDC to buy it. Top up either first.", net.name, net.name, Fmt::usd(usd)));
        }
        let Some(proxy) = relay::approval_proxy(chain).await?.map(|p| p.to_lowercase()) else {
            return fail(format!("Relay publishes no approval proxy for {}.", net.name));
        };
        // Relay's quote with every check: from and to this wallet, its call to the proxy after an exact approve, what it gives.
        let checked = || async {
            let quote = self.relay_quote(&relay::Ask {
                user: me.into(), recipient: me.into(), origin_chain: chain, destination_chain: chain, origin_currency: net.usdc.into(),
                destination_currency: "0x0000000000000000000000000000000000000000".into(), amount: units, units: Some(net.raw(units).to_string()),
                ..Default::default()
            }).await?;
            let binding = guard::binding_check(&pairs(&[("sender", &quote.sender.to_lowercase()), ("recipient", &quote.recipient.to_lowercase())]),
                                               &pairs(&[("sender", &me.to_lowercase()), ("recipient", &me.to_lowercase())]), "Relay quote");
            if !binding.ok() { return stop(binding.problems.join(" ")) }
            let approves = |step: &relay::EvmStep| {
                let args = &step.data[step.data.len().min(4)..];
                step.to.to_lowercase() == net.usdc && step.data.get(..4) == Some(&[0x09, 0x5e, 0xa7, 0xb3][..])
                    && address_in(args, 0).as_deref() == Some(proxy.as_str()) && u256_in(args, 1) == Some(net.raw(units))
            };
            let call = match quote.evm.split_last() {
                Some((call, approves_first)) if call.to.to_lowercase() == proxy && call.chain_id == chain && (call.value == "0" || call.value == "<null>")
                    && approves_first.iter().all(approves) => call.clone(),
                _ => return fail(format!("Relay's transactions don't go to the approval proxy it publishes for {}, for exactly this amount. Nothing was signed.",
                                         net.name)),
            };
            let worth_out = decimal(&quote.amount_out_usd).unwrap_or_default();
            if worth_out <= Decimal::ZERO { return fail(format!("Relay doesn't quote {} for {} now. Nothing was signed.", net.gas_coin, Fmt::usd(usd))) }
            Self::check_loss(num(Num::of(usd) - worth_out)?, usd, num(Num::of(usd) * d("0.05"))?.max(d("0.15")), "Relay's swap")?;
            Ok(call)
        };
        checked().await?;
        let before = rpc.balance(me).await?;
        // Batched (EIP-7702), the approve goes with the swap; otherwise first, on its own.
        let approve_note = format!("Approve exactly {} USDC to Relay for gas", Fmt::num(usd, 2));
        if !self.can_batch(rpc) { self.approve(rpc, net.usdc, &proxy, net.raw(units), &approve_note, &[]).await? }
        // Quoted again after the approve's wait, and once more if its dry run reverts: a route is priced for its moment.
        for attempt in 0..2 {
            let call = checked().await?;
            match self.approve_and_send(rpc, net.usdc, &proxy, net.raw(units), &approve_note, &call.to, &call.data,
                                        &format!("Relay: {} of USDC into {} on {}", Fmt::usd(usd), net.gas_coin, net.name), &[], 0).await {
                Ok(_) => break,
                Err(e) if attempt == 0 && e.to_string().contains("eth_estimateGas") => sleep(2).await,
                Err(e) => return Err(e),
            }
        }
        for _ in 0..30 {
            if rpc.balance(me).await? > before { break }
            sleep(1).await;
        }
        Ok(())
    }

    /// Tempo: the wallet's fee token set to `token` once, before a call that isn't a plain transfer of it.
    async fn choose_fee_token(&self, net: &EvmNetwork, token: &str) -> Result<()> {
        let answer = net.rpc.read(tempo::FEE_MANAGER, &tempo::user_token(&self.wallet.evm)?).await?;
        if address_in(&answer, 0).as_deref() == Some(token) { return Ok(()) }
        self.send_evm(&net.rpc, tempo::FEE_MANAGER, &tempo::set_user_token(token)?, &format!("Pay {} fees in {} from now on", net.name, net.gas_coin),
                      &[], None, 0).await?;
        Ok(())
    }
}

/// BNB Chain's paymaster, MegaFuel (MegaFuel.swift's network half): asks whether a zero-fee transaction is sponsored,
/// gives the nonce and takes it. Public, no key.
mod mega_fuel {
    use super::*;

    pub const ENDPOINT: &str = "https://bsc-megafuel.nodereal.io/";

    async fn call(method: &str, params: Value) -> Result<Value> {
        let body = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        Ok(net::run(&net::DELAYS, |_| net::post_json(ENDPOINT, &body), |data, status| {
            let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(data) else {
                return Err(net::Again::new(status == 429 || status >= 500, format!("MegaFuel: {}", net::said(data, status))));
            };
            if let Some(e) = j.get("error").filter(|e| e.is_object()) {
                let message = e.get("message").and_then(Value::as_str).map_or_else(|| e.to_string(), str::to_string);
                return Err(net::Again::fin(wallet::WalletError(format!("MegaFuel: {message}"))));
            }
            j.get("result").cloned().ok_or_else(|| net::Again::fin(wallet::WalletError("MegaFuel answered nothing.".into())))
        }).await?)
    }

    /// Whether BNB Chain pays the gas for this transfer from `from`.
    pub async fn sponsors(t: &SponsoredTransfer, from: &str) -> Result<bool> {
        let tx = serde_json::json!({"from": from, "to": t.token, "value": "0x0", "data": format!("0x{}", to_hex(&t.calldata()?)),
                                    "gas": format!("0x{:x}", t.gas_limit)});
        Ok(call("pm_isSponsorable", serde_json::json!([tx])).await?.get("sponsorable").and_then(Value::as_bool).unwrap_or(false))
    }

    /// The next nonce, pending transactions counted (MegaFuel's own view).
    pub async fn nonce(address: &str) -> Result<u64> {
        let answer = call("eth_getTransactionCount", serde_json::json!([address, "pending"])).await?;
        answer.as_str().and_then(|h| h.get(2..)).and_then(|h| u64::from_str_radix(h, 16).ok())
            .ok_or_else(|| Error::Wallet("MegaFuel gave no nonce.".into()))
    }

    /// Sends the signed zero-fee transaction; MegaFuel bundles it with its own that pays the gas. Returns the hash. A
    /// second ask can meet the first one already in, so an error is held against MegaFuel's own count, not read by its
    /// words: past `nonce`, the transfer is in, and the receipt decides.
    pub async fn send(raw: &[u8], nonce: u64, from: &str) -> Result<String> {
        let sent = match call("eth_sendRawTransaction", serde_json::json!([format!("0x{}", to_hex(raw))])).await {
            Ok(Value::String(hash)) => return Ok(hash),
            Ok(_) => Error::Wallet("MegaFuel gave no transaction hash.".into()),
            Err(e) => e,
        };
        match self::nonce(from).await {
            Ok(now) if now > nonce => Ok(format!("0x{}", to_hex(&keccak(raw)))),
            _ => Err(sent),
        }
    }
}
