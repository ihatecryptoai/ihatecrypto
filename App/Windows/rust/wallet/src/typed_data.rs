//! eth_signTypedData_v4 as sites send it, parsed and hashed here rather than trusted from the page (`TypedData.swift`).
//! Only flat structs (what logins use); anything nested is refused, and token spending permissions (Permit, Permit2
//! and their kin), Hyperliquid's own actions and payments by signature are recognised so they can be refused outright.

use serde_json::{Map, Value as Json};
use unicode_segmentation::UnicodeSegmentation;

use crate::address_check::hidden;
use crate::evm::eip712::{self, Field, Value};
use crate::evm::keccak;
use crate::{Result, fail, from_hex};

#[derive(Debug, Clone)]
pub struct TypedData {
    pub primary_type: String,
    pub domain_name: String,
    pub chain_id: String,
    /// The domain's verifyingContract, lowercased, or "".
    pub verifying_contract: String,
    /// name, type and the value as shown, in the order the site declared them.
    pub fields: Vec<(String, String, String)>,
    pub digest: [u8; 32],
}

const PERMIT_TYPES: [&str; 8] = [
    "Permit", "PermitSingle", "PermitBatch", "PermitTransferFrom", "PermitBatchTransferFrom",
    "PermitWitnessTransferFrom", "PermitBatchWitnessTransferFrom", "PermitForAll",
];
/// EIP-3009: USDC and its kin move by a signed message that whoever holds it submits.
const AUTHORIZATIONS: [&str; 2] = ["TransferWithAuthorization", "ReceiveWithAuthorization"];
const PAYEE_NAMES: [&str; 6] = ["to", "recipient", "destination", "receiver", "beneficiary", "dst"];
/// The prompt shows every field; one with more than this many is refused rather than half shown.
pub const SHOWN_FIELDS: usize = 16;

impl TypedData {
    /// A signature that lets someone else spend the person's tokens: the drainers' favourite.
    pub fn is_token_permission(&self) -> bool {
        if PERMIT_TYPES.contains(&self.primary_type.as_str()) || self.primary_type.to_lowercase().contains("permit") { return true }
        let names: Vec<String> = self.fields.iter().map(|f| f.0.to_lowercase()).collect();
        names.iter().any(|n| n == "spender") || (names.iter().any(|n| n == "operator") && names.iter().any(|n| n == "approved"))
    }

    /// Why a site's typed data isn't signed, or None. Besides `refusal`: bytes to run as this wallet, more fields than
    /// the prompt shows, and, given the wallet's own address, any struct that names someone else's address beside an
    /// amount, whatever its fields are called. This wallet's address and the verifying contract aren't someone else.
    pub fn refusal_for(&self, owner: Option<&str>) -> Option<String> {
        if let Some(r) = self.refusal() { return Some(r) }
        let p = &self.primary_type;
        // Bytes signed to run as this wallet (a meta-transaction's functionSignature, a forwarded call's data) can be any call.
        if let Some(call) = self.fields.iter().find(|f| f.1 == "bytes") {
            return Some(format!("{p} carries «{}», data to run as this wallet. ihatecrypto doesn't sign that for a site.", call.0));
        }
        if self.fields.len() > SHOWN_FIELDS {
            return Some(format!("{p} has {} fields, more than the prompt can show. ihatecrypto doesn't sign it.", self.fields.len()));
        }
        let owner = owner?.to_lowercase();
        let others: Vec<_> = self.fields.iter().filter(|f| f.1 == "address" && ![&owner, &self.verifying_contract].contains(&&f.2.to_lowercase())).collect();
        // A right given by a yes, no amount at all (research E-2): Morpho's Authorization(authorizer, authorized,
        // isAuthorized…) and Compound's (owner, manager, isAllowed…) let someone else manage the wallet's positions.
        // A false is a revoke and signs.
        if let (Some(other), Some(flag)) = (others.first(), self.fields.iter().find(|f| f.1 == "bool" && (f.2 == "1" || f.2 == "true"))) {
            return Some(format!("{p} gives {} («{}») rights over your account («{}»): Morpho's and Compound's Authorization work like this. ihatecrypto doesn't sign it for a site.",
                                other.2, other.0, flag.0));
        }
        let not_amounts = ["nonce", "deadline", "expir", "valid", "time", "chainid", "salt", "version", "choice"];
        let amount = self.fields.iter().find(|f| {
            f.1.starts_with("uint") && f.2 != "0" && !not_amounts.iter().any(|w| f.0.to_lowercase().contains(w))
        });
        let (other, amount) = (others.first()?, amount?);
        Some(format!("{p} names {} («{}») beside a number («{}»): a signature like that can move money. ihatecrypto doesn't sign it for a site.",
                     other.2, other.0, amount.0))
    }

    pub fn refusal(&self) -> Option<String> {
        let p = &self.primary_type;
        // ERC-4337's EntryPoints v0.6, v0.7, v0.8: a wallet delegated to an account contract (EIP-7702) does whatever a
        // user operation signed for one says.
        let entry_points = ["0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789", "0x0000000071727de22e5e9d8baf0edac6f37da032",
                            "0x4337084d9e255ff0702461cf8895ce9e3b5ff108"];
        if entry_points.contains(&self.verifying_contract.as_str()) || self.domain_name == "ERC4337" || p.contains("UserOperation") {
            return Some(format!("{p}: an ERC-4337 user operation — it would run any calls from this wallet. ihatecrypto doesn't sign these."));
        }
        // A Safe multisig's own types: moves out of a multisig are signed only in ihatecrypto's own steps, never for a site.
        if p == "SafeTx" || p == "SafeMessage" {
            return Some(format!("{p}: a signature for a Safe multisig. ihatecrypto signs a multisig's moves only in its own steps, never for a site."));
        }
        if self.is_token_permission() {
            return Some(format!("{p}: this is permission to spend your tokens (Permit) — the drainers' main trick. ihatecrypto doesn't sign these."));
        }
        if self.domain_name == "HyperliquidSignTransaction" || (self.domain_name == "Exchange" && p == "Agent") {
            return Some(format!("{p}: a Hyperliquid action signed with your main key (a withdrawal, a transfer, a key approval). ihatecrypto signs those only in its own steps, never for a site."));
        }
        if AUTHORIZATIONS.contains(&p.as_str()) {
            return Some(format!("{p}: moves your tokens by signature (EIP-3009). ihatecrypto doesn't sign that for a site."));
        }
        let names: Vec<String> = self.fields.iter().map(|f| f.0.to_lowercase()).collect();
        if let Some(payee) = names.iter().find(|n| PAYEE_NAMES.contains(&n.as_str())) {
            if names.iter().any(|n| n == "value" || n == "wad" || n.contains("amount")) {
                return Some(format!("{p} sends an amount to its «{payee}»: ihatecrypto doesn't sign payments as typed data."));
            }
        }
        None
    }

    /// The prompt's text: the type, the site's name for itself and the fields with their values, as text that can't
    /// draw a line of its own (breaks, controls and bidi marks dropped) and doesn't run on: a cut one ends in «…».
    pub fn prompt(&self) -> String {
        fn plain(s: &str, n: usize) -> String {
            let t: String = s.chars().filter(|&c| !hidden(c)).collect();
            if t.graphemes(true).count() > n { t.graphemes(true).take(n).collect::<String>() + "…" } else { t }
        }
        let mut lines = vec![
            format!("sign {} for «{}»", plain(&self.primary_type, 60), plain(&self.domain_name, 60)),
            "ihatecrypto can't tell what this signature allows. Sign only if you trust this site.".to_string(),
        ];
        lines.extend(self.fields.iter().take(SHOWN_FIELDS).map(|f| format!("{}: {}", plain(&f.0, 30), plain(&f.2, 70))));
        lines.join("\n")
    }

    pub fn parse(json: &[u8]) -> Result<Self> {
        let unreadable = || fail("Unreadable typed data.");
        let Ok(Json::Object(j)) = serde_json::from_slice::<Json>(json) else { return unreadable() };
        let Some(types) = j.get("types").and_then(type_table) else { return unreadable() };
        let (Some(Json::String(primary)), Some(Json::Object(domain)), Some(Json::Object(message))) =
            (j.get("primaryType"), j.get("domain"), j.get("message")) else { return unreadable() };
        let Some(declared) = types.get(primary) else { return unreadable() };
        let domain_type = types.get("EIP712Domain").cloned().unwrap_or_else(|| {
            ["name", "version", "chainId", "verifyingContract", "salt"].iter().filter(|k| domain.contains_key(**k)).map(|&k| {
                let ty = match k { "chainId" => "uint256", "verifyingContract" => "address", "salt" => "bytes32", _ => "string" };
                (Some(k.to_string()), Some(ty.to_string()))
            }).collect()
        });
        let fields = |declared: &[(Option<String>, Option<String>)], values: &Map<String, Json>| -> Result<Vec<Field>> {
            declared.iter().map(|(name, ty)| {
                let (Some(name), Some(ty)) = (name, ty) else { return fail("A field without a name or type.") };
                if types.contains_key(ty) || ty.ends_with(']') { return fail(format!("ihatecrypto doesn't sign nested typed data ({ty}) yet.")) }
                Ok(Field::new(name, ty, value(values.get(name), ty)?))
            }).collect()
        };
        let domain_fields = fields(&domain_type, domain)?;
        let message_fields = fields(declared, message)?;
        let shown = declared.iter().map(|(n, t)| {
            let n = n.clone().unwrap_or_default();
            (n.clone(), t.clone().unwrap_or_default(), describe(message.get(&n)))
        }).collect();
        Ok(TypedData {
            primary_type: primary.clone(),
            domain_name: domain.get("name").and_then(Json::as_str).unwrap_or("").to_string(),
            chain_id: describe(domain.get("chainId")),
            verifying_contract: domain.get("verifyingContract").and_then(Json::as_str).unwrap_or("").to_lowercase(),
            fields: shown,
            digest: eip712::digest(&domain_fields, primary, &message_fields)?,
        })
    }
}

type Declared = Vec<(Option<String>, Option<String>)>;

/// `types` as `[String: [[String: String]]]`: anything else in it makes the whole thing unreadable, as in Swift.
fn type_table(v: &Json) -> Option<std::collections::HashMap<String, Declared>> {
    v.as_object()?.iter().map(|(k, list)| {
        let fields = list.as_array()?.iter().map(|d| {
            let d = d.as_object()?;
            if !d.values().all(Json::is_string) { return None }
            let get = |key: &str| d.get(key).and_then(Json::as_str).map(str::to_string);
            Some((get("name"), get("type")))
        }).collect::<Option<Declared>>()?;
        Some((k.clone(), fields))
    }).collect()
}

/// A value as the prompt shows it. JSON's true and false read 1 and 0, as Foundation's NSNumber says them.
fn describe(v: Option<&Json>) -> String {
    match v {
        None => "—".into(),
        Some(Json::String(s)) => s.clone(),
        Some(Json::Bool(b)) => (*b as u8).to_string(),
        Some(Json::Number(n)) => n.to_string(),
        Some(Json::Null) => "<null>".into(),
        Some(other) => other.to_string(),
    }
}

fn value(v: Option<&Json>, ty: &str) -> Result<Value> {
    match ty {
        "string" => Ok(Value::String(v.and_then(Json::as_str).unwrap_or("").into())),
        "address" => match v.and_then(Json::as_str) {
            Some(a) => Ok(Value::Address(a.into())),
            None => fail("No address."),
        },
        // NSNumber 0 and 1 read as Bool in Swift, so they do here.
        "bool" => Ok(Value::Bool(match v {
            Some(Json::Bool(b)) => *b,
            Some(Json::Number(n)) => n.as_u64() == Some(1),
            Some(Json::String(s)) => s == "true",
            _ => false,
        })),
        "bytes32" => match v.and_then(Json::as_str).and_then(from_hex) {
            Some(d) if d.len() == 32 => Ok(Value::Bytes32(d)),
            _ => fail("Invalid bytes32."),
        },
        "bytes" => match v.and_then(Json::as_str).and_then(from_hex) {
            Some(d) => Ok(Value::Bytes32(keccak(&d).to_vec())),
            None => fail("Invalid bytes."),
        },
        t if t.starts_with("uint") => Ok(Value::Bytes32(word(v)?.to_vec())),
        t => fail(format!("ihatecrypto doesn't sign type {t} yet.")),
    }
}

/// An unsigned integer as a 32-byte big-endian word, from a JSON number, decimal or 0x-hex string.
/// ponytail: ASCII digits only, where Swift's `isNumber` also lets other scripts' digits through.
pub fn word(v: Option<&Json>) -> Result<[u8; 32]> {
    let text = match v {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let out_of_range = || fail("Number out of uint256 range.");
    if let Some(h) = text.strip_prefix("0x") {
        let padded = "0".repeat(h.len() % 2) + h;
        let Some(d) = from_hex(&padded).filter(|d| d.len() <= 32) else { return out_of_range() };
        let mut w = [0u8; 32];
        w[32 - d.len()..].copy_from_slice(&d);
        return Ok(w);
    }
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) { return fail(format!("Not a number: {text}.")) }
    let mut bytes = [0u8; 32];
    for digit in text.bytes().map(|b| (b - b'0') as u32) {
        // bytes = bytes × 10 + digit, big-endian
        let mut carry = digit;
        for b in bytes.iter_mut().rev() {
            let x = *b as u32 * 10 + carry;
            *b = (x & 0xff) as u8;
            carry = x >> 8;
        }
        if carry != 0 { return out_of_range() }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::to_hex;

    /// EIP-712's own example (Mail from Cow to Bob) uses nested structs, which are refused; a flat login is hashed.
    #[test]
    fn hashes_a_flat_login_and_refuses_nesting() {
        let login = br#"{"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"chainId","type":"uint256"}],
            "Login":[{"name":"user","type":"address"},{"name":"nonce","type":"uint256"},{"name":"ok","type":"bool"}]},
            "primaryType":"Login","domain":{"name":"Site","chainId":1},
            "message":{"user":"0x70997970C51812dc3A010C7d01b50e0d17dc79C8","nonce":"0x2a","ok":true}}"#;
        let t = TypedData::parse(login).unwrap();
        let expected = eip712::digest(
            &[Field::new("name", "string", Value::String("Site".into())), Field::new("chainId", "uint256", Value::Uint(1))],
            "Login",
            &[Field::new("user", "address", Value::Address("0x70997970C51812dc3A010C7d01b50e0d17dc79C8".into())),
              Field::new("nonce", "uint256", Value::Uint(42)), Field::new("ok", "bool", Value::Bool(true))],
        ).unwrap();
        assert_eq!(to_hex(&t.digest), to_hex(&expected));
        assert_eq!(t.fields[2].2, "1");
        assert_eq!(t.chain_id, "1");
        assert!(t.refusal_for(Some("0x70997970C51812dc3A010C7d01b50e0d17dc79C8")).is_none());
        // Someone else's address beside a yes is a right given (Morpho's Authorization), refused.
        assert!(t.refusal_for(Some("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266")).unwrap().contains("rights over your account («ok»)"));

        let mail = br#"{"types":{"Person":[{"name":"name","type":"string"}],"Mail":[{"name":"from","type":"Person"}]},
            "primaryType":"Mail","domain":{"name":"Ether Mail"},"message":{"from":{"name":"Cow"}}}"#;
        assert_eq!(TypedData::parse(mail).unwrap_err().0, "ihatecrypto doesn't sign nested typed data (Person) yet.");
        assert_eq!(TypedData::parse(b"{}").unwrap_err().0, "Unreadable typed data.");
    }

    #[test]
    fn refuses_what_moves_money() {
        let permit = br#"{"types":{"Permit":[{"name":"spender","type":"address"},{"name":"value","type":"uint256"}]},
            "primaryType":"Permit","domain":{"name":"USDC","verifyingContract":"0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"},
            "message":{"spender":"0x0000000000000000000000000000000000000001","value":"1000"}}"#;
        assert!(TypedData::parse(permit).unwrap().refusal().unwrap().contains("Permit"));
        let order = br#"{"types":{"Order":[{"name":"taker","type":"address"},{"name":"total","type":"uint256"}]},
            "primaryType":"Order","domain":{"name":"X"},
            "message":{"taker":"0x0000000000000000000000000000000000000002","total":5}}"#;
        let t = TypedData::parse(order).unwrap();
        assert!(t.refusal().is_none());
        assert!(t.refusal_for(Some("0x0000000000000000000000000000000000000009")).unwrap().contains("«taker»"));
        assert!(t.refusal_for(Some("0x0000000000000000000000000000000000000002")).is_none());
        let authorization = br#"{"types":{"Authorization":[{"name":"authorizer","type":"address"},{"name":"authorized","type":"address"},
            {"name":"isAuthorized","type":"bool"},{"name":"nonce","type":"uint256"}]},"primaryType":"Authorization","domain":{"name":"Morpho"},
            "message":{"authorizer":"0x0000000000000000000000000000000000000009","authorized":"0x0000000000000000000000000000000000000002",
            "isAuthorized":true,"nonce":0}}"#;
        let t = TypedData::parse(authorization).unwrap();
        assert!(t.refusal_for(Some("0x0000000000000000000000000000000000000009")).unwrap().contains("rights over your account («isAuthorized»)"));
        let revoke = String::from_utf8(authorization.to_vec()).unwrap().replace("\"isAuthorized\":true", "\"isAuthorized\":false");
        assert!(TypedData::parse(revoke.as_bytes()).unwrap().refusal_for(Some("0x0000000000000000000000000000000000000009")).is_none());
    }

    #[test]
    fn words() {
        assert_eq!(word(Some(&Json::from("255"))).unwrap()[31], 255);
        assert_eq!(word(Some(&Json::from("0x100"))).unwrap()[30], 1);
        assert!(word(Some(&Json::from("1".repeat(80)))).is_err());
        assert!(word(Some(&Json::from("1.5"))).is_err());
        assert!(word(None).is_err());
    }

    #[test]
    fn prompt_drops_what_could_draw_a_line() {
        let t = TypedData { primary_type: "A\u{202e}B\nC".into(), domain_name: "s".into(), chain_id: "1".into(),
                            verifying_contract: String::new(), fields: vec![], digest: [0; 32] };
        assert!(t.prompt().starts_with("sign ABC for «s»"));
        let long = TypedData { primary_type: "x".repeat(61), ..t };
        assert!(long.prompt().starts_with(&format!("sign {}… for", "x".repeat(60))));
    }
}
