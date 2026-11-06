//! Wallet.swift's offline cases: ExecutorTests (Kamino's «all», the SOL line, the spread's moves), EVMTests (Kamino's
//! first-deposit rent), KaminoBorrowTests (the rent beyond a SOL deposit), OneClickTests (1Click's words), V4LiveCheck
//! (where «buy NEAR» pays from), GuardTests (what a JupiterZ fill swaps) and CosignedTests (rent from the network, a swap
//! Jupiter didn't confirm, settled from the chain).

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use execution::{Entry, Kind};
use rust_decimal::Decimal;
use serde_json::Value;

use super::d;
use super::g2_fake::{TempJournal, executor, node, result};
use crate::Executor;

#[test]
fn kamino_withdraw_all_asks_for_tokens_not_dollars() {
    // 1.3 SOL worth $200 at ≈ $153.85: ask ≈ 1.313 SOL (1% over), not 202.
    let sol = Executor::kamino_all(d("200"), d("153.846153"));
    assert!(sol > d("1.312") && sol < d("1.314"), "{sol}");
    assert_eq!(Executor::kamino_all(d("100"), d("1")), d("101")); // a stablecoin: as before
}

#[test]
fn rent_is_read_from_the_simulation_lines() {
    let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(Executor::sol_spent(&lines(&["USDC: -1000000 (raw units)", "SOL: −0.029794 (fees and rent for new accounts)"])), d("0.029794"));
    assert_eq!(Executor::sol_spent(&lines(&["SOL: −1,250.5 (fees and rent for new accounts)"])), d("1250.5"));
    assert_eq!(Executor::sol_spent(&lines(&["SOL: 0.001479 (fees and rent for new accounts)"])), Decimal::ZERO); // SOL came back
    assert_eq!(Executor::sol_spent(&[]), Decimal::ZERO);
    // A first 0.1 SOL deposit's simulation (2026-11-05): 0.13837 SOL left the wallet, 0.03837 of it rent and fee.
    assert_eq!(Executor::kamino_rent_spent(&lines(&["SOL: -0.13837 (fee and rent for new accounts)"]), d("0.1")), d("0.03837"));
    assert_eq!(Executor::kamino_rent_spent(&lines(&["USDC: -1000000 (raw units)", "SOL: -0.0298 (fee and rent for new accounts)"]), Decimal::ZERO), d("0.0298"));
}

#[test]
fn a_spread_takes_idle_usdc_where_it_is_and_pulls_only_what_solana_lacks() {
    let idle = |l: &[(&str, &str)]| l.iter().map(|(k, v)| (k.to_string(), d(v))).collect::<Vec<_>>();
    // The live case: Morpho on Base from Base's own USDC, Jupiter Lend from Solana's 17.77 plus the rest pulled over.
    let pulls = Executor::pull_to_solana(&[(None, d("1493.52")), (Some("base".into()), d("1494.52"))], &idle(&[("base", "2960"), ("robinhood", "10.27")]),
                                         d("17.77"));
    assert_eq!(pulls, vec![("base".to_string(), d("1465.48")), ("robinhood".to_string(), d("10.27"))]);
    // Enough on Solana already: nothing moves; an EVM slice alone never pulls its own network's money away.
    assert!(Executor::pull_to_solana(&[(None, d("10"))], &idle(&[("base", "100")]), d("20")).is_empty());
    assert!(Executor::pull_to_solana(&[(Some("base".into()), d("80"))], &idle(&[("base", "100")]), Decimal::ZERO).is_empty());
    // Short by a few cents: under a bridge's half dollar, so no bridge; the slice takes what is there.
    assert!(Executor::pull_to_solana(&[(None, d("20.2"))], &idle(&[("base", "100")]), d("20")).is_empty());
}

#[test]
fn a_spread_moves_money_straight_from_where_it_lies() {
    // The live case: 18,587 USDC on Ethereum, 1 on Base, none on Solana; Jupiter Lend 500 and Morpho on Base 500. Base's
    // gap comes straight from Ethereum, not Ethereum → Solana → Base.
    let (moves, drawn) = Executor::spread_moves(&[(None, d("500")), (Some("base".into()), d("500"))],
                                                &[("base".into(), d("1")), ("ethereum".into(), d("18587.73"))], Decimal::ZERO);
    assert_eq!(moves.iter().map(|m| (m.0.as_str(), m.1.as_str())).collect::<Vec<_>>(), [("ethereum", "base"), ("ethereum", "solana")]);
    assert_eq!(moves.iter().map(|m| m.2).collect::<Vec<_>>(), [d("499.1"), d("500.1")]);
    assert_eq!(drawn.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["base", "ethereum"]);
    assert_eq!(drawn.last().unwrap().1, d("999.2"));
    // A slice on the network that holds the money moves nothing; Solana's leftover fills an EVM slice no network can.
    assert!(Executor::spread_moves(&[(Some("ethereum".into()), d("500"))], &[("ethereum".into(), d("1000"))], Decimal::ZERO).0.is_empty());
    let (from_solana, _) = Executor::spread_moves(&[(Some("base".into()), d("50"))], &[], d("80"));
    assert_eq!(from_solana, vec![("solana".to_string(), "base".to_string(), d("50"))]);
}

/// Kamino's first-deposit rent on a small deposit is asked about, not refused: over a tenth of the deposit stops with the
/// numbers and names Jupiter Lend; a big enough deposit, or the rent taken on with «Send anyway», goes.
#[test]
fn kamino_rent_on_a_small_deposit_is_asked_not_refused() {
    let (rent, sol) = (d("0.0298"), d("117"));
    let stop = Executor::kamino_rent(rent, sol, d("3"), Decimal::ZERO).expect("asked");
    assert!(stop.lost == rent * sol && stop.paid == d("3"));
    let said = stop.description();
    assert!(said.starts_with("The first Kamino deposit creates your accounts: rent ≈ 0.0298 SOL (≈ $3.49) on a $3.00 deposit"), "{said}");
    assert!(said.contains("Jupiter Lend has no rent") && said.ends_with("Nothing was signed."), "{said}");
    assert_eq!(Executor::kamino_rent(rent, sol, d("50"), Decimal::ZERO), None);
    assert_eq!(Executor::kamino_rent(rent, sol, d("3"), stop.lost * d("1.25") + d("0.05")), None);
}

#[test]
fn one_click_states_read_in_words() {
    for state in ["PENDING_DEPOSIT", "KNOWN_DEPOSIT_TX", "INCOMPLETE_DEPOSIT", "PROCESSING", "SUCCESS", "REFUNDED", "FAILED"] {
        assert!(!Executor::one_click_word(state).unwrap_or("").is_empty(), "{state}");
    }
}

/// «buy NEAR» pays where the USDC is: Solana when it holds the amount, else Base or Arbitrum before Ethereum, never a
/// network 1Click doesn't take USDC from (Polygon, Robinhood, Tempo).
#[test]
fn buy_near_pays_from_solana_else_a_network_1click_takes() {
    let held = |k: &str| match k { "base" => Some(d("50")), "ethereum" => Some(d("500")), "polygon" | "robinhood" => Some(d("900")), _ => None };
    assert!(Executor::near_source(d("10"), d("10"), held).is_none());
    assert_eq!(Executor::near_source(d("10"), d("2"), held).map(|n| n.key), Some("base"));
    assert_eq!(Executor::near_source(d("100"), d("2"), held).map(|n| n.key), Some("ethereum"));
    assert!(Executor::near_source(d("600"), d("2"), held).is_none()); // only Polygon and Robinhood hold it
}

/// A JupiterZ fill of USDC for Ondo's NVDA, USDC's mint named by a lookup table: the app's own check of what it swaps
/// proves it by the taker's own account for it.
#[test]
fn a_jupiter_z_fill_swaps_only_what_was_asked() {
    let (usdc, nvda, usdt) = ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "gEGtLTPNQ7jcg25zTetkbmF7teoDLcrfTnQfmn2ondo", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");
    let wire = STANDARD.decode("AgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAIABhCWmpufqjVYZfnyuFyDIYJfGWS2mAfjxKCUSfgP6ocw/xJPu1CG1eT0myNTijJn+XIY+eWIRKQHl7jC8aQ1uVa2CeMxq5qhwAgmIW0MF4dLMwkxIHrBvr5c08/crA4Q6T4KDKkOmBBVt2cJe1e58QIGFNB0ELCh8y9LyeeJjoR4CihDMtHUfShf/Oa+osQ679KsykqKYBb4lOUWiWQGOHeiS8S88QqamOrvVmZOo1uOpWQDtUp3ZTx3MrWCN4rX5LRfSm4AXB93lixCTAA65IyZL1CfDcROhO+FoE99e2FdSYSAR+QmNK7fLerYNxa0XtOC1dfBbrPRu3kH56B66UwkzRhwOQPjN/vIpl7Xium7Iy3jagX5rfmT/XPNgYLV8+5Y/FEh+eXX45qjIynXZgTkY7cIE4pMYcB9TX1GrxEDaAMGRm/lIRcy/+ytunLDm+e8jOW7xfcSayxDmzpAAAAABMb8IPBQzPBVhNchHJ+M9Z7BR4W7FmoeKDDoEiAAAAAH8GPXUx0J5M4QhSMGkyrDXnCjhql+ffCiasaQs5lcKgr+HsLLby6mG8NIKPJ3FF1seUNQjRv5tY9hZNnovtSxSlhJ+3Kju+kf3FsOalf2PFoctFsgZ6btDKzTY5XIoQKMlyWPTiSJ8bs9ECkUjg2DC1oTmdr/EIQEjnvY2+n4WWEZw4aLgkz9+/ZC/96Me/cEcyrZNeKZHt0ZqF4FpEKRCAoACQMtDgAAAAAAAAoABQLAJwkADQ4AFRglEBkfFiEeHA8aGxBnE04Y8AmHP8DGLQAAAAAACwCBAQEgAAMMAANhACAAA/8tBPYALBDrpwKtr+L0PKxZlsR3sxtpnTz2K4pXv/Z97/PhqjZLzNF7QixOsnC+EoqocPwaNqtDy+2GtaTvWO+DHOYtxTEvldatk/POSSeVTe99NgEs4WarmsuZQRz+8QNnk3bNdj6uHSFTC6z1d1yZ2brP4QwZAAMiBwQCBggmHRoPHCQSFyUYERMjFBsgDDCAHIWtR465zhub09K8oEa6oMM1Kc3sCEPb9kYiMgAAAIRh1AAAAAAAtk7iagAAAAAPBgEJAQMaHQEBDgsBAAUYCQYlHAMdGiWoYLejXAoooMDGLQAAAAAAhGHUAAAAAAC0TuJqAAAAAAoAAAAADQ4AFRglEBkfFiEeHA8aGxDVL5mJVPNe6MDGLQAAAAAAATvS46ffe5EJgS8syMCyEUfC+/dSXfa1UwnJq4WTjUwjCg4KFh0UDBEXHBANBQQGBwEPFQ0SEwsJGw==").unwrap();
    assert!(Executor::fill_swaps(&wire, usdc, nvda) && !Executor::fill_swaps(&wire, nvda, usdc));
    assert!(!Executor::fill_swaps(&wire, usdt, nvda));
    // The low-SOL path takes a gasless order without comparing prices: a JupiterZ fill still has to swap what was asked.
    assert!(Executor::sponsorable("jupiterz", &wire, usdc, nvda));
    assert!(!Executor::sponsorable("jupiterz", &wire, usdt, nvda));
    assert!(Executor::sponsorable("metis", &wire, usdt, nvda));
    assert!(!Executor::fill_swaps(&[1, 2, 3], usdc, nvda));
}

// MARK: The chain's word (CosignedTests)

const OURS: &str = "OurSecondSignature";
const SPONSOR: &str = "SponsorsFirstSignature";
const WALLET: &str = "5PkKzEWa11etTestTestTestTestTestTestTestZhb1";
/// The slot of the simulation before signing.
const SIMULATED: u64 = 100;

/// CosignedNode.State: what the stand-in node answers.
#[derive(Clone, Default)]
struct Chain {
    blockhash_valid: bool,
    list: Vec<(&'static str, u64)>,
    transactions: Vec<(&'static str, Vec<&'static str>, Option<&'static str>)>,
    silent: bool,
    rent_rate: u64,
}

async fn chain_node(c: Chain) -> String {
    node(move |method, params| {
        if c.silent { return (400, String::new()) }
        let least = params.last().and_then(|p| p.get("minContextSlot")).and_then(Value::as_u64).unwrap_or(0);
        let behind = (200, r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32016,"message":"Minimum context slot has not been reached"}}"#.to_string());
        match method {
            "isBlockhashValid" if least > 130 => behind,
            "isBlockhashValid" => result(&format!(r#"{{"context":{{"slot":130}},"value":{}}}"#, c.blockhash_valid)),
            "getSignaturesForAddress" if least > 130 => behind,
            "getSignaturesForAddress" => result(&format!("[{}]", c.list.iter().map(|(s, slot)| format!(
                r#"{{"signature":"{s}","slot":{slot},"err":null,"confirmationStatus":"confirmed","blockTime":1790000000}}"#)).collect::<Vec<_>>().join(","))),
            "getTransaction" => {
                let id = params.first().and_then(Value::as_str).unwrap_or("");
                match c.transactions.iter().find(|t| t.0 == id) {
                    Some((_, signatures, err)) => result(&format!(r#"{{"slot":125,"transaction":{{"signatures":[{}],"message":{{}}}},"meta":{{"err":{}}}}}"#,
                                                                  signatures.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(","), err.unwrap_or("null"))),
                    None => result("null"),
                }
            }
            "getMinimumBalanceForRentExemption" => result(&((128 + params.first().and_then(Value::as_u64).unwrap_or(0)) * c.rent_rate).to_string()),
            _ => result("null"),
        }
    }).await
}

fn landed(err: Option<&'static str>) -> Chain {
    Chain { blockhash_valid: true, list: vec![(SPONSOR, 125), ("older", 90)], transactions: vec![(SPONSOR, vec![WALLET, OURS], err)], ..Default::default() }
}

/// Gone: the blockhash is past use at slot 130, and the wallet's transactions since the signing hold none with ours.
fn gone() -> Chain {
    Chain { list: vec![("someoneElses", 125), ("older", 90)], transactions: vec![("someoneElses", vec![WALLET], None)], ..Default::default() }
}

/// Rent is the network's word, never a constant: Kamino's first-deposit rent, measured at 5,080 lamports a byte, scales
/// with it; a silent node leaves the measure.
#[tokio::test]
async fn kamino_first_rent_is_read_from_the_network() {
    let url = chain_node(Chain { rent_rate: 2_575, ..Default::default() }).await;
    let kamino = Executor::kamino_first_rent(&net::solana_rpc::SolanaRpc::pinned(&url)).await;
    assert!((kamino - d("0.0151")).abs() < d("0.0001"), "{kamino}");
    let silent = chain_node(Chain { silent: true, ..Default::default() }).await;
    assert_eq!(Executor::kamino_first_rent(&net::solana_rpc::SolanaRpc::pinned(&silent)).await, d("0.0298"));
}

/// settle_sponsored against a node: the journal after it and what it said.
async fn settle(c: Chain) -> (Vec<Entry>, Vec<Entry>, Vec<Entry>, Option<String>) {
    let journal = TempJournal::new("cosigned");
    journal.journal().append(&Entry::new("solana", OURS, Kind::Signed,
                                         "Jupiter USDC→SOL, fee paid by JupiterZ's market maker; Jupiter order r1, valid until block 0")).unwrap();
    let url = chain_node(c).await;
    let e = executor(WALLET, Some(&url), &journal);
    let said = match e.settle_sponsored(OURS, "hash", SIMULATED, "The request timed out.", Duration::from_secs(1)).await {
        Ok(()) => None,
        Err(error) => {
            // The swap's caller takes the pools' route after these two: a fill that went out would be swapped a second time.
            let text = error.to_string();
            assert!(!(text.starts_with("The guard stopped") || text.starts_with("The simulation failed")), "{text}");
            assert!(!matches!(error, crate::Error::Guard(_)));
            Some(text)
        }
    };
    let j = journal.journal();
    (j.entries().unwrap(), j.unresolved().unwrap(), j.went_through().unwrap(), said)
}

#[tokio::test]
async fn settle_sponsored_found_goes_on_as_the_swap() {
    let (entries, unresolved, went, error) = settle(landed(None)).await;
    assert!(error.is_none() && unresolved.is_empty(), "{error:?}");
    assert!(entries.last().unwrap().kind == Kind::Confirmed && entries.last().unwrap().note == format!("transaction {SPONSOR}"));
    // What went through links the sponsor's signature, the one an explorer knows.
    assert_eq!(went.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), [SPONSOR]);
}

#[tokio::test]
async fn settle_sponsored_gone_says_nothing_moved() {
    let (entries, unresolved, _, error) = settle(gone()).await;
    assert!(entries.last().unwrap().kind == Kind::Expired && unresolved.is_empty());
    assert!(error.unwrap().contains("can't land: nothing moved"));
}

#[tokio::test]
async fn settle_sponsored_failed_on_chain_is_failed() {
    let (entries, _, _, error) = settle(landed(Some(r#"{"InstructionError":[2,{"Custom":6001}]}"#))).await;
    assert!(entries.last().unwrap().kind == Kind::Failed && entries.last().unwrap().note.contains("6001"));
    assert!(error.unwrap().contains("reached the chain and failed"));
}

/// The node answers nothing: the entry stays «signed», so the chat says «sent, status unknown» with the wallet's page to
/// check — never «failed» on a guess.
#[tokio::test]
async fn settle_sponsored_silent_chain_stays_signed() {
    let (entries, unresolved, _, error) = settle(Chain { silent: true, ..Default::default() }).await;
    assert!(entries.len() == 1 && entries.last().unwrap().kind == Kind::Signed);
    assert_eq!(unresolved.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), [OURS]);
    assert!(error.unwrap().contains(&format!("solscan.io/account/{WALLET}")));
    assert!(entries.iter().all(|e| e.kind != Kind::Failed));
}

/// sign_and_send's checks before a signature, on a dry run: the guard, the balances before in one read, the simulation
/// held to the rules — here a token account's close, its rent back to the wallet — and the change lines it returns.
#[tokio::test]
async fn a_dry_run_is_guarded_and_simulated_and_signs_nothing() {
    let wallet = "2EUrWmf5xMmWER9BtDbXbGbZjoL7R3eTDMXYR6H6cKPj";
    let account = solana::spl::associated_account(wallet, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", solana::spl::TOKEN_PROGRAM).unwrap();
    let url = node(|method, _| match method {
        "getMultipleAccounts" => result(r#"{"context":{"slot":300},"value":[{"lamports":1000000000,"owner":"11111111111111111111111111111111","data":["","base64"]}]}"#),
        "simulateTransaction" => result(r#"{"context":{"slot":301},"value":{"err":null,"logs":[],"accounts":[{"lamports":1002034280,"owner":"11111111111111111111111111111111","data":["","base64"]}]}}"#),
        _ => result("null"),
    }).await;
    let journal = TempJournal::new("dry");
    let e = executor(wallet, Some(&url), &journal);
    let close = solana::Instruction {
        program: solana::spl::TOKEN_PROGRAM.into(), accounts: vec![(account.clone(), false, true), (wallet.into(), false, true), (wallet.into(), true, false)], data: vec![9],
    };
    let blockhash = wallet::keys::base58::encode(&[1; 32]);
    let wire = solana::unsigned_transaction(&solana::compile_message(&[close], wallet, &blockhash, 20_000).unwrap());
    let owners = std::collections::BTreeMap::from([(account.clone(), wallet.to_string())]);
    let none = std::collections::HashMap::new();
    let changes = e.sign_and_send(&wire, 9, &owners, &[], "close it", 1_000_000, &none, true, None).await.unwrap();
    assert_eq!(changes, ["SOL: 0.002034 (rent back from closed accounts)"]);
    // Less back than the rule asks: the guard stops it, in its words, before any signer.
    let stopped = e.sign_and_send(&wire, 9, &owners, &[], "close it", 3_000_000, &none, true, None).await.unwrap_err();
    assert_eq!(stopped, crate::Error::Guard("The guard stopped close it: Simulation: lamports changes by 2034280; the minimum allowed is 3000000.".into()));
    // Money to someone not approved: the guard stops it before the simulation.
    let thief = wallet::keys::base58::encode(&[7; 32]);
    let steal = solana::Instruction {
        program: solana::spl::TOKEN_PROGRAM.into(), accounts: vec![(account.clone(), false, true), (thief, false, true), (wallet.into(), true, false)], data: vec![9],
    };
    let wire = solana::unsigned_transaction(&solana::compile_message(&[steal], wallet, &blockhash, 20_000).unwrap());
    let refused = e.sign_and_send(&wire, 9, &owners, &[], "close it", 0, &none, true, None).await.unwrap_err();
    assert!(matches!(&refused, crate::Error::Guard(m) if m.starts_with("The guard stopped close it: ")), "{refused:?}");
    assert!(journal.journal().entries().unwrap().is_empty());
}
