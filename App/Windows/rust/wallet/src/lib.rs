//! Keys, addresses and signatures — the Rust port of `App/Sources/Wallet/Core`. Same results as the Swift module
//! for the same input (the tests carry its vectors), same words in its errors, since the window shows them.

pub mod address_check;
pub mod evm;
pub mod keys;
pub mod mnemonic;
pub mod near;
pub mod typed_data;

/// An error the person reads: `WalletError` in Swift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletError(pub String);

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for WalletError {}

pub type Result<T> = std::result::Result<T, WalletError>;

pub(crate) fn fail<T>(message: impl Into<String>) -> Result<T> { Err(WalletError(message.into())) }

/// `Data(hex:)`: "0x…" or bare hex; None on odd length or a non-hex character.
pub fn from_hex(s: &str) -> Option<Vec<u8>> { hex::decode(s.strip_prefix("0x").unwrap_or(s)).ok() }

/// `Data.hex`: lowercase, no 0x.
pub fn to_hex(bytes: &[u8]) -> String { hex::encode(bytes) }

/// Swift's `Codable` writes `Data` as base64; fields that cross a pipe as `Data` use this.
pub mod b64 {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> { s.serialize_str(&STANDARD.encode(bytes)) }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        STANDARD.decode(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
