//! Keccak, EIP-55, EIP-712 for flat structs, recoverable ECDSA and EIP-191 (`Keccak.swift`, `EIP712.swift`,
//! `Accounts.swift`'s checksum, `TypedData.swift`'s `PersonalMessage`).

use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

use crate::{Result, fail, to_hex};

/// Keccak-256 as Ethereum uses it (original Keccak padding, not SHA3-256's).
pub fn keccak(data: &[u8]) -> [u8; 32] { Keccak256::digest(data).into() }

/// EIP-55: a hex letter is uppercase when the matching nibble of keccak(lowercase address) is ≥ 8.
pub fn checksum(hex: &str) -> String {
    let lower = hex.strip_prefix("0x").unwrap_or(hex).to_lowercase();
    let hash = to_hex(&keccak(lower.as_bytes()));
    "0x".to_string()
        + &lower.chars().zip(hash.chars())
            .map(|(c, h)| if c.is_alphabetic() && h.to_digit(16).unwrap_or(0) >= 8 { c.to_ascii_uppercase() } else { c })
            .collect::<String>()
}

/// EIP-712 typed-data hashing for flat structs, which is all Hyperliquid and a deposit need.
pub mod eip712 {
    use super::keccak;
    use crate::{Result, fail, from_hex};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        String(String),
        Address(String),
        Uint(u64),
        Bool(bool),
        Bytes32(Vec<u8>),
        /// Dynamic `bytes`: hashed into its word (keccak256).
        Bytes(Vec<u8>),
    }

    #[derive(Debug, Clone, PartialEq)]
    pub struct Field {
        pub name: String,
        pub ty: String,
        pub value: Value,
    }

    impl Field {
        pub fn new(name: &str, ty: &str, value: Value) -> Self { Self { name: name.into(), ty: ty.into(), value } }
    }

    pub fn hash_struct(ty: &str, fields: &[Field]) -> Result<[u8; 32]> {
        let declared = fields.iter().map(|f| format!("{} {}", f.ty, f.name)).collect::<Vec<_>>().join(",");
        let mut encoded = keccak(format!("{ty}({declared})").as_bytes()).to_vec();
        for f in fields { encoded.extend(encode(&f.value)?) }
        Ok(keccak(&encoded))
    }

    /// The domain separator and the message's struct hash: what a hardware wallet takes to sign typed data by hash.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Parts {
        pub domain: [u8; 32],
        pub message: [u8; 32],
    }

    impl Parts {
        /// keccak256(0x19 0x01 ‖ domainSeparator ‖ hashStruct(message)) — the digest a wallet signs.
        pub fn digest(&self) -> [u8; 32] { keccak(&[&[0x19, 0x01][..], &self.domain, &self.message].concat()) }
    }

    /// Typed data as a whole: what a Ledger takes to show each field by name before it signs.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Typed {
        pub domain: Vec<Field>,
        pub ty: String,
        pub message: Vec<Field>,
    }

    impl Typed {
        pub fn parts(&self) -> Result<Parts> { parts(&self.domain, &self.ty, &self.message) }
    }

    pub fn parts(domain: &[Field], ty: &str, message: &[Field]) -> Result<Parts> {
        Ok(Parts { domain: hash_struct("EIP712Domain", domain)?, message: hash_struct(ty, message)? })
    }

    pub fn digest(domain: &[Field], ty: &str, message: &[Field]) -> Result<[u8; 32]> { Ok(parts(domain, ty, message)?.digest()) }

    fn encode(value: &Value) -> Result<[u8; 32]> {
        let mut word = [0u8; 32];
        match value {
            Value::String(s) => return Ok(keccak(s.as_bytes())),
            Value::Address(a) => match from_hex(a) {
                Some(bytes) if bytes.len() == 20 => word[12..].copy_from_slice(&bytes),
                _ => return fail(format!("Invalid address: {a}")),
            },
            Value::Uint(n) => word[24..].copy_from_slice(&n.to_be_bytes()),
            Value::Bool(b) => word[31] = *b as u8,
            Value::Bytes32(d) => {
                let Ok(d) = <[u8; 32]>::try_from(d.as_slice()) else { return fail("bytes32 must be 32 bytes.") };
                return Ok(d);
            }
            Value::Bytes(d) => return Ok(keccak(d)),
        }
        Ok(word)
    }
}

/// A recoverable signature; `v` is 27 or 28, as eth_account and Hyperliquid expect. `r` and `s` cross a pipe as
/// base64, the way Swift's `Codable` writes `Data`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvmSignature {
    #[serde(with = "crate::b64")]
    pub r: Vec<u8>,
    #[serde(with = "crate::b64")]
    pub s: Vec<u8>,
    pub v: i64,
}

/// Recoverable ECDSA over a 32-byte digest, RFC 6979 nonces and low-s: the same signature eth_account and
/// libsecp256k1 produce for the same key and digest.
pub fn sign(digest: &[u8], private_key: &[u8]) -> Result<EvmSignature> {
    if digest.len() != 32 { return fail("Only a 32-byte hash can be signed.") }
    let Ok(key) = SigningKey::from_slice(private_key) else { return fail("Not a valid secp256k1 private key.") };
    let Ok((signature, recovery)) = key.sign_prehash_recoverable(digest) else { return fail("The signature failed.") };
    let (r, s) = signature.split_bytes();
    Ok(EvmSignature { r: r.to_vec(), s: s.to_vec(), v: recovery.to_byte() as i64 + 27 })
}

/// The EIP-55 address that made `signature` over `digest`: how a signature from a device is checked before use.
pub fn recover(digest: &[u8], signature: &EvmSignature) -> Result<String> {
    let bad = || fail("Not a signature over a 32-byte hash.");
    if digest.len() != 32 || signature.r.len() != 32 || signature.s.len() != 32 || !(27..=28).contains(&signature.v) { return bad() }
    // A signature that recovers no key (r or s of 0 or ≥ n, an r that is no point's x, a key at infinity) is refused,
    // never a crash: these bytes come from a device or a co-signer.
    let unverified = || fail("The signature didn't verify.");
    let Ok(sig) = Signature::from_slice(&[signature.r.as_slice(), &signature.s].concat()) else { return unverified() };
    // libsecp256k1 recovers from a high-s signature too; normalising keeps that and flips the parity to match.
    let (sig, flip) = match sig.normalize_s() { Some(low) => (low, 1), None => (sig, 0) };
    let Some(id) = RecoveryId::from_byte((signature.v - 27) as u8 ^ flip) else { return unverified() };
    let Ok(key) = VerifyingKey::recover_from_prehash(digest, &sig, id) else { return unverified() };
    let point = key.to_encoded_point(false);
    Ok(checksum(&to_hex(&keccak(&point.as_bytes()[1..])[12..])))
}

/// EIP-191 personal_sign: what «Sign-In with Ethereum» and most site logins ask for.
pub mod personal {
    use super::keccak;

    pub fn digest(message: &[u8]) -> [u8; 32] {
        keccak(&[format!("\u{19}Ethereum Signed Message:\n{}", message.len()).as_bytes(), message].concat())
    }

    /// 32 raw bytes are a hash, not text: signing one blind can authorise anything.
    pub fn is_blind_hash(message: &[u8]) -> bool {
        message.len() == 32 && std::str::from_utf8(message).map_or(true, |s| s.chars().any(|c| (c as u32) < 0x20))
    }

    /// The prompt for text a site asks to sign (personal_sign, Solana's signMessage): the first `shown` characters. A
    /// longer text says its whole length on the first line, before the site's text so nothing in it can move that
    /// line, and ends in «…» where it was cut.
    pub fn prompt(text: &str, shown: usize) -> String {
        use unicode_segmentation::UnicodeSegmentation;
        let count = text.graphemes(true).count();
        if count <= shown { return format!("sign text for a site:\n{text}") }
        format!("sign text for a site ({count} characters, the first {shown} shown):\n{}…", text.graphemes(true).take(shown).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_hex;

    #[test]
    fn keccak_matches_ethereum_vectors() {
        assert_eq!(to_hex(&keccak(b"")), "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");
        assert_eq!(to_hex(&keccak(b"abc")), "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45");
    }

    #[test]
    fn checksum_matches_eip55() {
        assert_eq!(checksum("0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed"), "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
        assert_eq!(checksum("FB6916095CA1DF60BB79CE92CE3EA74C37C5D359"), "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359");
    }

    #[test]
    fn signs_and_recovers() {
        let key = from_hex("0123456789012345678901234567890123456789012345678901234567890123").unwrap();
        let digest = keccak(b"ihatecrypto");
        let sig = sign(&digest, &key).unwrap();
        assert_eq!(recover(&digest, &sig).unwrap(), crate::keys::evm_address(&key).unwrap());
        // The same signature with a high s and the other parity is the same signer.
        let n = from_hex("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141").unwrap();
        let mut high = [0u8; 32];
        let mut borrow = 0i16;
        for i in (0..32).rev() {
            let d = n[i] as i16 - sig.s[i] as i16 - borrow;
            high[i] = d.rem_euclid(256) as u8;
            borrow = (d < 0) as i16;
        }
        let flipped = EvmSignature { r: sig.r.clone(), s: high.to_vec(), v: 55 - sig.v };
        assert_eq!(recover(&digest, &flipped).unwrap(), recover(&digest, &sig).unwrap());
        assert!(sign(&digest[..31], &key).is_err());
    }

    /// LedgerTests' cases: a signature that recovers no key is refused with main's words, not a crash.
    #[test]
    fn a_signature_that_recovers_no_key_is_refused_not_a_crash() {
        let digest = [0x11u8; 32];
        let n = from_hex("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141").unwrap();
        let gx = from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798").unwrap();
        let one = [vec![0u8; 31], vec![1]].concat();
        let bad = [(vec![0u8; 32], one.clone()), (one.clone(), n), (one.clone(), vec![0xff; 32]),
                   ([vec![0u8; 31], vec![5]].concat(), one.clone()), (gx, digest.to_vec())];
        for (r, s) in bad {
            assert_eq!(recover(&digest, &EvmSignature { r, s, v: 27 }).unwrap_err().0, "The signature didn't verify.");
        }
        assert_eq!(recover(&digest[..31], &EvmSignature { r: one.clone(), s: one, v: 27 }).unwrap_err().0, "Not a signature over a 32-byte hash.");
    }

    #[test]
    fn signature_crosses_the_pipe_as_swift_writes_it() {
        let sig = EvmSignature { r: vec![1; 32], s: vec![2; 32], v: 27 };
        let json = serde_json::to_string(&sig).unwrap();
        assert_eq!(json, r#"{"r":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=","s":"AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=","v":27}"#);
        assert_eq!(serde_json::from_str::<EvmSignature>(&json).unwrap(), sig);
    }

    #[test]
    fn personal_message() {
        assert!(personal::is_blind_hash(&[0xff; 32]));
        assert!(personal::is_blind_hash(&[0x01; 32]));
        assert!(!personal::is_blind_hash(&[b'a'; 32]));
        assert!(!personal::is_blind_hash(&[0xff; 31]));
        assert_eq!(personal::prompt("hi", 160), "sign text for a site:\nhi");
        assert_eq!(personal::prompt("abcdef", 4), "sign text for a site (6 characters, the first 4 shown):\nabcd…");
        // eth_account's encode_defunct("hello") hash.
        assert_eq!(to_hex(&personal::digest(b"hello")), "50b2c43fd39106bafbba0da34fc430e1f91e3c96ea2acee2bc34119f92b37750");
    }
}
