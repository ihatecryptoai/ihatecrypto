//! Moves.swift's offline cases (Auto7Tests, Holds10Tests, Moves6Tests, Refusals8Tests, Sends9Tests, Engine11Tests):
//! in-process, the book, the multisigs and the wallet set by hand; nothing goes to the network or the signer. Each test
//! keeps its files in its own folder (`store::test_folder`).

use std::rc::Rc;
use std::sync::Arc;

use chrono::Utc;
use ihatecrypto_core::intent::TradeIntent;
use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use rust_decimal::Decimal;
use signer_api::{SignerAddress, SignerReply, SignerWallet};
use trade::executor::{Order, Position};

use super::Engine;
use crate::chains::AfterCall;
use crate::engine::{Card, Costly, Pending};
use crate::multisig::Multisig;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

// Public test data only: Hardhat's accounts, the abandon-about phrase's Solana address, a NEAR implicit account of zeros.
const ME: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const PHONE: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const MOM: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";
const NEWER: &str = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC";
const SOL: &str = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk";

/// An engine in this test's own folder, English, no state drawn (nothing here yields to the push).
async fn run(test: impl AsyncFnOnce(Rc<Engine>)) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("L1's tests draw no state"));
        e.st_mut().settings.language = Some("en".into());
        test(e).await
    }).await;
}

fn address(name: &str, network: &str, address: &str, memo: &str) -> SignerAddress {
    SignerAddress { name: name.into(), network: network.into(), address: address.into(), memo: memo.into() }
}

fn safe(address: &str, salt: u64, networks: &[&str]) -> Multisig {
    Multisig { address: address.into(), keys: vec![], threshold: 1, salt, networks: networks.iter().map(|n| n.to_string()).collect() }
}

fn wallet(id: &str, label: &str, kind: &str, solana: Option<&str>, evm: Option<&str>) -> SignerWallet {
    SignerWallet { id: id.into(), label: label.into(), kind: kind.into(), solana: solana.map(Into::into), evm: evm.map(Into::into), near: None, test: None, automation: None }
}

/// The wallet that's on: a phrase wallet, or a key for one chain only.
fn signer(wallets: Vec<SignerWallet>, active: &str) -> SignerReply {
    let on = wallets.iter().find(|w| w.id == active).cloned();
    SignerReply {
        ok: true, has_wallet: Some(true), solana_address: on.as_ref().and_then(|w| w.solana.clone()), evm_address: on.and_then(|w| w.evm),
        wallets: Some(wallets), active_wallet: Some(active.into()), ..Default::default()
    }
}

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

#[tokio::test]
async fn a_key_wallet_is_refused_only_what_its_chain_cant_sign() {
    run(async |e| {
        let sol = SpotAsset::all()[0].clone();
        let key = |kind: &str| signer(vec![wallet("1", "Main", kind, (kind != "evm").then_some(SOL), (kind != "solana").then_some(ME))], "1");
        e.st_mut().status = Some(key("evm"));
        assert!(e.missing_key(&WalletCommand::StopSchedules).is_none());
        assert!(e.missing_key(&WalletCommand::SwapOn { place: "base".into(), from: "USDC".into(), to: "ETH".into(), amount: Some(5.into()) }).is_none());
        assert!(e.missing_key(&WalletCommand::Buy { asset: sol.clone(), usdc: Some(5.into()) }).is_some());
        e.st_mut().status = Some(key("solana"));
        assert!(e.missing_key(&WalletCommand::JupiterDca { asset: sol.clone(), usdc: Some(100.into()), orders: Some(2), every: 86_400 }).is_none());
        assert!(e.missing_key(&WalletCommand::Buy { asset: sol, usdc: Some(5.into()) }).is_none());
        let bridge = WalletCommand::Bridge { from: "base".into(), to: "arbitrum".into(), amount: Some(5.into()) };
        assert_eq!(e.missing_key(&bridge).as_deref(),
                   Some("The wallet «Main» has no EVM (Hyperliquid, Base…) key: it holds only a Solana key. Switch wallets (Settings › Wallet) or import one that has it."));
        e.st_mut().status = Some(key("phrase"));
        assert!(e.missing_key(&bridge).is_none());
    }).await;
}

#[tokio::test]
async fn a_key_wallet_moves_money_with_the_other_chain_empty() {
    run(async |e| {
        e.st_mut().status = Some(signer(vec![wallet("key", "Key", "evm", None, Some(ME))], "key"));
        assert!(e.executor().is_ok());
        // A Solana move from it is said before its card, not failed at Confirm.
        assert!(e.missing_key(&WalletCommand::Stake(Some(1.into()))).is_some());
        e.st_mut().status = None;
        assert_eq!(e.executor().err().map(|e| e.0).as_deref(), Some("Add a wallet first."));
    }).await;
}

#[tokio::test]
async fn a_saved_name_is_found_as_the_mac_finds_it() {
    run(async |e| {
        let one = "0x1111111111111111111111111111111111111111";
        e.st_mut().book = vec![address("Automation", "solana", SOL, ""), address("Automation", "base", PHONE, ""), address("Mom Base", "base", MOM, "")];
        // Hyperliquid is EVM: the Base entry, not the Solana one that can't receive there.
        assert_eq!(e.entry("Automation", Some("hyperliquid")).map(|a| a.address).as_deref(), Some(PHONE));
        // A name that contains the word.
        assert_eq!(e.entry("mom", Some("base")).map(|a| a.address).as_deref(), Some(MOM));
        // The multisig, only on a network it's made on.
        e.st_mut().multisigs = vec![safe(one, 0, &["base"])];
        assert_eq!(e.entry("the multisig", Some("base")).map(|a| a.address).as_deref(), Some(one));
        assert!(e.entry("the multisig", Some("arbitrum")).is_none());
    }).await;
}

#[tokio::test]
async fn an_older_multisig_entry_gives_way_to_the_newest_safe() {
    run(async |e| {
        e.st_mut().book = vec![address("Multisig", "base", MOM, "")];
        e.st_mut().multisigs = vec![safe(&MOM.to_lowercase(), 0, &["base"]), safe(&NEWER.to_lowercase(), 1, &["base"])];
        assert_eq!(e.entry("Multisig", Some("base")).map(|a| a.address), Some(NEWER.to_lowercase()));
        // A Safe the person saved under a name of their own stays theirs, on its networks.
        e.st_mut().book = vec![address("Treasury", "base", MOM, "")];
        assert_eq!(e.entry("Treasury", Some("base")).map(|a| a.address).as_deref(), Some(MOM));
        assert!(e.entry("Treasury", Some("arbitrum")).is_none());
    }).await;
}

#[tokio::test]
async fn another_wallet_here_is_its_solana_address() {
    run(async |e| {
        e.st_mut().status = Some(signer(vec![wallet("1", "Main", "phrase", Some(SOL), Some(ME)), wallet("2", "Test", "phrase", Some(SOL), Some(MOM))], "1"));
        let found = e.entry("Test", Some("base"));
        assert!(found.as_ref().is_some_and(|f| f.network == "solana" && f.address == SOL), "{found:?}");
        e.prepare_transfer(10.into(), "Test", Some("base"), None);
        assert!(e.st().card.is_none());
        assert!(last(&e).contains("is a solana address: it can't receive on base"), "{}", last(&e));
    }).await;
}

#[tokio::test]
async fn a_multisig_never_answers_on_a_network_it_isnt_made_on() {
    run(async |e| {
        e.st_mut().book = vec![address("Multisig", "base", MOM, "")];
        e.st_mut().multisigs = vec![safe(&MOM.to_lowercase(), 0, &["base"])];
        // An older build's own «Multisig» entry gives way to the Safe itself (multisig_entry), as on the Mac.
        assert_eq!(e.entry("Multisig", Some("base")).map(|a| a.address.to_lowercase()), Some(MOM.to_lowercase()));
        assert_eq!(e.entry("Multisig", None).map(|a| a.network).as_deref(), Some("base"));
        assert!(e.entry("Multisig", Some("arbitrum")).is_none());
        assert!(e.entry("Multisig", Some("hyperliquid")).is_none());
        assert!(e.entry("the multisig", Some("arbitrum")).is_none());
        // The send itself: said, no card.
        e.prepare_transfer(10.into(), "Multisig", Some("arbitrum"), None);
        assert!(e.st().card.is_none() && e.st().pending.is_none());
        assert!(last(&e).contains("There’s no “Multisig” in Addresses"), "{}", last(&e));
        // Made on Arbitrum too: there it answers.
        e.st_mut().book = vec![];
        e.st_mut().multisigs = vec![safe(&MOM.to_lowercase(), 0, &["base", "arbitrum"])];
        assert_eq!(e.entry("Multisig", Some("arbitrum")).map(|a| a.address), Some(MOM.to_lowercase()));
    }).await;
}

#[tokio::test]
async fn a_memo_address_is_refused_for_sol_and_near_alone_and_at_the_move() {
    run(async |e| {
        let near = "0".repeat(64);
        e.st_mut().book = vec![address("Binance", "solana", SOL, "104"), address("Binance", "near", &near, "77"), address("Mom", "solana", SOL, "")];
        for c in [WalletCommand::SendSol { name: "Binance".into(), amount: Decimal::new(5, 2) }, WalletCommand::SendNear { name: "Binance".into(), amount: Some(1.into()) },
                  WalletCommand::CloseNear { name: "Binance".into() }] {
            assert!(e.native_recipient(&c).is_err());
            e.handle_wallet(&c, None).await;
            assert!(e.st().card.is_none(), "{c:?}");
            assert!(last(&e).contains("needs a memo"), "{}", last(&e));
            let moved = e.r#move(&c).await;
            assert!(moved.as_ref().is_err_and(|err| err.to_string().contains("needs a memo")), "{c:?} {moved:?}");
        }
        // An address with no memo goes on.
        let mom = e.native_recipient(&WalletCommand::SendSol { name: "Mom".into(), amount: 1.into() });
        assert_eq!(mom.ok().flatten().map(|a| a.address).as_deref(), Some(SOL));
    }).await;
}

#[tokio::test]
async fn sol_and_near_recipients_are_found_as_on_the_mac() {
    run(async |e| {
        e.st_mut().book = vec![address("Bob", "base", MOM, ""), address("Alice savings", "solana", SOL, "")];
        // Saved only on Base: no NEAR address for him (before, the card drew and the signer refused it).
        assert!(e.native_recipient(&WalletCommand::SendNear { name: "Bob".into(), amount: Some(1.into()) }).is_err());
        // A name that contains the word.
        assert_eq!(e.saved_address("Alice", "solana").map(|a| a.address).as_deref(), Some(SOL));
        assert!(e.saved_address("", "solana").is_none());
        // «my Ledger» is the Ledger by what it is, whatever it's called.
        assert!(Engine::means_ledger(&Engine::own_key("my Ledger"), "Cold"));
        assert!(Engine::means_ledger(&Engine::own_key("леджер"), "Cold"));
        assert!(Engine::means_ledger(&Engine::own_key("Cold"), "Cold"));
        assert!(!Engine::means_ledger(&Engine::own_key("my Bob"), "Cold"));
        assert!(Engine::names_multisig("My Multisig") && Engine::names_multisig("мультисигу") && !Engine::names_multisig("multisig fund"));
    }).await;
}

#[test]
fn a_favicons_png_frame_is_its_logo() {
    let png: Vec<u8> = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A].into_iter().chain([7; 24]).collect();
    let ico = |frames: &[(u8, &[u8])]| {
        let mut d = vec![0, 0, 1, 0, frames.len() as u8, 0];
        let mut offset = 6 + frames.len() * 16;
        for (width, data) in frames {
            let size = data.len();
            d.extend([*width, *width, 0, 0, 1, 0, 32, 0]);
            d.extend([(size & 0xff) as u8, (size >> 8 & 0xff) as u8, 0, 0, (offset & 0xff) as u8, (offset >> 8 & 0xff) as u8, 0, 0]);
            offset += size;
        }
        for (_, data) in frames { d.extend_from_slice(data) }
        d
    };
    let bmp: Vec<u8> = [0x28, 0, 0, 0].into_iter().chain([1; 28]).collect();
    assert_eq!(Engine::png_in_icon(&ico(&[(16, &png), (48, &png), (32, &bmp)])), Some(png.clone()));
    assert_eq!(Engine::png_in_icon(&ico(&[(16, &png)])), None);
    assert_eq!(Engine::png_in_icon(&ico(&[(64, &bmp)])), None);
    assert_eq!(Engine::png_in_icon(&png), Some(png.clone()));
    assert_eq!(Engine::png_in_icon(&[1, 2, 3]), None);
    // A frame that says it runs past the file is skipped, never read out of bounds.
    let mut past = ico(&[(48, &png)]);
    past[6 + 8] = 0xff;
    past[6 + 9] = 0xff;
    assert_eq!(Engine::png_in_icon(&past), None);
}

#[test]
fn send_anyway_takes_a_little_more_than_the_loss_it_named_and_the_second_check_says_when_it_could_not_run() {
    assert_eq!(Engine::accepted_loss(2.into()), Decimal::new(255, 2));
    assert_eq!(Engine::accepted_loss(Decimal::ZERO), Decimal::new(5, 2));
    assert_eq!(Engine::not_checking(Some("timed out"), false), "The second check couldn't run: timed out. Only code checks apply.");
    assert_eq!(Engine::not_checking(None, false), "The second check couldn't run: connect a model in Settings › Sign-ins. Only code checks apply.");
    assert_eq!(Engine::not_checking(None, true), "Вторая проверка не сработала: подключи модель в «Настройки › Входы». Действуют только проверки в коде.");
}

fn position(coin: &str, size: Decimal) -> Position { Position { coin: coin.into(), size, entry: 2000.into(), unrealized_pnl: Decimal::ZERO, liquidation: None } }

#[tokio::test]
async fn the_coin_just_traded_is_the_one_a_position_command_means() {
    run(async |e| {
        e.st_mut().positions = vec![position("ETH", 1.into()), position("BTC", Decimal::new(1, 2))];
        assert!(e.target_coin("move stop to breakeven").is_none());
        e.st_mut().traded_plan = Some(TradeIntent { asset: "ETH".into(), ..TradeIntent::default() });
        assert_eq!(e.target_coin("move stop to breakeven").as_deref(), Some("ETH"));
    }).await;
}

/// Hyperliquid's $10 minimum counts for a close too: a part worth less is said before any card, with «Close it all» as
/// a card; a part worth more waits for Confirm as before.
#[tokio::test]
async fn a_close_under_ten_dollars_is_said_with_close_it_all() {
    run(async |e| {
        e.st_mut().positions = vec![position("ETH", Decimal::new(4, 3))];
        e.st_mut().markets.insert("ETH".into(), ihatecrypto_core::plan::Market::new("ETH", 2650.into(), 4, 25));
        e.pend_position(ihatecrypto_core::position_command::PositionCommand::Close(Decimal::new(75, 2)), "ETH", "close 75% ETH");
        assert!(e.st().pending.is_none());
        assert!(last(&e).contains("Hyperliquid won't take a close under $10"), "{}", last(&e));
        assert_eq!(e.st().options.iter().map(|o| o.command.as_str()).collect::<Vec<_>>(), ["close ETH"]);
        e.st_mut().positions = vec![position("ETH", 1.into())];
        e.pend_position(ihatecrypto_core::position_command::PositionCommand::Close(Decimal::new(5, 1)), "ETH", "close half ETH");
        assert!(matches!(e.st().pending, Some(Pending::Position { .. })));
    }).await;
}

/// Limit orders belong to the NEAR Intents partner key: without one, no card, no list, no cancel.
#[tokio::test]
async fn limit_orders_without_the_partner_key_are_refused_before_any_card() {
    run(async |e| {
        protocols::one_click::set_key(None);
        for text in ["limit sell 0.1 SOL at 200", "show limit orders", "cancel limit orders"] {
            e.handle_wallet(&WalletCommand::parse(text).unwrap(), None).await;
            assert!(e.st().pending.is_none(), "{text}");
            assert!(last(&e).contains("NEAR Intents key"), "{text}: {}", last(&e));
        }
    }).await;
}

#[tokio::test]
async fn a_limit_waiting_counts_as_open() {
    run(async |e| {
        e.st_mut().hl_orders = vec![Order {
            coin: "ETH".into(), oid: 1, is_trigger: false, trigger_px: None, limit_px: Some(2400.into()), size: Some(Decimal::new(1, 2)),
            side: Some("B".into()), order_type: Some("Limit".into()), reduce_only: false, children: vec![],
        }];
        assert_eq!(e.target_coin("cancel orders").as_deref(), Some("ETH"));
        assert_eq!(e.limit_waiting_line().as_deref(), Some("Limit waiting: long ETH at 2,400"));
        e.st_mut().hl_orders = vec![];
        assert!(e.target_coin("cancel orders").is_none() && e.limit_waiting_line().is_none());
    }).await;
}

#[tokio::test]
async fn cancel_takes_the_steps_held_behind_its_card() {
    run(async |e| {
        // The card as show() leaves it (show() also pins its block and asks for quotes: other files').
        let id = "CARD-1".to_string();
        {
            let mut st = e.st_mut();
            st.pending = Some(Pending::Address(address("Mom", "base", MOM, "")));
            st.card = Some(Card { id: id.clone(), title: "Save Mom".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![] });
            let canvas = st.canvas_id.clone();
            st.chain_state.after_call = Some(AfterCall { board: id.clone(), canvas, rest: vec!["chart SOL".into()], since: Utc::now() });
        }
        e.cancel_pending(Some("another"));
        assert!(e.st().chain_state.after_call.is_some() && e.st().card.is_some());
        e.cancel_pending(Some(&id));
        assert!(e.st().chain_state.after_call.is_none() && e.st().card.is_none() && e.st().pending.is_none());
        // The steps a chain held behind a card that failed go with it, another card's stay.
        e.st_mut().chain_state.after_call = Some(AfterCall { board: id.clone(), canvas: String::new(), rest: vec![], since: Utc::now() });
        e.drop_held(Some("another"));
        assert!(e.st().chain_state.after_call.is_some());
        e.drop_held(Some(&id));
        assert!(e.st().chain_state.after_call.is_none());
    }).await;
}

#[tokio::test]
async fn the_history_lists_the_journal_newest_first_with_its_explorer() {
    run(async |e| {
        for (chain, id) in [("solana", "5sig"), ("base", "0xabc"), ("near", "Hnear"), ("1click", "dep")] {
            e.journal().append(&execution::Entry::new(chain, id, execution::Kind::Signed, "test")).unwrap();
        }
        let h = e.history();
        assert_eq!(h.iter().map(|l| l.link.as_deref()).collect::<Vec<_>>(),
                   [None, Some("https://nearblocks.io/txns/Hnear"), Some("https://basescan.org/tx/0xabc"), Some("https://solscan.io/tx/5sig")]);
        assert!(h.iter().all(|l| l.kind == "signed"));
    }).await;
}

/// «Send anyway» sends only the move its button was offered for: a button left from another move (another canvas, an
/// earlier refusal) says so, signs nothing and leaves the move that waits now waiting.
#[tokio::test]
async fn send_anyway_sends_only_the_move_its_button_was_for() {
    run(async |e| {
        let waiting = Pending::Wallet(WalletCommand::ClaimRewards);
        e.st_mut().costly = Some(Costly { id: "b".into(), pending: waiting, title: "claim".into(), lost: 3.into() });
        e.send_anyway("a").await;
        let st = e.st();
        assert!(st.costly.as_ref().is_some_and(|c| c.id == "b") && st.pending.is_none() && st.card.is_none());
        assert!(st.messages().last().is_some_and(|m| m.text == "That move isn't waiting any more: nothing was signed. Ask for it again."));
    }).await;
}
