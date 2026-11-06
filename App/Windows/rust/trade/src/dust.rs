//! Leftover tokens into USDC on their own networks (Dust.swift).

use evm::protocols::uniswap;
use ihatecrypto_core::foundation::Num;
use ihatecrypto_core::intent::Fmt;
use rust_decimal::Decimal;

use crate::aave::EvmNetwork;
use crate::wallet::usd;
use crate::{Executor, Result, fail};

/// A token to sweep: on which network, its contract and symbol, and what the portfolio says it's worth.
#[derive(Debug, Clone, PartialEq)]
pub struct Dust {
    pub chain: String,
    pub contract: String,
    pub symbol: String,
    pub usd: Decimal,
}

impl Dust {
    pub fn new(chain: &str, contract: &str, symbol: &str, usd: Decimal) -> Self {
        Self { chain: chain.into(), contract: contract.into(), symbol: symbol.into(), usd }
    }
}

impl Executor {
    /// The networks dust is swept on: their gas costs cents, so a dollar of a token is worth selling there.
    pub const DUST_NETWORKS: [&str; 3] = ["base", "arbitrum", "polygon"];

    /// Leftover tokens into USDC on their own networks, one sale each (Uniswap, the USDC kept there): each one quoted first,
    /// and left alone when there's no route or its gas would take half of what it brings. What was swept and what was
    /// left, in a line each.
    pub async fn sweep_dust(&self, tokens: &[Dust]) -> Result<String> {
        let (mut swept, mut left, mut got) = (Vec::<String>::new(), Vec::<String>::new(), Num::of(Decimal::ZERO));
        for t in tokens {
            let Some((net, uni)) = EvmNetwork::named(&t.chain).and_then(|n| uniswap::v3(n.rpc.chain_id).map(|u| (n, u))) else {
                left.push(format!("{}: not on a network dust is swept on", t.symbol));
                continue;
            };
            let step: Result<()> = async {
                let token = self.evm_token(&t.contract, net).await?;
                let held = self.balance(&token.address, &net.rpc).await?;
                let best = if held > Decimal::ZERO { self.best_uniswap(net, uni, &token.address, net.usdc, held).await } else { None };
                let Some(best) = best else {
                    left.push(format!("{} on {}: no Uniswap route to USDC", t.symbol, net.name));
                    return Ok(());
                };
                let out = best.out / Decimal::from(1_000_000);
                let gas = self.gas_usd(best.gas(), net, uni).await?;
                if !(Num::of(gas) * 2).0.is_some_and(|g| g < out) {
                    left.push(format!("{} on {}: {} would cost {} of gas", t.symbol, net.name, usd(out), usd(gas)));
                    return Ok(());
                }
                self.report(&format!("Dust: {} on {} → ≈ {} USDC on {}, {} of gas.", t.symbol, net.name, Fmt::num(out, 2), best.title(), usd(gas)));
                let line = self.sell_on(net, &token.address, None, None, true, None).await?;
                got = got + out;
                swept.push(format!("{} on {}", t.symbol, net.name));
                self.report(&line);
                Ok(())
            }.await;
            if let Err(e) = step { left.push(format!("{} on {}: {e}", t.symbol, t.chain)) }
        }
        if swept.is_empty() {
            return fail(if tokens.is_empty() {
                "No dust to sweep: no verified token under the limit on Base, Arbitrum or Polygon.".to_string()
            } else {
                format!("Nothing swept: {}.", left.join("; "))
            });
        }
        Ok(format!("Swept {} token{} into ≈ {} USDC: {}.{}", swept.len(), if swept.len() == 1 { "" } else { "s" },
                   got.0.map_or_else(|| "NaN".to_string(), |g| Fmt::num(g, 2)), swept.join(", "),
                   if left.is_empty() { String::new() } else { format!(" Left: {}.", left.join("; ")) }))
    }
}
