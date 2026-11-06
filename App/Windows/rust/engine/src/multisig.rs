//! Safe multisigs made in ihatecrypto (Multisig.swift): kept in multisigs.json, for the user, not per wallet.
//! Layer 2 adds here: the set-up card (multisig_key, update_multisig_draft, candidates), creating one, a payment out of
//! one and its signatures, multisig_fees, reading what each holds.

use ihatecrypto_core::foundation::number;
use ihatecrypto_core::position_command::WalletCommand;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use signer_api::SignerAddress;

/// A Safe multisig: where it is, its keys and how many must sign. The chain is the truth — its owners and threshold are
/// read from it before every move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Multisig {
    pub address: String,
    pub keys: Vec<MultisigKey>,
    pub threshold: i64,
    pub salt: u64,
    /// The networks it's made on (EvmNetwork keys): the same address on each.
    pub networks: Vec<String>,
}

impl Multisig {
    pub fn id(&self) -> &str { &self.address }
}

/// `Multisig.Key`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MultisigKey {
    pub name: String,
    /// Lowercase.
    pub address: String,
    /// «pc»: the signer's key on this computer (Windows Hello); «ledger»: the connected Ledger; «address»: a saved
    /// address (a phone's wallet, a person) that signs in Safe's own app.
    pub kind: String,
}

impl MultisigKey {
    pub fn id(&self) -> &str { &self.address }
}

/// What a multisig holds on one of its networks, for its block (the Mac's MultisigBoard «By network»): its coins as a
/// line and their dollars; both None while the network's node hasn't answered.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MultisigNetHolding {
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coins: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usd: Option<Decimal>,
}

/// The set-up card's choices, before Confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultisigDraft {
    pub keys: Vec<MultisigKey>,
    pub threshold: i64,
    pub networks: Vec<String>,
    pub salt: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

impl MultisigDraft {
    /// More than half: lose fewer than half the devices and it still opens; a thief needs more than half of them.
    pub fn majority(&self) -> i64 { self.keys.len() as i64 / 2 + 1 }
}

/// A payment out of a multisig, read for its card: what, to whom, and which of its keys sign it here.
#[derive(Debug, Clone, PartialEq)]
pub struct MultisigPayment {
    pub command: WalletCommand,
    pub multisig: Multisig,
    pub net: String,
    pub to: SignerAddress,
    pub symbol: String,
    pub token: Option<String>,
    pub amount: Decimal,
    pub units: Decimal,
    pub threshold: i64,
    pub signers: Vec<MultisigKey>,
    /// What the multisig holds of it now, for the card's balance box (the Mac's MultisigPayment.held).
    pub held: Decimal,
}

/// Where Safe's contracts run among ihatecrypto's networks.
pub const MULTISIG_NETWORKS: [&str; 9] = ["base", "arbitrum", "optimism", "ethereum", "polygon", "bnb", "hyperevm", "robinhood", "tempo"];
