//! The approvals this wallet has given on its EVM networks — who may still spend which token — and their revoking
//! (Approvals.swift). Found from the wallet's own transactions on each network's Blockscout (approve calls, Permit2's
//! too), then read from the chain as they stand now: an approve spent by a swap is zero again, an old one a site asked
//! for may not be.

use std::collections::BTreeSet;
use std::time::Duration;

use chrono::Utc;
use evm::call::Call;
use evm::protocols::{RELAY_PROXIES, aave, batch7702, morpho, uniswap, v4};
use evm::{U256, abi, erc20};
use ihatecrypto_core::foundation::shifted;
use ihatecrypto_core::holdings;
use ihatecrypto_core::intent::Fmt;
use rust_decimal::Decimal;
use serde_json::{Map, Value};

use crate::aave::EvmNetwork;
use crate::wallet::{prefix, sleep};
use crate::{Executor, Result, decimal};

#[derive(Debug, Clone, PartialEq)]
pub struct TokenApproval {
    pub chain: String,
    pub token: String,
    pub symbol: String,
    /// None: the token didn't answer decimals(), so the amount can't be shown (never 18's guess). Revoking doesn't need it.
    pub decimals: Option<u32>,
    pub spender: String,
    /// Uniswap's router, Relay, Aave… — None for a contract ihatecrypto doesn't know.
    pub spender_name: Option<String>,
    /// The allowance in the token's smallest units; Decimal::MAX for one past Decimal's range (2²⁵⁶ − 1, «unlimited»).
    pub amount: Decimal,
    /// Given through Permit2 (its own allowance, with an expiry) rather than to the spender itself.
    pub permit2: bool,
}

impl TokenApproval {
    pub fn id(&self) -> String { format!("{}:{}:{}:{}", self.chain, self.token, self.spender, self.permit2) }

    /// ≥ 2⁹⁶ units: the drainers' «unlimited».
    pub fn unlimited(&self) -> bool { self.amount >= Decimal::from_i128_with_scale(79_000_000_000_000_000_000_000_000_000, 0) }

    /// The amount as the approvals board shows it.
    pub fn shown(&self) -> String {
        if self.unlimited() { return "unlimited".into() }
        let Some(decimals) = self.decimals else { return "amount unknown: decimals() didn't answer".into() };
        Fmt::num(shifted(self.amount, decimals as i64).unwrap_or_default(), 6)
    }
}

/// The last `n` characters (`suffix(n)`).
fn suffix(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

/// One page of the wallet's own transactions on a Blockscout, asked again on a throttle or a server error (2, 4, 8 s);
/// None when it didn't answer.
async fn transactions_page(host: &str, address: &str, next: Option<&Map<String, Value>>) -> Option<Map<String, Value>> {
    let mut query = vec![("filter".to_string(), "from".to_string())];
    let mut keys: Vec<&String> = next.map(|n| n.keys().collect()).unwrap_or_default();
    keys.sort();
    for k in keys {
        let v = &next?[k];
        query.push((k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_string)));
    }
    // URLComponents' query: letters, digits and «-._~» as they are, the rest percent-encoded.
    let encode = |v: &str| v.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect::<String>();
    let url = format!("https://{host}/api/v2/addresses/{address}/transactions?{}",
                      query.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&"));
    for attempt in 0..4u32 {
        let mut request = net::HTTP.get(&url).timeout(Duration::from_secs(30));
        for (k, v) in holdings::BROWSER { request = request.header(k, v) }
        let Ok(response) = request.send().await else { return None };
        let code = response.status().as_u16();
        if (code == 429 || code >= 500) && attempt < 3 {
            sleep(2 << attempt).await;
            continue;
        }
        let data = response.bytes().await.ok()?;
        return serde_json::from_slice::<Value>(&data).ok().and_then(|v| v.as_object().cloned());
    }
    None
}

impl Executor {
    /// The networks approvals are read on: those with a Blockscout ihatecrypto reads.
    pub const APPROVAL_NETWORKS: [&str; 5] = ["base", "arbitrum", "ethereum", "polygon", "robinhood"];

    /// Who a spender is, when ihatecrypto knows it.
    pub fn spender_name(spender: &str, chain_id: u64) -> Option<&'static str> {
        let s = spender.to_lowercase();
        if s == v4::PERMIT2 { return Some("Permit2 (Uniswap)") }
        if uniswap::v3(chain_id).is_some_and(|u| u.1 == s) || (chain_id == 4663 && s == uniswap::ROBINHOOD_ROUTER) { return Some("Uniswap v3 router") }
        if uniswap::v2(chain_id).is_some_and(|u| u.0 == s) { return Some("Uniswap v2 router") }
        if v4::on(chain_id).is_some_and(|u| u.router == s) { return Some("Uniswap's Universal Router") }
        if s == "0x4cd00e387622c35bddb9b4c962c136462338bc31" || RELAY_PROXIES.contains(&s.as_str()) { return Some("Relay") }
        if aave::MARKETS.iter().any(|m| m.chain_id == chain_id && m.pool.to_lowercase() == s) { return Some("Aave pool") }
        if morpho::VAULTS.iter().any(|v| v.chain_id == chain_id && v.address.to_lowercase() == s) { return Some("Morpho vault") }
        None
    }

    /// Every approval this wallet still has on its networks, the unlimited ones first, then by network; and the networks
    /// whose Blockscout didn't answer (said by the caller as «couldn't read»).
    pub async fn approvals(&self) -> (Vec<TokenApproval>, Vec<String>) {
        let mut set = tokio::task::JoinSet::new();
        for chain in Self::APPROVAL_NETWORKS {
            let me = self.arc();
            set.spawn(async move { (chain, me.approvals_on(chain).await) });
        }
        let (mut list, mut unread): (Vec<TokenApproval>, Vec<String>) = (vec![], vec![]);
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((_, Some(found))) => list.extend(found),
                Ok((chain, None)) => unread.push(chain.into()),
                Err(_) => {}
            }
        }
        list.sort_by(|a, b| (!a.unlimited(), &a.chain, &a.symbol).cmp(&(!b.unlimited(), &b.chain, &b.symbol)));
        unread.sort();
        (list, unread)
    }

    /// One network's: the (token, spender) pairs the wallet's own transactions approved — a token's approve or
    /// increaseAllowance, Permit2's approve, each also inside a 7702 batch — each read now; None when Blockscout didn't
    /// answer. Its v2 list, not the logs API: that one takes 10 calls an hour without a key. (Swift's `approvals(on:)`.)
    // ponytail: approvals given by a signature (EIP-2612 permit, Permit2's permit) leave no transaction of the wallet's and
    // aren't found; the signer refuses both. The newest 1000 transactions at most, and the allowances are read one at a time.
    pub async fn approvals_on(&self, chain: &str) -> Option<Vec<TokenApproval>> {
        let (Some(host), Some(net)) = (holdings::lookup(&holdings::BLOCKSCOUT, chain), EvmNetwork::named(chain)) else { return Some(vec![]) };
        let me = self.wallet.evm.to_lowercase();
        let mut pairs: BTreeSet<(String, String, bool)> = BTreeSet::new();
        let mut next: Option<Map<String, Value>> = None;
        for page in 0..20 {
            if page > 0 && next.is_none() { break }
            let list = transactions_page(host, &self.wallet.evm, next.as_ref()).await;
            let items = list.as_ref().and_then(|l| l.get("items")).and_then(Value::as_array)?;
            for t in items {
                let (Some(to), Some(input)) = (t.pointer("/to/hash").and_then(Value::as_str).map(str::to_lowercase),
                                               t.get("raw_input").and_then(Value::as_str).and_then(wallet::from_hex)) else { continue };
                let calls = if to == me { batch7702::calls(&input).unwrap_or_default() } else { vec![batch7702::BatchCall { to: to.clone(), data: input }] };
                for c in calls {
                    match Call::decode(&c.data) {
                        Call::Approve { spender, .. } | Call::IncreaseAllowance { spender, .. } => {
                            pairs.insert((c.to.to_lowercase(), spender.to_lowercase(), false));
                        }
                        Call::Permit2Approve { token, spender, .. } if c.to.to_lowercase() == v4::PERMIT2 => {
                            pairs.insert((token.to_lowercase(), spender.to_lowercase(), true));
                        }
                        _ => {}
                    }
                }
            }
            next = list.as_ref().and_then(|l| l.get("next_page_params")).and_then(Value::as_object).cloned();
        }
        let mut out = vec![];
        let mut token_info: std::collections::HashMap<String, (String, Option<u32>)> = Default::default();
        let now = Utc::now().timestamp().max(0) as u64;
        for (token, spender, permit2) in pairs {
            let asked = if permit2 {
                v4::permit2_allowance(&self.wallet.evm, &token, &spender).map(|call| (v4::PERMIT2.to_string(), call))
            } else {
                erc20::allowance(&self.wallet.evm, &spender).map(|call| (token.clone(), call))
            };
            let answer = match asked {
                Ok((to, call)) => net.rpc.read(&to, &call).await,
                Err(e) => Err(e),
            };
            let answer = match answer {
                Ok(a) => a,
                // A revert is an answer — no ERC-20 allowance there (an NFT's approve shares the selector). Any other
                // failure isn't a zero: the network is said unread rather than shown clean.
                Err(e) if e.0.to_lowercase().contains("revert") => continue,
                Err(_) => return None,
            };
            // Permit2's allowance counts until it expires.
            let expired = permit2 && abi::uint_in(&answer, 1).unwrap_or(0) <= now;
            // Past Decimal's range (an «unlimited» 2²⁵⁶ − 1): Decimal::MAX, still unlimited — never dropped.
            let Some(amount) = abi::u256_in(&answer, 0).map(|n| decimal(n).unwrap_or(Decimal::MAX)) else { continue };
            if expired || amount <= Decimal::ZERO { continue }
            if !token_info.contains_key(&token) {
                let symbol = net.rpc.read(&token, &[0x95, 0xd8, 0x9b, 0x41]).await.ok().and_then(|d| abi::string(&d))
                    .unwrap_or_else(|| format!("{}…{}", prefix(&token, 6), suffix(&token, 4)));
                let decimals = abi::decimals(net.rpc.read(&token, &[0x31, 0x3c, 0xe5, 0x67]).await.ok().as_deref());
                let shown = if holdings::lookup(&holdings::BRIDGED_USDC, chain) == Some(token.as_str()) { "USDC.e".into() } else { symbol };
                token_info.insert(token.clone(), (shown, decimals));
            }
            let (symbol, decimals) = token_info[&token].clone();
            out.push(TokenApproval {
                chain: chain.into(), token: token.clone(), symbol, decimals, spender_name: Self::spender_name(&spender, net.rpc.chain_id).map(str::to_string),
                spender, amount, permit2,
            });
        }
        Some(out)
    }

    /// Sets an approval to zero: the token's approve(spender, 0), or Permit2's own approve(token, spender, 0, 0). Nothing
    /// is spent and nobody gains a right, so the signer asks no recipient for it.
    pub async fn revoke_approval(&self, net: &EvmNetwork, token: &str, spender: &str, permit2: bool) -> Result<String> {
        let data = if permit2 { v4::permit2_approve(token, spender, U256::ZERO, 0)? } else { erc20::approve(spender, U256::ZERO)? };
        let note = format!("Revoke {}…{}'s {}approval of {}…{} on {}", prefix(spender, 6), suffix(spender, 4), if permit2 { "Permit2 " } else { "" },
                           prefix(token, 6), suffix(token, 4), net.name);
        let hash = self.send_evm(&net.rpc, if permit2 { v4::PERMIT2 } else { token }, &data, &note, &[], None, 0).await?;
        Ok(format!("Revoked: {} can no longer spend that token on {}{}. {}/tx/{hash}", Self::spender_name(spender, net.rpc.chain_id).unwrap_or(spender), net.name,
                   if permit2 { " through Permit2" } else { "" }, net.explorer))
    }
}
