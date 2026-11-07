//! What a NEAR account owner signs off-chain, for NEAR Intents' Agent API (`NearOwnerSigning.swift`): a NEP-413 message
//! and a NEP-366 delegate action (a gasless transaction a relayer sends). Plain functions over plain fields, so the
//! signer checks exactly what it signs; the borsh layouts follow the NEPs and nearcore.

use std::collections::HashSet;

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use crate::near::{self, Action};
use crate::{Result, fail, to_hex};

fn borsh_string(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u32).to_le_bytes());
    out.extend(s.as_bytes());
}

fn signing_key(private_key: &[u8]) -> Result<SigningKey> {
    let Ok(secret) = <[u8; 32]>::try_from(private_key) else { return fail("An ed25519 private key is 32 bytes.") };
    Ok(SigningKey::from_bytes(&secret))
}

/// NEP-413: sha256 of the tag 2³¹+413 and borsh {message, nonce[32], recipient, callbackUrl: None}, signed ed25519.
pub fn nep413_hash(message: &str, nonce: &[u8], recipient: &str) -> [u8; 32] {
    let mut out = 2_147_484_061u32.to_le_bytes().to_vec(); // 2^31 + 413
    borsh_string(&mut out, message);
    out.extend(nonce);
    borsh_string(&mut out, recipient);
    out.push(0); // callbackUrl: None
    Sha256::digest(&out).into()
}

pub fn nep413(message: &str, nonce: &[u8], recipient: &str, private_key: &[u8]) -> Result<Vec<u8>> {
    if nonce.len() != 32 { return fail(format!("A NEP-413 nonce is 32 bytes, not {}.", nonce.len())) }
    Ok(signing_key(private_key)?.sign(&nep413_hash(message, nonce, recipient)).to_bytes().to_vec())
}

/// borsh DelegateAction: sender, receiver, actions (no nested delegate), nonce, max block height, ed25519 key.
pub fn delegate_action(sender: &str, receiver: &str, actions: &[Action], nonce: u64, max_block_height: u64, public_key: &[u8]) -> Vec<u8> {
    // A transaction's body from the receiver on is the same borsh: reuse it, then put the fields in NEP-366's order.
    let body = near::serialize(sender, public_key, nonce, receiver, &[0; 32], actions);
    let actions_at = 4 + sender.len() + 1 + public_key.len() + 8 + 4 + receiver.len() + 32;
    let mut out = Vec::new();
    borsh_string(&mut out, sender);
    borsh_string(&mut out, receiver);
    out.extend(&body[actions_at..]);
    out.extend(nonce.to_le_bytes());
    out.extend(max_block_height.to_le_bytes());
    out.push(0);
    out.extend(public_key);
    out
}

/// The borsh SignedDelegateAction: the action and its ed25519 signature over sha256(2³⁰+366 tag ‖ action).
pub fn sign_delegate(receiver: &str, actions: &[Action], nonce: u64, max_block_height: u64, private_key: &[u8]) -> Result<Vec<u8>> {
    let key = signing_key(private_key)?;
    let public_key = key.verifying_key().to_bytes();
    let mut action = delegate_action(&to_hex(&public_key), receiver, actions, nonce, max_block_height, &public_key);
    let hash: [u8; 32] = Sha256::new().chain_update(1_073_742_190u32.to_le_bytes()).chain_update(&action).finalize().into(); // 2^30 + 366
    action.push(0);
    action.extend(key.sign(&hash).to_bytes());
    Ok(action)
}

/// A JSON value written as JavaScript's JSON.stringify writes it: compact, keys in the order they came, «/» and
/// non-ASCII as they are. The Agent API signs `args` that way, and serde_json reorders keys.
pub fn stringify(json: &[u8]) -> Result<Vec<u8>> {
    let mut p = Parser { bytes: json, i: 0, depth: 0 };
    let mut out = Vec::new();
    p.value(&mut out)?;
    p.space();
    if p.i != json.len() { return fail("Trailing bytes after the JSON value.") }
    Ok(out)
}

/// The raw text of `key`'s value in a JSON object, as it came: for fields that must be signed byte for byte.
pub fn raw_value<'a>(key: &str, object: &'a [u8]) -> Result<&'a [u8]> {
    let mut p = Parser { bytes: object, i: 0, depth: 0 };
    p.space();
    if p.peek() != Some(b'{') { return fail("Not a JSON object.") }
    p.i += 1;
    loop {
        p.space();
        if p.peek() == Some(b'}') { break }
        let mut name = Vec::new();
        p.string(&mut name)?;
        p.space();
        if p.peek() != Some(b':') { return fail("Bad JSON.") }
        p.i += 1;
        p.space();
        let start = p.i;
        p.value(&mut Vec::new())?;
        if serde_json::from_slice::<String>(&name).ok().as_deref() == Some(key) { return Ok(&object[start..p.i]) }
        p.space();
        if p.peek() == Some(b',') { p.i += 1 }
    }
    fail(format!("No «{key}» in the JSON object."))
}

/// The raw text of each element of a JSON array, as it came.
pub fn raw_elements(array: &[u8]) -> Result<Vec<&[u8]>> {
    let mut p = Parser { bytes: array, i: 0, depth: 0 };
    p.space();
    if p.peek() != Some(b'[') { return fail("Not a JSON array.") }
    p.i += 1;
    p.space();
    if p.peek() == Some(b']') { return Ok(vec![]) }
    let mut out = Vec::new();
    loop {
        p.space();
        let start = p.i;
        p.value(&mut Vec::new())?;
        out.push(&array[start..p.i]);
        p.space();
        if p.peek() == Some(b',') { p.i += 1; continue }
        if p.peek() != Some(b']') { return fail("Bad JSON array.") }
        return Ok(out);
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    i: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> { self.bytes.get(self.i).copied() }

    fn space(&mut self) { while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) { self.i += 1 } }

    fn value(&mut self, out: &mut Vec<u8>) -> Result<()> {
        self.space();
        let Some(c) = self.peek() else { return fail("JSON ends early.") };
        match c {
            b'{' | b'[' => {
                // serde_json's limit: deeper input would only exhaust the signer's stack.
                if self.depth >= 128 { return fail("The JSON is nested too deep.") }
                self.depth += 1;
                let close = if c == b'{' { b'}' } else { b']' };
                out.push(c);
                self.i += 1;
                self.space();
                if self.peek() == Some(close) {
                    out.push(close);
                    self.i += 1;
                    self.depth -= 1;
                    return Ok(());
                }
                // A key twice reads differently here (the first wins) and in JavaScript or serde_json (the last wins): refused.
                let mut keys = HashSet::new();
                loop {
                    if c == b'{' {
                        self.space();
                        let key_start = self.i;
                        self.string(out)?;
                        let name = serde_json::from_slice::<String>(&self.bytes[key_start..self.i]).unwrap_or_default();
                        if !keys.insert(name.clone()) { return fail(format!("A JSON key appears twice («{name}»): not signed.")) }
                        self.space();
                        if self.peek() != Some(b':') { return fail("Bad JSON object.") }
                        out.push(b':');
                        self.i += 1;
                    }
                    self.value(out)?;
                    self.space();
                    if self.peek() == Some(b',') {
                        out.push(b',');
                        self.i += 1;
                        continue;
                    }
                    if self.peek() != Some(close) { return fail("Bad JSON.") }
                    out.push(close);
                    self.i += 1;
                    self.depth -= 1;
                    return Ok(());
                }
            }
            b'"' => self.string(out),
            _ => {
                // A number, true, false or null: its own characters, unchanged.
                let start = self.i;
                while self.peek().is_some_and(|d| !matches!(d, b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t')) { self.i += 1 }
                if self.i == start { return fail("Bad JSON value.") }
                out.extend(&self.bytes[start..self.i]);
                Ok(())
            }
        }
    }

    /// A string re-escaped as JSON.stringify does: \" \\ \b \f \n \r \t, other controls as \u00XX, the rest as is.
    fn string(&mut self, out: &mut Vec<u8>) -> Result<()> {
        if self.peek() != Some(b'"') { return fail("Expected a JSON string.") }
        let start = self.i;
        self.i += 1;
        while let Some(c) = self.peek().filter(|&c| c != b'"') { self.i += if c == b'\\' { 2 } else { 1 } }
        if self.peek() != Some(b'"') { return fail("Unterminated JSON string.") }
        self.i += 1;
        let Ok(text) = serde_json::from_slice::<String>(&self.bytes[start..self.i]) else { return fail("Bad JSON string.") };
        out.push(b'"');
        for ch in text.chars() {
            match ch {
                '"' => out.extend(b"\\\""),
                '\\' => out.extend(b"\\\\"),
                '\u{08}' => out.extend(b"\\b"),
                '\u{0C}' => out.extend(b"\\f"),
                '\n' => out.extend(b"\\n"),
                '\r' => out.extend(b"\\r"),
                '\t' => out.extend(b"\\t"),
                c if (c as u32) < 0x20 => out.extend(format!("\\u{:04x}", c as u32).bytes()),
                c => out.extend(c.encode_utf8(&mut [0; 4]).bytes()),
            }
        }
        out.push(b'"');
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier};

    #[test]
    fn stringify_keeps_key_order_and_writes_like_javascript() {
        let raw = r#"{ "b": 1, "a": [ true, null, "x/y" ], "c": { "z": "é\n\u0001" } }"#;
        assert_eq!(String::from_utf8(stringify(raw.as_bytes()).unwrap()).unwrap(), r#"{"b":1,"a":[true,null,"x/y"],"c":{"z":"é\n\u0001"}}"#);
        assert_eq!(String::from_utf8(stringify(br#""a\/b""#).unwrap()).unwrap(), r#""a/b""#);
        assert!(stringify(br#"{"a":1} x"#).is_err());
        assert!(stringify(&[b'['; 200]).is_err());
    }

    #[test]
    fn raw_value_is_the_field_as_it_came() {
        let object = br#"{"receiverId":"x.near","actions":[{"type":"FunctionCall","params":{"methodName":"m","args":{"z":1,"a":2}}}]}"#;
        assert_eq!(raw_value("actions", object).unwrap(), br#"[{"type":"FunctionCall","params":{"methodName":"m","args":{"z":1,"a":2}}}]"#);
        assert!(raw_value("missing", object).is_err());
        assert_eq!(raw_elements(br#"[ 1 , {"a":[2]} ]"#).unwrap(), [&b"1"[..], br#"{"a":[2]}"#]);
    }

    #[test]
    fn delegate_action_is_nep366_borsh() {
        let key = [7u8; 32];
        let action = delegate_action("a.near", "b.near", &[Action::FunctionCall { method: "m".into(), args: b"{}".to_vec(), gas: 5, deposit: 1 }], 9, 100, &key);
        let mut expected = [&[6, 0, 0, 0][..], b"a.near", &[6, 0, 0, 0], b"b.near"].concat();
        expected.extend([1, 0, 0, 0, 2, 1, 0, 0, 0]);
        expected.extend(b"m");
        expected.extend([2, 0, 0, 0]);
        expected.extend(b"{}");
        expected.extend([5, 0, 0, 0, 0, 0, 0, 0]);
        expected.extend([1].into_iter().chain([0; 15]));
        expected.extend([9, 0, 0, 0, 0, 0, 0, 0]);
        expected.extend([100, 0, 0, 0, 0, 0, 0, 0]);
        expected.push(0);
        expected.extend(key);
        assert_eq!(action, expected);
    }

    #[test]
    fn signatures_verify_under_the_owners_key() {
        let secret = [5u8; 32];
        let public = SigningKey::from_bytes(&secret).verifying_key();
        let nonce = [3u8; 32];
        let sig = nep413(r#"{"type":"grant_issue"}"#, &nonce, "api.example", &secret).unwrap();
        let hash = nep413_hash(r#"{"type":"grant_issue"}"#, &nonce, "api.example");
        assert!(public.verify(&hash, &Signature::from_slice(&sig).unwrap()).is_ok());
        assert!(nep413("m", &[0; 31], "r", &secret).is_err());

        let signed = sign_delegate("b.near", &[Action::FunctionCall { method: "m".into(), args: b"{}".to_vec(), gas: 5, deposit: 0 }], 1, 2, &secret).unwrap();
        let (body, sig) = signed.split_at(signed.len() - 65);
        assert_eq!(sig[0], 0);
        let hash: [u8; 32] = Sha256::digest([&[0x6E, 0x01, 0x00, 0x40][..], body].concat()).into();
        assert!(public.verify(&hash, &Signature::from_slice(&sig[1..]).unwrap()).is_ok());
        assert!(body.starts_with(&[64, 0, 0, 0]) && body[4..68] == *to_hex(public.as_bytes()).as_bytes());
    }
}
