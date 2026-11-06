//! Cases from EVMTests.swift and SafeTests.swift with their vectors.

use super::*;
use crate::call::Call;
use crate::protocols::{batch7702, safe, v4};
use wallet::evm::recover;

const DEAD: &str = "0x000000000000000000000000000000000000dead";

#[test]
fn signed_transaction_is_type2_and_signed_by_the_key() {
    let key = [0x46; 32];
    let tx = Transaction { chain_id: 42161, nonce: 0, max_priority_fee_per_gas: 0, max_fee_per_gas: 20_000_000, gas_limit: 21_000,
                           to: "0x3535353535353535353535353535353535353535".into(), value: 1_000_000_000_000, data: vec![], authorizations: vec![] };
    let (raw, hash) = tx.signed(&key).unwrap();
    assert_eq!(raw[0], 2);
    assert_eq!(hash.len(), 66);
    assert_eq!(tx.fee_text(), "up to 0.00000042 ETH"); // 21,000 × 0.02 gwei
    assert_eq!(tx.fee_problem(), None);
    let digest = tx.signing_hash().unwrap();
    let (raw2, _) = tx.signed_with(&wallet::evm::sign(&digest, &key).unwrap()).unwrap();
    assert_eq!(raw, raw2);
    assert_eq!(recover(&digest, &wallet::evm::sign(&digest, &key).unwrap()).unwrap(), wallet::keys::evm_address(&key).unwrap());
    let greedy = Transaction { gas_limit: 10_000_000, max_fee_per_gas: 10_000_000_000, ..tx };
    assert_eq!(greedy.fee_problem().unwrap(), "A network fee of up to 0.1 ETH is more than ihatecrypto signs on Arbitrum.");
}

#[test]
fn rlp_matches_the_spec() {
    assert_eq!(Rlp::Bytes(b"dog".to_vec()).encoded(), [0x83, b'd', b'o', b'g']);
    assert_eq!(Rlp::uint(0).encoded(), [0x80]);
    assert_eq!(Rlp::uint(15).encoded(), [0x0f]);
    assert_eq!(Rlp::uint(1024).encoded(), [0x82, 0x04, 0x00]);
    assert_eq!(Rlp::List(vec![]).encoded(), [0xc0]);
    let long = Rlp::Bytes(vec![b'a'; 56]).encoded();
    assert_eq!(long[..2], [0xb8, 56]);
}

#[test]
fn the_prompt_says_what_is_signed() {
    let (payee, relay) = ("0x060b0fb0be9d90557577b3aee480711067149ff0", "0x4cd00e387622c35bddb9b4c962c136462338bc31");
    let pay = Call::decode(&erc20::transfer(payee, U256::from(185_357)).unwrap());
    assert_eq!(pay.summary(4217, TEMPO_USDC, 0), "send 0.185357 USDC to 0x060b…9ff0 on Tempo");
    assert_eq!(Call::decode(&erc20::approve(relay, U256::from(2_000_000)).unwrap()).summary(42161, ARBITRUM_USDC, 0),
               "let 0x4cd0…bc31 spend exactly 2 USDC on Arbitrum");
    assert_eq!(Call::decode(&tempo::set_user_token(TEMPO_USDC).unwrap()).summary(4217, tempo::FEE_MANAGER, 0), "pay Tempo's fees in USDC from now on");
    assert_eq!(Call::decode(&[]).summary(8453, payee, 1_000_000_000_000_000), "send 0.001 ETH to 0x060b…9ff0 on Base");
    assert_eq!(pay.summary(4217, payee, 0), "send 185357 units of the token 0x060b…9ff0 to 0x060b…9ff0 on Tempo");
    assert_eq!(Call::decode(&erc20::approve(relay, U256::from(7)).unwrap()).summary(8453, payee, 0),
               "let 0x4cd0…bc31 spend exactly 7 units of the token 0x060b…9ff0 on Base");
    assert_eq!(Call::decode(&[0xde, 0xad, 0xbe, 0xef]).summary(1, payee, 2_000_000_000_000_000_000), "call the contract 0x060b…9ff0 on Ethereum, paying 2 ETH");
}

#[test]
fn the_signer_names_every_payee_of_a_call() {
    let stranger = format!("0x{}", "5c".repeat(20));
    let odd = Call::decode(&[0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(odd.payees(&stranger, 1, 1), [stranger.clone()]);
    assert!(odd.payees(&stranger, 0, 1).is_empty());
    assert_eq!(Call::Plain.payees(&stranger, 5, 8453), [stranger.clone()]);
    assert_eq!(Call::decode(&erc20::transfer(&stranger, U256::from(1)).unwrap()).payees(BASE_USDC, 0, 8453), [stranger.clone()]);
    // An approve to a spender ihatecrypto uses itself needs no one approved; to a stranger it does.
    let uni = protocols::uniswap::v3(8453).unwrap().1;
    assert!(Call::decode(&erc20::approve(uni, U256::from(5)).unwrap()).payees(BASE_USDC, 0, 8453).is_empty());
    assert_eq!(Call::decode(&erc20::approve(&stranger, U256::from(5)).unwrap()).payees(BASE_USDC, 0, 8453), [stranger.clone()]);
    assert!(Call::decode(&erc20::approve(&stranger, U256::MAX).unwrap()).refusal(&stranger, Some(8453)).unwrap().starts_with("Unlimited token spending"));
}

#[test]
fn safe_address_and_move_hash_match_the_contracts() {
    let owners: Vec<String> = ["11", "22", "33"].iter().map(|b| format!("0x{}", b.repeat(20))).collect();
    assert_eq!(safe::setup(&owners, 2).unwrap().len(), 420);
    assert_eq!(safe::address(&owners, 2, 42).unwrap(), "0xdbcf0e3023809d9436b468e63fd86cc61b4dddaf");
    let m = safe::Move { chain_id: 8453, safe: safe::SINGLETON.into(), to: BASE_USDC.into(), value: 0,
                         data: erc20::transfer(DEAD, U256::from(1_000_000)).unwrap(), nonce: 7 };
    assert_eq!(wallet::to_hex(&m.hash().unwrap()), "a2f0968324819faabd2ea549d929ed4b2d0606130f6368938548963ddacecca3");
    assert_eq!(m.summary(), "send 1 USDC to 0x0000…dead on Base from the multisig 0x29fc…c762");
    // execTransaction read back is the same move, and a payout passes the self-sent rules.
    let sig = wallet::evm::sign(&m.hash().unwrap(), &[0x46; 32]).unwrap();
    let exec = m.exec(&[(wallet::keys::evm_address(&[0x46; 32]).unwrap(), sig)]).unwrap();
    let (to, value, data, op) = safe::read_exec(&exec).unwrap();
    assert_eq!((to.as_str(), value, data, op), (BASE_USDC, vec![0; 32], m.data.clone(), 0));
    assert_eq!(safe::exec_refusal(&exec, &m.safe), None);
    assert!(matches!(Call::decode(&exec), Call::MultisigExec { .. }));
    assert_eq!(safe::owners_problem(&[], 1).unwrap(), "A multisig needs at least one key.");
    assert_eq!(safe::owners_problem(&owners, 4).unwrap(), "A multisig needs 1 to 3 signatures, not 4.");
}

#[test]
fn v4_swap_reads_back_and_its_rules_hold() {
    let key = v4::PoolKey::new(v4::NATIVE, BASE_USDC, 500, 10, v4::NATIVE);
    let data = v4::execute(&key, true, U256::from(1000), U256::from(990), 99).unwrap();
    let Call::V4Swap(s) = Call::decode(&data) else { panic!("not read as a v4 swap") };
    assert_eq!((s.amount_in, s.amount_out_minimum, s.deadline, s.settle_max, s.take_min), (U256::from(1000), U256::from(990), 99, U256::from(1000), U256::from(990)));
    assert_eq!(v4::refusal(&s, Some(8453)), None);
    let router = v4::on(8453).unwrap().router;
    // Coins in that come straight back as the swap's input aren't a payment to the router.
    assert!(Call::V4Swap(s.clone()).payees(router, 1000, 8453).is_empty());
    assert_eq!(Call::V4Swap(s).payees(router, 1001, 8453), [router.to_string()]);
    let hooked = v4::PoolKey::new(v4::NATIVE, BASE_USDC, 500, -10, "0x0000000000000000000000000000000000001000");
    let Call::V4Swap(h) = Call::decode(&v4::execute(&hooked, false, U256::from(5), U256::from(1), 1).unwrap()) else { panic!() };
    assert_eq!(h.key.tick_spacing, -10);
    assert!(v4::refusal(&h, Some(8453)).unwrap().contains("hooks contract"));
}

#[test]
fn batches_read_back() {
    let calls = vec![batch7702::BatchCall { to: BASE_USDC.into(), data: erc20::approve(protocols::uniswap::v3(8453).unwrap().1, U256::from(5)).unwrap() },
                     batch7702::BatchCall { to: DEAD.into(), data: vec![1, 2, 3] }];
    assert_eq!(batch7702::calls(&batch7702::execute_batch(&calls).unwrap()).unwrap(), calls);
    assert!(batch7702::problems(&calls, DEAD, 8453).iter().any(|p| p.contains("own address")));
    assert!(batch7702::problems(&calls, "0x0000000000000000000000000000000000000001", 999).iter().any(|p| p.contains("network 999")));
    assert_eq!(batch7702::foreign_delegate("0xef010063c0c19a282a1b52b07dd5a65b58948a07dae32b").unwrap(), "0x63c0c19a282a1b52b07dd5a65b58948a07dae32b");
    assert_eq!(batch7702::foreign_delegate(&format!("0xef0100{}", &batch7702::DELEGATE[2..])), None);
    let (raw, _) = batch7702::signed(8453, 3, 1, 2, 100_000, &wallet::keys::evm_address(&[7; 32]).unwrap(), &calls[..1], &[7; 32]).unwrap();
    assert_eq!(raw[0], 4);
}

#[test]
fn nodes_and_gas_words() {
    assert!(not_this_node("Method not found"));
    assert!(!not_this_node("execution reverted: unauthorized"));
    assert_eq!(short_of_gas("insufficient funds for gas * price + value: address 0x1 have 120000000000000 want 340000000000000", 8453).unwrap(),
               "Not enough ETH for gas: the wallet has 0.00012 ETH and needs 0.00034 ETH with the fee. Send it a little ETH, then try again.");
    assert_eq!(short_of_gas("EVM error: OutOfFunds", 4217).unwrap(), "Not enough USD for gas: the wallet can't pay this network's fee. Send it a little USD, then try again.");
    assert_eq!(short_of_gas("execution reverted: insufficient funds", 1), None);
    assert_eq!(units(&U256::from(1_500_000u64), 6), "1.5");
    assert_eq!(units(&U256::from(5u64), 18), "0.000000000000000005");
    assert_eq!(units(&U256::ZERO, 6), "0");
}

/// The rest of main's WalletTests, case by case.
mod calls;
mod orders;
