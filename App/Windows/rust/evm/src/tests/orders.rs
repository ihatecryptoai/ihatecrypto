//! RelayOrderTests.swift's real orders (api.relay.link, 2026-10-24, the public test wallet), and the other protocols' cases.

use super::calls::h;
use crate::protocols::SponsoredTransfer;
use crate::relay_order::RelayOrder;
use crate::*;

const ORDERS: [(&str, &str); 3] = [
    ("0x9152ea5c8536a888468b70453ceb0ffa04863f504323891056659c5afe531b56", r#"{"version":"v1","solverChainId":"base","solver":"0xf70da97812cb96acdf810712aa562db8dfa3dbef","salt":"0x96b62513f6c215d0ac3606a62a62791754ab9b97fbb5ca42da9e22faa6d19095","inputs":[{"payment":{"chainId":"solana","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","amount":"5000000","weight":"1"},"refunds":[{"chainId":"solana","recipient":"HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","minimumAmount":"0","deadline":1793396777,"extraData":"0x"},{"chainId":"base","recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"0","deadline":1793396777,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"}]}],"output":{"chainId":"base","payments":[{"recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"4878026","expectedAmount":"4977577"}],"calls":[],"deadline":1793396777,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"},"fees":[]}"#),
    ("0xf127e4feda47952372d7e607a05dbeeb394eb8f0c953ef85eab2dc32fbe5b546", r#"{"version":"v1","solverChainId":"base","solver":"0xf70da97812cb96acdf810712aa562db8dfa3dbef","salt":"0x7ddac287d557d21935a3eeefd355c80b564e09715cf6b571f47aafda82a6d97d","inputs":[{"payment":{"chainId":"hyperliquid","currency":"0x00000000000000000000000000000000","amount":"500000000","weight":"1"},"refunds":[{"chainId":"hyperliquid","recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x00000000000000000000000000000000","minimumAmount":"0","deadline":1793396850,"extraData":"0x"},{"chainId":"base","recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"0","deadline":1793396850,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"}]}],"output":{"chainId":"base","payments":[{"recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"4878146","expectedAmount":"4977700"}],"calls":[],"deadline":1793396850,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"},"fees":[]}"#),
    ("0x098dbfaf667b9a37d851bd79adee07393cd99499fc091c5785c0b80b9eb6a2f2", r#"{"version":"v1","solverChainId":"base","solver":"0xf70da97812cb96acdf810712aa562db8dfa3dbef","salt":"0xff566540576a54aeb2c99679f6fb8af8a8b306110ce0b8e10bcadd3585022963","inputs":[{"payment":{"chainId":"base","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","amount":"5000000","weight":"1"},"refunds":[{"chainId":"base","recipient":"0x9858EfFD232B4033E47d90003D41EC34EcaEda94","currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"0","deadline":1793396849,"extraData":"0x000000000000000000000000b92fe925dc43a0ecde6c8b1a2709c170ec4fff4f"},{"chainId":"solana","recipient":"HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","minimumAmount":"0","deadline":1793396849,"extraData":"0x"}]}],"output":{"chainId":"solana","payments":[{"recipient":"HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk","currency":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v","minimumAmount":"4878735","expectedAmount":"4978301"}],"calls":[],"deadline":1793396849,"extraData":"0x"},"fees":[]}"#),
];
const EVM: &str = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94";
const SOLANA: &str = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk";

/// The signer recomputes a Relay order's id itself: the 32 bytes its deposit names are the hash of an order paying this wallet.
#[test]
fn relay_order_ids_are_recomputed_from_the_order() {
    for (id, json) in ORDERS {
        let order = RelayOrder::read(json.as_bytes(), Some(EVM), Some(SOLANA), 1_700_000_000).unwrap();
        assert_eq!(format!("0x{}", to_hex(&order.id)), id);
    }
    assert_eq!(RelayOrder::read(ORDERS[0].1.as_bytes(), Some(EVM), Some(SOLANA), 1_700_000_000).unwrap().recipient, EVM.to_lowercase());
}

/// Another payee changes the id; a refund to someone else, a call on the other network or an expired order is refused.
#[test]
fn relay_orders_that_pay_someone_else_are_refused() {
    let base = ORDERS[0].1;
    let thief = "0x000000000000000000000000000000000000dEaD";
    let swapped = base.replace(&format!(r#""payments":[{{"recipient":"{EVM}""#), &format!(r#""payments":[{{"recipient":"{thief}""#));
    assert_ne!(swapped, base);
    let other = RelayOrder::read(swapped.as_bytes(), Some(EVM), Some(SOLANA), 1_700_000_000).unwrap();
    assert!(format!("0x{}", to_hex(&other.id)) != ORDERS[0].0 && other.recipient == thief.to_lowercase());
    assert!(RelayOrder::read(base.as_bytes(), Some(thief), Some(SOLANA), 1_700_000_000).is_err());
    assert!(RelayOrder::read(base.replace(r#""calls":[]"#, r#""calls":["0x1234"]"#).as_bytes(), Some(EVM), Some(SOLANA), 1_700_000_000).is_err());
    assert!(RelayOrder::read(base.as_bytes(), Some(EVM), Some(SOLANA), 4_000_000_000).is_err());
}

/// MegaFuel's zero-fee transfer on BNB Chain: only a dollar it knows, a whole amount, a real recipient, under 100,000 gas,
/// signed as a legacy EIP-155 transaction whose sender recovers to the key.
/// What an order takes and gives back (the Mac's relayOrdersSayWhatComesBackInDollars and …SayWhatTheyPayOnThePrompt):
/// for the automation wallet's limits and for the deposit's prompt, in the order's own words.
#[test]
fn relay_orders_say_what_they_pay() {
    let d = |s: &str| s.parse::<rust_decimal::Decimal>().unwrap();
    let read = |json: &str| RelayOrder::read(json.as_bytes(), Some(EVM), Some(SOLANA), 1_700_000_000);
    let orders: Vec<RelayOrder> = ORDERS.iter().map(|(_, json)| read(json).unwrap()).collect();
    // Solana → Base, Hyperliquid → Base, Base → Solana: 5 USDC in, at least 4.878… USDC out.
    assert_eq!(orders.iter().map(|o| o.input_currency.as_str()).collect::<Vec<_>>(),
               ["EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "0x00000000000000000000000000000000", BASE_USDC]);
    assert_eq!(orders.iter().map(|o| o.input_amount.as_str()).collect::<Vec<_>>(), ["5000000", "500000000", "5000000"]);
    assert_eq!(orders.iter().map(|o| o.minimum_dollars()).collect::<Vec<_>>(), [Some(d("4.878026")), Some(d("4.878146")), Some(d("4.878735"))]);
    let me = EVM.to_lowercase();
    assert_eq!(orders[0].pays(), format!("at least 4.878026 USDC to {me} on base"));
    assert_eq!(orders[0].summary(), format!("bridge 5 USDC from solana through Relay: at least 4.878026 USDC to {me} on base"));
    assert!(orders[1].summary().starts_with("bridge 5 USDC from hyperliquid"));
    // Paid in anything else — another token, the network's coin — nothing is counted as coming back, and the prompt
    // names its smallest units and the coin.
    let paid = r#""currency":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913","minimumAmount":"4878026""#;
    let coin = ORDERS[0].1.replace(paid, r#""currency":"0x0000000000000000000000000000000000000000","minimumAmount":"4878026""#);
    assert_ne!(coin, ORDERS[0].1);
    let other = read(&coin).unwrap();
    assert_eq!(other.minimum_dollars(), None);
    assert_eq!(other.pays(), format!("at least 4878026 units of 0x0000000000000000000000000000000000000000 to {me} on base"));
    // An amount the prompt would read one way and the id's hash another is refused: marked digits, a hex, a number.
    for odd in ["\"minimumAmount\":\"4878026\u{200d}\"", "\"minimumAmount\":\"0x4a6e8a\"", "\"minimumAmount\":4878026", "\"minimumAmount\":\"\"",
                "\"minimumAmount\":\"٤٨٧٨٠٢٦\""] {
        let changed = ORDERS[0].1.replace(r#""minimumAmount":"4878026""#, odd);
        assert_ne!(changed, ORDERS[0].1);
        assert!(read(&changed).is_err(), "{odd}");
    }
    let input = ORDERS[0].1.replace(r#""amount":"5000000""#, "\"amount\":\"5000000\u{200d}\"");
    assert!(input != ORDERS[0].1 && read(&input).is_err());
}

#[test]
fn sponsored_transfers_are_built_by_the_signer() {
    let t = SponsoredTransfer { chain_id: 56, nonce: 3, gas_limit: 60_000, token: BNB_USDC.into(), to: "0x000000000000000000000000000000000000dEaD".into(),
                                amount: "20000000000000000000".into() };
    assert_eq!(t.refusal(), None);
    assert_eq!(t.summary(), "send 20 USDC on BNB Chain to 0x000000000000000000000000000000000000dEaD (BNB Chain pays the gas)");
    let (raw, hash) = t.signed(&[0x46; 32]).unwrap();
    assert!(raw[0] >= 0xc0 && hash.len() == 66);
    assert!(SponsoredTransfer { token: BASE_USDC.into(), ..t.clone() }.refusal().unwrap().contains("isn't a dollar BNB Chain pays the gas for"));
    assert_eq!(SponsoredTransfer { amount: "1.5".into(), ..t.clone() }.refusal().unwrap(), "The transfer's amount isn't a whole number of units.");
    assert_eq!(SponsoredTransfer { gas_limit: 100_001, ..t.clone() }.refusal().unwrap(), "A token transfer takes under 100,000 gas.");
    assert_eq!(SponsoredTransfer { to: "0x0000000000000000000000000000000000000000".into(), ..t }.refusal().unwrap(), "The transfer has no recipient.");
    let _ = h("00");
}
