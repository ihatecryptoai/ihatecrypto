//! Settings › Modules (Modules.swift): every switch with the words the Mac shows beside it, one switch changed, and a
//! module a later version brings asked about once in the chat. Settings' `mirror()` (this file's in Swift) is in store.rs,
//! beside Settings. `Modules.current` is `trade::modules()`, which `mirror()` sets.
//! Also here until ihatecrypto-core ports it: `Modules.refusal(_ command: WalletCommand, ru:)` (main's Modules.swift), as
//! `CommandRefusal::refusal_command`.

use ihatecrypto_core::modules::{ModuleKind, ModuleNetwork, ModuleProtocol, Modules};
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::stablecoins;
use serde::Serialize;
use wallet::WalletError;

use crate::engine::Engine;
use crate::from_core::RouterOption;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModulesView {
    pub networks: Vec<NetworkSwitch>,
    pub kinds: Vec<KindSwitch>,
}

/// `ModulesView.Network`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetworkSwitch {
    pub id: String,
    pub name: String,
    pub uses: String,
    pub logo: String,
    pub users: String,
    pub on: bool,
}

/// `ModulesView.Item`: a protocol's switch.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSwitch {
    pub id: String,
    pub name: String,
    pub line: String,
    pub logo: String,
    pub on: bool,
    /// It works: its own switch, its kind's and its networks' are on.
    pub allowed: bool,
    /// One of its networks is off (the line says which).
    pub networks_off: bool,
}

/// `ModulesView.Kind`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KindSwitch {
    pub id: String,
    pub key: String,
    pub name: String,
    pub line: String,
    pub on: bool,
    pub protocols: Vec<ItemSwitch>,
}

/// The chat's answers to «turn on the new modules?».
pub const MODULE_ON: &str = "turn on module ";
pub const MODULE_KEEP_OFF: &str = "keep the new modules off";

/// Tests that make an engine take turns: its settings set the switches, and they're the process's own (`trade::modules`).
#[cfg(test)]
pub(crate) static SWITCHES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Whether a protocol works on `n`: Uniswap, Aave and Morpho check the network per move, so their place names count too.
pub fn runs(p: ModuleProtocol, n: ModuleNetwork) -> bool { p.networks().contains(&n) || p.place().contains(n.name()) }

impl Engine {
    pub fn modules_view(&self) -> ModulesView {
        let m = trade::modules();
        ModulesView {
            networks: ModuleNetwork::ALL.into_iter().map(|n| {
                let users: Vec<&str> = ModuleProtocol::ALL.into_iter().filter(|&p| runs(p, n)).map(ModuleProtocol::name).collect();
                NetworkSwitch {
                    id: format!("network.{}", n.raw()), name: n.name().into(), uses: n.uses().into(), logo: n.logo().into(),
                    users: if users.is_empty() { self.say("Balances only", "Только балансы") } else { self.say("Used by ", "Нужна для: ") + &users.join(", ") },
                    on: m.is_on(n),
                }
            }).collect(),
            kinds: ModuleKind::ALL.into_iter().map(|k| KindSwitch {
                id: format!("kind.{}", k.raw()), key: k.raw().into(), name: k.name().into(), line: k.line().into(), on: m.is_on(k),
                protocols: ModuleProtocol::ALL.into_iter().filter(|p| p.kind() == k).map(|p| {
                    let (line, networks_off) = Self::module_line(p, &m);
                    ItemSwitch { id: format!("protocol.{}", p.raw()), name: p.name().into(), line, logo: p.logo().into(), on: m.is_on(p), allowed: m.allows(p), networks_off }
                }).collect(),
            }).collect(),
        }
    }

    /// The line under a protocol: where it works, or which of its networks are off (ModulesSettings.line).
    pub fn module_line(p: ModuleProtocol, m: &Modules) -> (String, bool) {
        let used: Vec<ModuleNetwork> = ModuleNetwork::ALL.into_iter().filter(|&n| runs(p, n)).collect();
        let off: Vec<ModuleNetwork> = used.iter().copied().filter(|&n| !m.is_on(n)).collect();
        let names = off.iter().map(|n| n.name()).collect::<Vec<_>>().join(", ");
        let verb = if off.len() == 1 { "is" } else { "are" };
        let text = if off.is_empty() {
            p.place()
        } else if off.iter().any(|n| p.networks().contains(n)) || off.len() == used.len() {
            format!("{names} {verb} off")
        } else {
            format!("{} · {names} {verb} off, the rest work", p.place())
        };
        (text, !off.is_empty())
    }

    /// One switch: «network.base», «kind.perps», «protocol.kamino».
    pub fn set_module(&self, id: Option<&str>, on: bool) -> Result<(), WalletError> {
        let Some(id) = id.filter(|id| Modules::all_ids().iter().any(|a| a == id)) else {
            return Err(WalletError(format!("Unknown module «{}».", id.unwrap_or(""))));
        };
        let mut m = trade::modules();
        m.choices.insert(id.into(), on);
        let stored = m.stored();
        self.set_settings(|s| s.modules = Some(stored));
        Ok(())
    }

    // MARK: New modules (Workspace.offerNewModules)

    /// A module a later version brings starts off and is asked about once, in the chat. The first run records every
    /// module there is, so nothing the person already has changes.
    pub fn offer_new_modules(&self) {
        if self.st().settings.modules_known.is_none() {
            self.set_settings(|s| s.modules_known = Some(Modules::all_ids()));
            return;
        }
        let new = trade::modules().new_ids();
        // Not during the first run (the Mac's ModulesUI.swift:334): the question waits for the next launch.
        let busy = { let st = self.st(); st.card.is_some() || st.onboarding };
        if new.is_empty() || busy { return }
        let names = new.iter().map(|id| Modules::name(id)).collect::<Vec<_>>().join(", ");
        self.reply(&self.say(&format!("New in ihatecrypto: {names}. It stays off until you say — turn it on?"),
                             &format!("Новое в ihatecrypto: {names}. Пока выключено — включить?")), None);
        let mut options: Vec<RouterOption> = new.iter().take(4).map(|id| {
            let name = Modules::name(id);
            RouterOption::new(&self.say(&format!("Turn on {name}"), &format!("Включить {name}")), &self.say("Settings › Modules", "Настройки › Модули"),
                              &format!("{MODULE_ON}{id}"), false)
        }).collect();
        options.push(RouterOption::new(&self.say("Keep it off", "Оставить выключенным"),
                                       &self.say("Settings › Modules turns it on any time", "Включить можно в Настройках › Модули"), MODULE_KEEP_OFF, false));
        self.set_options(options);
    }

    /// The answer to that question; false for any other card.
    pub fn answer_new_modules(&self, command: &str) -> bool {
        if !(command.starts_with(MODULE_ON) || command == MODULE_KEEP_OFF) { return false }
        let mut m = trade::modules();
        if let Some(id) = command.strip_prefix(MODULE_ON) {
            // Only a module asked about here: a card the model wrote can't turn on what the person switched off.
            if !m.new_ids().iter().any(|n| n == id) {
                self.reply(&self.say("That one is switched in Settings › Modules.", "Это включается в Настройках › Модули."), None);
                return true;
            }
            m.choices.insert(id.into(), true);
            let name = Modules::name(id);
            self.reply(&self.say(&format!("{name} is on."), &format!("{name} включено.")), None);
        } else {
            for id in m.new_ids() { m.choices.insert(id, false); }
            self.reply(&self.say("Left off. Settings › Modules turns it on any time.", "Оставил выключенным. Включить можно в Настройках › Модули."), None);
        }
        let stored = m.stored();
        self.set_settings(|s| s.modules = Some(stored));
        true
    }
}

/// `Modules.refusal(_ command: WalletCommand, ru:)` of main's Modules.swift (ihatecrypto-core's modules.rs leaves it
/// pending): why a wallet command can't be planned with the switches as they are; None when it can.
pub trait CommandRefusal {
    fn refusal_command(&self, command: &WalletCommand, ru: bool) -> Option<String>;
}

impl CommandRefusal for Modules {
    fn refusal_command(&self, command: &WalletCommand, ru: bool) -> Option<String> {
        use ModuleKind::{Bridges, Earn, Swaps};
        use ModuleNetwork as N;
        use ModuleProtocol as P;
        use WalletCommand as W;
        let r = |p: P, via: &[N]| self.refusal(p, via, ru);
        let place = |p: &str| ModuleNetwork::from_place(p).into_iter().collect::<Vec<_>>();
        let places = |ps: [&str; 2]| ps.into_iter().filter_map(ModuleNetwork::from_place).collect::<Vec<_>>();
        let network = |n: N| self.refusal_network(n, ru);
        let first_off = |nets: Vec<N>| nets.into_iter().find_map(network);
        let kind_off = |k: ModuleKind| Some(if ru { format!("{} выключены в Settings › Modules.", k.name()) } else { format!("{} is off in Settings › Modules.", k.name()) });
        // `isOn(.swaps) ? … : «Swaps is off»`.
        let swaps = |on: &dyn Fn() -> Option<String>| if self.is_on(Swaps) { on() } else { kind_off(Swaps) };
        match command {
            W::Buy { .. } | W::Sell { .. } | W::Schedule { .. } => r(P::Jupiter, &[]),
            W::JupiterDca { .. } | W::JupiterDcaCancel(_) | W::JupiterDcaOrders => r(P::JupiterDca, &[]),
            W::Stake(_) | W::Unstake(_) => r(P::JitoSol, &[]),
            W::LendDeposit(..) | W::LendWithdraw(..) => r(P::JupiterLend, &[]),
            W::Kamino { .. } | W::KaminoBorrow { .. } | W::KaminoRepay { .. } => r(P::Kamino, &[]),
            W::Hlp { .. } => r(P::Hlp, &[]),
            W::Aave { network: net, .. } => r(P::Aave, &[]).or_else(|| r(P::Relay, &place(net))),
            W::AaveBorrow { network: net, .. } | W::AaveRepay { network: net, .. } | W::AaveCollateral { network: net, .. } => r(P::Aave, &place(net)),
            W::Morpho { network: net, .. } => r(P::Morpho, &[]).or_else(|| r(P::Relay, &place(net))),
            W::WithdrawToSolana(_) => r(P::Relay, &[N::Hyperliquid]),
            W::BaseToSolana(_) => r(P::Relay, &[N::Base]),
            W::ArbitrumToSolana(_) => r(P::Relay, &[N::Arbitrum]),
            W::EvmToSolana(net, _) => r(P::Relay, &place(net)),
            W::RobinhoodToSolana(_) => r(P::Relay, &[N::Robinhood]),
            W::SolanaTo(p, _) => r(P::Relay, &place(p)),
            W::BuyStock { symbol, .. } => {
                (if symbol.starts_with("0x") { r(P::Uniswap, &[N::Robinhood]) } else { r(P::RobinhoodStocks, &[]) }).or_else(|| r(P::Relay, &[N::Robinhood]))
            }
            W::SellStock { symbol, .. } => if symbol.starts_with("0x") { r(P::Uniswap, &[N::Robinhood]) } else { r(P::RobinhoodStocks, &[]) },
            // Base and Arbitrum: Relay is needed either way (its swap, or its bridge before Uniswap); with Uniswap off only
            // Relay's swap is compared.
            W::BuyOn { place: p, .. } | W::SellOn { place: p, .. } => swaps(&|| r(P::Relay, &place(p))),
            // One token for another on the network itself.
            W::SwapOn { place: p, .. } if p == "solana" => swaps(&|| r(P::Jupiter, &[])),
            // Hyperliquid's spot book is a swap too: the Swaps switch covers it, as it does Tempo's exchange.
            W::SwapOn { place: p, .. } if p == "hyperliquid" => swaps(&|| network(N::Hyperliquid)),
            W::HlSpotBuy { .. } | W::HlSpotSell { .. } => swaps(&|| network(N::Hyperliquid)),
            W::SwapOn { place: p, .. } if p == "near" => r(P::OneClick, &[N::Near]),
            W::CrossSwap { on, at, .. } if on == "near" || at == "near" => swaps(&|| r(P::OneClick, &places([on, at]))),
            W::SwapOn { place: p, .. } if p == "tempo" => swaps(&|| network(N::Tempo)),
            // Whichever of Uniswap, KyberSwap and Relay's swap is on there (the executor's bestPair takes only those);
            // refused, in Uniswap's words, when none is.
            W::SwapOn { place: p, .. } => swaps(&|| {
                let via = place(p);
                let ways = [r(P::Uniswap, &via), r(P::Kyberswap, &via), r(P::Relay, &via)];
                if ways.iter().any(Option::is_none) { None } else { ways[0].clone() }
            }),
            W::SwapPrivately { on, at, .. } => swaps(&|| r(P::OneClick, &places([on, at]))),
            W::CrossSwap { on, at, .. } => swaps(&|| r(P::Relay, &places([on, at]))),
            // 1Click's other end: a buy pays with Solana USDC, what's received lands there, a sale pays out on `to` (nil: Solana).
            W::BuyNear { .. } | W::ReceiveAcross { .. } => r(P::OneClick, &[N::Solana]),
            W::SellNear { to, .. } => r(P::OneClick, &to.as_deref().map_or_else(|| vec![N::Solana], place)),
            W::SendNear { .. } | W::CloseNear { .. } => network(N::Near),
            W::SendSol { .. } => network(N::Solana),
            W::SendToken { network: net, .. } => net.as_deref().and_then(ModuleNetwork::from_place).and_then(network),
            W::AgentCreate { .. } | W::AgentSend { .. } | W::AgentFreeze(_) => None,
            W::AgentFund { .. } => r(P::OneClick, &[N::Solana]),
            W::SendPrivately { network: net, .. } => {
                r(P::OneClick, &[vec![N::Solana], net.as_deref().and_then(ModuleNetwork::from_place).into_iter().collect()].concat())
            }
            W::Intents { from, to, .. } => r(P::OneClick, &places([from, to])),
            // Reading them and setting one to zero is always allowed.
            W::ShowApprovals | W::RevokeApproval { .. } => None,
            W::ContractCall(c) => ModuleNetwork::from_place(&c.network).and_then(network),
            W::SweepDust { .. } => swaps(&|| r(P::Uniswap, &[])),
            // Each vault is checked by the planner; with the whole kind off there's nothing to plan.
            W::Allocate { .. } | W::Rebalance { .. } | W::ClaimRewards | W::SweepRewards { .. } => if self.is_on(Earn) { None } else { kind_off(Earn) },
            W::Pay { .. } => r(P::Mpp, &[]),
            W::Gas { place: p, .. } => r(P::Relay, &place(p)),
            W::Bridge { from, to, .. } => r(P::Relay, &places([from, to])),
            W::Cctp { from, to, .. } => first_off(places([from, to])).or_else(|| if self.is_on(Bridges) { None } else { kind_off(Bridges) }),
            W::Usdt { from, to, layer_zero, .. } => {
                if *layer_zero && !(stablecoins::USDT0_NETWORKS.contains(&from.as_str()) && stablecoins::USDT0_NETWORKS.contains(&to.as_str())) {
                    return Some(if ru {
                        format!("Мост USDT0 на LayerZero ходит между Ethereum, Arbitrum, Optimism, Polygon и HyperEVM. Между другими сетями USDT меняет Relay: «swap N USDT on {from} for USDT on {to}».")
                    } else {
                        format!("Tether's USDT0 bridge on LayerZero runs between Ethereum, Arbitrum, Optimism, Polygon and HyperEVM. Between other networks Relay swaps USDT: «swap N USDT on {from} for USDT on {to}».")
                    });
                }
                if let Some(off) = first_off(places([from, to])) { return Some(off) }
                if *layer_zero { return if self.is_on(Bridges) { None } else { kind_off(Bridges) } }
                swaps(&|| r(P::Relay, &[]))
            }
            W::MultisigCreate { networks, .. } => first_off(networks.iter().filter_map(|n| ModuleNetwork::from_place(n)).collect()),
            W::MultisigSend { network: net, .. } => net.as_deref().and_then(ModuleNetwork::from_place).and_then(network),
            W::SendTo { .. } | W::Yields(_) | W::Receive | W::StopSchedules | W::MultisigShow => None,
        }
    }
}

#[cfg(test)]
#[path = "modules/tests.rs"]
mod tests;
