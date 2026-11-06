//! Checks an address the person saves: the right shape for its network, the checksum when there is one, and known
//! traps (`AddressCheck.swift`). An address is never guessed or corrected — a wrong one is refused with the reason.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};
use unicode_segmentation::UnicodeSegmentation;

use crate::evm::checksum;
use crate::keys::base58;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Solana, Hyperliquid, Ethereum, Arbitrum, Base, Robinhood, Near, Tempo, Polygon, Hyperevm, Optimism, Bnb,
    /// Networks ihatecrypto has no wallet on: NEAR Intents' 1Click delivers there to a saved address (an exchange's).
    Bitcoin, Zcash, Tron, Xrp, Dogecoin, Sui,
}

impl Network {
    pub const ALL: [Network; 18] = [
        Self::Solana, Self::Hyperliquid, Self::Ethereum, Self::Arbitrum, Self::Base, Self::Robinhood, Self::Near, Self::Tempo,
        Self::Polygon, Self::Hyperevm, Self::Optimism, Self::Bnb, Self::Bitcoin, Self::Zcash, Self::Tron, Self::Xrp, Self::Dogecoin, Self::Sui,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Solana => "Solana", Self::Hyperliquid => "Hyperliquid", Self::Ethereum => "Ethereum", Self::Arbitrum => "Arbitrum",
            Self::Base => "Base", Self::Robinhood => "Robinhood Chain", Self::Near => "NEAR", Self::Tempo => "Tempo",
            Self::Polygon => "Polygon", Self::Hyperevm => "HyperEVM", Self::Optimism => "Optimism", Self::Bnb => "BNB Chain",
            Self::Bitcoin => "Bitcoin", Self::Zcash => "Zcash", Self::Tron => "TRON", Self::Xrp => "XRP Ledger",
            Self::Dogecoin => "Dogecoin", Self::Sui => "Sui",
        }
    }

    fn is_evm(self) -> bool {
        matches!(self, Self::Hyperliquid | Self::Ethereum | Self::Arbitrum | Self::Base | Self::Robinhood | Self::Tempo
                       | Self::Polygon | Self::Hyperevm | Self::Optimism | Self::Bnb)
    }
}

/// Addresses that swallow money sent the usual way.
fn trap(lower: &str) -> Option<String> {
    (lower == "0x2df1c51e09aecf9cacb7bc98cb1742757f163df7").then(|| {
        "This is Hyperliquid’s old bridge (Bridge2): USDC sent here the wrong way is lost. Deposit through the Hyperliquid app instead.".into()
    })
}

fn is_hex(s: &str) -> bool { s.bytes().all(|b| b.is_ascii_hexdigit()) }

/// None when the address can be saved for this network; otherwise why not.
pub fn problem(address: &str, network: Network) -> Option<String> {
    let a = address.trim();
    if a.is_empty() { return Some("Paste the address.".into()) }
    let no = |s: &str| Some(s.to_string());
    match network {
        Network::Solana => {
            let ok = (32..=44).contains(&a.chars().count()) && base58::decode(a).is_ok_and(|b| b.len() == 32);
            if ok { None } else { no("Not a Solana address: 32–44 letters and digits in base58.") }
        }
        n if n.is_evm() => {
            let hex = a.strip_prefix("0x").filter(|h| h.len() == 40 && is_hex(h));
            let Some(hex) = hex else { return Some(format!("Not an {} address: 0x and 40 hex characters.", n.title())) };
            // Mixed case carries an EIP-55 checksum: one wrong character breaks it.
            if hex != hex.to_lowercase() && hex != hex.to_uppercase() && checksum(a) != a {
                return no("The checksum doesn’t match, so a character is wrong. Copy the address again from the source.");
            }
            trap(&a.to_lowercase())
        }
        Network::Near => {
            // An implicit account (64 hex, lower case) or a named one: lower-case parts joined by dots, ending in .near.
            let implicit = a.len() == 64 && a.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            let parts: Vec<&str> = a.split('.').collect();
            let named = a.chars().count() <= 64 && parts.len() >= 2 && parts.last() == Some(&"near")
                && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'));
            if implicit || named { None } else { no("Not a NEAR account: 64 hex characters, or a name ending in .near (lower case).") }
        }
        Network::Bitcoin => {
            if a.to_lowercase().starts_with("bc1") {
                return segwit(a, "bc").is_none().then(|| "Not a Bitcoin address: its bc1… checksum doesn't match. Copy it again.".into());
            }
            match base58_check(a, base58::ALPHABET) {
                Some(p) if p.len() == 21 && [0x00, 0x05].contains(&p[0]) => None,
                _ => no("Not a Bitcoin address: bc1…, 1… or 3…, with a checksum that matches."),
            }
        }
        Network::Dogecoin => match base58_check(a, base58::ALPHABET) {
            Some(p) if p.len() == 21 && [0x1e, 0x16].contains(&p[0]) => None,
            _ => no("Not a Dogecoin address: D… (or 9…/A…), with a checksum that matches."),
        },
        Network::Tron => match base58_check(a, base58::ALPHABET) {
            Some(p) if p.len() == 21 && p[0] == 0x41 => None,
            _ => no("Not a TRON address: T… and 34 characters, with a checksum that matches."),
        },
        Network::Zcash => match base58_check(a, base58::ALPHABET) {
            Some(p) if p.len() == 22 && p[0] == 0x1c && [0xb8, 0xbd].contains(&p[1]) => None,
            _ => no("Not a transparent Zcash address (t1… or t3…). NEAR Intents pays out to those; a shielded or unified one isn't taken here."),
        },
        // A classic r… account, or an X-address, which carries the destination tag an exchange needs.
        Network::Xrp => match base58_check(a, XRP_ALPHABET) {
            Some(p) if (p.len() == 21 && p[0] == 0) || (p.len() == 31 && p[..2] == [0x05, 0x44]) => None,
            _ => no("Not an XRP address: r… or X…, with a checksum that matches."),
        },
        Network::Sui => {
            let ok = a.strip_prefix("0x").is_some_and(|h| h.len() == 64 && is_hex(h));
            if ok { None } else { no("Not a Sui address: 0x and 64 hex characters.") }
        }
        _ => unreachable!("every EVM network is matched above"),
    }
}

const XRP_ALPHABET: &[u8; 58] = b"rpshnaf39wBUDNEGHJKLM4PQRST7VWXYZ2bcdeCg65jkm8oFqi1tuvAxyz";

fn double_sha(data: &[u8]) -> [u8; 32] { Sha256::digest(Sha256::digest(data)).into() }

/// Base58 with a 4-byte double SHA-256 checksum (Bitcoin's alphabet, or XRP's own): the payload, or None.
fn base58_check(s: &str, alphabet: &[u8; 58]) -> Option<Vec<u8>> {
    let mapped: String = s.chars().map(|c| alphabet.iter().position(|&a| a as char == c).map(|i| base58::ALPHABET[i] as char)).collect::<Option<_>>()?;
    let raw = base58::decode(&mapped).ok().filter(|r| r.len() > 4)?;
    let (payload, check) = raw.split_at(raw.len() - 4);
    (double_sha(payload)[..4] == *check).then(|| payload.to_vec())
}

/// A segwit address's witness version and program, its checksum checked (bech32 for version 0, bech32m after,
/// BIP-173 and BIP-350); None when anything is off.
fn segwit(s: &str, hrp: &str) -> Option<(u8, Vec<u8>)> {
    let a = s.to_lowercase();
    if !(s == a || s == s.to_uppercase()) || a.len() > 90 { return None }
    let sep = a.rfind('1')?;
    if &a[..sep] != hrp { return None }
    const CHARSET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
    let values: Vec<u32> = a[sep + 1..].bytes().map(|c| CHARSET.iter().position(|&x| x == c).map(|v| v as u32)).collect::<Option<_>>()?;
    let version = *values.first()?;
    if values.len() < 7 || version > 16 { return None }
    const GENERATOR: [u32; 5] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
    let mut check: u32 = 1;
    let expanded = hrp.bytes().map(|b| b as u32 >> 5).chain([0]).chain(hrp.bytes().map(|b| b as u32 & 31)).chain(values.iter().copied());
    for v in expanded {
        let top = check >> 25;
        check = (check & 0x1ffffff) << 5 ^ v;
        for (i, g) in GENERATOR.iter().enumerate() {
            if (top >> i) & 1 == 1 { check ^= g }
        }
    }
    if check != if version == 0 { 1 } else { 0x2bc830a3 } { return None }
    let (mut acc, mut bits, mut program) = (0u32, 0u32, Vec::new());
    for v in &values[1..values.len() - 6] {
        acc = (acc << 5 | v) & 0xfff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            program.push(((acc >> bits) & 0xff) as u8);
        }
    }
    let ok = bits < 5 && acc & ((1 << bits) - 1) == 0 && (2..=40).contains(&program.len()) && (version != 0 || [20, 32].contains(&program.len()));
    ok.then_some((version as u8, program))
}

/// An XRP account with a destination tag as one X-address (XLS-5d): how NEAR Intents takes a tag an exchange needs.
/// None when `classic` isn't an r… account.
pub fn x_address(classic: &str, tag: u32) -> Option<String> {
    let p = base58_check(classic, XRP_ALPHABET).filter(|p| p.len() == 21 && p[0] == 0)?;
    let mut payload = vec![0x05, 0x44];
    payload.extend(&p[1..]);
    payload.push(0x01);
    payload.extend(tag.to_le_bytes());
    payload.extend([0; 4]);
    let check = double_sha(&payload);
    payload.extend(&check[..4]);
    Some(base58::encode(&payload).bytes().map(|c| base58::ALPHABET.iter().position(|&a| a == c).map_or(c as char, |i| XRP_ALPHABET[i] as char)).collect())
}

/// Why a name or memo can't go on the signing prompt beside its address, or None: at most 64 characters, none that
/// break the line, reorder the text (bidi marks) or don't show — else a name could draw a fake address line.
pub fn label_problem(text: &str) -> Option<String> {
    if text.graphemes(true).count() > 64 { return Some("A name or memo is at most 64 characters.".into()) }
    if text.chars().any(hidden) { return Some("A name or memo can't hold line breaks or invisible characters.".into()) }
    None
}

/// Line breaks, control characters and invisible formatting (bidi overrides, zero-width marks).
pub fn hidden(c: char) -> bool {
    matches!(c.general_category(), GeneralCategory::Control | GeneralCategory::Format | GeneralCategory::LineSeparator | GeneralCategory::ParagraphSeparator)
}

/// «7xKX 9fQm … 3kPq 2Wd1»: the start and the end in groups of four, the way to compare an address by eye.
pub fn ends(address: &str) -> (String, String) {
    let chars: Vec<char> = address.chars().collect();
    let groups = |s: &[char]| s.chunks(4).map(|g| g.iter().collect::<String>()).collect::<Vec<_>>().join(" ");
    (groups(&chars[..chars.len().min(8)]), groups(&chars[chars.len().saturating_sub(8)..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_book_checks() {
        assert_eq!(problem("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", Network::Solana), None);
        assert!(problem("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyT", Network::Solana).is_some());
        assert!(problem("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt10", Network::Solana).is_some());
        assert_eq!(problem("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", Network::Base), None);
        assert_eq!(problem("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266", Network::Base), None);
        assert!(problem("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92267", Network::Base).unwrap().starts_with("The checksum"));
        assert!(problem("0x2Df1c51E09aECF9cacB7bc98cB1742757f163dF7", Network::Arbitrum).unwrap().contains("Bridge2"));
        assert_eq!(problem("0xabc", Network::Robinhood).unwrap(), "Not an Robinhood Chain address: 0x and 40 hex characters.");
        assert_eq!(problem("alice.near", Network::Near), None);
        assert_eq!(problem("5510e2b44cae6eb807e3e0e45d579dda058c274abcba15e5cb84636f5d1ee412", Network::Near), None);
        assert!(problem("Alice.near", Network::Near).is_some());
        assert!(problem("near", Network::Near).is_some());
        assert_eq!(problem("   ", Network::Sui).unwrap(), "Paste the address.");
    }

    #[test]
    fn other_networks() {
        // BIP-173 / BIP-350 vectors and well-known addresses.
        assert_eq!(problem("BC1QW508D6QEJXTDG4Y5R3ZARVARY0C5XW7KV8F3T4", Network::Bitcoin), None);
        assert_eq!(problem("bc1pw508d6qejxtdg4y5r3zarvary0c5xw7kw508d6qejxtdg4y5r3zarvary0c5xw7kt5nd6y", Network::Bitcoin), None);
        assert!(problem("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t5", Network::Bitcoin).is_some());
        assert_eq!(problem("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", Network::Bitcoin), None);
        assert!(problem("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb", Network::Bitcoin).is_some());
        assert_eq!(problem("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh", Network::Xrp), None);
        assert_eq!(problem("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t", Network::Tron), None);
        assert_eq!(problem(&format!("0x{}", "ab".repeat(32)), Network::Sui), None);
    }

    #[test]
    fn x_address_matches_xls5d_vector() {
        assert_eq!(x_address("rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf", 1).unwrap(), "XVLhHMPHU98es4dbozjVtdWzVrDjtV8xvjGQTYPiAx6gwDC");
        assert_eq!(x_address("rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf", 4294967294).unwrap(), "XVLhHMPHU98es4dbozjVtdWzVrDjtV1kAsixQTdMjbWi39u");
        assert_eq!(x_address("rGWrZyQqhTp9Xu7G5Pkayo7bXjH4k4QYpf", 4294967295).unwrap(), "XVLhHMPHU98es4dbozjVtdWzVrDjtV18pX8yuPT7y4xaEHi");
        assert_eq!(problem("XVLhHMPHU98es4dbozjVtdWzVrDjtV1kAsixQTdMjbWi39u", Network::Xrp), None);
    }

    #[test]
    fn labels_and_ends() {
        assert_eq!(label_problem("Mom"), None);
        assert!(label_problem("a\u{202e}b").is_some());
        assert!(label_problem("a\nb").is_some());
        assert!(label_problem(&"x".repeat(65)).is_some());
        assert_eq!(label_problem(&"é".repeat(64)), None);
        assert_eq!(ends("7xKX9fQmAAAA3kPq2Wd1"), ("7xKX 9fQm".into(), "3kPq 2Wd1".into()));
        assert_eq!(serde_json::to_string(&Network::Hyperevm).unwrap(), "\"hyperevm\"");
    }
}
