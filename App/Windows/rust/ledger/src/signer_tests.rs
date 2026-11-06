//! LedgerSigner against a software Ledger: a transport that answers the APDUs a device does — its apps, its addresses,
//! its signatures with keys of its own — so the whole path runs: the guard, the device's commands, the check that the
//! signature is this Ledger's.

use std::sync::Mutex;
use std::time::Duration;

use ed25519_dalek::Signer as _;
use execution::trade_signer::{EvmFees, TradeError, TradeSigner, TradeWallet};
use wallet::evm::{keccak, recover, sign};
use wallet::keys::{base58, ed25519_public, evm_address, secp256k1_public};

use crate::signer::LedgerSigner;
use crate::{Device, Exchanged, Transport};

const EVM_KEY: [u8; 32] = [0x46; 32];
const SOL_KEY: [u8; 32] = [7; 32];

struct SoftLedger {
    evm: [u8; 32],
    sol: [u8; 32],
    state: Mutex<(String, Vec<u8>)>,
    /// A status word to answer the next signing command with instead of signing (0x6985: declined).
    refuse: Option<u16>,
}

impl SoftLedger {
    fn new(evm: [u8; 32]) -> Self { SoftLedger { evm, sol: SOL_KEY, state: Mutex::new(("BOLOS".into(), vec![])), refuse: None } }

    fn answer(&self, apdu: &[u8]) -> Vec<u8> {
        let ok = |d: &[u8]| [d, &[0x90, 0x00]].concat();
        let (ins, p1, p2, data) = ((apdu[0], apdu[1]), apdu[2], apdu[3], &apdu[5..]);
        let path_len = |d: &[u8]| 1 + 4 * d[0] as usize;
        let mut state = self.state.lock().unwrap();
        match ins {
            (0xb0, 0x01) => ok(&[&[1u8, state.0.len() as u8][..], state.0.as_bytes(), &[5], b"1.0.0"].concat()),
            (0xb0, 0xa7) => { state.0 = "BOLOS".into(); ok(&[]) }
            (0xe0, 0xd8) => { state.0 = String::from_utf8_lossy(data).into(); ok(&[]) }
            (0xe0, 0x02) => {
                let address = evm_address(&self.evm).unwrap()[2..].to_lowercase();
                ok(&[&[65u8][..], &[4], &secp256k1_public(&self.evm).unwrap(), &[40], address.as_bytes()].concat())
            }
            (0xe0, 0x04) => {
                if p1 == 0 { state.1 = data[path_len(data)..].to_vec() } else { state.1.extend(data) }
                if let Some(status) = self.refuse { return status.to_be_bytes().to_vec() }
                let s = sign(&keccak(&state.1), &self.evm).unwrap();
                ok(&[&[(s.v - 27) as u8][..], &s.r, &s.s].concat())
            }
            // An Ethereum app too old for EIP-712 in full: the signer falls back to the two hashes.
            (0xe0, 0x1a) | (0xe0, 0x1c) => vec![0x6d, 0x00],
            (0xe0, 0x0c) => {
                let at = path_len(data);
                let digest = keccak(&[&[0x19, 0x01][..], &data[at..at + 32], &data[at + 32..at + 64]].concat());
                let s = sign(&digest, &self.evm).unwrap();
                ok(&[&[(s.v - 27) as u8][..], &s.r, &s.s].concat())
            }
            (0xe0, 0x05) => ok(&ed25519_public(&self.sol).unwrap()),
            (0xe0, 0x06) => {
                if p2 & 0x01 == 0 { state.1 = data.to_vec() } else { state.1.extend(data) }
                if p2 & 0x02 != 0 { return ok(&[]) }
                if let Some(status) = self.refuse { return status.to_be_bytes().to_vec() }
                let message = state.1[1 + path_len(&state.1[1..])..].to_vec();
                ok(&ed25519_dalek::SigningKey::from_bytes(&self.sol).sign(&message).to_bytes())
            }
            _ => vec![0x6d, 0x00],
        }
    }
}

impl Transport for SoftLedger {
    fn exchange(&self, apdu: Vec<u8>, _timeout: Duration) -> Exchanged<'_> {
        let reply = self.answer(&apdu);
        Box::pin(std::future::ready(Ok(reply)))
    }
}

fn wallet() -> TradeWallet {
    TradeWallet { solana: base58::encode(&ed25519_public(&SOL_KEY).unwrap()), evm: evm_address(&EVM_KEY).unwrap() }
}

fn signer(device: SoftLedger) -> LedgerSigner { LedgerSigner::new(Device::new(std::sync::Arc::new(device)), wallet()) }

fn rt() -> tokio::runtime::Runtime { tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap() }

fn fees() -> EvmFees { EvmFees { chain_id: 8453, nonce: 3, max_priority_fee: 1_000_000, max_fee: 20_000_000, gas_limit: 60_000 } }

#[test]
fn an_evm_transaction_signed_on_the_device_is_the_wallets_own() {
    let to = "0x000000000000000000000000000000000000dEaD".to_string();
    let raw = rt().block_on(signer(SoftLedger::new(EVM_KEY)).sign_evm(fees(), to.clone(), vec![], vec![])).unwrap();
    let tx = evm::Transaction { chain_id: 8453, nonce: 3, max_priority_fee_per_gas: 1_000_000, max_fee_per_gas: 20_000_000, gas_limit: 60_000,
                                to, value: 0, data: vec![], authorizations: vec![] };
    assert_eq!(raw, tx.signed(&EVM_KEY).unwrap().0); // the same bytes as signed here: RFC 6979 on both sides
}

#[test]
fn a_ledger_with_another_key_is_refused() {
    let e = rt().block_on(signer(SoftLedger::new([0x47; 32])).sign_evm(fees(), "0x000000000000000000000000000000000000dead".into(), vec![], vec![])).unwrap_err();
    assert_eq!(e, TradeError::Device { status: None, message: format!("The Ledger's signature isn't from {}: is it the Ledger you connected?", wallet().evm) });
}

#[test]
fn the_guard_runs_before_the_device() {
    let unlimited = evm::erc20::approve("0x0000000000000000000000000000000000000009", evm::U256::MAX).unwrap();
    let e = rt().block_on(signer(SoftLedger::new(EVM_KEY)).sign_evm(fees(), evm::BASE_USDC.into(), unlimited, vec![])).unwrap_err();
    assert!(matches!(&e, TradeError::Failed(m) if m.starts_with("Unlimited token spending")), "{e:?}");
    let e = rt().block_on(signer(SoftLedger::new(EVM_KEY)).sign_evm(EvmFees { chain_id: 31337, ..fees() }, "0x0000000000000000000000000000000000000009".into(), vec![], vec![])).unwrap_err();
    assert_eq!(e, TradeError::Failed("ihatecrypto doesn't sign for network 31337.".into()));
}

#[test]
fn a_hyperliquid_order_falls_back_to_the_two_hashes_and_recovers_to_the_ledger() {
    let s = signer(SoftLedger::new(EVM_KEY));
    let act = hyperliquid::action::cancel(&[(0, 1)]);
    let sig = rt().block_on(s.sign_hyperliquid(act.clone(), 1_700_000_000_000)).unwrap();
    let digest = hyperliquid::signer::l1_parts(&act, None, 1_700_000_000_000, None, true).unwrap().digest();
    assert_eq!(recover(&digest, &sig).unwrap(), wallet().evm);
    // A TWAP without the trading key: refused, the device never asked.
    let twap = hyperliquid::action::twap_order(0, true, hyperliquid::dec("1"), false, 30, false).unwrap();
    assert!(rt().block_on(s.sign_hyperliquid(twap, 1)).unwrap_err().to_string().starts_with("A TWAP needs the Ledger's trading key on"));
    // With the trading key on, orders go there and not to the device.
    let mut with_agent = signer(SoftLedger { refuse: Some(0x6985), ..SoftLedger::new(EVM_KEY) });
    with_agent.agent = Some(std::sync::Arc::new(|_, _| Box::pin(async { Ok(wallet::evm::EvmSignature { r: vec![1; 32], s: vec![2; 32], v: 27 }) })));
    assert_eq!(rt().block_on(with_agent.sign_hyperliquid(hyperliquid::action::cancel(&[(0, 1)]), 1)).unwrap().r, vec![1; 32]);
}

#[test]
fn a_solana_transfer_signed_on_the_device_and_a_decline_on_its_buttons() {
    let to = base58::encode(&[9; 32]);
    let blockhash = base58::encode(&[1; 32]);
    let (_, wire) = solana::transfer(&SOL_KEY, &to, 1_000, &blockhash, 20_000).unwrap();
    let mut blank = wire.clone();
    blank[1..65].fill(0);
    let accounts = solana::message::Message::parse_transaction(&wire).unwrap().static_keys;
    let prompts = std::sync::Arc::new(Mutex::new(Vec::new()));
    let p = prompts.clone();
    let mut s = signer(SoftLedger::new(EVM_KEY));
    s.prompt = std::sync::Arc::new(move |m| p.lock().unwrap().push(m));
    let signed = rt().block_on(s.sign_solana(blank.clone(), accounts.clone(), vec![to.clone()], Default::default())).unwrap();
    assert_eq!(signed, wire);
    let said = prompts.lock().unwrap();
    assert_eq!(said[0].as_deref(), Some("Open the Solana app on your Ledger (allow it on the device if it asks)."));
    assert!(said[1].as_deref().unwrap().starts_with("Review on your Ledger and approve:"));
    assert_eq!(said.last().unwrap(), &None);
    // Money to an address nobody approved: the guard, never the device.
    assert!(matches!(rt().block_on(s.sign_solana(blank.clone(), accounts.clone(), vec![], Default::default())), Err(TradeError::Failed(_))));
    // Turned down on the device's buttons: a device error the move says plainly.
    let declined = rt().block_on(signer(SoftLedger { refuse: Some(0x6985), ..SoftLedger::new(EVM_KEY) })
        .sign_solana(blank, accounts, vec![to], Default::default())).unwrap_err();
    assert!(declined.declined() && declined.to_string() == "You declined it on the Ledger. Nothing was signed.");
}
