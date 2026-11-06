//! The Ledger as a wallet (Ledger.swift): its addresses kept in ledger.json, its connect card, whether it signs now.
//! Layer 2 adds here: the transports (USB through winsec's HID calls, Bluetooth LE through its GATT calls, as ledger's
//! `winsec` modules), connect_ledger, show_on_ledger, forget_ledger, the signer a move uses while it's on, and the rest
//! of the file's `extension Engine`.

use serde::{Deserialize, Serialize};
use signer_api::SignerWallet;

/// The Ledger's addresses as a wallet in the list: nothing of it is on this computer but what it shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerAccount {
    #[serde(default = "ledger_label")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solana: Option<String>,
    #[serde(default = "evm_path")]
    pub evm_path: String,
    #[serde(default = "solana_path")]
    pub solana_path: String,
    /// It signs now instead of the signer's wallet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    /// Found over Bluetooth rather than USB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bluetooth: Option<bool>,
}

fn ledger_label() -> String { "Ledger".into() }
fn evm_path() -> String { ledger::EVM_PATH.into() }
fn solana_path() -> String { ledger::SOLANA_PATH.into() }

impl Default for LedgerAccount {
    fn default() -> Self {
        LedgerAccount { label: ledger_label(), model: None, evm: None, solana: None, evm_path: evm_path(), solana_path: solana_path(), active: None, bluetooth: None }
    }
}

impl LedgerAccount {
    pub const ID: &str = "ledger";

    pub fn wallet(&self) -> SignerWallet {
        SignerWallet { id: Self::ID.into(), label: self.label.clone(), kind: "ledger".into(), solana: self.solana.clone(), evm: self.evm.clone(), near: None,
                       test: None, automation: None }
    }
}

/// The connect card's progress on the canvas.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LedgerConnect {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solana: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Settings on the device to change (blind signing off, an app not installed).
    pub notes: Vec<String>,
    pub busy: bool,
}

impl Default for LedgerConnect {
    fn default() -> Self {
        LedgerConnect { model: None, evm: None, solana: None, status: "Looking for a Ledger on USB…".into(), problem: None, notes: vec![], busy: true }
    }
}
