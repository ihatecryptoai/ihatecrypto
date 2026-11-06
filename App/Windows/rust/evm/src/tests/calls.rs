//! EVMTests.swift, UniswapRoutesTests.swift, ContractGuardTests.swift and LedgerTests.swift's offline EVM cases, with
//! their vectors. Cases that need the Executor, the app, TypedData (the wallet crate's) or a network are left out.

use std::time::{SystemTime, UNIX_EPOCH};

use rust_decimal::Decimal;
use serde_json::{Value as Json, json};
use wallet::evm::{keccak, recover, sign, EvmSignature};
use wallet::{from_hex, to_hex};

use crate::abi::{self, address_in, u256_in, uint_in};
use crate::call::{Call, ADDRESS_THIS, MSG_SENDER};
use crate::protocols::{aave, batch7702, kyber, merkl, morpho, relay, safe, uniswap, v4};
use crate::simulation::{Grant, Simulation, NATIVE};
use crate::*;

pub(super) fn h(s: &str) -> Vec<u8> { from_hex(s).expect("hex") }
pub(super) fn word(hex: &str) -> String { format!("{hex:0>64}") }
pub(super) fn sel(signature: &str) -> String { to_hex(&keccak(signature.as_bytes())[..4]) }
pub(super) fn u(s: &str) -> U256 { s.parse().expect("a number") }
pub(super) fn d(s: &str) -> Decimal { s.parse().expect("a decimal") }
#[allow(dead_code)]
pub(super) fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() }
pub(super) fn a(s: &str) -> Vec<u8> { abi::address(s).unwrap().to_vec() }
pub(super) fn w(n: u64) -> Vec<u8> { abi::uint(n).to_vec() }

#[test]
fn rlp_matches_the_yellow_paper_examples() {
    assert_eq!(to_hex(&Rlp::Bytes(b"dog".to_vec()).encoded()), "83646f67");
    assert_eq!(to_hex(&Rlp::List(vec![Rlp::Bytes(b"cat".to_vec()), Rlp::Bytes(b"dog".to_vec())]).encoded()), "c88363617483646f67");
    assert_eq!(to_hex(&Rlp::Bytes(vec![]).encoded()), "80");
    assert_eq!(to_hex(&Rlp::List(vec![]).encoded()), "c0");
    assert_eq!(to_hex(&Rlp::uint(0).encoded()), "80");
    assert_eq!(to_hex(&Rlp::uint(15).encoded()), "0f");
    assert_eq!(to_hex(&Rlp::uint(1024).encoded()), "820400");
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipisicing elit";
    assert_eq!(to_hex(&Rlp::Bytes(lorem.as_bytes().to_vec()).encoded()[..2]), "b838");
}

#[test]
fn erc20_transfer_calldata() {
    assert_eq!(to_hex(&erc20::transfer("0x2df1c51e09aecf9cacb7bc98cb1742757f163df7", U256::from(5_000_000)).unwrap()),
               "a9059cbb0000000000000000000000002df1c51e09aecf9cacb7bc98cb1742757f163df700000000000000000000000000000000000000000000000000000000004c4b40");
}

#[test]
fn calldata_decoding_refuses_the_drainers_tools() {
    let me = "0x14dC79964da2C08b23698B3D3cc7Ca32193d9955";
    let spender = "5c".repeat(20);
    let exact = h(&format!("095ea7b3{}{}", word(&spender), word("f4240")));
    assert_eq!(Call::decode(&exact), Call::Approve { spender: format!("0x{spender}"), amount: h(&word("f4240")) });
    assert_eq!(Call::decode(&exact).refusal(me, None), None);
    assert!(Call::decode(&h(&format!("095ea7b3{}{}", word(&spender), "f".repeat(64)))).refusal(me, None).is_some());
    assert!(Call::decode(&h(&format!("a22cb465{}{}", word(&spender), word("1")))).refusal(me, None).is_some());
    assert_eq!(Call::decode(&h(&format!("a22cb465{}{}", word(&spender), word("0")))).refusal(me, None), None); // revoking is fine
    assert!(Call::decode(&h(&format!("23b872dd{}{}{}", word(&me[2..].to_lowercase()), word(&spender), word("1")))).refusal(me, None).is_some());
    assert!(Call::decode(&h(&format!("d505accf{}", word("0")))).refusal(me, None).is_some());
    assert_eq!(Call::decode(&[]), Call::Plain);
}

#[test]
fn aave_calldata() {
    let me = "0x1523000000000000000000000000000000000001";
    let supply = aave::supply(BASE_USDC, 2_000_000, me).unwrap();
    assert_eq!(to_hex(&supply[..4]), sel("supply(address,uint256,address,uint16)"));
    assert_eq!(supply.len(), 4 + 4 * 32);
    assert_eq!(uint_in(&supply[4..], 1), Some(2_000_000));
    assert_eq!(address_in(&supply[4..], 2).as_deref(), Some(me));
    let all = aave::withdraw(BASE_USDC, None, me).unwrap();
    assert_eq!(to_hex(&all[..4]), sel("withdraw(address,uint256,address)"));
    assert!(all[4 + 32..4 + 64].iter().all(|&b| b == 0xff));
    assert_eq!(to_hex(&aave::GET_POOL), sel("getPool()"));
    assert_eq!(to_hex(&aave::get_reserve_data(BASE_USDC).unwrap()[..4]), sel("getReserveData(address)"));
    let base_pool = aave::MARKETS[0].pool;
    assert_eq!(to_hex(&erc20::approve(base_pool, U256::from(5)).unwrap()[..4]), sel("approve(address,uint256)"));
    assert_eq!(to_hex(&erc20::allowance(me, base_pool).unwrap()[..4]), sel("allowance(address,address)"));
    assert_eq!(Call::decode(&erc20::approve(base_pool, U256::from(2_000_000)).unwrap()).refusal(me, None), None);
    assert_eq!(Call::decode(&supply), Call::Lending { beneficiary: me.into() });
    assert_eq!(Call::decode(&supply).refusal(me, None), None);
    assert_eq!(Call::decode(&all).refusal(me, None), None);
    let thief = "0x9999999999999999999999999999999999999999";
    assert!(Call::decode(&aave::supply(BASE_USDC, 2_000_000, thief).unwrap()).refusal(me, None).is_some());
    assert!(Call::decode(&aave::withdraw(BASE_USDC, None, thief).unwrap()).refusal(me, None).is_some());
    // 3.8946% APR in ray → ≈ 3.97% APY.
    let apy = aave::apy(d("38946000000000000000000000"));
    assert!(apy > d("3.96") && apy < d("3.98"), "{apy}");
}

#[test]
fn relay_depository_selector() { assert_eq!(sel("depositErc20(address,address,uint256,bytes32)"), "e8017952") }

#[test]
fn uniswap_calldata_and_swap_guard() {
    let (me, spcx) = ("0x14dc79964da2c08b23698b3d3cc7ca32193d9955", "0x4a0e65a3eccec6dbe60ae065f2e7bb85fae35eea");
    assert_eq!(to_hex(&uniswap::get_pool(ROBINHOOD_USDG, spcx, 500).unwrap()[..4]), sel("getPool(address,address,uint24)"));
    assert_eq!(to_hex(&uniswap::quote_exact_input_single(ROBINHOOD_USDG, spcx, U256::from(1_000_000), 500).unwrap()[..4]),
               sel("quoteExactInputSingle((address,address,uint256,uint24,uint160))"));
    let swap = uniswap::exact_input_single(ROBINHOOD_USDG, spcx, 500, me, U256::from(1_000_000), u("6600000000000000")).unwrap();
    assert_eq!(to_hex(&swap[..4]), sel("exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))"));
    assert_eq!(swap.len(), 4 + 7 * 32);
    assert_eq!(Call::decode(&swap).refusal(me, None), None);
    let other = uniswap::exact_input_single(ROBINHOOD_USDG, spcx, 500, "0x0000000000000000000000000000000000000bad", U256::from(1), U256::ZERO).unwrap();
    assert!(Call::decode(&other).refusal(me, None).is_some());
}

#[test]
fn morpho_vault_calls_use_the_erc4626_selectors_and_pay_only_us() {
    let (me, other) = (format!("0x{}", "a".repeat(40)), format!("0x{}", "b".repeat(40)));
    assert_eq!(to_hex(&morpho::deposit(1, &me).unwrap()[..4]), sel("deposit(uint256,address)"));
    assert_eq!(to_hex(&morpho::withdraw(1, &me, &me).unwrap()[..4]), sel("withdraw(uint256,address,address)"));
    assert_eq!(to_hex(&morpho::redeem(&[0; 32], &me, &me).unwrap()[..4]), sel("redeem(uint256,address,address)"));
    assert_eq!(to_hex(&morpho::max_withdraw(&me).unwrap()[..4]), sel("maxWithdraw(address)"));
    assert_eq!(to_hex(&morpho::ASSET), sel("asset()"));
    assert_eq!(Call::decode(&morpho::deposit(2_000_000, &me).unwrap()).refusal(&me, None), None);
    assert!(Call::decode(&morpho::deposit(2_000_000, &other).unwrap()).refusal(&me, None).is_some());
    assert_eq!(Call::decode(&morpho::redeem(&[0; 32], &me, &me).unwrap()).refusal(&me, None), None);
    assert!(Call::decode(&morpho::redeem(&[0; 32], &other, &me).unwrap()).refusal(&me, None).is_some());
    assert!(Call::decode(&morpho::withdraw(1, &me, &other).unwrap()).refusal(&me, None).is_some());
}

const T: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const PAD: &str = "0x000000000000000000000000";

fn log(address: &str, topics: &[&str], amount: u64) -> Json {
    json!({"address": address, "topics": topics, "data": format!("0x{}", to_hex(&abi::uint(amount)))})
}

fn calls_ok(logs: Vec<Json>) -> Json { json!([{"calls": [{"status": "0x1", "logs": logs}]}]) }

#[test]
fn simulation_reads_our_balance_changes_from_the_logs() {
    let me = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045";
    let (usdc, weth) = ("0xaf88d065e77c8cc2239327c5edb3a432268e5831", "0x4200000000000000000000000000000000000006");
    let other = format!("{PAD}1111111111111111111111111111111111111111");
    let mine = format!("{PAD}{}", &me[2..]);
    let deposit = "0xe1fffcc4923d04b559f4d29a8bfc6cda04eb5b0d3c460751c2402c5c5cc9109c";
    let logs = vec![
        log(NATIVE, &[T, &mine, &other], 3_000),
        log(usdc, &[T, &other, &mine], 5_000_000), log(usdc, &[T, &mine, &other], 1_000_000),
        log(usdc, &[T, &other, &other], 9),
        log(weth, &[deposit, &mine], 7),
        log("0x3333333333333333333333333333333333333333", &[T, &mine, &other, &format!("{PAD}0000000000000000000000000000000000000001")], 0),
        log(usdc, &[deposit, &mine], 99), // Deposit counts only from WETH9
    ];
    let ok = Simulation::parse(&calls_ok(logs), "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045").unwrap();
    assert_eq!(ok.revert, None);
    let expected: std::collections::BTreeMap<String, i128> = [(NATIVE, -3_000), (usdc, 4_000_000), (weth, 7), ("0x3333333333333333333333333333333333333333", -1)]
        .into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    assert_eq!(ok.changes, expected);
    assert_eq!(ok.nfts.iter().collect::<Vec<_>>(), ["0x3333333333333333333333333333333333333333"]);
    let reverted = Simulation::parse(&json!([{"calls": [{"status": "0x0", "logs": [], "error": {"message": "execution reverted: ERC20: transfer amount exceeds balance"}}]}]), me).unwrap();
    assert_eq!(reverted.revert.as_deref(), Some("execution reverted: ERC20: transfer amount exceeds balance"));
    assert!(Simulation::parse(&json!(["nope"]), me).is_err());
}

#[test]
fn simulation_reads_eip7708_coin_logs_as_the_coin() {
    let me = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045";
    let other = format!("{PAD}1111111111111111111111111111111111111111");
    let mine = format!("{PAD}{}", &me[2..]);
    let system = "0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFE";
    let after = Simulation::parse(&calls_ok(vec![log(system, &[T, &mine, &other], 3_000), log(system, &[T, &other, &mine], 500)]), me).unwrap();
    assert_eq!(after.changes.into_iter().collect::<Vec<_>>(), [(NATIVE.to_string(), -2_500)]);
    let both = Simulation::parse(&calls_ok(vec![log(system, &[T, &mine, &other], 3_000), log(NATIVE, &[T, &mine, &other], 3_000)]), me).unwrap();
    assert_eq!(both.changes.into_iter().collect::<Vec<_>>(), [(NATIVE.to_string(), -3_000)]);
}

#[test]
fn big_token_amounts_and_uniswap_paths() {
    assert_eq!(to_hex(&abi::word_decimal(d("0")).unwrap()), "0".repeat(64));
    assert!(to_hex(&abi::word_decimal(d("255")).unwrap()).ends_with("00ff"));
    let big = abi::word_decimal(d("1600000000000000000000")).unwrap();
    assert_eq!(to_hex(&big), "000000000000000000000000000000000000000000000056bc75e2d631000000");
    assert_eq!(u256_in(&big, 0), Some(u("1600000000000000000000")));
    assert!(abi::word_decimal(d("1.5")).is_err());
    // USDC → 0.05% → WETH → 0.3% → AERO on Base: 20 + 3 + 20 + 3 + 20 bytes.
    let (usdc, weth, aero) = ("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", "0x4200000000000000000000000000000000000006", "0x940181a94a35a4569e4529a3cdfb74e38fd98631");
    let path = uniswap::path(&[usdc, weth, aero], &[500, 3000]).unwrap();
    assert_eq!(path.len(), 66);
    assert_eq!(to_hex(&path), format!("{}0001f4{}000bb8{}", &usdc[2..], &weth[2..], &aero[2..]));
    let (me, thief) = ("0x2df1c51e09aecf9cacb7bc98cb1742757f163df7", "0x000000000000000000000000000000000000dead");
    let mine = uniswap::exact_input(&path, me, U256::from(5_000_000), u("6000000000000000000")).unwrap();
    assert_eq!(to_hex(&mine[..4]), "b858183f");
    assert_eq!(Call::decode(&mine), Call::Swap { recipient: me.into() });
    assert_eq!(Call::decode(&mine).refusal(me, None), None);
    let theirs = uniswap::exact_input(&path, thief, U256::from(5_000_000), U256::from(1)).unwrap();
    assert!(Call::decode(&theirs).refusal(me, None).is_some());
    let quote = uniswap::quote_exact_input(&path, U256::from(5_000_000));
    assert_eq!((to_hex(&quote[..4]).as_str(), quote.len()), ("cdca1753", 4 + 32 * 3 + 96));
    assert_eq!(uniswap::v3(8453).unwrap().1, "0x2626664c2603336e57b271c5c0b26f421741e481");
    assert!(uniswap::v3(42161).is_some() && uniswap::v3(1).is_some() && uniswap::v3(137).is_some());
    assert_eq!(uniswap::v3(10).unwrap().2, "0x4200000000000000000000000000000000000006");
    assert_eq!(uniswap::v3(999).unwrap().2, "0x5555555555555555555555555555555555555555");
    assert_eq!(uniswap::v3(56).unwrap().2, "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c");
    assert!(uniswap::v2(56).is_some() && v4::on(56).is_some() && uniswap::v3(43114).is_none());
}

#[test]
fn signable_networks() { assert_eq!(SIGNABLE, [1, 137, 42161, 8453, 4663, 4217, 999, 10, 56]) }

#[test]
fn aave_markets_and_morpho_vaults() {
    assert_eq!(aave::market("Aave · Polygon").unwrap().chain_id, 137);
    assert!(aave::market("Aave · BNB").is_none());
    let (arbitrum, polygon) = (&aave::MARKETS[1], &aave::MARKETS[2]);
    assert!(arbitrum.pool == polygon.pool && arbitrum.a_usdc != polygon.a_usdc);
    let usdc = |chain: u64| match chain { 1 => ETHEREUM_USDC, 137 => POLYGON_USDC, 42161 => ARBITRUM_USDC, 8453 => BASE_USDC, _ => "" };
    assert!(aave::MARKETS.iter().all(|m| m.usdc == usdc(m.chain_id)));
    assert!(morpho::VAULTS.iter().all(|v| v.usdc == usdc(v.chain_id)));
    assert_eq!(morpho::vault("Morpho · Ethereum").unwrap().chain_id, 1);
}

#[test]
fn tempo_transfer_with_memo_and_fees() {
    let payee = "0x060b0fb0be9d90557577b3aee480711067149ff0";
    let data = erc20::transfer_with_memo(payee, 60_000, &[0xab; 32]).unwrap();
    assert_eq!((data.len(), to_hex(&data[..4]).as_str()), (4 + 96, "95777d59"));
    assert_eq!(Call::decode(&data), Call::Transfer { to: payee.into(), amount: w(60_000) });
    assert!(erc20::transfer_with_memo(payee, 1, &[1]).is_err());
    assert_eq!(to_hex(&tempo::set_user_token(TEMPO_USDC).unwrap()), format!("e7897444{}{}", "0".repeat(24), &TEMPO_USDC[2..]));
    assert_eq!((tempo::fee_units(1_000_000_000_000), tempo::fee_units(1_000_000_000_001), tempo::fee_units(0)), (1, 2, 0));
    assert!(SIGNABLE.contains(&4217));
}

fn reward(token: &str, amount: &str, proofs: &[String]) -> merkl::Reward {
    merkl::Reward { chain_id: 8453, token: token.into(), symbol: "T".into(), decimals: 18, amount: d(amount), claimed: Decimal::ZERO,
                    proofs: proofs.to_vec(), root: "0x".into(), price: None }
}

/// Merkl's claim matches Foundry's `cast calldata` for the same arguments.
#[test]
fn merkl_claim() {
    let (me, other) = ("0x14dc79964da2c08b23698b3d3cc7ca32193d9955", format!("0x{}", "5c".repeat(20)));
    let (pa, pb, pc) = (format!("0x{}", "aa".repeat(32)), format!("0x{}", "bb".repeat(32)), format!("0x{}", "cc".repeat(32)));
    let rewards = vec![reward("0xbaa5cc21fd487b8fcc2f632f3f4e8d37262a0842", "4920135994870910511487", &[pa.clone(), pb.clone()]),
                       reward(BASE_USDC, "1500000", &[pc.clone()])];
    let data = merkl::claim(me, &rewards).unwrap();
    let cast: String = ["80", "e0", "140", "1a0", "2", &me[2..], &me[2..], "2", "baa5cc21fd487b8fcc2f632f3f4e8d37262a0842", &BASE_USDC[2..],
                        "2", "10ab89a73565dc5697f", "16e360", "2", "40", "a0", "2", &pa[2..], &pb[2..], "1", &pc[2..]].iter().map(|x| word(x)).collect();
    assert_eq!(to_hex(&data), format!("71ee95c0{cast}"));
    assert_eq!(to_hex(&data[..4]), sel("claim(address[],address[],uint256[],bytes32[][])"));
    assert_eq!(Call::decode(&data), Call::Claim { recipients: vec![me.into(), me.into()] });
    assert_eq!(Call::decode(&data).refusal(me, None), None);
    assert!(Call::decode(&merkl::claim(&other, &rewards).unwrap()).refusal(me, None).is_some());
    assert_eq!(sel("getMerkleRoot()"), to_hex(&merkl::ROOT_CALL));

    let upper = format!("0x{}", me[2..].to_uppercase());
    let answer = format!(r#"[{{"chain": {{"id": 8453}}, "rewards": [
      {{"root": "0x1f", "recipient": "{upper}", "amount": "4920135994870910511487", "claimed": "4502475146340745774208",
       "pending": "0", "proofs": ["{pa}"], "token": {{"chainId": 8453, "address": "0xbaa5", "decimals": 18, "symbol": "MORPHO", "price": 2.5}}}},
      {{"root": "0x1f", "recipient": "{me}", "amount": "7", "claimed": "7", "pending": "0", "proofs": [],
       "token": {{"chainId": 8453, "address": "0xusdc", "decimals": 6, "symbol": "USDC", "price": 1}}}},
      {{"root": "0x1f", "recipient": "{other}", "amount": "9", "claimed": "0", "pending": "0", "proofs": [],
       "token": {{"chainId": 8453, "address": "0xusdc", "decimals": 6, "symbol": "USDC", "price": 1}}}}
    ]}}]"#);
    let owed = merkl::parse(answer.as_bytes(), me).unwrap();
    assert_eq!(owed.len(), 1);
    assert_eq!(owed[0].symbol, "MORPHO");
    assert_eq!(owed[0].claimable(), d("417.660848530164737279"));
}

/// A sweep claims only once the rewards are worth the floor (never under $5) and sells what's worth a dollar or more.
#[test]
fn reward_sweep_sells_what_is_worth_it() {
    let r = |symbol: &str, worth: &str| merkl::Reward { chain_id: 8453, token: format!("0x{symbol}"), symbol: symbol.into(), decimals: 0,
                                                       amount: Decimal::ONE, claimed: Decimal::ZERO, proofs: vec![], root: "0x".into(), price: Some(d(worth)) };
    assert_eq!((merkl::sweep_floor(None), merkl::sweep_floor(Some(d("2"))), merkl::sweep_floor(Some(d("12")))), (d("5"), d("5"), d("12")));
    let owed = [r("MORPHO", "7"), r("WELL", "0.4"), r("USDC", "3"), r("WETH", "2")];
    assert_eq!(merkl::to_sell(&owed, None).iter().map(|x| x.symbol.as_str()).collect::<Vec<_>>(), ["MORPHO", "WETH"]);
    assert_eq!(merkl::to_sell(&owed, Some("weth")).iter().map(|x| x.symbol.as_str()).collect::<Vec<_>>(), ["MORPHO"]);
    // A price and an amount from Merkl whose product is past Decimal: unpriced, not a panic, and not sold.
    let huge = merkl::Reward { decimals: 0, price: Some(Decimal::MAX), ..reward("0x01", "1000", &[]) };
    assert_eq!((huge.worth_usd(), merkl::to_sell(&[huge], None).len()), (None, 0));
}

#[test]
fn signer_names_every_payee_of_a_call() {
    let (me, stranger) = ("0x14dc79964da2c08b23698b3d3cc7ca32193d9955", format!("0x{}", "5c".repeat(20)));
    let odd = Call::decode(&[0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(odd.payees(&stranger, 1, 1), [stranger.clone()]);
    assert!(odd.payees(&stranger, 0, 1).is_empty());
    assert_eq!(Call::Plain.payees(&stranger, 5, 8453), [stranger.clone()]);
    assert_eq!(Call::decode(&erc20::transfer(&stranger, U256::from(1)).unwrap()).payees(BASE_USDC, 0, 8453), [stranger.clone()]);
    assert_eq!(Call::decode(&erc20::approve(&stranger, U256::from(1)).unwrap()).payees(BASE_USDC, 0, 8453), [stranger.clone()]);
    let own = [(8453, aave::MARKETS[0].pool), (42161, aave::MARKETS[1].pool), (8453, morpho::VAULTS[0].address), (8453, uniswap::v3(8453).unwrap().1),
               (4663, uniswap::ROBINHOOD_ROUTER), (4663, v4::PERMIT2), (137, "0xccc88a9d1b4ed6b0eaba998850414b24f1c315be"),
               (4217, "0x4cd00e387622c35bddb9b4c962c136462338bc31"), (8453, "0x5567954cd026cebfe486062cb75848e0f1c12e58"),
               (10, v4::on(10).unwrap().router), (10, uniswap::v3(10).unwrap().1), (999, uniswap::v2(999).unwrap().0), (999, v4::PERMIT2)];
    for (chain, spender) in own {
        assert!(Call::decode(&erc20::approve(spender, U256::from(1)).unwrap()).payees(BASE_USDC, 0, chain).is_empty(), "{spender}");
    }
    assert_eq!(Call::decode(&erc20::approve(uniswap::ROBINHOOD_ROUTER, U256::from(1)).unwrap()).payees(BASE_USDC, 0, 1), [uniswap::ROBINHOOD_ROUTER]);
    let robinhood_v4 = v4::on(4663).unwrap().router;
    let permit2 = v4::permit2_approve(ROBINHOOD_USDG, robinhood_v4, U256::from(1), 1).unwrap();
    assert!(Call::decode(&permit2).payees(v4::PERMIT2, 0, 4663).is_empty());
    assert_eq!(Call::decode(&h(&format!("39509351{}{}", word(&stranger[2..]), word("1")))).payees(BASE_USDC, 0, 8453), [stranger.clone()]);

    // «Exact» means below 2⁹⁶: 2¹²⁸ − 1 and 10³⁰ are unlimited too.
    let max128 = format!("{}{}", "0".repeat(32), "f".repeat(32));
    assert!(Call::decode(&h(&format!("095ea7b3{}{max128}", word(&stranger[2..])))).refusal(me, None).is_some());
    assert!(Call::decode(&erc20::approve(&stranger, u("1000000000000000000000000000000")).unwrap()).refusal(me, None).is_some());
    assert_eq!(Call::decode(&h(&format!("095ea7b3{}{}", word(&stranger[2..]), word(&"f".repeat(24))))).refusal(me, None), None);

    let selectors: Vec<String> = ["safeTransferFrom(address,address,uint256)", "safeTransferFrom(address,address,uint256,bytes)",
        "safeTransferFrom(address,address,uint256,uint256,bytes)", "safeBatchTransferFrom(address,address,uint256[],uint256[],bytes)",
        "approveDelegation(address,uint256)", "setAuthorization(address,bool)"].iter().map(|s| sel(s)).collect();
    assert_eq!(selectors, ["42842e0e", "b88d4fde", "f242432a", "2eb2c2d6", "c04a8a10", "eecea000"]);
    let nft = Call::decode(&h(&format!("42842e0e{}{}{}", word(&me[2..]), word(&stranger[2..]), word("7"))));
    assert_eq!(nft, Call::NftTransfer { from: me.into(), to: stranger.clone() });
    assert_eq!(nft.payees(&format!("0x{}", "aa".repeat(20)), 0, 1), [stranger.clone()]);
    assert!(Call::decode(&h(&format!("c04a8a10{}{}", word(&stranger[2..]), word("ffff")))).refusal(me, None).is_some());
    assert_eq!(Call::decode(&h(&format!("c04a8a10{}{}", word(&stranger[2..]), word("0")))).refusal(me, None), None);
    assert!(Call::decode(&h(&format!("eecea000{}{}", word(&stranger[2..]), word("1")))).refusal(me, None).is_some());
    assert_eq!(Call::decode(&h(&format!("eecea000{}{}", word(&stranger[2..]), word("0")))).refusal(me, None), None);
}

/// A fee past the network's cap is refused, and gas × price past 64 bits doesn't overflow.
#[test]
fn signer_caps_the_network_fee() {
    let tx = |chain_id: u64, gas_limit: u64, max_fee_per_gas: u64| Transaction {
        chain_id, nonce: 0, max_priority_fee_per_gas: 0, max_fee_per_gas, gas_limit, to: format!("0x{}", "11".repeat(20)), value: 0, data: vec![],
        authorizations: vec![],
    };
    assert_eq!(tx(8453, 300_000, 20_000_000).fee_problem(), None);
    assert_eq!(tx(1, 570_000, 80_000_000_000).fee_problem(), None);
    assert_eq!(tx(1, 570_000, 80_000_000_000).fee_text(), "up to 0.0456 ETH");
    assert!(tx(42161, 10_000_000, 1_000_000_000_000).fee_problem().is_some());
    assert!(tx(137, u64::MAX, 2).fee_problem().is_some());
    assert!(tx(43114, 21_000, 1).fee_problem().is_some());
    assert_eq!(tx(56, 300_000, 50_000_000).fee_problem(), None);
    assert!(tx(56, 10_000_000, 10_000_000_000).fee_problem().is_some());
    assert_eq!(tx(999, 200_000, 100_000_000).fee_problem(), None);
    assert!(tx(999, 10_000_000, 100_000_000_000).fee_problem().is_some());
}

/// Offsets a page sets can't crash the signer.
#[test]
fn huge_calldata_offsets_are_refused_not_trapped() {
    let huge = format!("{}e0", "f".repeat(62));
    assert_eq!(Call::decode(&h(&format!("3593564c{huge}{}", "0".repeat(128)))), Call::Contract { selector: "0x3593564c".into() });
    assert_eq!(merkl::recipients(&h(&format!("71ee95c0{huge}{}", "0".repeat(64)))), None);
    assert_eq!(abi::string(&h(&format!("{}{}", "0".repeat(64 + 48), "f".repeat(16)))), None); // length 2⁶⁴ − 1
}

/// 54.56 AI of 18 decimals is past 64 bits: the quote and the swap carry it as a whole 256-bit word.
#[test]
fn uniswap_amounts_past_64_bits() {
    let big = u("54560000000000000000");
    let q = uniswap::quote_exact_input_single(ROBINHOOD_USDG, "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18", big, 3000).unwrap();
    assert_eq!(u256_in(&q[4..], 2), Some(big));
    let swap = uniswap::exact_input_single(ROBINHOOD_USDG, "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18", 3000,
                                           "0x90f79bf6eb2c4f870365e785982e1f101e93b906", U256::from(10_000_000), big).unwrap();
    assert_eq!((u256_in(&swap[4..], 5), u256_in(&swap[4..], 4)), (Some(big), Some(U256::from(10_000_000))));
}

/// SwapRouter02's multicall(bytes[]) or multicall(deadline, bytes[]), encoded the way Solidity does.
fn multicall(calls: &[Vec<u8>], deadline: bool) -> Vec<u8> {
    let mut out = h(if deadline { "5ae401dc" } else { "ac9650d8" });
    if deadline { out.extend(w(1_800_000_000)) }
    out.extend(w(if deadline { 64 } else { 32 }));
    out.extend(w(calls.len() as u64));
    let (mut tails, mut offset) = (Vec::new(), calls.len() as u64 * 32);
    for c in calls {
        let padded = abi::padded(c);
        out.extend(w(offset));
        tails.extend(w(c.len() as u64));
        tails.extend(&padded);
        offset += 32 + padded.len() as u64;
    }
    [out, tails].concat()
}

#[test]
fn uniswap_swaps_the_app_does_not_make_still_pay_only_this_wallet() {
    let (me, thief) = ("0x2df1c51e09aecf9cacb7bc98cb1742757f163df7", "0x000000000000000000000000000000000000dead");
    let (usdc, weth) = ("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", "0x4200000000000000000000000000000000000006");
    let call = |selector: &str, words: Vec<Vec<u8>>| [h(selector), words.concat()].concat();
    let exact_input_single = |to: &str| call("04e45aaf", vec![a(usdc), a(weth), w(500), a(to), w(5_000_000), w(1), w(0)]);
    let unwrap = |to: &str| call("49404b7c", vec![w(1), a(to)]);
    let interface = multicall(&[exact_input_single(ADDRESS_THIS), unwrap(me)], true);
    assert_eq!(to_hex(&interface), "5ae401dc000000000000000000000000000000000000000000000000000000006b49d200000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000016000000000000000000000000000000000000000000000000000000000000000e404e45aaf000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda02913000000000000000000000000420000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000001f4000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000004c4b400000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004449404b7c00000000000000000000000000000000000000000000000000000000000000010000000000000000000000002df1c51e09aecf9cacb7bc98cb1742757f163df700000000000000000000000000000000000000000000000000000000");
    // Output held in the router isn't read as a swap paying this wallet: it stays a plain contract call.
    assert_eq!(Call::decode(&interface), Call::Contract { selector: "0x5ae401dc".into() });
    assert_eq!(Call::decode(&multicall(&[exact_input_single(ADDRESS_THIS)], false)), Call::Contract { selector: "0xac9650d8".into() });
    let (refund_eth, pull) = (h("12210e8a"), call("f2d5d56b", vec![a(usdc), w(5_000_000)]));
    assert_eq!(Call::decode(&multicall(&[exact_input_single(ADDRESS_THIS), refund_eth.clone()], false)), Call::Contract { selector: "0xac9650d8".into() });
    assert_eq!(Call::decode(&multicall(&[pull], false)), Call::Contract { selector: "0xac9650d8".into() });
    assert_eq!(Call::decode(&multicall(&[exact_input_single(me), refund_eth.clone()], false)), Call::Swap { recipient: me.into() });
    assert!(Call::decode(&multicall(&[exact_input_single(thief), refund_eth], false)).refusal(me, None).is_some());
    assert_eq!(Call::decode(&multicall(&[exact_input_single(MSG_SENDER)], false)).refusal(me, None), None);
    let v1_single = call("414bf389", vec![a(usdc), a(weth), w(500), a(MSG_SENDER), w(1_800_000_000), w(5_000_000), w(1), w(0)]);
    assert_eq!(Call::decode(&v1_single), Call::Contract { selector: "0x414bf389".into() });
    let self_permit = call("f3995c67", vec![a(usdc), w(1), w(1), w(27), vec![0; 32], vec![0; 32]]);
    assert_eq!(Call::decode(&multicall(&[self_permit, exact_input_single(MSG_SENDER)], false)), Call::Contract { selector: "0xac9650d8".into() });
    assert_eq!(Call::decode(&interface[..100]), Call::Contract { selector: "0x5ae401dc".into() });
    let v2_exact = call("8803dbee", vec![w(1), w(5), w(160), a(thief), w(1_800_000_000), w(2), a(usdc), a(weth)]);
    assert_eq!(Call::decode(&v2_exact), Call::Swap { recipient: thief.into() });
    assert!(Call::decode(&v2_exact).refusal(me, None).is_some());
    let v2_eth_in = call("7ff36ab5", vec![w(1), w(128), a(me), w(1_800_000_000), w(2), a(weth), a(usdc)]);
    assert_eq!(Call::decode(&v2_eth_in), Call::Swap { recipient: me.into() });
    let path = uniswap::path(&[usdc, weth], &[500]).unwrap();
    let exact_output = call("09b81346", vec![w(32), w(128), a(thief), w(1), w(5), w(path.len() as u64), abi::padded(&path)]);
    assert_eq!(Call::decode(&exact_output), Call::Swap { recipient: thief.into() });
    assert!(Call::decode(&exact_output).refusal(me, None).is_some());
    let lying = call("b858183f", vec![w(96), w(0), a(me), w(160), a(thief), w(5_000_000), w(1), w(path.len() as u64), abi::padded(&path)]);
    assert_eq!(Call::decode(&lying), Call::Contract { selector: "0xb858183f".into() });
}

/// A call the decoder can't read is asked about in person once, except to the contracts ihatecrypto's own moves call
/// that way. Uniswap's routers and Permit2 hold approvals: an unread call there asks.
#[test]
fn unread_contract_calls_are_asked_about_once() {
    let unread = Call::decode(&[&[0xde, 0xad, 0xbe, 0xef][..], &[0; 32]].concat());
    let site = "0x1111111111111111111111111111111111111111";
    assert_eq!(unread.unread_target(site, 0, 8453).as_deref(), Some(site));
    assert_eq!(unread.unread_target(site, 1, 8453), None);
    assert_eq!(unread.unread_target("0xccc88a9d1b4ed6b0eaba998850414b24f1c315be", 0, 8453), None);
    assert_eq!(unread.unread_target(safe::FACTORY, 0, 8453), None);
    let router = v4::on(8453).unwrap().router;
    assert_eq!(unread.unread_target(router, 0, 8453).as_deref(), Some(router));
    assert_eq!(unread.unread_target(v4::PERMIT2, 0, 8453).as_deref(), Some(v4::PERMIT2));
    assert_eq!(Call::decode(&erc20::transfer(site, U256::from(1)).unwrap()).unread_target(site, 0, 8453), None);
}

/// The 2026-10-24 review's EVM gaps, each held by the signer's own reading of the call.
#[test]
fn calls_the_review_found_are_payees_or_refused() {
    let (me, thief) = ("0x1111111111111111111111111111111111111111", "0x9999999999999999999999999999999999999999");
    let usdc = BASE_USDC;
    let pull = [h("23b872dd"), a(thief), a(thief), w(5)].concat();
    assert_eq!(Call::decode(&pull).payees(usdc, 0, 8453), [thief]);
    assert_eq!(Call::decode(&[1]).unread_target(thief, 0, 8453).as_deref(), Some(thief));
    assert_eq!(Call::decode(&[]), Call::Plain);
    let swap_here = [h("04e45aaf"), a(usdc), a(usdc), w(500), a(me), w(1), w(1), w(0)].concat();
    assert!(matches!(Call::decode(&swap_here), Call::Swap { .. }));
    assert_eq!(Call::decode(&swap_here).unread_target(thief, 0, 8453).as_deref(), Some(thief));
    assert_eq!(Call::decode(&swap_here).unread_target(uniswap::v3(8453).unwrap().1, 0, 8453), None);
    // Relay's depository: only a deposit naming this wallet as the depositor; its order id is the last word.
    let (depository, id) = (relay::DEPOSITORY, vec![7u8; 32]);
    let erc20 = keccak(b"depositErc20(address,address,uint256,bytes32)")[..4].to_vec();
    let mine = [erc20.clone(), a(me), a(usdc), w(5), id.clone()].concat();
    assert_eq!(Call::target_refusal(depository, &mine, me), None);
    assert_eq!(Call::relay_deposit_id(depository, &mine), Some(id.clone()));
    let theirs = [erc20, a(thief), a(usdc), w(5), id].concat();
    assert!(Call::target_refusal(depository, &theirs, me).is_some());
    assert!(Call::target_refusal(depository, &h("deadbeef"), me).is_some());
    // A Safe's execTransaction sent from this wallet: a payout passes; a call into the multisig or a gas refund doesn't.
    let safe_address = format!("0x{}", "ab".repeat(20));
    let pay = safe::Move { chain_id: 8453, safe: safe_address.clone(), to: usdc.into(), value: 0, data: erc20::transfer(me, U256::from(5)).unwrap(), nonce: 0 };
    let sig = EvmSignature { r: vec![1; 32], s: vec![2; 32], v: 27 };
    let signers = [(me.to_string(), sig.clone())];
    assert_eq!(safe::exec_refusal(&pay.exec(&signers).unwrap(), &safe_address), None);
    let self_call = safe::Move { chain_id: 8453, safe: safe_address.clone(), to: safe_address.clone(), value: 0, data: [h("0d582f13"), a(thief), w(1)].concat(), nonce: 0 };
    assert!(safe::exec_refusal(&self_call.exec(&signers).unwrap(), &safe_address).is_some());
    let mut refund = pay.exec(&signers).unwrap();
    refund[4 + 6 * 32..4 + 7 * 32].copy_from_slice(&abi::uint(1)); // gasPrice 1
    assert!(safe::exec_refusal(&refund, &safe_address).is_some());
    assert_eq!(safe::exec_refusal(&erc20::transfer(me, U256::from(5)).unwrap(), &safe_address), None);
    // An execTransaction off Safe's canonical layout (a data offset one byte off) is refused, not let through.
    let exec = self_call.exec(&signers).unwrap();
    let mut skewed = exec.clone();
    for word_index in [2usize, 9] {
        let n = uint_in(&exec[4..], word_index).unwrap() + 1;
        skewed[4 + word_index * 32..4 + (word_index + 1) * 32].copy_from_slice(&abi::uint(n));
    }
    skewed.insert(4 + 10 * 32, 0);
    assert_eq!(safe::read_exec(&skewed), None);
    assert!(safe::exec_refusal(&skewed, &safe_address).is_some());
    assert!(Call::target_refusal(&safe_address, &skewed, me).is_some());
    assert!(!batch7702::problems(&[batch7702::BatchCall { to: safe_address.clone(), data: skewed }], me, 8453).is_empty());
    // Nine head words, a data offset past the end, an operation word with stray bits: unread, so refused.
    assert!(safe::exec_refusal(&exec[..4 + 9 * 32], &safe_address).is_some());
    let mut past_end = exec.clone();
    past_end[4 + 2 * 32..4 + 3 * 32].copy_from_slice(&abi::uint(exec.len() as u64 - 4));
    assert!(safe::exec_refusal(&past_end, &safe_address).is_some());
    let mut dirty_op = pay.exec(&signers).unwrap();
    dirty_op[4 + 3 * 32..4 + 4 * 32].copy_from_slice(&abi::uint(0x0101));
    assert!(safe::read_exec(&dirty_op).is_none() && safe::exec_refusal(&dirty_op, &safe_address).is_some());
    // A data offset or length near 2⁶⁴ reads as nothing.
    for (word_index, n) in [(2usize, u64::MAX - 31), (10, u64::MAX)] {
        let mut huge = exec.clone();
        huge[4 + word_index * 32..4 + (word_index + 1) * 32].copy_from_slice(&abi::uint(n));
        assert_eq!(safe::read_exec(&huge), None);
    }
    // A move the signer reads is still only a payout.
    let mv = |to: &str, value: u64, data: Vec<u8>| safe::Move { chain_id: 8453, safe: safe_address.clone(), to: to.into(), value, data, nonce: 0 }.exec(&signers).unwrap();
    assert_eq!(safe::exec_refusal(&mv(thief, 1, vec![]), &safe_address), None);
    assert!(safe::exec_refusal(&mv(usdc, 1, pay.data.clone()), &safe_address).is_some());
    assert!(safe::exec_refusal(&mv(thief, 0, [h("a22cb465"), a(thief), w(1)].concat()), &safe_address).is_some());
    let withdraw = [h("b460af94"), w(5), a(thief), a(&safe_address)].concat();
    assert!(matches!(Call::decode(&withdraw), Call::Vault { .. }));
    assert!(safe::exec_refusal(&mv(thief, 0, withdraw), &safe_address).is_some());
    // approveHash(bytes32) to any address.
    let approve_hash = [h("d4d9bdcd"), vec![7; 32]].concat();
    assert!(Call::target_refusal(&safe_address, &approve_hash, me).is_some());
    assert!(Call::target_refusal(thief, &approve_hash, me).is_some());
}

#[test]
fn l1_fee_upper_bound_selector() { assert_eq!(sel("getL1FeeUpperBound(uint256)"), "f1c7a58b") }

fn kyber_call(receiver: &str, fees: &[&str], permit: bool, misaligned: bool, min_return: U256) -> Vec<u8> {
    let (usdc, aero, executor) = ("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", "0x940181a94a35a4569e4529a3cdfb74e38fd98631", "0x8f10b468b06c6fd214b65f87778827f7d113f996");
    let n = fees.len() as u64;
    let (arrays, fees_at) = (352u64, 352 + 128);
    let fee_amounts_at = fees_at + 32 + 32 * n;
    let permit_at = fee_amounts_at + 32 + 32 * n;
    let mut desc = [a(usdc), a(aero), w(arrays), w(arrays + 64), w(fees_at), w(fee_amounts_at), a(receiver), w(1_000_000),
                    abi::word(min_return).to_vec(), w(512), w(permit_at)].concat();
    desc.extend([w(1), a(executor), w(1), w(1_000_000)].concat());
    desc.extend(w(n));
    for f in fees { desc.extend(a(f)) }
    desc.extend(w(n));
    for _ in fees { desc.extend(w(1)) }
    desc.extend(if permit { [w(32), vec![0; 32]].concat() } else { w(0) });
    let params = [a(executor), w(0), w(160), w(if misaligned { 200 } else { 192 }), w(192 + desc.len() as u64), w(0), desc, w(0)].concat();
    [vec![0xe2, 0x1f, 0xd0, 0xe9], w(32), params].concat()
}

/// KyberSwap's router call in its API's shape (1 USDC → AERO on Base, built 2026-10-28), read by the signer itself.
#[test]
fn a_kyberswap_pays_only_this_wallet() {
    let (me, evil) = ("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266", "0x3333333333333333333333333333333333333333");
    let (usdc, aero) = ("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", "0x940181a94a35a4569e4529a3cdfb74e38fd98631");
    let min = u("1425956540186467126");
    let call = |receiver: &str, fees: &[&str], permit: bool, misaligned: bool, min: U256| kyber_call(receiver, fees, permit, misaligned, min);
    let swap = kyber::decode(&call(me, &[], false, false, min)).unwrap();
    assert_eq!((swap.src_token.as_str(), swap.dst_token.as_str(), swap.dst_receiver.as_str(), swap.amount), (usdc, aero, me, U256::from(1_000_000)));
    assert!(swap.min_return == min && swap.fee_receivers.is_empty() && !swap.permit);
    let read = Call::decode(&call(me, &[], false, false, min));
    assert_eq!(read.refusal(me, Some(8453)), None);
    assert!(read.payees(kyber::ROUTER, 0, 8453).is_empty());
    assert_eq!(read.unread_target(kyber::ROUTER, 0, 8453), None);
    assert_eq!(read.summary(8453, kyber::ROUTER, 0),
               "swap 1 USDC on KyberSwap on Base for at least 1425956540186467126 units of the token 0x9401…8631, paying this wallet");
    assert!(Call::decode(&call(evil, &[], false, false, min)).refusal(me, Some(8453)).unwrap().contains(evil));
    assert!(Call::decode(&call(me, &[evil], false, false, min)).refusal(me, Some(8453)).unwrap().contains(evil));
    assert!(Call::decode(&call(me, &[], true, false, min)).refusal(me, Some(8453)).is_some());
    assert!(Call::decode(&call(me, &[], false, false, U256::ZERO)).refusal(me, Some(8453)).unwrap().contains("least"));
    let approve = [vec![0x09, 0x5e, 0xa7, 0xb3], a(kyber::ROUTER), w(1_000_000)].concat();
    assert!(Call::decode(&approve).payees(usdc, 0, 8453).is_empty());
    assert_eq!(Call::target_refusal(kyber::ROUTER, &call(me, &[], false, false, min), me), None);
    assert!(Call::target_refusal(kyber::ROUTER, &[vec![0x8a, 0xf0, 0x33, 0xfb], vec![0; 64]].concat(), me).is_some());
    assert_eq!(kyber::decode(&call(me, &[], false, true, min)), None);
    assert!(Call::target_refusal(kyber::ROUTER, &call(me, &[], false, true, min), me).is_some());
}

/// eth_getBalance past 64 bits: 20 ETH is 2·10¹⁹ wei.
#[test]
fn coin_balance_past_64_bits() {
    assert_eq!(wei(&json!("0x1158e460913d00000")), Some(20_000_000_000_000_000_000));
    assert_eq!((wei(&json!("0x0")), wei(&json!("0xffffffffffffffff"))), (Some(0), Some(u64::MAX as u128)));
    assert_eq!((wei(&json!(format!("0x{}", "f".repeat(32)))), wei(&json!(format!("0x{}", "f".repeat(33))))), (Some(u128::MAX), None));
    for bad in [json!("0x"), json!("1158e460913d00000"), json!("0xzz"), Json::Null] { assert_eq!(wei(&bad), None) }
}

/// Research E-2 and E-3: increaseApproval and the operator calls allow and setOperator are read, not signed blind.
#[test]
fn increase_approval_and_operator_calls_are_read_not_signed_blind() {
    let (me, stranger) = ("0x1111111111111111111111111111111111111111", "0x000000000000000000000000000000000000dead");
    let call = |signature: &str, args: &[&str]| h(&format!("{}{}", sel(signature), args.iter().map(|x| word(x)).collect::<String>()));
    let (x, wbtc) = (&stranger[2..], "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599");
    assert_eq!(sel("increaseApproval(address,uint256)"), "d73dd623");
    assert!(Call::decode(&call("increaseApproval(address,uint256)", &[x, &"f".repeat(64)])).refusal(me, None).unwrap().contains("Unlimited"));
    let one = Call::decode(&call("increaseApproval(address,uint256)", &[x, "1"]));
    assert_eq!(one.refusal(me, None), None);
    assert_eq!(one.payees(wbtc, 0, 1), [stranger]);
    assert_eq!(one.summary(1, wbtc, 0), "let 0x0000…dead spend 1 units of the token 0x2260…c599 more on Ethereum");
    for f in ["allow(address,bool)", "setOperator(address,bool)"] {
        assert!(Call::decode(&call(f, &[x, "1"])).refusal(me, None).unwrap().contains(stranger), "{f}");
        assert_eq!(Call::decode(&call(f, &[x, "0"])).refusal(me, None), None, "{f}");
    }
    assert_eq!([sel("allow(address,bool)"), sel("setOperator(address,bool)")], ["110496e5", "558a7297"]);
}

// MARK: UniswapRoutesTests.swift

const ME: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";
const BASE_WETH: &str = "0x4200000000000000000000000000000000000006";
const AERO: &str = "0x940181a94a35a4569e4529a3cdfb74e38fd98631";

/// Uniswap v2's calls byte for byte as Foundry's `cast calldata` writes them (2026-10-05).
#[test]
fn v2_calldata_matches_cast() {
    assert_eq!(to_hex(&uniswap::get_amounts_out(U256::from(1_000_000), &[BASE_USDC, BASE_WETH, AERO]).unwrap()),
               "d06ca61f00000000000000000000000000000000000000000000000000000000000f424000000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000003000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda029130000000000000000000000004200000000000000000000000000000000000006000000000000000000000000940181a94a35a4569e4529a3cdfb74e38fd98631");
    assert_eq!(to_hex(&uniswap::swap_exact_tokens(U256::from(1_000_000), u("1234567890123456789"), &[BASE_USDC, AERO], ME, 1_791_200_000, false).unwrap()),
               "5c11d79500000000000000000000000000000000000000000000000000000000000f4240000000000000000000000000000000000000000000000000112210f47de9811500000000000000000000000000000000000000000000000000000000000000a00000000000000000000000009858effd232b4033e47d90003d41ec34ecaeda94000000000000000000000000000000000000000000000000000000006ac38b000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda02913000000000000000000000000940181a94a35a4569e4529a3cdfb74e38fd98631");
    assert_eq!(to_hex(&uniswap::swap_exact_tokens(u("5000000000000000000"), U256::from(1), &[AERO, BASE_WETH], ME, 1_791_200_000, true).unwrap()),
               "791ac9470000000000000000000000000000000000000000000000004563918244f40000000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000a00000000000000000000000009858effd232b4033e47d90003d41ec34ecaeda94000000000000000000000000000000000000000000000000000000006ac38b000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000940181a94a35a4569e4529a3cdfb74e38fd986310000000000000000000000004200000000000000000000000000000000000006");
    let answer = [w(0x20), w(3), w(1_000_000), w(400_000_000_000_000), w(777)].concat();
    assert_eq!(uniswap::amount_out(&answer), Some(U256::from(777)));
    assert_eq!(uniswap::amount_out(&[w(0x20), w(9)].concat()), None);
}

/// The signer reads every v2 swap form: only this wallet may get what comes out, the router is a known spender.
#[test]
fn signer_reads_v2_swaps_and_knows_its_routers() {
    for chain in [8453, 42161, 1, 137, 4663, 10, 999, 56] {
        let (router, _) = uniswap::v2(chain).unwrap();
        assert!(Call::known_spenders(chain).contains(router));
        for to_eth in [false, true] {
            let mine = uniswap::swap_exact_tokens(U256::from(1), U256::from(1), &[BASE_USDC, AERO], ME, 1, to_eth).unwrap();
            assert_eq!(Call::decode(&mine), Call::Swap { recipient: ME.into() });
            assert_eq!(Call::decode(&mine).refusal(ME, None), None);
            let theirs = uniswap::swap_exact_tokens(U256::from(1), U256::from(1), &[BASE_USDC, AERO], BASE_WETH, 1, to_eth).unwrap();
            assert!(Call::decode(&theirs).refusal(ME, None).is_some());
        }
    }
    let mut plain = uniswap::swap_exact_tokens(U256::from(1), U256::from(1), &[BASE_USDC, AERO], BASE_WETH, 1, false).unwrap();
    plain[..4].copy_from_slice(&[0x38, 0xed, 0x17, 0x39]);
    assert_eq!(Call::decode(&plain), Call::Swap { recipient: BASE_WETH.into() });
    assert!(Call::decode(&plain).refusal(ME, None).is_some());
    assert!(!Call::known_spenders(8453).contains(BASE_WETH));
}

/// v4 hooks: only one Uniswap's registry vouches for on that network, taking no swap delta, not setting the fee.
#[test]
fn v4_hooks_only_from_the_registry_on_their_own_network() {
    let elsewhere: std::collections::HashSet<&str> = SIGNABLE.iter().filter(|&&c| c != 8453).flat_map(|&c| crate::hooks::vanilla_hooks(c).iter().copied()).collect();
    let mut base: Vec<&str> = crate::hooks::vanilla_hooks(8453).to_vec();
    base.sort();
    let listed = *base.iter().find(|x| !elsewhere.contains(*x)).unwrap();
    let key = |hooks: &str, fee: u32| v4::PoolKey::new(BASE_USDC, AERO, fee, 60, hooks);
    assert!(v4::allows_hooks(&key("0x0000000000000000000000000000000000000000", 3000), 8453));
    assert!(v4::allows_hooks(&key(listed, 3000), 8453));
    assert!(!v4::allows_hooks(&key(listed, 0x800000), 8453));
    assert!(!v4::allows_hooks(&key(listed, 3000), 42161) && !v4::allows_hooks(&key(listed, 3000), 1));
    assert!(!v4::allows_hooks(&key("0x1111111111111111111111111111111111110080", 3000), 8453));
    assert!(v4::takes_swap_delta("0x00000000000000000000000000000000000000c8") && v4::takes_swap_delta("0x0000000000000000000000000000000000000044"));
    assert!(!v4::takes_swap_delta("0x0000000000000000000000000000000000000080") && v4::takes_swap_delta("0xzz"));
    for chain in [8453, 42161, 1, 137, 4663, 10, 56] {
        let hooks = crate::hooks::vanilla_hooks(chain);
        assert!(!hooks.is_empty(), "network {chain}");
        for h in hooks { assert!(h.len() == 42 && *h == h.to_lowercase() && !v4::takes_swap_delta(h)) }
    }
    let swap = v4::execute(&key(listed, 3000), true, U256::from(1_000_000), U256::from(1), 1).unwrap();
    assert_eq!(Call::decode(&swap).refusal(ME, Some(8453)), None);
    assert!(Call::decode(&swap).refusal(ME, Some(1)).unwrap().contains("hooks"));
    assert!(Call::decode(&swap).refusal(ME, None).unwrap().contains("hooks"));
    let unlisted = v4::execute(&key("0x1111111111111111111111111111111111110080", 3000), true, U256::from(1), U256::from(1), 1).unwrap();
    assert!(Call::decode(&unlisted).refusal(ME, Some(8453)).unwrap().contains("hooks"));
    assert!(!batch7702::problems(&[batch7702::BatchCall { to: v4::on(8453).unwrap().router.into(), data: unlisted }], ME, 8453).is_empty());
}

/// Coins sent with a call go only back to this wallet: a wrap by the network's WETH, a v4 swap of exactly its input.
#[test]
fn signer_lets_coins_go_only_back_to_this_wallet() {
    let (wrap, wei) = (vec![0xd0, 0xe3, 0x0d, 0xb0], 1_000_000_000_000_000u64);
    assert_eq!(Call::decode(&wrap), Call::Wrap);
    assert!(Call::decode(&wrap).payees(BASE_WETH, wei, 8453).is_empty());
    assert_eq!(Call::decode(&wrap).payees(AERO, wei, 8453), [AERO]);
    assert_eq!(Call::decode(&wrap).payees(BASE_WETH, wei, 42161), [BASE_WETH]);
    assert_eq!(Call::decode(&wrap).summary(8453, BASE_WETH, wei), "wrap 0.001 ETH into WETH on Base");
    let unwrap = [vec![0x2e, 0x1a, 0x7d, 0x4d], w(wei)].concat();
    assert_eq!(Call::decode(&unwrap), Call::Unwrap { amount: w(wei) });
    assert!(Call::decode(&unwrap).payees(BASE_WETH, 0, 8453).is_empty());
    assert!(Call::decode(&unwrap).summary(8453, BASE_WETH, 0).starts_with("unwrap 0.001 WETH"));
    let router = v4::on(8453).unwrap().router;
    let native = v4::PoolKey::new(v4::NATIVE, AERO, 3000, 60, v4::NATIVE);
    let eth_in = v4::execute(&native, true, U256::from(wei), U256::from(1), 1).unwrap();
    assert!(Call::decode(&eth_in).payees(router, wei, 8453).is_empty());
    assert_eq!(Call::decode(&eth_in).payees(router, wei + 1, 8453), [router]);
    assert_eq!(Call::decode(&eth_in).payees(AERO, wei, 8453), [AERO]);
    assert_eq!(Call::decode(&eth_in).summary(8453, router, wei), "swap 0.001 ETH on Uniswap v4 on Base, paying this wallet");
    let token_in = v4::execute(&native, false, U256::from(wei), U256::from(1), 1).unwrap();
    assert_eq!(Call::decode(&token_in).payees(router, wei, 8453), [router]);
    let v2 = uniswap::swap_exact_tokens(U256::from(1), U256::from(1), &[BASE_WETH, AERO], ME, 1, false).unwrap();
    assert_eq!(Call::decode(&v2).payees(uniswap::v2(8453).unwrap().0, wei, 8453).len(), 1);
}

/// A revoke gives nobody anything; approve(spender, 0) still names its spender (ERC-721 shares the selector).
#[test]
fn signer_lets_a_revoke_go_without_an_approval() {
    let stranger = "0x1111111111111111111111111111111111111111";
    assert_eq!(Call::decode(&erc20::approve(stranger, U256::ZERO).unwrap()).payees(BASE_USDC, 0, 8453), [stranger]);
    assert!(Call::decode(&erc20::approve(&format!("0x{}", "0".repeat(40)), U256::ZERO).unwrap()).payees(BASE_USDC, 0, 8453).is_empty());
    assert_eq!(Call::decode(&erc20::approve(stranger, U256::from(1)).unwrap()).payees(BASE_USDC, 0, 8453), [stranger]);
    let p2zero = v4::permit2_approve(BASE_USDC, stranger, U256::ZERO, 0).unwrap();
    assert!(Call::decode(&p2zero).payees(v4::PERMIT2, 0, 8453).is_empty());
    assert_eq!(Call::decode(&p2zero).refusal(ME, Some(8453)), None);
}

// MARK: ContractGuardTests.swift (the simulation's grants; the guard itself is the engine's) and LedgerTests.swift

#[test]
fn simulations_show_the_rights_a_wallet_hands_out() {
    let (me, vault, evil, usdc) = ("0x1111111111111111111111111111111111111111", "0x2222222222222222222222222222222222222222",
                                   "0x3333333333333333333333333333333333333333", "0xaf88d065e77c8cc2239327c5edb3a432268e5831");
    let topic = |x: &str| format!("{PAD}{}", &x[2..]);
    let event = |s: &str| format!("0x{}", to_hex(&keccak(s.as_bytes())));
    let n = |v: u64| format!("0x{}", to_hex(&abi::uint(v)));
    let sim = |logs: Json| Simulation::parse(&json!([{"calls": [{"status": "0x1", "logs": logs}]}]), me).unwrap();
    let (approval, all) = (event("Approval(address,address,uint256)"), event("ApprovalForAll(address,address,bool)"));
    let exact = sim(json!([{"address": usdc, "topics": [approval, topic(me), topic(vault)], "data": n(10_000_000)}]));
    assert_eq!(exact.grants, [Grant { kind: "approve".into(), asset: usdc.into(), to: vault.into(), unlimited: false }]);
    let unlimited = sim(json!([{"address": usdc, "topics": [approval, topic(me), topic(vault)], "data": format!("0x{}", "f".repeat(64))}]));
    assert!(unlimited.grants[0].unlimited);
    let nfts = sim(json!([{"address": evil, "topics": [all, topic(me), topic(vault)], "data": n(1)}]));
    assert_eq!(nfts.grants, [Grant { kind: "all".into(), asset: evil.into(), to: vault.into(), unlimited: true }]);
    let none = sim(json!([{"address": usdc, "topics": [approval, topic(me), topic(evil)], "data": n(0)},
                          {"address": usdc, "topics": [approval, topic(evil), topic(me)], "data": n(5)}]));
    assert!(none.grants.is_empty());
}

/// decimals(): a token that doesn't answer gets none, never 18.
#[test]
fn decimals_are_read_never_guessed() {
    let n = |v: u64| abi::uint(v).to_vec();
    assert_eq!(abi::decimals(Some(&n(6))), Some(6));
    assert_eq!(abi::decimals(Some(&n(18))), Some(18));
    assert_eq!(abi::decimals(None), None);
    assert_eq!(abi::decimals(Some(&[])), None);
    assert_eq!(abi::decimals(Some(&n(37))), None);
    assert_eq!(abi::decimals(Some(&n(1 << 63))), None);
    assert_eq!(abi::decimals(Some(&[0xff; 32])), None);
}

#[test]
fn a_signature_from_elsewhere_makes_the_same_transaction() {
    let key = h("0123456789012345678901234567890123456789012345678901234567890123");
    let tx = Transaction { chain_id: 8453, nonce: 3, max_priority_fee_per_gas: 1_000_000, max_fee_per_gas: 20_000_000, gas_limit: 60_000,
                           to: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".into(), value: 0,
                           data: erc20::approve("0x0000000000000000000000000000000000000001", U256::from(5)).unwrap(), authorizations: vec![] };
    let signature = sign(&tx.signing_hash().unwrap(), &key).unwrap();
    assert_eq!(tx.signed_with(&signature).unwrap().0, tx.signed(&key).unwrap().0);
    let me = wallet::keys::evm_address(&key).unwrap();
    assert_eq!(recover(&tx.signing_hash().unwrap(), &signature).unwrap(), me);
    let flipped = EvmSignature { v: if signature.v == 27 { 28 } else { 27 }, ..signature };
    assert_ne!(recover(&tx.signing_hash().unwrap(), &flipped).ok(), Some(me));
}
