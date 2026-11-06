use std::collections::HashSet;

use crate::modules::*;

// pending (round 2: `Modules::refusal(_ command: WalletCommand)`): the WalletCommand halves of
// everythingIsOnUntilSwitchedOff, aSwitchOffStopsItsMovesAndSurvivesStorage, aKindOffTakesItsProtocolsWithIt,
// nearMovesFollowTheNetworkTheMoneyIsOn, hyperliquidSpotFollowsTheSwapsSwitch, aSwapOnANetworkNeedsOneOfItsWays and
// aModuleNewToThisMacStaysOffUntilAnswered.

#[test]
fn everything_is_on_until_switched_off() {
    let m = Modules::default();
    assert!(ModuleProtocol::ALL.into_iter().all(|p| m.allows(p)));
    assert!(m.off_summary().is_empty());
}

#[test]
fn a_switch_off_stops_its_moves_and_survives_storage() {
    let mut m = Modules::default();
    m.set(ModuleProtocol::Kamino, false);
    m.set(ModuleNetwork::Base, false);
    let m = Modules::from_stored(&m.stored(), None);
    assert_eq!(m.refusal(ModuleProtocol::Kamino, &[], false).as_deref(), Some("Kamino is off in Settings › Modules."));
    // Aave needs Base where the move goes; Jupiter Lend doesn't.
    assert_eq!(m.refusal(ModuleProtocol::Aave, &[ModuleNetwork::Base], false).as_deref(), Some("Base is off in Settings › Modules."));
    assert_eq!(m.refusal(ModuleProtocol::JupiterLend, &[], false), None);
    assert!(m.refusal(ModuleProtocol::Relay, &[ModuleNetwork::Base], false).is_some());
    assert!(m.refusal(ModuleProtocol::Relay, &[ModuleNetwork::Arbitrum], false).is_none());
    assert!(m.refusal_vault("Kamino Lend").is_some());
    assert!(m.refusal_vault("Jupiter Lend").is_none());
}

#[test]
fn a_kind_off_takes_its_protocols_with_it() {
    let mut m = Modules::default();
    m.set(ModuleKind::Perps, false);
    assert_eq!(m.refusal_trade(false, false, false).as_deref(), Some("Perps is off in Settings › Modules, so Hyperliquid is too."));
    assert_eq!(m.refusal_trade(true, false, false), None);   // oil and stocks are their own kind
    m.set(ModuleKind::Earn, false);
    assert!(m.refusal(ModuleProtocol::JitoSol, &[], false).is_some());
    assert!(m.refusal(ModuleProtocol::Jupiter, &[], false).is_none());
    // A Russian reply for a Russian chat.
    assert_eq!(m.refusal(ModuleProtocol::JitoSol, &[], true).as_deref(), Some("Earn · DeFi выключены в Settings › Modules, а с ними и JitoSOL staking."));
    m.set(ModuleNetwork::Solana, false);
    m.set(ModuleKind::Earn, true);
    assert_eq!(m.refusal(ModuleProtocol::JitoSol, &[], true).as_deref(), Some("Solana выключена в Settings › Modules."));
    m.set(ModuleProtocol::Kamino, false);
    assert_eq!(m.refusal(ModuleProtocol::Kamino, &[], true).as_deref(), Some("Kamino выключен в Settings › Modules."));
}

#[test]
fn vaults_on_every_network_follow_their_networks_switch() {
    let mut m = Modules::default();
    assert_eq!(m.refusal_vault("Aave · Polygon"), None);
    assert_eq!(m.refusal_vault("Morpho · Ethereum"), None);
    m.set(ModuleNetwork::Polygon, false);
    assert!(m.refusal_vault("Aave · Polygon").is_some());
    assert_eq!(m.refusal_vault("Aave · Arbitrum"), None);
    m.set(ModuleProtocol::Aave, false);
    assert!(m.refusal_vault("Aave · Arbitrum").is_some());
    // A place no switch covers is refused, not waved through.
    assert_eq!(Modules::default().refusal_vault("Aave · BNB").as_deref(), Some("Aave · BNB isn't a place ihatecrypto knows."));
    assert!(Modules::default().refusal_vault("Somewhere").is_some());
    assert!(ModuleNetwork::from_place("polygon") == Some(ModuleNetwork::Polygon) && ModuleNetwork::from_place("ethereum") == Some(ModuleNetwork::Ethereum));
}

#[test]
fn a_module_new_to_this_computer_stays_off_until_answered() {
    let known: HashSet<String> = Modules::all_ids().into_iter().filter(|id| id != "network.bnb").collect();
    let mut m = Modules::new(Default::default(), Some(known));
    assert!(!m.is_on(ModuleNetwork::Bnb) && m.is_on(ModuleNetwork::Base));
    assert_eq!(m.new_ids(), ["network.bnb"]);
    assert_eq!(m.refusal_network(ModuleNetwork::Bnb, false).as_deref(), Some("BNB Chain is off in Settings › Modules."));
    m.set(ModuleNetwork::Bnb, true);
    assert!(m.is_on(ModuleNetwork::Bnb) && m.new_ids().is_empty());
    // Nothing recorded yet: every module as it was.
    assert!(Modules::new(Default::default(), None).new_ids().is_empty());
}

#[test]
fn the_stored_form_skips_what_it_cant_read() {
    let m = Modules::from_stored("network.base=0,,junk,kind.perps=1=2,protocol.kamino=1", None);
    assert_eq!(m.choices, [("network.base".to_string(), false), ("protocol.kamino".to_string(), true)].into());
    assert_eq!(m.stored(), "network.base=0,protocol.kamino=1");
    assert_eq!(Modules::from_stored("", None), Modules::default());
}

#[test]
fn places_and_ids_name_their_modules() {
    for n in ModuleNetwork::ALL { assert_eq!(ModuleNetwork::from_place(n.raw()), Some(n)) }
    assert!(ModuleNetwork::from_place("HL") == Some(ModuleNetwork::Hyperliquid) && ModuleNetwork::from_place("HyperEVM") == Some(ModuleNetwork::Hyperevm));
    assert_eq!(ModuleNetwork::from_place("sui"), None);
    assert!(Modules::name("network.bnb") == "BNB Chain" && Modules::name("kind.earn") == "Earn · DeFi" && Modules::name("protocol.mpp") == "MPP · x402");
    assert!(Modules::name("network.mars") == "mars" && Modules::name("bogus") == "bogus");
    assert_eq!(Modules::all_ids().len(), ModuleNetwork::ALL.len() + ModuleKind::ALL.len() + ModuleProtocol::ALL.len());
    assert_eq!(ModuleProtocol::Hlp.place(), "Hyperliquid");
}

#[test]
fn the_brief_names_what_is_off() {
    let mut m = Modules::default();
    m.set(ModuleNetwork::Tempo, false);
    m.set(ModuleKind::Perps, false);
    m.set(ModuleProtocol::Kamino, false);
    assert_eq!(m.off_summary(), "Tempo, Perps, Kamino");
    // A protocol whose kind is off is off for the brief by its kind, not listed again.
    assert!(!m.off_summary().contains("Hyperliquid"));
}
