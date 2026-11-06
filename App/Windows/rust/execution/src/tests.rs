//! EnvelopeTests.swift and JournalTests.swift, with their cases.

use std::str::FromStr;

use super::*;

fn d(s: &str) -> Decimal { Decimal::from_str(s).unwrap() }
fn now() -> DateTime<Utc> { Utc.timestamp_opt(1_800_000_000, 0).unwrap() }

fn journal() -> (Journal, PathBuf) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let path = std::env::temp_dir().join(format!("journal-{}-{}.jsonl", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_file(&path);
    (Journal::new(&path), path)
}

#[test]
fn live_values_inside_the_envelope_pass() {
    let e = Envelope::approve_default(d("0.13"), d("14.85"), now());
    assert_eq!(e.min_output, d("14.7015"));
    assert!(e.violations(d("0.13"), d("14.80"), None, now() + Duration::seconds(30)).is_empty());
}

#[test]
fn anything_outside_the_envelope_stops_the_route() {
    let mut e = Envelope::approve_default(d("0.13"), d("14.85"), now());
    assert_eq!(e.violations(d("0.13"), d("14.5"), None, now()), ["14.5 would arrive, but at least 14.7015 was approved."]);
    assert_eq!(e.violations(d("0.14"), d("14.85"), None, now()), ["Needs to send 0.14, but at most 0.13 was approved."]);
    assert_eq!(e.violations(d("0.13"), d("14.85"), None, now() + Duration::seconds(601)), ["The confirmation expired."]);
    e.destination = Some("DepositA".into());
    assert_eq!(e.violations(d("0.13"), d("14.85"), Some("DepositB"), now()).len(), 1);
    assert!(e.violations(d("0.13"), d("14.85"), Some("DepositA"), now()).is_empty());
}

#[test]
fn an_envelope_reads_what_swift_wrote() {
    // Swift's JSONEncoder: Decimal as a number, Date as seconds since 2001.
    let e: Envelope = serde_json::from_str(r#"{"maxInput":0.13,"minOutput":14.7015,"expires":821692800}"#).unwrap();
    assert_eq!((e.max_input, e.min_output, e.expires), (d("0.13"), d("14.7015"), now()));
    let back: Envelope = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
    assert_eq!(back, e);
}

#[test]
fn the_journal_keeps_order_and_finds_unresolved_sends() {
    let (j, path) = journal();
    j.append(&Entry::new("solana", "sigA", Kind::Signed, "swap")).unwrap();
    j.append(&Entry::new("solana", "sigB", Kind::Signed, "bridge deposit")).unwrap();
    j.append(&Entry::new("solana", "sigA", Kind::Confirmed, "")).unwrap();
    j.append(&Entry::new("1click", "depositX", Kind::Signed, "")).unwrap();
    assert_eq!(j.entries().unwrap().len(), 4);
    assert_eq!(j.unresolved().unwrap().iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["sigB", "depositX"]);
    #[cfg(unix)]
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&path).unwrap().permissions()) & 0o777, 0o600);
    // A line as Swift's `.iso8601` writes it reads back.
    let line = std::fs::read_to_string(&path).unwrap();
    assert!(line.lines().next().unwrap().contains(r#""kind":"signed""#));
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_send_written_now_is_found_from_the_second_it_started() {
    // The journal keeps whole seconds, so the start is floored the same way or a send in that second is missed.
    let (j, path) = journal();
    let started = Utc::now();
    j.append(&Entry::new("base", "0xabc", Kind::Signed, "Relay deposit")).unwrap();
    let from = Utc.timestamp_opt(started.timestamp(), 0).unwrap();
    assert_eq!(j.unresolved().unwrap().iter().filter(|e| e.at >= from).map(|e| e.id.as_str()).collect::<Vec<_>>(), ["0xabc"]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_move_that_went_through_is_found_but_its_steps_that_move_no_money_are_not() {
    let (j, path) = journal();
    use Kind::*;
    let steps = [
        ("solana", "swapSig", Signed, "Jupiter 3 USDC→USDT"), ("solana", "swapSig", Confirmed, ""),
        ("base", "0xapprove", Signed, "Approve exactly 3 USDC to Uniswap"), ("base", "0xapprove", Confirmed, ""),
        ("base", "0xbatch", Signed, "Approve exactly 3 USDC to Uniswap + Swap 3 USDC for WETH"), ("base", "0xbatch", Confirmed, ""),
        ("hyperliquid", "1", Signed, "referral code IHATECRYPTO"), ("hyperliquid", "1", Confirmed, "ok"),
        ("hyperliquid", "2", Signed, "leverage"), ("hyperliquid", "2", Confirmed, "ok"),
        ("hyperliquid", "3", Signed, "approve ihatecrypto's fee 0.05%"), ("hyperliquid", "3", Confirmed, "approve ihatecrypto's fee 0.05%: ok"),
        ("relay", "req", Signed, "Base USDC→Solana 3, tx 0xbatch"), ("relay", "req", Settled, "success"),
        ("solana", "failedSig", Signed, "send 1 USDC"), ("solana", "failedSig", Failed, "custom program error"),
        ("base", "0xpermit", Signed, "Permit2: let Uniswap's router take exactly 3 USDC, for 30 minutes"), ("base", "0xpermit", Confirmed, ""),
        ("arbitrum", "0xclear", Signed, "Clear the USDT0 bridge's old allowance"), ("arbitrum", "0xclear", Confirmed, ""),
        ("base", "0xrevoke", Signed, "Revoke 0x1234…abcd's approval of 0x5678…ef01 on Base"), ("base", "0xrevoke", Confirmed, ""),
        ("hyperliquid", "4", Signed, "cancel limit"), ("hyperliquid", "4", Confirmed, "ok"),
        ("hyperliquid", "5", Signed, "stop and take-profit"), ("hyperliquid", "5", Confirmed, "resting on the book"),
        ("hyperliquid", "6", Signed, "spot buy HYPE/USDC"), ("hyperliquid", "6", Confirmed, "filled 0.5 at 40"),
        ("hyperliquid", "7", Signed, "usdClassTransfer perp → spot 20"), ("hyperliquid", "7", Confirmed, "ok"),
        ("hyperliquid", "8", Signed, "HLP deposit"), ("hyperliquid", "8", Confirmed, "ok"),
        // A swap with no gas of ours: the journal keys it by our signature, the explorer knows it by the sponsor's.
        ("solana", "ourSig", Signed, "Jupiter 1 USDC→SOL; Jupiter order r1"), ("solana", "ourSig", Confirmed, "transaction sponsorSig"),
        ("solana", "pendingSig", Signed, "send 2 USDC"),
        ("near", "nearHash", Confirmed, "send 1 NEAR to Automation"),
    ];
    for (chain, id, kind, note) in steps { j.append(&Entry::new(chain, id, kind, note)).unwrap() }
    let went = j.went_through().unwrap();
    assert_eq!(went.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["swapSig", "0xbatch", "req", "6", "7", "8", "sponsorSig", "nearHash"]);
    assert_eq!(went.iter().find(|e| e.id == "req").unwrap().note, "Base USDC→Solana 3, tx 0xbatch");
    assert_eq!(went.iter().find(|e| e.id == "6").unwrap().note, "spot buy HYPE/USDC");
    assert_eq!(j.unresolved().unwrap().iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["pendingSig"]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn money_in_transit_is_the_unsettled_bridges_newest_first() {
    let (j, path) = journal();
    j.append(&Entry::new("relay", "old", Kind::Signed, "")).unwrap();
    j.append(&Entry::new("solana", "notABridge", Kind::Signed, "")).unwrap();
    j.append(&Entry::new("cctp", "burn", Kind::Signed, "")).unwrap();
    j.append(&Entry::new("1click", "done", Kind::Signed, "")).unwrap();
    j.append(&Entry::new("1click", "done", Kind::Settled, "")).unwrap();
    let ids = |v: Vec<Entry>| v.into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(ids(j.in_transit(86_400, Utc::now()).unwrap()), ["burn", "old"]);
    assert!(j.in_transit(86_400, Utc::now() + Duration::days(2)).unwrap().is_empty());
    let _ = std::fs::remove_file(path);
}
