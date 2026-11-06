//! Settings › Modules (Modules.swift): every switch with the words the Mac shows beside it. Settings' `mirror()` (this
//! file's in Swift) is in store.rs, beside Settings.
//! Layer 2 adds here: modules_view, set_module, offer_new_modules.

use ihatecrypto_core::modules::{ModuleNetwork, ModuleProtocol};
use serde::Serialize;

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

/// Whether a protocol works on `n`: Uniswap, Aave and Morpho check the network per move, so their place names count too.
pub fn runs(p: ModuleProtocol, n: ModuleNetwork) -> bool { p.networks().contains(&n) || p.place().contains(n.name()) }
