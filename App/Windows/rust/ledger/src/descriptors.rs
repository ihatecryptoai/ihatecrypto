//! Ledger's own signed descriptors (`Descriptors.swift`), fetched the way Ledger Live fetches them: with one, the device
//! shows a USDC transfer or an approval in words («Send 25 USDC», «To AULW…») instead of asking for blind signing. The
//! device checks Ledger's signature on each, so a wrong or tampered descriptor is refused there, never trusted here.
//! Without the network the move still works, blind-signed.

use serde_json::Value;
use wallet::{from_hex, to_hex};

use crate::{LedgerError, Result};

pub const CAL: &str = "https://crypto-assets-service.api.ledger.com/v1";
pub const NAMES: &str = "https://nft.api.live.ledger.com/v2";

async fn json(url: &str) -> Result<Value> {
    // Ledger's servers refuse some default user agents.
    let (body, status) = net::get(url, &[("User-Agent", "ihatecrypto/1.0")]).await.map_err(|e| LedgerError::new(e.to_string()))?;
    if status != 200 { return Err(LedgerError::new(format!("Ledger's descriptor service answered {status}."))) }
    serde_json::from_slice(&body).map_err(|e| LedgerError::new(e.to_string()))
}

/// `descriptor.data` and its production signature from one of Ledger's entries.
pub fn signed(entry: Option<&Value>) -> Result<(Vec<u8>, Vec<u8>)> {
    let d = entry.and_then(|e| e.get("descriptor"));
    let data = d.and_then(|d| d.get("data")?.as_str()).and_then(from_hex);
    let signature = d.and_then(|d| d.get("signatures")?.get("prod")?.as_str()).and_then(from_hex);
    match (data, signature) {
        (Some(data), Some(signature)) => Ok((data, signature)),
        _ => Err(LedgerError::new("Ledger's descriptor had no signed data.")),
    }
}

/// The Ethereum app's token payload from Ledger's answer: ticker length ‖ ticker…, address, decimals, chain ‖ Ledger's
/// DER signature.
pub fn erc20_payload(list: &Value, chain_id: u64, contract: &str) -> Result<Vec<u8>> {
    let entry = list.as_array().and_then(|l| l.first());
    let Some(ticker) = entry.and_then(|e| e.get("ticker")?.as_str()) else {
        return Err(LedgerError::new(format!("Ledger doesn't list token {contract} on chain {chain_id}.")));
    };
    let (data, signature) = signed(entry)?;
    Ok([vec![ticker.len() as u8], data, signature].concat())
}

pub async fn erc20(chain_id: u64, contract: &str) -> Result<Vec<u8>> {
    let list = json(&format!("{CAL}/tokens?chain_id={chain_id}&contract_address={}&output=ticker,decimals,descriptor", contract.to_lowercase())).await?;
    erc20_payload(&list, chain_id, contract)
}

/// The descriptor service's name for a device model.
pub fn target(model: Option<&str>) -> &'static str {
    match model { Some("Nano S") => "nanos", Some("Nano X") => "nanox", Some("Stax") => "stax", Some("Flex") => "flex", Some("Apex") => "apexp", _ => "nanosp" }
}

/// The certificate payload from Ledger's answer: data ‖ 0x15 ‖ length ‖ signature.
pub fn certificate_payload(list: &Value) -> Result<Vec<u8>> {
    let (data, signature) = signed(list.as_array().and_then(|l| l.first()))?;
    Ok([data, vec![0x15, signature.len() as u8], signature].concat())
}

/// Ledger's trusted-name certificate for this model, which the device checks against Ledger's root before it accepts
/// name descriptors.
pub async fn trusted_name_certificate(model: Option<&str>) -> Result<Vec<u8>> {
    let list = json(&format!("{CAL}/certificates?output=id,descriptor&target_device={}&latest=true&public_key_usage=trusted_name&public_key_id=domain_metadata_key", target(model))).await?;
    certificate_payload(&list)
}

/// Ledger's signed statement that `token_account` is `owner`'s account for `mint`, bound to the device's `challenge`:
/// the Solana app then shows the owner as the recipient. A new account (made by the same transaction) is «computed»; an
/// existing one is looked up.
pub async fn solana_token_account(owner: &str, mint: &str, token_account: &str, challenge: &[u8]) -> Result<Vec<u8>> {
    let c = format!("0x{}", to_hex(challenge));
    for url in [format!("{NAMES}/solana/computed-token-account/{owner}/{mint}?challenge={c}"), format!("{NAMES}/solana/owner/{token_account}?challenge={c}")] {
        let Ok(r) = json(&url).await else { continue };
        if r.get("tokenAccount").and_then(Value::as_str) != Some(token_account) { continue }
        if let Some(tlv) = r.get("signedDescriptor").and_then(Value::as_str).and_then(from_hex) { return Ok(tlv) }
    }
    Err(LedgerError::new(format!("Ledger couldn't name the owner of {token_account}.")))
}
