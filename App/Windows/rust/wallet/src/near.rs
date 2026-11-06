//! A NEAR transaction in borsh (nearcore's Transaction V0), built from plain fields so that what is signed is what
//! was checked (`NearTransaction.swift`). The hash that is signed is sha256 of it.

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use crate::{Result, fail, to_hex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    FunctionCall { method: String, args: Vec<u8>, gas: u64, deposit: u128 },
    Transfer { deposit: u128 },
    /// Deletes the signer's own account; NEAR sends all that is left on it to `beneficiary` (which must exist, or
    /// the balance is burned).
    DeleteAccount { beneficiary: String },
}

pub const WRAP_CONTRACT: &str = "wrap.near";
/// wrap.near's storage_balance_bounds (min = max = 0.00125 NEAR), read 2026-09-28.
pub const WRAP_STORAGE: u128 = 1_250_000_000_000_000_000_000;
const TGAS: u64 = 1_000_000_000_000;

/// Native NEAR into a 1Click deposit: ONE wrap.near transaction — storage_deposit for us and for the deposit address
/// (wrap.near refunds it when already registered; an unregistered receiver makes ft_transfer revert), near_deposit of
/// exactly `amount`, ft_transfer of that wNEAR to the deposit address. Actions in one receipt are atomic.
pub fn intent_deposit(account: &str, deposit_address: &str, amount: u128) -> Vec<Action> {
    // serde_json's map keeps keys sorted, as Swift's `.sortedKeys`.
    let register = |id: &str| serde_json::json!({"account_id": id, "registration_only": true}).to_string().into_bytes();
    let call = |method: &str, args: Vec<u8>, gas: u64, deposit: u128| Action::FunctionCall { method: method.into(), args, gas, deposit };
    vec![
        call("storage_deposit", register(account), 15 * TGAS, WRAP_STORAGE),
        call("storage_deposit", register(deposit_address), 15 * TGAS, WRAP_STORAGE),
        call("near_deposit", b"{}".to_vec(), 15 * TGAS, amount),
        call("ft_transfer", serde_json::json!({"receiver_id": deposit_address, "amount": amount.to_string()}).to_string().into_bytes(), 30 * TGAS, 1),
    ]
}

/// Besides the amount, what `intent_deposit` needs on the account when it is sent: the two storage deposits and the
/// 75 Tgas of prepaid gas at NEAR's pessimistic price — measured 10.44× the block's gas price (2026-09-28), so 11×.
/// ponytail: a measured multiplier; if NEAR changes its inflation rule, read the cost from a failed send instead.
pub fn intent_deposit_overhead(gas_price: u128) -> u128 { 2 * WRAP_STORAGE + 1 + (75 * TGAS) as u128 * gas_price * 11 }

pub fn serialize(signer: &str, public_key: &[u8], nonce: u64, receiver: &str, block_hash: &[u8], actions: &[Action]) -> Vec<u8> {
    fn string(out: &mut Vec<u8>, s: &str) {
        out.extend((s.len() as u32).to_le_bytes());
        out.extend(s.as_bytes());
    }
    let mut out = Vec::new();
    string(&mut out, signer);
    out.push(0); // ed25519
    out.extend(public_key);
    out.extend(nonce.to_le_bytes());
    string(&mut out, receiver);
    out.extend(block_hash);
    out.extend((actions.len() as u32).to_le_bytes());
    for action in actions {
        match action {
            Action::FunctionCall { method, args, gas, deposit } => {
                out.push(2);
                string(&mut out, method);
                out.extend((args.len() as u32).to_le_bytes());
                out.extend(args);
                out.extend(gas.to_le_bytes());
                out.extend(deposit.to_le_bytes());
            }
            Action::Transfer { deposit } => {
                out.push(3);
                out.extend(deposit.to_le_bytes());
            }
            Action::DeleteAccount { beneficiary } => {
                out.push(7);
                string(&mut out, beneficiary);
            }
        }
    }
    out
}

/// The borsh SignedTransaction ready for `send_tx`, and the transaction hash (base58 of it is the explorer id).
pub fn sign(receiver: &str, nonce: u64, block_hash: &[u8], actions: &[Action], private_key: &[u8]) -> Result<(Vec<u8>, [u8; 32])> {
    let Ok(secret) = <[u8; 32]>::try_from(private_key) else { return fail("An ed25519 private key is 32 bytes.") };
    let key = SigningKey::from_bytes(&secret);
    let public_key = key.verifying_key().to_bytes();
    let mut tx = serialize(&to_hex(&public_key), &public_key, nonce, receiver, block_hash, actions);
    let hash: [u8; 32] = Sha256::digest(&tx).into();
    tx.push(0);
    tx.extend(key.sign(&hash).to_bytes());
    Ok((tx, hash))
}

/// yoctoNEAR as NEAR with up to 6 decimals: «0.1», «0.758442».
pub fn near(yocto: u128) -> String {
    let micro = yocto / 1_000_000_000_000_000_000;
    let fraction = format!("{:06}", micro % 1_000_000);
    let fraction = fraction.trim_end_matches('0');
    format!("{}{}", micro / 1_000_000, if fraction.is_empty() { String::new() } else { format!(".{fraction}") })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    #[test]
    fn amounts() {
        assert_eq!(near(100_000_000_000_000_000_000_000), "0.1");
        assert_eq!(near(758_442_000_000_000_000_000_000), "0.758442");
        assert_eq!(near(2_000_000_000_000_000_000_000_000), "2");
        assert_eq!(near(0), "0");
    }

    #[test]
    fn signed_transaction_layout() {
        let key = [7u8; 32];
        let actions = intent_deposit("alice.near", "dep.near", 5);
        let (signed, hash) = sign(WRAP_CONTRACT, 9, &[1; 32], &actions, &key).unwrap();
        let public = SigningKey::from_bytes(&key).verifying_key();
        let body = serialize(&to_hex(public.as_bytes()), public.as_bytes(), 9, WRAP_CONTRACT, &[1; 32], &actions);
        assert_eq!(&signed[..body.len()], &body[..]);
        assert_eq!(signed[body.len()], 0);
        let sig = Signature::from_slice(&signed[body.len() + 1..]).unwrap();
        assert!(VerifyingKey::from_bytes(public.as_bytes()).unwrap().verify(&hash, &sig).is_ok());
        let Action::FunctionCall { args, .. } = &actions[0] else { panic!() };
        assert_eq!(std::str::from_utf8(args).unwrap(), r#"{"account_id":"alice.near","registration_only":true}"#);
        let Action::FunctionCall { args, .. } = &actions[3] else { panic!() };
        assert_eq!(std::str::from_utf8(args).unwrap(), r#"{"amount":"5","receiver_id":"dep.near"}"#);
    }
}
