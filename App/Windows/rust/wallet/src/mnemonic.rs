//! BIP-39 recovery phrase: validation, generation and the seed (`Mnemonic.swift`). Unlike the Swift version, the seed
//! comes back in memory that is wiped when it is dropped.

use std::sync::LazyLock;

use sha2::{Digest, Sha256, Sha512};
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::{Result, fail};

/// BIP-39 English word list, bitcoin/bips bip-0039/english.txt
/// (sha256 2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda). Sorted, so lookups are a binary search.
pub static WORDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| include_str!("english.txt").lines().collect());

pub fn index(word: &str) -> Option<usize> { WORDS.binary_search(&word).ok() }

/// Checks word count, words and checksum. Returns the normalized phrase (lowercase, single spaces).
pub fn validate(phrase: &str) -> Result<String> {
    let lower = phrase.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    if ![12, 15, 18, 21, 24].contains(&words.len()) {
        return fail(format!("The phrase must have 12, 15, 18, 21 or 24 words, not {}.", words.len()));
    }
    let mut bits = Vec::with_capacity(words.len() * 11);
    for (position, word) in words.iter().enumerate() {
        let Some(i) = index(word) else { return fail(format!("Word #{} isn't in the BIP-39 word list.", position + 1)) };
        bits.extend((0..11).rev().map(|shift| i >> shift & 1 == 1));
    }
    let checksum_bits = bits.len() / 33;
    let entropy_bits = bits.len() - checksum_bits;
    let mut entropy = Zeroizing::new(vec![0u8; entropy_bits / 8]);
    for i in (0..entropy_bits).filter(|&i| bits[i]) {
        entropy[i / 8] |= 0x80 >> (i % 8);
    }
    let hash = Sha256::digest(&*entropy);
    if (0..checksum_bits).any(|i| bits[entropy_bits + i] != (hash[i / 8] & (0x80 >> (i % 8)) != 0)) {
        return fail("The phrase checksum doesn't match: check the word order and spelling.");
    }
    Ok(words.join(" "))
}

/// A new phrase from 128 bits of system randomness (12 words), or from given entropy for test vectors.
pub fn generate(entropy: Option<&[u8]>) -> String {
    let mut fresh = Zeroizing::new([0u8; 16]);
    let bytes: &[u8] = match entropy {
        Some(e) => e,
        None => {
            getrandom::fill(&mut *fresh).expect("the system's randomness");
            &*fresh
        }
    };
    let checksum = Sha256::digest(bytes)[0];
    let mut bits: Vec<bool> = bytes.iter().flat_map(|b| (0..8).map(move |i| (b >> (7 - i)) & 1 == 1)).collect();
    bits.extend((0..bytes.len() / 4).map(|i| (checksum >> (7 - i)) & 1 == 1));
    bits.chunks(11)
        .map(|chunk| WORDS[chunk.iter().fold(0usize, |n, &bit| n << 1 | bit as usize)])
        .collect::<Vec<_>>()
        .join(" ")
}

/// 64-byte seed: PBKDF2-HMAC-SHA512, 2048 rounds, salt "mnemonic" + passphrase, both NFKD.
pub fn seed(phrase: &str, passphrase: &str) -> Result<Zeroizing<[u8; 64]>> {
    let password = Zeroizing::new(validate(phrase)?.nfkd().collect::<String>());
    let salt = format!("mnemonic{passphrase}").nfkd().collect::<String>();
    let mut seed = Zeroizing::new([0u8; 64]);
    pbkdf2::pbkdf2_hmac::<Sha512>(password.as_bytes(), salt.as_bytes(), 2048, &mut *seed);
    Ok(seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{from_hex, to_hex};

    pub const ABANDON: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn seed_matches_trezor_vector() {
        assert_eq!(to_hex(&*seed(ABANDON, "TREZOR").unwrap()),
                   "c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04");
    }

    #[test]
    fn rejects_bad_checksum_unknown_word_and_count() {
        assert!(validate(&vec!["abandon"; 12].join(" ")).is_err());
        assert_eq!(validate(&ABANDON.replace("about", "abut")).unwrap_err().0, "Word #12 isn't in the BIP-39 word list.");
        assert_eq!(validate("abandon about").unwrap_err().0, "The phrase must have 12, 15, 18, 21 or 24 words, not 2.");
        assert_eq!(validate("  Abandon ABANDON abandon abandon abandon abandon abandon abandon abandon abandon abandon about ").unwrap(), ABANDON);
    }

    #[test]
    fn generation_matches_trezor_vectors() {
        assert_eq!(generate(Some(&[0; 16])), ABANDON);
        assert_eq!(generate(Some(&from_hex("7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f").unwrap())),
                   "legal winner thank year wave sausage worth useful legal winner thank yellow");
        assert_eq!(generate(Some(&from_hex("80808080808080808080808080808080").unwrap())),
                   "letter advice cage absurd amount doctor acoustic avoid letter advice cage above");
        assert_eq!(generate(Some(&[0xff; 16])), "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong");
        let fresh = generate(None);
        assert_eq!(validate(&fresh).unwrap(), fresh);
        assert_ne!(fresh, generate(None));
    }

    #[test]
    fn word_list_is_sorted_and_whole() {
        assert_eq!(WORDS.len(), 2048);
        assert!(WORDS.windows(2).all(|w| w[0] < w[1]));
    }
}
