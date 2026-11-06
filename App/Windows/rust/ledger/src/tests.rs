//! LedgerTests.swift's cases, and the apps' commands against a scripted device.

use std::collections::VecDeque;
use std::sync::Mutex;

use super::*;
use wallet::evm::eip712::Field;
use wallet::evm::{recover, sign};
use wallet::keys::evm_address;

#[test]
fn hid_packets_carry_the_length_and_reassemble() {
    let apdu: Vec<u8> = (0..200u32).map(|i| (i & 0xff) as u8).collect();
    let packets = framing::packets(&apdu, framing::HID_PACKET_SIZE, true);
    // 2 length bytes + 200 = 202 bytes in 59-byte slices.
    assert_eq!(packets.len(), 4);
    assert!(packets.iter().all(|p| p.len() == 64));
    assert_eq!(packets[0][..7], [0x01, 0x01, 0x05, 0x00, 0x00, 0x00, 200]);
    assert_eq!(packets[3][..5], [0x01, 0x01, 0x05, 0x00, 0x03]);
    // The device answers in the same framing; the reply is complete only with its last packet.
    assert_eq!(framing::reply(&packets[..3], true).unwrap(), None);
    assert_eq!(framing::reply(&packets, true).unwrap(), Some(apdu));
    assert!(framing::reply(&packets[1..2], true).is_err());
}

#[test]
fn ble_packets_fit_the_link_and_reassemble() {
    let apdu: Vec<u8> = (0..200u32).map(|i| (i & 0xff) as u8).collect();
    // MTU 20: 5 header bytes and 15 of data first, then 3 and 17 each.
    let packets = framing::packets(&apdu, 20, false);
    assert_eq!(packets[0][..5], [0x05, 0x00, 0x00, 0x00, 200]);
    assert_eq!(packets[1][..3], [0x05, 0x00, 0x01]);
    assert!(packets.iter().all(|p| p.len() <= 20));
    assert_eq!(packets.len(), 1 + ((200 - 15) + 16) / 17);
    assert_eq!(framing::reply(&packets[..packets.len() - 1], false).unwrap(), None);
    assert_eq!(framing::reply(&packets, false).unwrap(), Some(apdu.clone()));
    // The same APDU over USB is framed differently: HID packets are not BLE packets.
    assert!(framing::reply(&framing::packets(&apdu, 64, true), false).is_err());
}

#[test]
fn typed_data_parts_give_the_same_digest() {
    let key = from_hex("0123456789012345678901234567890123456789012345678901234567890123").unwrap();
    let action = hyperliquid::action::cancel(&[(0, 1)]);
    let parts = hyperliquid::signer::l1_parts(&action, None, 1_700_000_000_000, None, true).unwrap();
    let signature = hyperliquid::signer::sign_l1(&action, &key, None, 1_700_000_000_000, None, true).unwrap();
    // What a Ledger signs from the two hashes recovers to the same key the app signs with.
    assert_eq!(recover(&parts.digest(), &signature).unwrap(), evm_address(&key).unwrap());
}

#[test]
fn a_signature_from_elsewhere_makes_the_same_transaction() {
    let key = from_hex("0123456789012345678901234567890123456789012345678901234567890123").unwrap();
    let tx = evm::Transaction { chain_id: 8453, nonce: 3, max_priority_fee_per_gas: 1_000_000, max_fee_per_gas: 20_000_000, gas_limit: 60_000,
                                to: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".into(), value: 0,
                                data: evm::erc20::approve("0x0000000000000000000000000000000000000001", evm::U256::from(5)).unwrap(), authorizations: vec![] };
    let signature = sign(&tx.signing_hash().unwrap(), &key).unwrap();
    assert_eq!(tx.signed_with(&signature).unwrap().0, tx.signed(&key).unwrap().0);
    // The other parity recovers someone else: the Ledger signer tries both and keeps the one that is ours.
    let flipped = EvmSignature { v: if signature.v == 27 { 28 } else { 27 }, ..signature };
    assert_ne!(recover(&tx.signing_hash().unwrap(), &flipped).unwrap(), evm_address(&key).unwrap());
}

/// A device that answers from a script and keeps what it was sent.
struct Scripted {
    replies: Mutex<VecDeque<Vec<u8>>>,
    sent: Mutex<Vec<Vec<u8>>>,
}

impl Scripted {
    fn new(replies: Vec<Vec<u8>>) -> Arc<Self> { Arc::new(Scripted { replies: Mutex::new(replies.into()), sent: Mutex::new(vec![]) }) }
}

impl Transport for Scripted {
    fn exchange(&self, apdu: Vec<u8>, _timeout: Duration) -> Exchanged<'_> {
        self.sent.lock().unwrap().push(apdu);
        let reply = self.replies.lock().unwrap().pop_front().unwrap_or_default();
        Box::pin(std::future::ready(Ok(reply)))
    }
}

fn ok(data: &[u8]) -> Vec<u8> { [data, &[0x90, 0x00]].concat() }

fn rt() -> tokio::runtime::Runtime { tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap() }

#[test]
fn the_ethereum_app_gives_an_address_and_says_its_errors_plainly() {
    let address = b"f39fd6e51aad88f6f4ce6ab8827279cfffb92266";
    let reply = [&[65u8][..], &[4; 65], &[40], address, &[0; 32]].concat();
    let script = Scripted::new(vec![ok(&reply), vec![0x69, 0x85], vec![0x6a, 0x80], vec![0x6d, 0x00]]);
    let device = Device::new(script.clone());
    let rt = rt();
    assert_eq!(rt.block_on(device.evm_address(EVM_PATH, false)).unwrap(), "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
    assert_eq!(script.sent.lock().unwrap()[0], [&[0xe0, 0x02, 0, 0, 21, 5][..], &[0x80, 0, 0, 44], &[0x80, 0, 0, 60], &[0x80, 0, 0, 0], &[0, 0, 0, 0], &[0, 0, 0, 0]].concat());
    let declined = rt.block_on(device.evm_address(EVM_PATH, true)).unwrap_err();
    assert!(declined.declined() && declined.message == "You declined it on the Ledger. Nothing was signed.");
    assert!(rt.block_on(device.evm_blind_signing()).unwrap_err().message.contains("«Blind signing»"));
    let wrong = rt.block_on(device.solana_address(SOLANA_PATH, false)).unwrap_err();
    assert!(wrong.wrong_app() && wrong.message == "Open the Solana app on your Ledger.");
    assert_eq!(LedgerError::from_status(0x1234, "Solana").message, "The Ledger answered 0x1234 in the Solana app.");
}

#[test]
fn a_long_solana_message_goes_in_marked_pieces() {
    let message = vec![7u8; 600];
    let script = Scripted::new(vec![ok(&[]), ok(&[]), ok(&[9; 64])]);
    let device = Device::new(script.clone());
    assert_eq!(rt().block_on(device.sign_solana(&message, SOLANA_PATH)).unwrap(), vec![9; 64]);
    let sent = script.sent.lock().unwrap();
    // 1 signer + 13-byte path + 600 = 614 bytes: 255, 255, 104.
    assert_eq!(sent.iter().map(|a| (a[2], a[3], a[4] as usize)).collect::<Vec<_>>(), [(1, 0x02, 255), (1, 0x03, 255), (1, 0x01, 104)]);
    assert_eq!(sent[0][5..7], [1, 3]); // one signer, a path of three steps
}

#[test]
fn a_transaction_goes_in_150_byte_pieces_the_first_with_the_path() {
    let payload = vec![2u8; 300];
    let mut signature = vec![1u8];
    signature.extend([0xaa; 32]);
    signature.extend([0xbb; 32]);
    let script = Scripted::new(vec![ok(&[]), ok(&[]), ok(&signature)]);
    let device = Device::new(script.clone());
    let (v, r, s) = rt().block_on(device.sign_evm_transaction(&payload, EVM_PATH)).unwrap();
    assert_eq!((v, r, s), (1, vec![0xaa; 32], vec![0xbb; 32]));
    let sent = script.sent.lock().unwrap();
    assert_eq!(sent.iter().map(|a| (a[2], a[4] as usize)).collect::<Vec<_>>(), [(0, 150), (0x80, 150), (0x80, 21)]);
}

#[test]
fn typed_data_is_sent_field_by_field_and_signed() {
    let typed = Typed {
        domain: vec![Field::new("name", "string", Value::String("Exchange".into())), Field::new("chainId", "uint256", Value::Uint(1337))],
        ty: "Agent".into(),
        message: vec![Field::new("source", "string", Value::String("a".into())), Field::new("connectionId", "bytes32", Value::Bytes32(vec![3; 32]))],
    };
    // 2 struct names + 4 fields, then 2 struct names + 4 values, then the signature.
    let mut replies = vec![ok(&[]); 12];
    replies.push(ok(&[[0u8].as_slice(), &[1; 32], &[2; 32]].concat()));
    let script = Scripted::new(replies);
    let s = rt().block_on(Device::new(script.clone()).sign_typed_data(&typed, EVM_PATH)).unwrap();
    assert_eq!((s.v, s.r.clone(), s.s.clone()), (27, vec![1; 32], vec![2; 32]));
    let sent = script.sent.lock().unwrap();
    assert_eq!(sent[1][5..], [&[0x05u8, 4][..], b"name"].concat()); // string, then its name
    assert_eq!(sent[2][5..7], [0x42, 32]); // uint256: 0x40 | 2, 32 bytes
    assert_eq!(sent[8][5..], [0, 2, 0x05, 0x39]); // 1337 in as few bytes as it needs, after its length
    assert_eq!(field_type("int64").unwrap(), [0x41, 8]);
    assert_eq!(field_type("bytes4").unwrap(), [0x46, 4]);
    assert!(field_type("Person").is_err());
    assert_eq!(field_value(&Value::Uint(0)).unwrap(), [0]);
}

#[test]
fn opening_an_app_quits_the_one_open_first() {
    let app = |name: &str| ok(&[&[1u8, name.len() as u8][..], name.as_bytes(), &[5], b"1.2.3"].concat());
    let script = Scripted::new(vec![app("Solana"), ok(&[]), app("BOLOS"), ok(&[]), app("Ethereum")]);
    let device = Device::new(script.clone());
    rt().block_on(device.open("Ethereum")).unwrap();
    let sent = script.sent.lock().unwrap();
    assert_eq!(sent.iter().map(|a| (a[0], a[1])).collect::<Vec<_>>(), [(0xb0, 0x01), (0xb0, 0xa7), (0xb0, 0x01), (0xe0, 0xd8), (0xb0, 0x01)]);
    assert_eq!(&sent[3][5..], b"Ethereum");
}

#[test]
fn descriptors_are_put_together_as_the_device_reads_them() {
    let entry = serde_json::json!([{"ticker": "USDC", "descriptor": {"data": "0a0b", "signatures": {"prod": "3045"}}}]);
    assert_eq!(descriptors::erc20_payload(&entry, 8453, "0xabc").unwrap(), [4, 0x0a, 0x0b, 0x30, 0x45]);
    assert_eq!(descriptors::erc20_payload(&serde_json::json!([]), 8453, "0xabc").unwrap_err().message, "Ledger doesn't list token 0xabc on chain 8453.");
    assert_eq!(descriptors::certificate_payload(&entry).unwrap(), [0x0a, 0x0b, 0x15, 2, 0x30, 0x45]);
    assert!(descriptors::signed(Some(&serde_json::json!({"descriptor": {"data": "0a"}}))).is_err());
    assert_eq!((descriptors::target(Some("Flex")), descriptors::target(None)), ("flex", "nanosp"));
}
