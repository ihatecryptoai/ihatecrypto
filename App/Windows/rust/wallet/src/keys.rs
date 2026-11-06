//! Keys from a seed, the addresses they sign for, base58 and pasted keys (`Derivation.swift`, `Accounts.swift`,
//! `ImportedKey.swift`).

use ed25519_dalek::SigningKey as EdKey;
use hmac::{Hmac, Mac};
use k256::elliptic_curve::{PrimeField, sec1::ToEncodedPoint};
use k256::{NonZeroScalar, Scalar, SecretKey};
use sha2::Sha512;
use zeroize::Zeroizing;

use crate::evm::{checksum, keccak};
use crate::{Result, fail, from_hex, to_hex};

const HARDENED: u32 = 0x8000_0000;

/// Paths common wallets use, so imported addresses match what the person already sees.
pub mod path {
    /// MetaMask, Rabby, Hardhat: m/44'/60'/0'/0/i
    pub fn evm(account: u32) -> String { format!("m/44'/60'/0'/0/{account}") }
    /// Phantom, Solflare: m/44'/501'/i'/0'
    pub fn solana(account: u32) -> String { format!("m/44'/501'/{account}'/0'") }
    /// MyNearWallet, Ledger, near-seed-phrase: m/44'/397'/i'
    pub fn near(account: u32) -> String { format!("m/44'/397'/{account}'") }
}

pub fn parse_path(path: &str) -> Result<Vec<u32>> {
    // Swift's split(separator:) drops empty parts, so "m//0" reads as "m/0".
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.first() != Some(&"m") { return fail(format!("The path must start with m/: {path}")) }
    parts[1..].iter().map(|part| {
        let hardened = part.ends_with('\'') || part.ends_with('h');
        let digits = if hardened { &part[..part.len() - 1] } else { part };
        match digits.parse::<u32>() {
            Ok(i) if i < HARDENED => Ok(if hardened { i | HARDENED } else { i }),
            _ => fail(format!("Invalid path step: {part}")),
        }
    }).collect()
}

type Secret = Zeroizing<[u8; 32]>;

fn hmac512(key: &[u8], data: &[u8]) -> (Secret, Secret) {
    let mut mac = Hmac::<Sha512>::new_from_slice(key).expect("HMAC takes any key");
    mac.update(data);
    let out = Zeroizing::new(<[u8; 64]>::from(mac.finalize().into_bytes()));
    let (mut left, mut right) = (Zeroizing::new([0u8; 32]), Zeroizing::new([0u8; 32]));
    left.copy_from_slice(&out[..32]);
    right.copy_from_slice(&out[32..]);
    (left, right)
}

/// BIP-32 on secp256k1: EVM and Hyperliquid keys. Returns the 32-byte private key.
pub fn secp256k1(seed: &[u8], path: &str) -> Result<Secret> {
    let steps = parse_path(path)?;
    let (mut key, mut chain) = hmac512(b"Bitcoin seed", seed);
    let invalid = || fail("The key this path leads to is invalid; BIP-32 skips it, and so does ihatecrypto.");
    let Ok(mut secret) = SecretKey::from_slice(&*key) else { return invalid() };
    for index in steps {
        let mut data = Zeroizing::new(Vec::with_capacity(37));
        if index >= HARDENED {
            data.push(0);
            data.extend_from_slice(&*key);
        } else {
            data.extend_from_slice(secret.public_key().to_encoded_point(true).as_bytes()); // compressed, 33 bytes
        }
        data.extend_from_slice(&index.to_be_bytes());
        let (tweak, next_chain) = hmac512(&*chain, &data);
        // child = (parent + tweak) mod n; a tweak ≥ n or a zero child is refused, as BIP-32 (and libsecp256k1) require.
        let Some(tweak) = Option::<Scalar>::from(Scalar::from_repr((*tweak).into())) else { return invalid() };
        let child = *secret.to_nonzero_scalar() + tweak;
        let Some(child) = Option::<NonZeroScalar>::from(NonZeroScalar::new(child)) else { return invalid() };
        secret = SecretKey::from(child);
        key.copy_from_slice(&secret.to_bytes());
        chain = next_chain;
    }
    Ok(key)
}

/// SLIP-0010 on ed25519: Solana and NEAR keys. Every step must be hardened.
pub fn ed25519(seed: &[u8], path: &str) -> Result<Secret> {
    let steps = parse_path(path)?;
    let (mut key, mut chain) = hmac512(b"ed25519 seed", seed);
    for index in steps {
        if index < HARDENED { return fail("For ed25519 every path step must be hardened (with ').") }
        let mut data = Zeroizing::new([0u8; 37]);
        data[1..33].copy_from_slice(&*key);
        data[33..].copy_from_slice(&index.to_be_bytes());
        (key, chain) = hmac512(&*chain, &*data);
    }
    Ok(key)
}

/// The uncompressed secp256k1 public key without its 0x04 prefix: 64 bytes, what an EVM address hashes.
pub fn secp256k1_public(private_key: &[u8]) -> Result<[u8; 64]> {
    let Ok(secret) = SecretKey::from_slice(private_key) else { return fail("Not a valid secp256k1 private key.") };
    let point = secret.public_key().to_encoded_point(false);
    Ok(point.as_bytes()[1..].try_into().expect("65-byte point"))
}

/// EIP-55 checksummed address of a secp256k1 private key.
pub fn evm_address(private_key: &[u8]) -> Result<String> {
    Ok(checksum(&to_hex(&keccak(&secp256k1_public(private_key)?)[12..])))
}

pub fn ed25519_public(private_key: &[u8]) -> Result<[u8; 32]> {
    let Ok(secret) = <[u8; 32]>::try_from(private_key) else { return fail("An ed25519 private key is 32 bytes.") };
    Ok(EdKey::from_bytes(&secret).verifying_key().to_bytes())
}

/// Base58 of the ed25519 public key.
pub fn solana_address(private_key: &[u8]) -> Result<String> { Ok(base58::encode(&ed25519_public(private_key)?)) }

/// The NEAR implicit account: lowercase hex of the ed25519 public key. It exists on chain once NEAR arrives.
pub fn near_implicit(private_key: &[u8]) -> Result<String> { Ok(to_hex(&ed25519_public(private_key)?)) }

pub mod base58 {
    use crate::{Result, fail};

    pub const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    pub fn encode(data: &[u8]) -> String {
        let mut digits: Vec<u8> = Vec::new();
        for &byte in data {
            let mut carry = byte as u32;
            for d in digits.iter_mut() {
                carry += (*d as u32) << 8;
                *d = (carry % 58) as u8;
                carry /= 58;
            }
            while carry > 0 {
                digits.push((carry % 58) as u8);
                carry /= 58;
            }
        }
        let zeros = data.iter().take_while(|&&b| b == 0).count();
        "1".repeat(zeros) + &digits.iter().rev().map(|&d| ALPHABET[d as usize] as char).collect::<String>()
    }

    pub fn decode(s: &str) -> Result<Vec<u8>> {
        let mut bytes: Vec<u8> = Vec::new();
        for c in s.chars() {
            let Some(value) = ALPHABET.iter().position(|&a| a as char == c) else { return fail(format!("Not base58: {s}")) };
            let mut carry = value as u32;
            for b in bytes.iter_mut() {
                carry += *b as u32 * 58;
                *b = (carry & 0xff) as u8;
                carry >>= 8;
            }
            while carry > 0 {
                bytes.push((carry & 0xff) as u8);
                carry >>= 8;
            }
        }
        let zeros = s.chars().take_while(|&c| c == '1').count();
        let mut out = vec![0u8; zeros];
        out.extend(bytes.iter().rev());
        Ok(out)
    }
}

/// A private key pasted from another wallet: which chain it belongs to and its 32 secret bytes. Phantom exports a
/// Solana key as base58 of the 64-byte keypair (secret ‖ public), the Solana CLI as a JSON array of those 64 numbers;
/// MetaMask exports an EVM key as 64 hex digits, with or without 0x. A key covers only its own chain.
#[derive(Debug, PartialEq, Eq)]
pub enum ImportedKey {
    /// The 32-byte ed25519 secret.
    Solana(Secret),
    /// The 32-byte secp256k1 secret.
    Evm(Secret),
}

impl ImportedKey {
    pub fn parse(text: &str) -> Result<Self> {
        let s = text.trim();
        if s.starts_with('[') {
            let numbers: Option<Vec<u8>> = serde_json::from_str::<Vec<i64>>(s).ok()
                .filter(|n| n.len() == 64)
                .and_then(|n| n.into_iter().map(|x| u8::try_from(x).ok()).collect());
            let Some(numbers) = numbers else { return fail("A Solana key file is a list of 64 numbers from 0 to 255.") };
            return Self::solana_keypair(&Zeroizing::new(numbers));
        }
        let hex = if s.to_lowercase().starts_with("0x") { &s[2..] } else { s };
        if hex.len() == 64 {
            if let Some(bytes) = from_hex(hex).map(Zeroizing::new) {
                if evm_address(&bytes).is_err() { return fail("Those 64 hex digits aren't a valid EVM private key.") }
                return Ok(Self::Evm(secret(&bytes)));
            }
        }
        let Ok(bytes) = base58::decode(s).map(Zeroizing::new) else {
            return fail("Not a key that can be imported: a Solana key (base58 from Phantom, or the 64 numbers of a key file) or an EVM key (64 hex digits from MetaMask).");
        };
        match bytes.len() {
            64 => Self::solana_keypair(&bytes),
            32 => Ok(Self::Solana(secret(&bytes))),
            n => fail(format!("A Solana private key is 64 bytes (32 for the secret alone); this one decodes to {n}.")),
        }
    }

    /// secret ‖ public: the public half must be the one the secret makes, else the key was copied incompletely.
    fn solana_keypair(bytes: &[u8]) -> Result<Self> {
        if ed25519_public(&bytes[..32])? != bytes[32..] {
            return fail("The key's second half isn't its own public key: it was copied incompletely, or it isn't a Solana key.");
        }
        Ok(Self::Solana(secret(&bytes[..32])))
    }

    /// The address this key signs for, to compare with Phantom or MetaMask before saving.
    pub fn address(&self) -> Result<String> {
        match self {
            Self::Solana(s) => solana_address(&**s),
            Self::Evm(s) => evm_address(&**s),
        }
    }

    pub fn chain(&self) -> &'static str {
        match self {
            Self::Solana(_) => "Solana",
            Self::Evm(_) => "EVM · Hyperliquid",
        }
    }
}

fn secret(bytes: &[u8]) -> Secret {
    let mut s = Zeroizing::new([0u8; 32]);
    s.copy_from_slice(bytes);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mnemonic::seed;

    const ABANDON: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    const HARDHAT: &str = "test test test test test test test test test test test junk";

    #[test]
    fn base58_matches_bitcoin_alphabet() {
        assert_eq!(base58::encode(&from_hex("0000287fb4cd").unwrap()), "11233QC4");
        assert_eq!(base58::encode(b"hello world"), "StV1DL6CwTryKyV");
        assert_eq!(base58::decode("11233QC4").unwrap(), from_hex("0000287fb4cd").unwrap());
        assert!(base58::decode("0OIl").is_err());
    }

    #[test]
    fn evm_addresses_match_metamask_and_hardhat() {
        let seed = seed(HARDHAT, "").unwrap();
        assert_eq!(evm_address(&*secp256k1(&*seed, &path::evm(0)).unwrap()).unwrap(), "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
        assert_eq!(evm_address(&*secp256k1(&*seed, &path::evm(1)).unwrap()).unwrap(), "0x70997970C51812dc3A010C7d01b50e0d17dc79C8");
    }

    #[test]
    fn solana_addresses_match_phantom() {
        let seed = seed(ABANDON, "").unwrap();
        assert_eq!(solana_address(&*ed25519(&*seed, &path::solana(0)).unwrap()).unwrap(), "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk");
        assert_eq!(solana_address(&*ed25519(&*seed, &path::solana(1)).unwrap()).unwrap(), "Hh8QwFUA6MtVu1qAoq12ucvFHNwCcVTV7hpWjeY1Hztb");
    }

    #[test]
    fn near_implicit_account_matches_slip10() {
        let key = ed25519(&*seed(ABANDON, "").unwrap(), &path::near(0)).unwrap();
        assert_eq!(near_implicit(&*key).unwrap(), "5510e2b44cae6eb807e3e0e45d579dda058c274abcba15e5cb84636f5d1ee412");
    }

    #[test]
    fn slip10_matches_spec_vector() {
        let seed = from_hex("000102030405060708090a0b0c0d0e0f").unwrap();
        assert_eq!(to_hex(&*ed25519(&seed, "m").unwrap()), "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7");
        assert_eq!(to_hex(&*ed25519(&seed, "m/0'").unwrap()), "68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3");
        assert!(ed25519(&seed, "m/0").is_err());
    }

    /// BIP-32 test vector 1, both hardened and normal steps.
    #[test]
    fn bip32_matches_spec_vector() {
        let seed = from_hex("000102030405060708090a0b0c0d0e0f").unwrap();
        assert_eq!(to_hex(&*secp256k1(&seed, "m/0'/1/2'/2/1000000000").unwrap()),
                   "471b76e389e528d6de6d816857e012c5455051cad6660850e58372a6c3e6e7c8");
        assert!(parse_path("0/1").is_err());
        assert!(parse_path("m/2147483648").is_err());
    }

    #[test]
    fn imported_keys() {
        let evm = secp256k1(&*seed(HARDHAT, "").unwrap(), &path::evm(0)).unwrap();
        let k = ImportedKey::parse(&format!("  0x{}\n", to_hex(&*evm))).unwrap();
        assert_eq!(k.address().unwrap(), "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266");
        assert_eq!(ImportedKey::parse(&"00".repeat(32)).unwrap_err().0, "Those 64 hex digits aren't a valid EVM private key.");

        let sol = ed25519(&*seed(ABANDON, "").unwrap(), &path::solana(0)).unwrap();
        let mut pair = sol.to_vec();
        pair.extend(ed25519_public(&*sol).unwrap());
        assert_eq!(ImportedKey::parse(&base58::encode(&pair)).unwrap().address().unwrap(), "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk");
        assert_eq!(ImportedKey::parse(&serde_json::to_string(&pair).unwrap()).unwrap(), ImportedKey::Solana(sol.clone()));
        assert_eq!(ImportedKey::parse(&base58::encode(&*sol)).unwrap(), ImportedKey::Solana(sol));
        pair[63] ^= 1;
        assert!(ImportedKey::parse(&base58::encode(&pair)).unwrap_err().0.starts_with("The key's second half"));
        assert_eq!(ImportedKey::parse("[1,2]").unwrap_err().0, "A Solana key file is a list of 64 numbers from 0 to 255.");
    }
}
