//! Settings › Modules (ModulesTests, Settings13Tests and main's IhatecryptoCoreTests/ModulesTests for the command's
//! refusal): a switched-off protocol, kind or network is refused before any card, a module a later version brings starts
//! off and is asked about once (never during the first run), and each protocol's line says which of its networks are off.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use ihatecrypto_core::modules::{ModuleKind, ModuleNetwork, ModuleProtocol, Modules};
use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use rust_decimal::Decimal;

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

/// An engine on the test's own folder, in English; locked, so a state push draws the lock alone. Its settings set the
/// process's switches (`trade::modules`), so the tests that make one take turns.
async fn engine() -> (Rc<Engine>, tokio::sync::MutexGuard<'static, ()>) {
    let turn = crate::modules::SWITCHES.lock().await;
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the test engine stays locked"));
    e.st_mut().lock.locked = true;
    e.set_settings(|s| s.language = Some("en".into()));
    (e, turn)
}

fn off(ids: &[&str]) -> Modules { Modules::from_stored(&ids.iter().map(|id| format!("{id}=0")).collect::<Vec<_>>().join(","), None) }

fn d(n: i64) -> Decimal { Decimal::from(n) }

/// The switches as this engine's settings have them: put back first, as another test's engine (not taking turns) may
/// have put its own meanwhile.
fn switches(e: &Engine) -> Modules {
    e.st().settings.mirror();
    trade::modules()
}

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

#[test]
fn everything_is_on_until_switched_off() {
    let m = Modules::default();
    assert!(ModuleProtocol::ALL.into_iter().all(|p| m.allows(p)));
    assert_eq!(m.refusal_command(&WalletCommand::Kamino { deposit: true, symbol: "USDC".into(), amount: Some(d(1)) }, false), None);
}

#[test]
fn a_switch_off_stops_its_moves_and_survives_storage() {
    let m = Modules::from_stored(&off(&["protocol.kamino", "network.base"]).stored(), None);
    assert_eq!(m.refusal_command(&WalletCommand::Kamino { deposit: true, symbol: "USDC".into(), amount: Some(d(1)) }, false).as_deref(),
               Some("Kamino is off in Settings › Modules."));
    // Aave needs Base; Jupiter Lend doesn't.
    assert_eq!(m.refusal_command(&WalletCommand::Aave { deposit: true, amount: Some(d(1)), network: "base".into() }, false).as_deref(),
               Some("Base is off in Settings › Modules."));
    assert_eq!(m.refusal_command(&WalletCommand::LendDeposit("USDC".into(), Some(d(1))), false), None);
    assert!(m.refusal_command(&WalletCommand::SolanaTo("base".into(), Some(d(1))), false).is_some());
    assert!(m.refusal_command(&WalletCommand::SolanaTo("arbitrum".into(), Some(d(1))), false).is_none());
    // Loans on Kamino go with Kamino.
    assert!(m.refusal_command(&WalletCommand::KaminoBorrow { amount: Some(d(1)), against: None, adding: None }, false).is_some());
}

#[test]
fn a_kind_off_takes_its_protocols_with_it() {
    let m = off(&["kind.earn"]);
    let allocate = WalletCommand::Allocate { symbol: "USDC".into(), max_share: None, amount: None, without: vec![], from: None };
    assert!(m.refusal_command(&allocate, false).is_some());
    assert_eq!(m.refusal_command(&WalletCommand::Stake(Some(d(1))), false).as_deref(), Some("Earn · DeFi is off in Settings › Modules, so JitoSOL staking is too."));
    assert_eq!(m.refusal_command(&WalletCommand::Buy { asset: SpotAsset::usdc(), usdc: Some(d(1)) }, false), None);
    // A Russian reply for a Russian chat.
    assert!(m.refusal_command(&WalletCommand::Stake(Some(d(1))), true).is_some_and(|r| r.contains("выключены")));
}

#[test]
fn near_moves_follow_the_network_the_money_is_on() {
    let m = off(&["network.solana"]);
    // A buy pays with Solana USDC, what's received lands there, a sale with no network pays out there.
    assert_eq!(m.refusal_command(&WalletCommand::BuyNear { usdc: Some(d(10)) }, false).as_deref(), Some("Solana is off in Settings › Modules."));
    assert!(m.refusal_command(&WalletCommand::ReceiveAcross { network: "sui".into(), token: "USDC".into(), amount: Some(d(50)) }, false).is_some());
    assert!(m.refusal_command(&WalletCommand::SellNear { amount: Some(d(1)), to: None }, false).is_some());
    assert!(m.refusal_command(&WalletCommand::SellNear { amount: Some(d(1)), to: Some("base".into()) }, false).is_none());
    let m = off(&["network.base"]);
    assert_eq!(m.refusal_command(&WalletCommand::SellNear { amount: Some(d(1)), to: Some("base".into()) }, false).as_deref(), Some("Base is off in Settings › Modules."));
    assert_eq!(m.refusal_command(&WalletCommand::BuyNear { usdc: Some(d(10)) }, false), None);
    let m = off(&["network.base", "network.near"]);
    assert_eq!(m.refusal_command(&WalletCommand::BuyNear { usdc: Some(d(10)) }, false).as_deref(), Some("NEAR is off in Settings › Modules."));
}

#[test]
fn hyperliquid_spot_follows_the_swaps_switch() {
    let buy = WalletCommand::HlSpotBuy { symbol: "HYPE".into(), usdc: Some(d(10)) };
    let sell = WalletCommand::HlSpotSell { symbol: "HYPE".into(), amount: None };
    assert_eq!(Modules::default().refusal_command(&buy, false), None);
    let m = off(&["kind.swaps"]);
    assert_eq!(m.refusal_command(&buy, false).as_deref(), Some("Swaps is off in Settings › Modules."));
    assert!(m.refusal_command(&sell, false).is_some());
    assert!(m.refusal_command(&WalletCommand::SwapOn { place: "hyperliquid".into(), from: "HYPE".into(), to: "PURR".into(), amount: Some(d(1)) }, false).is_some());
    assert_eq!(off(&["network.hyperliquid"]).refusal_command(&sell, false).as_deref(), Some("Hyperliquid is off in Settings › Modules."));
}

/// One token for another on an EVM network takes whichever of Uniswap, KyberSwap and Relay's swap is on there: with
/// Uniswap off it still swaps; with all three off it's refused, in Uniswap's words.
#[test]
fn a_swap_on_a_network_needs_one_of_its_ways() {
    let swap = WalletCommand::SwapOn { place: "base".into(), from: "DEGEN".into(), to: "AERO".into(), amount: Some(d(1)) };
    assert_eq!(off(&["protocol.uniswap"]).refusal_command(&swap, false), None);
    assert_eq!(off(&["protocol.uniswap", "protocol.kyberswap", "protocol.relay"]).refusal_command(&swap, false).as_deref(),
               Some("Uniswap is off in Settings › Modules."));
    assert_eq!(off(&["protocol.uniswap", "protocol.relay"]).refusal_command(&swap, false), None);
    // A module new to this computer stays off until answered.
    let known: HashSet<String> = Modules::all_ids().into_iter().filter(|id| id != "network.bnb").collect();
    let m = Modules::new(Default::default(), Some(known));
    assert_eq!(m.refusal_command(&WalletCommand::SwapOn { place: "bnb".into(), from: "BNB".into(), to: "USDC".into(), amount: Some(d(1)) }, false).as_deref(),
               Some("BNB Chain is off in Settings › Modules."));
    // USDT0's own bridge only between its networks; reading approvals is always allowed.
    let usdt = WalletCommand::Usdt { from: "base".into(), to: "arbitrum".into(), amount: Some(d(5)), layer_zero: true };
    assert!(Modules::default().refusal_command(&usdt, false).is_some_and(|r| r.starts_with("Tether's USDT0 bridge on LayerZero runs between")));
    assert_eq!(off(&["kind.swaps", "kind.earn"]).refusal_command(&WalletCommand::ShowApprovals, false), None);
}

#[test]
fn each_protocols_line_says_which_of_its_networks_are_off() {
    let all = Modules::default();
    assert_eq!(Engine::module_line(ModuleProtocol::Uniswap, &all), (ModuleProtocol::Uniswap.place(), false));
    // A network a protocol needs is off: that's all the line says.
    assert_eq!(Engine::module_line(ModuleProtocol::Jupiter, &off(&["network.solana"])), ("Solana is off".to_string(), true));
    // One of several places off: the rest work.
    let (line, networks_off) = Engine::module_line(ModuleProtocol::Uniswap, &off(&["network.base", "network.polygon"]));
    assert_eq!(line, format!("{} · Base, Polygon are off, the rest work", ModuleProtocol::Uniswap.place()));
    assert!(networks_off);
}

#[tokio::test]
async fn a_switched_off_module_is_refused_and_unknown_ones_are_errors() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        assert_eq!(e.set_module(Some("protocol.nothing"), false).unwrap_err().0, "Unknown module «protocol.nothing».");
        assert_eq!(e.set_module(None, true).unwrap_err().0, "Unknown module «».");
        e.set_module(Some("protocol.jitoSOL"), false).unwrap();
        e.st().settings.mirror();
        let view = e.modules_view();
        let item = view.kinds.iter().flat_map(|k| &k.protocols).find(|p| p.id == "protocol.jitoSOL").unwrap();
        assert!(!item.on && !item.allowed);
        let stake = WalletCommand::parse("stake 1 SOL").unwrap();
        assert_eq!(switches(&e).refusal_command(&stake, false).as_deref(), Some("JitoSOL staking is off in Settings › Modules."));
        // The kind's switch covers its protocols, and a network's covers what runs there.
        e.set_module(Some("protocol.jitoSOL"), true).unwrap();
        e.set_module(Some("kind.earn"), false).unwrap();
        assert!(switches(&e).refusal_command(&stake, false).is_some_and(|r| r.contains("Earn · DeFi is off in Settings › Modules")));
        e.set_module(Some("kind.earn"), true).unwrap();
        e.set_module(Some("network.solana"), false).unwrap();
        e.st().settings.mirror();
        let solana = e.modules_view().networks.into_iter().find(|n| n.id == "network.solana").unwrap();
        assert!(!solana.on && solana.users.starts_with("Used by Jupiter, Jupiter DCA"));
        assert!(switches(&e).refusal_command(&stake, false).is_some_and(|r| r.contains("Solana")));
        e.set_module(Some("network.solana"), true).unwrap();
        assert_eq!(switches(&e).refusal_command(&stake, false), None);
        assert_eq!(e.st().settings.modules.as_deref(), Some("kind.earn=1,network.solana=1,protocol.jitoSOL=1"));
        // What the window draws: every switch, by its id.
        e.st().settings.mirror();
        let view = e.modules_view();
        let ids: Vec<String> = view.networks.iter().map(|n| n.id.clone()).chain(view.kinds.iter().map(|k| k.id.clone()))
            .chain(view.kinds.iter().flat_map(|k| k.protocols.iter().map(|p| p.id.clone()))).collect();
        assert_eq!(ids.len(), Modules::all_ids().len());
        assert!(ids.contains(&"network.bnb".into()) && ids.contains(&"protocol.kamino".into()) && ids.contains(&"kind.perps".into()));
        assert_eq!(view.kinds.iter().find(|k| k.id == "kind.perps").unwrap().key, ModuleKind::Perps.raw());
        e.set_settings(|s| s.modules = None);
    }).await;
}

#[tokio::test]
async fn a_module_a_later_version_brings_starts_off_and_is_asked_about_once() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        // The first run records every module there is, so nothing changes for someone who has them all.
        e.st().settings.mirror();
        e.offer_new_modules();
        assert_eq!(e.st().settings.modules_known.as_ref(), Some(&Modules::all_ids()));
        assert!(e.st().messages().is_empty());
        e.st().settings.mirror();
        e.offer_new_modules();
        assert!(e.st().messages().is_empty() && e.st().options.is_empty());

        // A later version brings BNB Chain: off, and asked about — but not during the first run.
        e.set_settings(|s| s.modules_known = Some(Modules::all_ids().into_iter().filter(|id| id != "network.bnb").collect()));
        e.st_mut().onboarding = true;
        e.st().settings.mirror();
        e.offer_new_modules();
        assert!(e.st().messages().is_empty());
        e.st_mut().onboarding = false;
        e.st().settings.mirror();
        assert!(!e.modules_view().networks.iter().find(|n| n.id == "network.bnb").unwrap().on);
        e.st().settings.mirror();
        e.offer_new_modules();
        assert_eq!(last(&e), "New in ihatecrypto: BNB Chain. It stays off until you say — turn it on?");
        let options = e.st().options.clone();
        assert_eq!(options.iter().map(|o| o.title.as_str()).collect::<Vec<_>>(), ["Turn on BNB Chain", "Keep it off"]);
        assert_eq!(options[0].command, "turn on module network.bnb");
        assert_eq!(options[0].comment, "Settings › Modules");

        // Only a module asked about: a card the model wrote can't turn on what the person switched off.
        e.st().settings.mirror();
        assert!(!e.answer_new_modules("buy SOL"));
        e.st().settings.mirror();
        assert!(e.answer_new_modules("turn on module protocol.kamino"));
        assert_eq!(last(&e), "That one is switched in Settings › Modules.");
        e.st().settings.mirror();
        assert!(e.answer_new_modules(&options[0].command));
        assert_eq!(last(&e), "BNB Chain is on.");
        e.st().settings.mirror();
        assert!(e.modules_view().networks.iter().find(|n| n.id == "network.bnb").unwrap().on);
        assert!(switches(&e).new_ids().is_empty());

        // «Keep it off»: every new one switched off, said once.
        e.set_settings(|s| { s.modules = None; s.modules_known = Some(vec![]) });
        e.st().settings.mirror();
        assert!(e.answer_new_modules(MODULE_KEEP_OFF));
        assert_eq!(last(&e), "Left off. Settings › Modules turns it on any time.");
        assert!(switches(&e).new_ids().is_empty() && !switches(&e).is_on(ModuleNetwork::Base));
        e.set_settings(|s| { s.modules = None; s.modules_known = None });
    }).await;
}
