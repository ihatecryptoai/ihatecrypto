//! Safe multisigs (Multisig.swift): making one, and paying out of one with its owners' signatures.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use evm::abi::{u256_in, uint_in};
use evm::call::Call;
use evm::protocols::safe;
use evm::{U256, erc20, word_value};
use execution::trade_signer::TradeSigner;
use serde_json::json;
use wallet::evm::{EvmSignature, recover};

use crate::{EvmNetwork, Executor, Result, fail};

/// The code at `address` on `net`, «0x» for none.
async fn code(address: &str, net: &EvmNetwork) -> Result<String> {
    Ok(net.rpc.call("eth_getCode", json!([address, "latest"])).await?.as_str().unwrap_or("0x").to_string())
}

impl Executor {
    /// Makes the multisig on `net`: one call from this wallet to Safe's factory, the fee paid here. Its address is known
    /// before (safe::address); one already there is left as it is. Returns that address.
    pub async fn create_multisig(&self, net: &EvmNetwork, owners: &[String], threshold: usize, salt: u64) -> Result<String> {
        let address = safe::address(owners, threshold, salt)?;
        if Self::deployed(&address, net).await? {
            self.report(&format!("The multisig {address} is on {} already.", net.name));
            return Ok(address);
        }
        self.send_evm(&net.rpc, safe::FACTORY, &safe::create(owners, threshold, salt)?, &format!("Create the multisig on {}", net.name), &[], None, 0).await?;
        // The receipt can come from a node a few seconds ahead of the one asked next (Base's public nodes, 2026-10-09).
        for _ in 0..20 {
            if Self::deployed(&address, net).await? { return Ok(address) }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        fail(format!("The multisig isn't at {address} on {} 20 s after its transaction went through: check {}/address/{address}.", net.name, net.explorer))
    }

    /// Whether a contract is at `address` on `net`.
    pub async fn deployed(address: &str, net: &EvmNetwork) -> Result<bool> { Ok(code(address, net).await?.chars().count() > 2) }

    /// Whether `address` is a contract rather than a key a person signs with. A wallet delegated by EIP-7702 (its code is
    /// 0xef0100 ‖ the delegate: MetaMask's smart account, Batch7702) still signs with its own key, so it counts as a
    /// person's.
    pub async fn is_contract(address: &str, net: &EvmNetwork) -> Result<bool> {
        let code = code(address, net).await?.to_lowercase();
        let n = code.chars().count();
        Ok(n > 2 && !(code.starts_with("0xef0100") && n == 2 + 46))
    }

    /// A payment out of the multisig `safe` on `net` — `value` wei of the coin to `to`, or a token's transfer as `data`
    /// to the token `to`. Each of `cosigners` that holds one of its keys signs the move (Windows Hello, a Ledger's
    /// buttons) until enough have; then this wallet sends it with their signatures and pays its gas: the multisig needs
    /// none of the coin.
    #[allow(clippy::too_many_arguments)]
    pub async fn multisig_pay(&self, net: &EvmNetwork, safe: &str, to: &str, value: u64, data: &[u8], cosigners: &[Arc<dyn TradeSigner>],
                              recipients: &[String], note: &str) -> Result<String> {
        let rpc = &net.rpc;
        let answering = async {
            let Some(nonce) = uint_in(&rpc.read(safe, &safe::nonce_call()).await?, 0) else { return Ok(None) };
            let Some(threshold) = uint_in(&rpc.read(safe, &safe::threshold_call()).await?, 0) else { return Ok(None) };
            Ok::<_, crate::Error>(safe::owners(&rpc.read(safe, &safe::owners_call()).await?).map(|owners| (nonce, threshold, owners)))
        };
        let Some((nonce, threshold, owners)) = answering.await? else {
            return fail(format!("{safe} doesn't answer as a multisig on {}. Nothing was signed.", net.name));
        };
        // What it pays must be there before anyone signs.
        if let Call::Transfer { amount, .. } = Call::decode(data) {
            let held = u256_in(&rpc.read(to, &erc20::balance_of(safe)?).await?, 0).unwrap_or(U256::ZERO);
            if held < word_value(&amount) {
                return fail(format!("The multisig holds less of that token on {} than this move pays. Nothing was signed.", net.name));
            }
        } else if u128::from(value) > rpc.balance(safe).await? {
            return fail(format!("The multisig holds less {} on {} than this move pays. Nothing was signed.", net.gas_coin, net.name));
        }
        let step = safe::Move { chain_id: rpc.chain_id, safe: safe.into(), to: to.into(), value, data: data.to_vec(), nonce };
        let hash = step.hash()?;
        let keys: HashSet<String> = owners.iter().map(|o| o.to_lowercase()).collect();
        let mut signatures: Vec<(String, EvmSignature)> = Vec::new();
        for cosigner in cosigners {
            if signatures.len() as u64 >= threshold { break }
            let signature = cosigner.sign_safe_move(step.clone(), recipients.to_vec()).await?;
            let owner = recover(&hash, &signature)?.to_lowercase();
            if !keys.contains(&owner) { return fail(format!("{owner} isn't one of the multisig's keys. Nothing was sent.")) }
            if !signatures.iter().any(|(o, _)| *o == owner) { signatures.push((owner, signature)) }
        }
        if (signatures.len() as u64) < threshold {
            return fail(format!("The multisig needs {threshold} signatures and only {} of its keys are here. Nothing was sent.", signatures.len()));
        }
        self.report(&format!("Signed by {} of its keys, as many as it needs: sending the move.", signatures.len()));
        self.send_evm(rpc, safe, &step.exec(&signatures)?, note, recipients, None, 0).await
    }
}
