//! Importing, read before anything is kept (Imports.swift, the Mac's ImportWalletView): a private key shown as its address
//! to compare with Phantom or MetaMask before «Save as a wallet», and a phrase's first addresses shown without the signer
//! when the person only wants to see its balances. Neither keeps or sends the secret anywhere. And what making the
//! drafted multisig costs, per network.

use std::collections::HashMap;

use ihatecrypto_core::foundation::Num;
use ihatecrypto_core::intent::Fmt;
use rust_decimal::Decimal;
use serde_json::json;
use trade::EvmNetwork;
use wallet::WalletError;
use wallet::keys::ImportedKey;

use crate::engine::Engine;

impl Engine {
    /// The key's chain and address; an error says what's wrong with what was pasted.
    pub fn key_address(&self, text: &str) -> Result<HashMap<String, String>, WalletError> {
        let key = ImportedKey::parse(text)?;
        Ok(HashMap::from([("chain".into(), key.chain().into()), ("address".into(), key.address()?)]))
    }

    /// A phrase's first three addresses, as MetaMask and Phantom list them: shown, nothing saved.
    pub fn phrase_accounts(&self, text: &str, passphrase: &str) -> Result<HashMap<String, String>, WalletError> {
        let words = text.to_lowercase().split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
        wallet::mnemonic::validate(&words)?;
        let accounts = Self::first_accounts(&words, passphrase, 3)?;
        Ok(HashMap::from([("accounts".into(), serde_json::to_string(&accounts).unwrap_or_else(|_| "[]".into()))]))
    }

    /// What making the drafted multisig costs on each network it names (the Mac's multisigFees): the Safe's deploy gas
    /// at the network's price now, in dollars where Relay prices its coin.
    pub async fn multisig_fees(&self) -> HashMap<String, String> {
        let Some(d) = self.st().multisig_draft.clone() else { return HashMap::new() };
        let mut out = HashMap::new();
        for key in &d.networks {
            let Some(net) = EvmNetwork::named(key) else { continue };
            let Ok(price) = net.rpc.quantity("eth_gasPrice", json!([])).await else { continue };
            if net.fee_token.is_some() { out.insert(key.clone(), self.say("paid in USDC", "в USDC")); continue }
            let gas = Decimal::from(260_000 + 25_000 * d.keys.len() as i64);
            let Some(coins) = (Num::of(gas) * Decimal::from(price) / Decimal::from(1_000_000_000_000_000_000u64)).0 else { continue };
            let usd = protocols::relay::native_price(net.rpc.chain_id).await.ok().flatten().and_then(|p| (Num::of(p) * coins).0);
            out.insert(key.clone(), usd.map_or_else(|| format!("≈ {} {}", Fmt::num(coins, 6), net.gas_coin),
                                                     |u| if u < Decimal::new(1, 2) { "< $0.01".into() } else { format!("≈ {}", Fmt::usd(u)) }));
        }
        out
    }
}

#[cfg(test)]
#[path = "imports/tests.rs"]
mod tests;
