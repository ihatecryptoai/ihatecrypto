//! A Ledger talks in APDUs (`Wallet/Ledger/Ledger.swift`): a command (class, instruction, two parameters, up to 255
//! bytes) and a reply that ends in a two-byte status word. The keys never leave the device; every signature is shown on
//! its screen and approved with its buttons. Over USB the APDUs go in 64-byte HID packets, over Bluetooth in packets as
//! large as the link allows (both through winsec on Windows), and the Speculos emulator takes them over TCP.

pub mod descriptors;
pub mod tcp;
#[cfg(windows)]
pub mod windows;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use wallet::evm::eip712::{Parts, Typed, Value};
use wallet::evm::{EvmSignature, checksum};
use wallet::keys::{base58, parse_path};
use wallet::from_hex;

/// An error the person reads, with the device's status word when it gave one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerError {
    pub status: Option<u16>,
    pub message: String,
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.message) }
}

impl std::error::Error for LedgerError {}

impl LedgerError {
    pub fn new(message: impl Into<String>) -> Self { LedgerError { status: None, message: message.into() } }

    pub fn declined(&self) -> bool { matches!(self.status, Some(0x6985 | 0x5501)) }

    /// Another app, or the dashboard, is open instead of the one the command was for.
    pub fn wrong_app(&self) -> bool { matches!(self.status, Some(0x6d00 | 0x6e00 | 0x6e01 | 0x6511 | 0x6a82)) }

    /// What a status word means for the person holding the device, in plain words.
    pub fn from_status(status: u16, app: &str) -> Self {
        let message = match status {
            0x6985 | 0x5501 => "You declined it on the Ledger. Nothing was signed.".to_string(),
            0x5515 | 0x6b0c | 0x6982 => "The Ledger is locked: enter your PIN on it.".to_string(),
            0x6d00 | 0x6e00 | 0x6e01 | 0x6511 | 0x6a82 => format!("Open the {app} app on your Ledger."),
            0x6807 => format!("The {app} app isn't installed on this Ledger: add it in Ledger Live (My Ledger)."),
            0x6a80 if app == "Ethereum" => "The Ethereum app refused the data: turn on «Blind signing» in its settings on the Ledger — contract calls and Hyperliquid orders need it.".to_string(),
            0x6808 if app == "Solana" => "The Solana app can't show this transaction in full: turn on «Blind signing» in its settings on the Ledger — USDC transfers, swaps and lending need it for now.".to_string(),
            _ => format!("The Ledger answered 0x{status:x} in the {app} app."),
        };
        LedgerError { status: Some(status), message }
    }
}

pub type Result<T> = std::result::Result<T, LedgerError>;

/// Ledger's framing: tag 0x05, a sequence number, and on the first packet the APDU's length. Over USB (HID) each packet
/// starts with channel 0x0101 and is padded to 64 bytes; over Bluetooth there is neither, and a packet is at most the
/// link's MTU.
pub mod framing {
    use super::*;

    pub const HID_PACKET_SIZE: usize = 64;

    pub fn packets(apdu: &[u8], size: usize, hid: bool) -> Vec<Vec<u8>> {
        let mut rest = [&[(apdu.len() >> 8) as u8, (apdu.len() & 0xff) as u8][..], apdu].concat();
        let mut out: Vec<Vec<u8>> = Vec::new();
        loop {
            let n = out.len();
            let mut p = if hid { vec![0x01, 0x01] } else { vec![] };
            p.extend([0x05, (n >> 8) as u8, (n & 0xff) as u8]);
            let take = size.saturating_sub(p.len()).min(rest.len());
            p.extend(rest.drain(..take));
            if hid { p.resize(size, 0) }
            out.push(p);
            if rest.is_empty() { return out }
        }
    }

    /// The reply once every packet of it has arrived; None while more are due.
    pub fn reply(packets: &[Vec<u8>], hid: bool) -> Result<Option<Vec<u8>>> {
        let head = if hid { 5 } else { 3 };
        let mut body = Vec::new();
        for (i, b) in packets.iter().enumerate() {
            let ok = b.len() >= head && (!hid || (b[0] == 1 && b[1] == 1)) && b[head - 3] == 5
                && ((b[head - 2] as usize) << 8 | b[head - 1] as usize) == i;
            if !ok { return Err(LedgerError::new("The Ledger sent a packet out of order.")) }
            body.extend(&b[head..]);
        }
        if body.len() < 2 { return Ok(None) }
        let length = (body[0] as usize) << 8 | body[1] as usize;
        if body.len() - 2 < length { return Ok(None) }
        Ok(Some(body[2..2 + length].to_vec()))
    }
}

/// One APDU out and its reply with the status word back. `timeout`: how long the person may take on the device.
pub type Exchanged<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send + 'a>>;

pub trait Transport: Send + Sync {
    fn exchange(&self, apdu: Vec<u8>, timeout: Duration) -> Exchanged<'_>;
}

/// Seconds the person has to read and approve on the device.
const CONFIRM: Duration = Duration::from_secs(180);
const QUICK: Duration = Duration::from_secs(10);

/// One Ledger and the apps on it. Every call names the app it needs; the caller opens it first (`open`).
#[derive(Clone)]
pub struct Device {
    pub transport: Arc<dyn Transport>,
}

pub const EVM_PATH: &str = "m/44'/60'/0'/0/0";
/// Ledger Live's first Solana account.
pub const SOLANA_PATH: &str = "m/44'/501'/0'";

fn signature65(r: &[u8], what: &str) -> Result<EvmSignature> {
    if r.len() < 65 { return Err(LedgerError::new(format!("The {what} app sent no signature."))) }
    let v = r[0] as i64;
    Ok(EvmSignature { r: r[1..33].to_vec(), s: r[33..65].to_vec(), v: if v < 27 { v + 27 } else { v } })
}

impl Device {
    pub fn new(transport: Arc<dyn Transport>) -> Self { Device { transport } }

    pub async fn send(&self, cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8], app: &str, timeout: Duration) -> Result<Vec<u8>> {
        if data.len() > 255 { return Err(LedgerError::new("A command for the Ledger is at most 255 bytes.")) }
        let apdu = [&[cla, ins, p1, p2, data.len() as u8][..], data].concat();
        let reply = self.transport.exchange(apdu, timeout).await?;
        if reply.len() < 2 { return Err(LedgerError::new("The Ledger sent an empty reply.")) }
        let status = u16::from_be_bytes([reply[reply.len() - 2], reply[reply.len() - 1]]);
        if status != 0x9000 { return Err(LedgerError::from_status(status, app)) }
        Ok(reply[..reply.len() - 2].to_vec())
    }

    /// BIP-32 path as the apps take it: the number of steps, then each as a big-endian 32-bit index.
    pub fn path(path: &str) -> Result<Vec<u8>> {
        let steps = parse_path(path).map_err(|e| LedgerError::new(e.0))?;
        let mut out = vec![steps.len() as u8];
        for s in steps { out.extend(s.to_be_bytes()) }
        Ok(out)
    }

    // MARK: Dashboard

    /// The app open now and its version; «BOLOS» is the dashboard.
    pub async fn open_app(&self) -> Result<(String, String)> {
        let r = self.send(0xb0, 0x01, 0, 0, &[], "dashboard", QUICK).await?;
        let unsaid = || LedgerError::new("The Ledger didn't say which app is open.");
        if r.len() < 3 || r[0] != 1 { return Err(unsaid()) }
        let n = r[1] as usize;
        if r.len() < 3 + n { return Err(unsaid()) }
        let name = String::from_utf8_lossy(&r[2..2 + n]).into_owned();
        let v = r[2 + n] as usize;
        let version = if r.len() >= 3 + n + v { String::from_utf8_lossy(&r[3 + n..3 + n + v]).into_owned() } else { String::new() };
        Ok((name, version))
    }

    async fn open_now(&self) -> Option<String> { self.open_app().await.ok().map(|a| a.0) }

    /// Opens `app` («Ethereum», «Solana»), quitting whatever app is open first. The device may ask to allow it. The
    /// Ledger reconnects over USB after a switch, so the next call finds it anew.
    pub async fn open(&self, app: &str) -> Result<()> {
        let current = self.open_app().await?.0;
        if current == app { return Ok(()) }
        if current != "BOLOS" {
            self.send(0xb0, 0xa7, 0, 0, &[], &current, QUICK).await?;
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_millis(300)).await;
                if self.open_now().await.as_deref() == Some("BOLOS") { break }
            }
        }
        self.send(0xe0, 0xd8, 0, 0, app.as_bytes(), app, CONFIRM).await?;
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(300)).await;
            if self.open_now().await.as_deref() == Some(app) { return Ok(()) }
        }
        Err(LedgerError::new(format!("The {app} app didn't open on the Ledger: open it by hand and try again.")))
    }

    // MARK: Ethereum app

    /// The EIP-55 address at `path`; `show`: the Ledger displays it for the person to compare, and waits for their OK.
    pub async fn evm_address(&self, path: &str, show: bool) -> Result<String> {
        let r = self.send(0xe0, 0x02, show as u8, 0, &Self::path(path)?, "Ethereum", if show { CONFIRM } else { QUICK }).await?;
        let none = || LedgerError::new("The Ethereum app sent no address.");
        let key_length = *r.first().ok_or_else(none)? as usize;
        if r.len() <= key_length + 1 { return Err(none()) }
        let address_length = r[key_length + 1] as usize;
        let start = key_length + 2;
        if r.len() < start + address_length || address_length != 40 { return Err(none()) }
        Ok(checksum(&String::from_utf8_lossy(&r[start..start + address_length])))
    }

    /// Whether «Blind signing» is on in the Ethereum app: contract calls and typed-data hashes need it.
    pub async fn evm_blind_signing(&self) -> Result<(bool, String)> {
        let r = self.send(0xe0, 0x06, 0, 0, &[], "Ethereum", QUICK).await?;
        if r.len() < 4 { return Err(LedgerError::new("The Ethereum app sent no settings.")) }
        Ok((r[0] & 0x01 != 0, format!("{}.{}.{}", r[1], r[2], r[3])))
    }

    /// Signs an unsigned EIP-2718 transaction (type ‖ rlp) after the person reviews it on the device. Returns the raw
    /// first byte (the y parity for a typed transaction), r and s.
    pub async fn sign_evm_transaction(&self, payload: &[u8], path: &str) -> Result<(u8, Vec<u8>, Vec<u8>)> {
        // 150-byte pieces, as Ledger's own libraries send them; the first one starts with the path.
        let head = Self::path(path)?;
        let (mut rest, mut first, mut reply) = (payload, true, Vec::new());
        while !rest.is_empty() || first {
            let room = if first { 150 - head.len() } else { 150 };
            let (piece, after) = rest.split_at(room.min(rest.len()));
            rest = after;
            let data = if first { [head.as_slice(), piece].concat() } else { piece.to_vec() };
            reply = self.send(0xe0, 0x04, if first { 0 } else { 0x80 }, 0, &data, "Ethereum", if rest.is_empty() { CONFIRM } else { QUICK }).await?;
            first = false;
        }
        if reply.len() < 65 { return Err(LedgerError::new("The Ethereum app sent no signature.")) }
        Ok((reply[0], reply[1..33].to_vec(), reply[33..65].to_vec()))
    }

    /// Ledger's signed token descriptor, so the next transaction's ERC-20 transfer or approval shows its amount and
    /// ticker instead of asking for blind signing.
    pub async fn provide_erc20(&self, payload: &[u8]) -> Result<()> {
        self.send(0xe0, 0x0a, 0, 0, payload, "Ethereum", QUICK).await.map(|_| ())
    }

    /// EIP-712 by its two hashes: the device shows them and signs keccak(0x1901 ‖ domain ‖ message). Needs «Blind
    /// signing»: the device can't show what the hashes stand for.
    pub async fn sign_typed_data_hashes(&self, parts: &Parts, path: &str) -> Result<EvmSignature> {
        let data = [Self::path(path)?.as_slice(), &parts.domain, &parts.message].concat();
        signature65(&self.send(0xe0, 0x0c, 0, 0, &data, "Ethereum", CONFIRM).await?, "Ethereum")
    }

    /// EIP-712 in full: the Ledger gets the types and every value, hashes them itself and shows each field by name
    /// before it signs. The app may show a field as it arrives and wait for the person, so every step may take as long
    /// as a signature.
    pub async fn sign_typed_data(&self, typed: &Typed, path: &str) -> Result<EvmSignature> {
        let structs = [("EIP712Domain", &typed.domain), (typed.ty.as_str(), &typed.message)];
        for (name, fields) in structs {
            self.send(0xe0, 0x1a, 0, 0x00, name.as_bytes(), "Ethereum", QUICK).await?;
            for f in fields.iter() {
                let data = [field_type(&f.ty)?, vec![f.name.len() as u8], f.name.as_bytes().to_vec()].concat();
                self.send(0xe0, 0x1a, 0, 0xff, &data, "Ethereum", QUICK).await?;
            }
        }
        for (name, fields) in structs {
            self.send(0xe0, 0x1c, 0, 0x00, name.as_bytes(), "Ethereum", QUICK).await?;
            for f in fields.iter() {
                let value = field_value(&f.value)?;
                let whole = [&[(value.len() >> 8) as u8, (value.len() & 0xff) as u8][..], &value].concat();
                let mut pieces = whole.chunks(255).peekable();
                while let Some(piece) = pieces.next() {
                    self.send(0xe0, 0x1c, if pieces.peek().is_some() { 1 } else { 0 }, 0xff, piece, "Ethereum", CONFIRM).await?;
                }
            }
        }
        signature65(&self.send(0xe0, 0x0c, 0, 0x01, &Self::path(path)?, "Ethereum", CONFIRM).await?, "Ethereum")
    }

    // MARK: Solana app

    pub async fn solana_address(&self, path: &str, show: bool) -> Result<String> {
        let r = self.send(0xe0, 0x05, show as u8, 0, &Self::path(path)?, "Solana", if show { CONFIRM } else { QUICK }).await?;
        if r.len() != 32 { return Err(LedgerError::new("The Solana app sent no address.")) }
        Ok(base58::encode(&r))
    }

    /// A certificate Ledger signed with its root: the key the next name descriptors are checked with. An OS command,
    /// answered by whichever app is open.
    pub async fn load_certificate(&self, payload: &[u8], usage: u8, app: &str) -> Result<()> {
        self.send(0xb0, 0x06, usage, 0, payload, app, QUICK).await.map(|_| ())
    }

    /// Four fresh bytes the next trusted-name descriptor must carry, so an old one can't be replayed.
    pub async fn challenge(&self, app: &str) -> Result<Vec<u8>> {
        let r = self.send(0xe0, 0x20, 0, 0, &[], app, QUICK).await?;
        if r.len() != 4 { return Err(LedgerError::new(format!("The {app} app sent no challenge."))) }
        Ok(r)
    }

    /// «This token account is that owner's», signed by Ledger for the challenge: the Solana app then names the recipient.
    pub async fn provide_trusted_name(&self, tlv: &[u8], app: &str) -> Result<()> {
        self.send(0xe0, 0x21, 0, 0, tlv, app, QUICK).await.map(|_| ())
    }

    pub async fn solana_blind_signing(&self) -> Result<(bool, String)> {
        let r = self.send(0xe0, 0x04, 0, 0, &[], "Solana", QUICK).await?;
        if r.len() < 5 { return Err(LedgerError::new("The Solana app sent no settings.")) }
        Ok((r[0] != 0, format!("{}.{}.{}", r[2], r[3], r[4])))
    }

    /// Signs a Solana transaction message (what follows the signatures in the wire) after the person reviews it.
    pub async fn sign_solana(&self, message: &[u8], path: &str) -> Result<Vec<u8>> {
        // One signer, its path, then the message; in 255-byte pieces marked «more» / «extends the last one».
        let payload = [&[1u8][..], &Self::path(path)?, message].concat();
        let (mut offset, mut p2) = (0usize, 0u8);
        while payload.len() - offset > 255 {
            self.send(0xe0, 0x06, 1, p2 | 0x02, &payload[offset..offset + 255], "Solana", QUICK).await?;
            offset += 255;
            p2 |= 0x01;
        }
        let r = self.send(0xe0, 0x06, 1, p2, &payload[offset..], "Solana", CONFIRM).await?;
        if r.len() != 64 { return Err(LedgerError::new("The Solana app sent no signature.")) }
        Ok(r)
    }
}

/// A field's type as the Ethereum app reads it: a kind in the low bits, 0x40 when a byte size follows.
pub fn field_type(ty: &str) -> Result<Vec<u8>> {
    match ty {
        "string" => return Ok(vec![0x05]),
        "address" => return Ok(vec![0x03]),
        "bool" => return Ok(vec![0x04]),
        "bytes" => return Ok(vec![0x07]),
        _ => {}
    }
    for (prefix, kind) in [("uint", 2u8), ("int", 1), ("bytes", 6)] {
        if let Some(n) = ty.strip_prefix(prefix) {
            let Ok(n) = n.parse::<usize>() else { break };
            return Ok(vec![0x40 | kind, (if prefix == "bytes" { n } else { n / 8 }) as u8]);
        }
    }
    Err(LedgerError::new(format!("The Ledger can't be sent a field of type {ty}.")))
}

/// A value as raw bytes: an integer in as few bytes as it needs, an address in 20.
pub fn field_value(value: &Value) -> Result<Vec<u8>> {
    Ok(match value {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Address(a) => match from_hex(a) {
            Some(b) if b.len() == 20 => b,
            _ => return Err(LedgerError::new(format!("Invalid address: {a}"))),
        },
        Value::Uint(n) => {
            let trimmed: Vec<u8> = n.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect();
            if trimmed.is_empty() { vec![0] } else { trimmed }
        }
        Value::Bool(b) => vec![*b as u8],
        Value::Bytes32(d) | Value::Bytes(d) => d.clone(),
    })
}

#[cfg(test)]
mod tests;
