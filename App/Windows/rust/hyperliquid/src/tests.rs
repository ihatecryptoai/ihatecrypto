//! Vectors from hyperliquid-python-sdk tests/signing_test.py and 0.24.0 with the same key, as in WalletTests.swift.

use super::*;
use wallet::to_hex;

fn key() -> Vec<u8> { from_hex("0x0123456789012345678901234567890123456789012345678901234567890123").unwrap() }

/// The SDK prints r and s as integers, so leading zeros are dropped; compare numerically.
fn same(sig: &EvmSignature, r: &str, s: &str, v: i64) -> bool {
    let trim = |h: &str| h.trim_start_matches('0').to_string();
    trim(&to_hex(&sig.r)) == trim(&r[2..]) && trim(&to_hex(&sig.s)) == trim(&s[2..]) && sig.v == v
}

fn order(kind: OrderKind, cloid: Option<&str>) -> MsgPack {
    action::order(&[Order { asset: 1, is_buy: true, price: dec("100"), size: dec("100"), reduce_only: false, kind, cloid: cloid.map(str::to_string) }], "na").unwrap()
}

fn gtc() -> OrderKind { OrderKind::Limit { tif: "Gtc".into() } }

#[test]
fn phantom_agent_matches_production() {
    let a = action::order(&[Order { asset: 4, is_buy: true, price: dec("1670.1"), size: dec("0.0147"), reduce_only: false,
                                    kind: OrderKind::Limit { tif: "Ioc".into() }, cloid: None }], "na").unwrap();
    assert_eq!(to_hex(&signer::action_hash(&a, None, 1677777606040, None).unwrap()), "0fcbeda5ae3c4950a548021552a4fea2226858c4453571bf3f24ba017eac2908");
}

#[test]
fn l1_signing_matches_sdk() {
    let dummy = MsgPack::map(vec![("type", MsgPack::str("dummy")), ("num", MsgPack::Uint(100_000_000_000))]);
    assert!(same(&signer::sign_l1(&dummy, &key(), None, 0, None, true).unwrap(),
                 "0x53749d5b30552aeb2fca34b530185976545bb22d0b3ce6f62e31be961a59298", "0x755c40ba9bf05223521753995abb2f73ab3229be8ec921f350cb447e384d8ed8", 27));
    assert!(same(&signer::sign_l1(&dummy, &key(), None, 0, None, false).unwrap(),
                 "0x542af61ef1f429707e3c76c5293c80d01f74ef853e34b76efffcb57e574f9510", "0x17b8b32f086e8cdede991f1e2c529f5dd5297cbe8128500e00cbaf766204a613", 28));
    let vault = "0x1719884eb866cb12b2287399b15f7db5e7d775ea";
    assert!(same(&signer::sign_l1(&dummy, &key(), Some(vault), 0, None, true).unwrap(),
                 "0x3c548db75e479f8012acf3000ca3a6b05606bc2ec0c29c50c515066a326239", "0x4d402be7396ce74fbba3795769cda45aec00dc3125a984f2a9f23177b190da2c", 28));
}

#[test]
fn order_signing_matches_sdk() {
    let limit = order(gtc(), None);
    assert!(same(&signer::sign_l1(&limit, &key(), None, 0, None, true).unwrap(),
                 "0xd65369825a9df5d80099e513cce430311d7d26ddf477f5b3a33d2806b100d78e", "0x2b54116ff64054968aa237c20ca9ff68000f977c93289157748a3162b6ea940e", 28));
    assert!(same(&signer::sign_l1(&limit, &key(), None, 0, None, false).unwrap(),
                 "0x82b2ba28e76b3d761093aaded1b1cdad4960b3af30212b343fb2e6cdfa4e3d54", "0x6b53878fc99d26047f4d7e8c90eb98955a109f44209163f52d8dc4278cbbd9f5", 27));
    let with_cloid = order(gtc(), Some("0x00000000000000000000000000000001"));
    assert!(same(&signer::sign_l1(&with_cloid, &key(), None, 0, None, true).unwrap(),
                 "0x41ae18e8239a56cacbc5dad94d45d0b747e5da11ad564077fcac71277a946e3", "0x3c61f667e747404fe7eea8f90ab0e76cc12ce60270438b2058324681a00116da", 27));
    let stop = order(OrderKind::Trigger { price: dec("103"), is_market: true, tpsl: "sl".into() }, None);
    assert!(same(&signer::sign_l1(&stop, &key(), None, 0, None, true).unwrap(),
                 "0x98343f2b5ae8e26bb2587daad3863bc70d8792b09af1841b6fdd530a2065a3f9", "0x6b5bb6bb0633b710aa22b721dd9dee6d083646a5f8e581a20b545be6c1feb405", 27));
    assert!(same(&signer::sign_l1(&stop, &key(), None, 0, None, false).unwrap(),
                 "0x971c554d917c44e0e1b6cc45d8f9404f32172a9d3b3566262347d0302896a2e4", "0x206257b104788f80450f8e786c329daa589aa0b32ba96948201ae556d5637eac", 28));
    assert!(action::order(&[Order { asset: 1, is_buy: true, price: dec("1"), size: dec("1"), reduce_only: false, kind: gtc(),
                                     cloid: Some("0x01".into()) }], "na").is_err());
}

#[test]
fn user_signed_transfer_matches_sdk() {
    let sig = signer::sign_user_action("UsdSend", vec![
        Field::new("destination", "string", Value::String("0x5e9ee1089755c3435139848e47e6635505d5a13a".into())),
        Field::new("amount", "string", Value::String("1".into())),
        Field::new("time", "uint64", Value::Uint(1_687_816_341_423)),
    ], &key(), false).unwrap();
    assert!(same(&sig, "0x637b37dd731507cdd24f46532ca8ba6eec616952c56218baeff04144e4a77073", "0x11a6a24900e6e314136d2592e2f8d502cd89b7c15b198e1bee043c9589f9fad7", 27));
    // The same through the action a signer in another process reads.
    let action = signer::usd_send_action("0x5e9ee1089755c3435139848e47e6635505d5a13a", "1", 1_687_816_341_423, false);
    let (ty, fields) = signer::user_action(&action).unwrap();
    assert!(same(&signer::sign_user_action(&ty, fields, &key(), false).unwrap(),
                 "0x637b37dd731507cdd24f46532ca8ba6eec616952c56218baeff04144e4a77073", "0x11a6a24900e6e314136d2592e2f8d502cd89b7c15b198e1bee043c9589f9fad7", 27));
    assert_eq!(signer::user_action(&MsgPack::map(vec![("type", MsgPack::str("vaultTransfer"))])).unwrap_err().0,
               "Signing this Hyperliquid action isn't supported.");
    assert_eq!(signer::user_action(&MsgPack::map(vec![("type", MsgPack::str("usdSend"))])).unwrap_err().0,
               "The action has no destination field of type string.");
}

#[test]
fn builder_fee_and_referral_match_sdk() {
    let paid = action::with_builder(&order(gtc(), None));
    assert!(same(&signer::sign_l1(&paid, &key(), None, 0, None, true).unwrap(),
                 "0x5e8c92ae5a6121cb3b525595e2901fd2de28827ae13c17f94be82a0cebf62db2", "0x5f0352b6529630059d7ff11bc9ee18e537cd3f0051c3567e23a558add870bccd", 28));
    assert!(same(&signer::sign_l1(&paid, &key(), None, 0, None, false).unwrap(),
                 "0x43aae64cf52cd48e2afd5e0398a653c359075622eb49bd0483449e7a563f4367", "0xddc237f4a723ac7127115bb2f4fef2e90b0f2142ec60dc2e835872cb7bf24c4", 27));
    assert_eq!(action::with_builder(&paid), paid);
    assert_eq!(action::with_builder(&action::cancel(&[(1, 2)])), action::cancel(&[(1, 2)]));

    assert!(same(&signer::sign_l1(&builder::referrer_action(), &key(), None, 0, None, true).unwrap(),
                 "0x3ecf6d083e20b2c32e7229b1b71d95514cb21638294db542908c856800193a28", "0x5b581d2ede67292edf94d686e49195e8a17c9878ee165d2a2391ab9f481e7c6c", 27));
    assert!(signer::is_trading(&builder::referrer_action()));

    let approval = |rate: &str| {
        let (ty, fields) = signer::user_action(&builder::approve_action(rate, 1_792_000_000_000, true)).unwrap();
        signer::sign_user_action(&ty, fields, &key(), true).unwrap()
    };
    assert!(same(&approval(builder::RATE), "0x9d626107e73eb1083567f12193d488e017b0d88e9c80b0bc7f9852523f7429e1",
                 "0x3002a3c1e7643fb43d7226afe2db284b24a2f9688eee279917b36a3befafa07c", 28));
    assert!(same(&approval(builder::REVOKE), "0x5eece18dcf83f4239b91b4d7da79940347502c4a28c0af61ae736b1987047185",
                 "0x255505b9ddc87a5d9e76a903f129db73df869a9888ab23ec0473c2b64494c9b", 27));
    assert_eq!(builder::share(), dec("0.0005"));
}

#[test]
fn wire_numbers_match_sdk_format() {
    assert_eq!(action::number(dec("100")).unwrap(), "100");
    assert_eq!(action::number(dec("100.000")).unwrap(), "100");
    assert_eq!(action::number(dec("1670.1")).unwrap(), "1670.1");
    assert_eq!(action::number(dec("0.0147")).unwrap(), "0.0147");
    assert_eq!(action::number(dec("0.00001231")).unwrap(), "0.00001231");
    assert_eq!(action::number(dec("0")).unwrap(), "0");
    assert_eq!(action::number(dec("0.000012312")).unwrap_err().0, "0.000012312: more than 8 decimal places.");
}

#[test]
fn prices_follow_hyperliquid_tick_rules() {
    assert_eq!(action::price(dec("2712.1349"), 4, false), dec("2712.1"));
    assert_eq!(action::price(dec("84489.7"), 5, false), dec("84490"));
    assert_eq!(action::price(dec("0.0123456789"), 0, false), dec("0.012346"));
    assert_eq!(action::size(dec("0.00419999"), 4), dec("0.0041"));
    // Swift keeps whole numbers past 5 figures (max(0, 4 − magnitude)): integer prices are always valid.
    assert_eq!(action::price(dec("123456.7"), 0, false), dec("123457"));
}

/// Actions cross the pipe as JSON; the signer must hash exactly the bytes the engine built.
#[test]
fn msgpack_survives_json_with_key_order() {
    let (action, _) = signer::send_asset(&format!("0x{}", "ab".repeat(20)), "", "spot", "USDC:0x6d1e7cde53ba9467b783cb7c530ce054",
                                         "1.5", 42, &[1; 32], false).unwrap();
    let json = serde_json::to_string(&action).unwrap();
    let back: MsgPack = serde_json::from_str(&json).unwrap();
    assert_eq!(back.packed(), action.packed());
    // Swift's Codable form.
    assert_eq!(serde_json::to_string(&MsgPack::map(vec![("a", MsgPack::Uint(1)), ("b", MsgPack::Null)])).unwrap(),
               r#"{"kind":"map","keys":["a","b"],"values":[{"kind":"uint","uint":1},{"kind":"null"}]}"#);
}

#[test]
fn packs_like_python_msgpack() {
    assert_eq!(MsgPack::Uint(127).packed(), [0x7f]);
    assert_eq!(MsgPack::Uint(128).packed(), [0xcc, 0x80]);
    assert_eq!(MsgPack::Uint(65536).packed(), [0xce, 0, 1, 0, 0]);
    assert_eq!(MsgPack::Uint(u64::MAX).packed()[0], 0xcf);
    assert_eq!(MsgPack::String("x".repeat(32)).packed()[..2], [0xd9, 32]);
    assert_eq!(MsgPack::Array(vec![MsgPack::Null; 16]).packed()[..3], [0xdc, 0, 16]);
}

#[test]
fn reduce_only_and_trading_kinds() {
    let close = action::order(&[Order { asset: 1, is_buy: false, price: dec("1"), size: dec("1"), reduce_only: true, kind: gtc(), cloid: None }], "na").unwrap();
    assert!(action::only_reduces(&close));
    assert!(!action::only_reduces(&order(gtc(), None)));
    assert!(action::only_reduces(&action::cancel(&[(1, 2)])));
    assert!(!action::only_reduces(&action::order(&[], "na").unwrap()));
    assert!(!signer::is_trading(&signer::usd_send_action("0x00", "1", 1, true)));
    let trail = action::trailing_stop(3, false, dec("0.5"), dec("2.5")).unwrap();
    assert!(action::only_reduces(&trail) && signer::is_trading(&trail));
    assert_eq!(trail.get("retracement").unwrap().text("pct"), Some("2.5%"));
    assert_eq!(action::with_builder(&trail), trail);
    assert!(!action::only_reduces(&action::twap_order(3, true, dec("1"), false, 30, true).unwrap()));
    assert!(action::only_reduces(&action::twap_order(3, false, dec("1"), true, -5, false).unwrap()));
    assert_eq!(action::twap_order(3, true, dec("1"), false, -5, false).unwrap().get("twap").unwrap().get("m"), Some(&MsgPack::Uint(0)));
    assert!(action::only_reduces(&action::twap_cancel(3, 9)));
    let body = exchange_body(&close, 7, &EvmSignature { r: vec![0; 32], s: vec![1; 32], v: 27 }, None);
    assert_eq!(body["vaultAddress"], Json::Null);
    assert_eq!(body["action"]["orders"][0]["r"], Json::Bool(true));
}
