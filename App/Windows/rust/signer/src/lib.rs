//! ihatecrypto's signing process for Windows (`App/Sources/Signer`): the vault under Windows Hello and DPAPI, the
//! trusted settings, the wallets, and — next — the signer's own decisions over requests from the engine.

pub mod core;
pub mod host;
pub mod platform;
pub mod trusted;
pub mod vault;
pub mod wallets;

#[cfg(test)]
mod tests;
