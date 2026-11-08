//! What the signer agrees to sign as the owner of a NEAR Intents agent wallet (Agent API), read from the API's own
//! payload — never from what the engine says about it (`AgentOwner.swift`). Each request is said in plain words for
//! the confirmation, and anything it can't say is refused.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::address_check::hidden;
use crate::near::Action;
use crate::near_owner::{raw_elements, raw_value, stringify};
use crate::{Result, fail, to_hex};

/// The Agent API hosts a NEP-413 message may be addressed to.
pub const HOSTS: [&str; 2] = ["api.demo.agentsonintents.com", "api.agentsonintents.com"];
/// The custody contract the owner's delegate actions call (GET /v1/network: contract_id, 2026-11-02).
pub const CUSTODY: [&str; 1] = ["outlayer.near"];
/// NEP-413 owner requests the signer signs. An approval vote (to the custody contract) isn't one yet.
const MESSAGES: [&str; 3] = ["grant_issue", "grant_revoke", "execution_cancel"];
/// The custody contract's methods an owner's delegate action may call, pinned from what the API was seen to ask (a live
/// generate-intent, 2026-11-04): agent_create stores the wallet's first policy with `store_wallet_policy` (args:
/// wallet_pubkey, encrypted_data, wallet_signature, agent_policy_hash, storage_refund_to), and agent_freeze stores it
/// again frozen, then calls `freeze_wallet` (2026-11-05), no NEAR attached.
// ponytail: unfreeze/policy_update methods get pinned the same way when a live run first asks for them.
pub const METHODS: [&str; 2] = ["store_wallet_policy", "freeze_wallet"];
/// The NEAR a policy's storage may take from the owner's account (store_wallet_policy asked 0.01882 NEAR).
const MAX_STORAGE: u128 = 50_000_000_000_000_000_000_000; // 0.05 NEAR
/// A grant lasts at most this long: a bearer token on the PC shouldn't outlive a month.
const GRANT_DAYS: i64 = 31;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub message: String,
    pub nonce: Vec<u8>,
    pub recipient: String,
    pub doing: String,
}

/// A NEP-413 payload {message, recipient, nonce}: the API host, 32 random bytes, an owner request it can say.
pub fn message(payload: &[u8]) -> Result<Message> {
    let p: Option<Value> = serde_json::from_slice(payload).ok();
    let field = |k: &str| p.as_ref().and_then(|p| p.get(k)).and_then(Value::as_str);
    let (Some(message), Some(recipient), Some(nonce)) = (field("message"), field("recipient"), field("nonce").and_then(|n| STANDARD.decode(n).ok())) else {
        return fail("Not a NEP-413 request.");
    };
    if !HOSTS.contains(&recipient) { return fail(format!("A message for «{recipient}», not NEAR Intents' Agent API: not signed.")) }
    if nonce.len() != 32 { return fail("A NEP-413 nonce is 32 bytes.") }
    stringify(message.as_bytes())?; // refuses a key written twice
    // A grant names itself by its domain (live, 2026-11-04: «near-intents-agent-api.agent-grant.v6», no «type»).
    let m = serde_json::from_str::<Value>(message).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
    let text = |k: &str| m.get(k).and_then(Value::as_str);
    let Some(ty) = text("domain").and_then(type_for_domain).or_else(|| text("type")) else {
        return fail("The message isn't an Agent API request: not signed.");
    };
    if !MESSAGES.contains(&ty) { return fail(format!("The signer doesn't sign «{ty}» for an agent wallet.")) }
    if text("recipient").is_some_and(|inner| inner != recipient) {
        return fail("The message names another host than it's addressed to: not signed.");
    }
    if ty == "grant_issue" {
        let expires_ms = m.get("expires_at_ms").and_then(Value::as_f64)
            .or_else(|| text("expires_at").and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|d| d.timestamp_millis() as f64));
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
        if !expires_ms.is_some_and(|e| e - now_ms <= (GRANT_DAYS * 86_400_000) as f64) {
            return fail(format!("A grant for longer than {GRANT_DAYS} days, or with no end: not signed."));
        }
    }
    Ok(Message { message: message.into(), nonce, recipient: recipient.into(), doing: say(ty, &m) })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegate {
    pub receiver: String,
    pub actions: Vec<Action>,
    pub doing: String,
}

/// A NEP-366 payload {receiverId, actions[{type: FunctionCall, params{methodName, args, gas, deposit}}]}: calls to the
/// custody contract only (`allowed`: its methods, `METHODS` but in tests), attaching no NEAR, the args signed byte for
/// byte as JSON.stringify writes them.
pub fn delegate(payload: &[u8], allowed: &[&str]) -> Result<Delegate> {
    let p: Option<Value> = serde_json::from_slice(payload).ok();
    let receiver = p.as_ref().and_then(|p| p.get("receiverId")).and_then(Value::as_str);
    let list: Option<Vec<&Map<String, Value>>> = p.as_ref().and_then(|p| p.get("actions")).and_then(Value::as_array)
        .and_then(|l| l.iter().map(Value::as_object).collect());
    let (Some(receiver), Some(list)) = (receiver, list.filter(|l| (1..=2).contains(&l.len()))) else { return fail("Not a NEP-366 request.") };
    if !CUSTODY.contains(&receiver) { return fail(format!("A delegate action for «{receiver}», not the agent wallets' custody: not signed.")) }
    stringify(payload)?; // refuses a key written twice anywhere in it
    // The raw text of each action's args, as the API wrote them: re-encoding would change the bytes it checks.
    let raw_actions = raw_elements(raw_value("actions", payload)?)?;
    if raw_actions.len() != list.len() { return fail("The actions don't read the same twice: not signed.") }
    let (mut actions, mut words) = (Vec::new(), Vec::new());
    for (a, raw) in list.iter().zip(raw_actions) {
        let params = a.get("params").and_then(Value::as_object);
        let method = params.and_then(|p| p.get("methodName")).and_then(Value::as_str);
        let gas = params.and_then(|p| number_text(p.get("gas")).parse::<u64>().ok());
        let deposit = params.and_then(|p| number_text(p.get("deposit")).parse::<u128>().ok());
        let (true, Some(method), Some(gas), Some(deposit)) = (a.get("type").and_then(Value::as_str) == Some("FunctionCall"), method, gas, deposit) else {
            return fail("Only function calls are signed for an agent wallet.");
        };
        if !allowed.contains(&method) { return fail(format!("The signer doesn't sign «{method}» on the agent wallets' custody.")) }
        // Only a policy's storage may cost NEAR, and only a little: it's said in the words the owner confirms.
        if !(deposit <= 1 || (method == "store_wallet_policy" && deposit <= MAX_STORAGE)) {
            return fail(format!("The call attaches {} NEAR: not signed.", near(deposit)));
        }
        if gas > 300_000_000_000_000 { return fail("The call asks for more than 300 Tgas: not signed.") }
        let args = stringify(raw_value("args", raw_value("params", raw)?)?)?;
        let Some(parsed) = serde_json::from_slice::<Value>(&args).ok().and_then(|v| v.as_object().cloned()) else {
            return fail("The call's arguments can't be read: not signed.");
        };
        let mut said = say_call(method, &parsed);
        if deposit > 1 {
            // The policy is stored encrypted for the custody: the signer can't read it, so its storage is all it says.
            let refund = parsed.get("storage_refund_to").and_then(Value::as_str).map_or(String::new(), |r| format!(" (refunded to {}…, not to you)", plain(r, 8)));
            said += &format!(", paying {} NEAR from your NEAR account for its storage{refund}", near(deposit));
        }
        words.push(said);
        actions.push(Action::FunctionCall { method: method.into(), args, gas, deposit });
    }
    Ok(Delegate { receiver: receiver.into(), actions, doing: words.join(", then ") })
}

/// An agent wallet's complete policy, checked by the signer before the owner signs anything that carries it (or its
/// hash): money leaves only for addresses the signer itself knows — this wallet's own (`own`), ones confirmed in person
/// (`known`) — or the owner's NEAR account; no native signing routes. Returns the policy in words for the prompt.
// ponytail: the delegate's args carry the policy's hash (docs); binding this policy to that hash waits for the scheme
// seen in a live generate-intent.
pub fn policy(json: &[u8], owner: &str, own: &HashSet<String>, known: &HashSet<String>) -> Result<String> {
    stringify(json)?; // a key written twice
    let Some(p) = serde_json::from_slice::<Value>(json).ok().and_then(|v| v.as_object().cloned()) else { return fail("Not a policy.") };
    if p.get("native").is_some_and(|n| !n.is_null()) { return fail("The policy lets the agent sign on chains directly (native routes): not signed.") }
    let d = p.get("destinations").and_then(Value::as_object);
    let list: Option<Vec<&Map<String, Value>>> = d.and_then(|d| d.get("list")).and_then(Value::as_array).and_then(|l| l.iter().map(Value::as_object).collect());
    let (true, Some(list)) = (d.and_then(|d| d.get("mode")).and_then(Value::as_str) == Some("only"), list) else {
        return fail("The policy lets the agent send to addresses you didn't list: not signed.");
    };
    let key = |a: &str| if a.starts_with("0x") { a.to_lowercase() } else { a.to_string() };
    let allowed: HashSet<String> = own.iter().chain(known).map(|a| key(a)).collect();
    for e in &list {
        let Some(address) = e.get("address").and_then(Value::as_str) else { return fail("A destination without an address: not signed.") };
        let action = e.get("action").and_then(Value::as_str);
        let ok = (action == Some("transfer") && address == owner) || (action == Some("withdraw") && allowed.contains(&key(address)));
        if !ok { return fail(format!("The policy lets the agent pay {address}, an address this wallet hasn't confirmed: not signed.")) }
    }
    let budget = p.get("budget").and_then(|b| b.get("daily_usd")).and_then(Value::as_str);
    let Some(budget) = budget.filter(|b| b.parse::<f64>().is_ok_and(|d| d.is_finite() && d > 0.0)) else {
        return fail("The policy has no daily cap: not signed.");
    };
    let n = list.len();
    Ok(format!("at most ${} a day, only to {n} address{} of yours{}", plain(budget, 20), if n == 1 { "" } else { "es" },
               if p.get("frozen") == Some(&Value::Bool(true)) { ", frozen" } else { "" }))
}

/// A call that stores rules is signed only beside the rules the signer checked (`policy`): the hash it carries
/// (`agent_policy_hash`) is the custody's own — a live agent_create (2026-11-03) carried 2021a689… for rules the API
/// hashed cd7ca4a9… — so it can't be recomputed here (`AgentOwner.rulesShown`). The app binds the API's preview hash
/// before and the applied hash after.
pub fn rules_shown(actions: &[Action], policy: Option<&[u8]>) -> Result<()> {
    let stores = actions.iter().any(|a| matches!(a, Action::FunctionCall { method, .. } if method == "store_wallet_policy"));
    if stores && policy.is_none() { return fail("The rules this call stores weren't shown to the signer: not signed.") }
    Ok(())
}

/// sha256 of a policy written with sorted keys and no spaces: how the Agent API names a policy (preview.policy_hash,
/// `AgentAPI.policyHash`). serde_json's map keeps keys sorted, as Swift's `.sortedKeys`.
// ponytail: a number written 1.0 comes back «1.0» here and «1» from Swift; the API's policies carry amounts as strings.
pub fn policy_hash(policy: &[u8]) -> Option<String> {
    let v = serde_json::from_slice::<Value>(policy).ok().filter(|v| v.is_object() || v.is_array())?;
    Some(to_hex(&Sha256::digest(v.to_string().as_bytes())))
}

/// yoctoNEAR as NEAR, to 5 decimals.
fn near(yocto: u128) -> String {
    let micro = yocto / 10_000_000_000_000_000_000; // 1e19: hundred-thousandths of a NEAR
    format!("{}.{:05}", micro / 100_000, micro % 100_000)
}

/// A value as Swift's `"\(x)"` reads a number: a string as is, a JSON number as written, anything else nothing.
fn number_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// The first `n` characters of what the API wrote, with nothing that could draw a line of its own on the prompt.
fn plain(s: &str, n: usize) -> String { s.chars().filter(|&c| !hidden(c)).take(n).collect() }

/// The request a domain names: «near-intents-agent-api.agent-grant.v6» is a grant, whatever its version.
fn type_for_domain(domain: &str) -> Option<&'static str> { domain.starts_with("near-intents-agent-api.agent-grant.").then_some("grant_issue") }

/// The owner request in words, from its own fields.
fn say(ty: &str, m: &Map<String, Value>) -> String {
    let agent = m.get("agent_id").and_then(Value::as_str).map_or(String::new(), |a| format!(" {}…", plain(a, 8)));
    match ty {
        "grant_issue" => {
            let until = m.get("expires_at_ms").and_then(Value::as_f64)
                .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms as i64)).map(|d| d.format("%Y-%m-%d").to_string())
                .or_else(|| m.get("expires_at").and_then(Value::as_str).map(|t| plain(t, 10)));
            format!("let this PC run agent wallet{agent} within its rules{}", until.map_or(String::new(), |u| format!(" until {u}")))
        }
        "grant_revoke" => format!("take back this PC's access to agent wallet{agent}"),
        "execution_cancel" => format!("cancel a queued move of agent wallet{agent}"),
        other => other.into(),
    }
}

fn say_call(method: &str, args: &Map<String, Value>) -> String {
    // The policy a call carries, when it carries one: the daily budget and where money may go.
    let policy = args.get("policy").and_then(Value::as_object).unwrap_or(args);
    if method == "freeze_wallet" { return "freeze the agent wallet: nothing leaves it until you unfreeze it".into() }
    let mut parts = vec![method.replace('_', " ")];
    if let Some(budget) = policy.get("budget").and_then(|b| b.get("daily_usd")).and_then(Value::as_str) { parts.push(format!("at most ${} a day", plain(budget, 20))) }
    let destinations = policy.get("destinations").and_then(Value::as_object);
    if let (Some(d), Some(mode)) = (destinations, destinations.and_then(|d| d.get("mode")).and_then(Value::as_str)) {
        let n = d.get("list").and_then(Value::as_array).map_or(0, Vec::len);
        let es = if n == 1 { "" } else { "es" };
        parts.push(match mode {
            "only" => format!("only to {n} address{es} you chose"),
            "any" => "to ANY address".into(),
            _ => format!("to any address but {n}"),
        });
    }
    if policy.get("frozen") == Some(&Value::Bool(true)) { parts.push("frozen".into()) }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso(days: f64) -> String {
        (chrono::Utc::now() + chrono::Duration::milliseconds((days * 86_400_000.0) as i64)).format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    fn payload(message: &str, recipient: &str) -> Vec<u8> {
        serde_json::json!({"message": message, "recipient": recipient, "nonce": STANDARD.encode([1u8; 32])}).to_string().into_bytes()
    }

    #[test]
    fn agent_owner_signs_only_what_it_can_say() {
        let demo = "api.demo.agentsonintents.com";
        let soon = iso(20.0);
        let grant = message(&payload(&format!(r#"{{"type":"grant_issue","agent_id":"abcdef0123","expires_at":"{soon}"}}"#), demo)).unwrap();
        assert!(grant.doing.contains("agent wallet abcdef01") && grant.doing.contains(&soon[..10]), "{}", grant.doing);
        assert!(message(&payload(r#"{"type":"grant_issue"}"#, "evil.example")).is_err());
        assert!(message(&payload(r#"{"type":"approval_vote"}"#, demo)).is_err());

        let create = r#"{"receiverId":"outlayer.near","actions":[{"type":"FunctionCall","params":{"methodName":"create_wallet","args":{"policy":{"budget":{"daily_usd":"20"},"destinations":{"mode":"only","list":[{"a":1}]}},"z":"x/y"},"gas":"100000000000000","deposit":"0"}}]}"#;
        // Nothing on the custody contract is signed until its method is pinned from a live run.
        assert!(delegate(create.as_bytes(), &METHODS).is_err());
        let d = delegate(create.as_bytes(), &["create_wallet"]).unwrap();
        assert_eq!(d.doing, "create wallet, at most $20 a day, only to 1 address you chose");
        let Action::FunctionCall { args, .. } = &d.actions[0] else { panic!("not a call") };
        assert_eq!(std::str::from_utf8(args).unwrap(), r#"{"policy":{"budget":{"daily_usd":"20"},"destinations":{"mode":"only","list":[{"a":1}]}},"z":"x/y"}"#);
        assert!(delegate(create.replace("outlayer.near", "evil.near").as_bytes(), &["create_wallet"]).is_err());
        assert!(delegate(create.replace(r#""deposit":"0""#, r#""deposit":"1000""#).as_bytes(), &["create_wallet"]).is_err());
        // A key written twice reads one way here and another way on the server: refused.
        assert!(delegate(create.replace(r#""mode":"only""#, r#""mode":"only","mode":"any""#).as_bytes(), &["create_wallet"]).is_err());
        // A grant for longer than a month isn't signed.
        let far = iso(60.0);
        assert!(message(&payload(&format!(r#"{{"type":"grant_issue","agent_id":"a","expires_at":"{far}"}}"#), demo)).is_err());
    }

    #[test]
    fn an_agent_policy_pays_only_addresses_the_signer_knows() {
        let owner = "ab".repeat(32);
        let (mine, evm) = ("So11111111111111111111111111111111111111112", "0x70997970C51812dc3A010C7d01b50e0d17dc79C8");
        let policy_of = |list: &str, mode: &str, extra: &str| {
            format!(r#"{{"frozen":false,"actions":["withdraw"],"destinations":{{"mode":"{mode}","list":[{list}]}},"budget":{{"daily_usd":"20"}}{extra}}}"#).into_bytes()
        };
        let ok = format!(r#"{{"action":"transfer","address":"{owner}"}},{{"action":"withdraw","chain":"sol","address":"{mine}"}},{{"action":"withdraw","chain":"base","address":"{}"}}"#, evm.to_lowercase());
        let set = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<HashSet<_>>();
        assert_eq!(policy(&policy_of(&ok, "only", ""), &owner, &set(&[mine]), &set(&[evm])).unwrap(), "at most $20 a day, only to 3 addresses of yours");
        assert!(policy(&policy_of(&ok, "any", ""), &owner, &set(&[mine]), &set(&[evm])).is_err());
        assert!(policy(&policy_of(&ok, "only", ""), &owner, &set(&[mine]), &set(&[])).is_err());
        assert!(policy(&policy_of(&ok, "only", r#","native":{"near":{"transfer":true}}"#), &owner, &set(&[mine]), &set(&[evm])).is_err());
        assert!(policy(&policy_of(r#"{"action":"transfer","address":"evil.near"}"#, "only", ""), &owner, &set(&[]), &set(&[])).is_err());
    }

    /// agent_create as the Agent API asked it live (2026-11-04): store_wallet_policy with 0.01882 NEAR for storage is
    /// signed and said; more NEAR, or NEAR on another method, is refused.
    #[test]
    fn agent_create_stores_its_policy_for_a_small_storage_deposit() {
        let call = |method: &str, deposit: &str| format!(r#"{{"actions":[{{"type":"FunctionCall","params":{{"gas":"100000000000000","args":{{"wallet_pubkey":"ed25519:00","encrypted_data":"AQ==","wallet_signature":"00","agent_policy_hash":"00","storage_refund_to":"relayer.near"}},"deposit":"{deposit}","methodName":"{method}"}}}}],"receiverId":"outlayer.near"}}"#);
        let d = delegate(call("store_wallet_policy", "18820000000000000000000").as_bytes(), &METHODS).unwrap();
        assert!(d.doing.contains("store wallet policy"));
        assert!(d.doing.contains("0.01882 NEAR"));
        assert!(d.doing.contains("not to you"));
        assert!(delegate(call("store_wallet_policy", "100000000000000000000000").as_bytes(), &METHODS).is_err());
        assert!(delegate(call("store_wallet_policy_v2", "0").as_bytes(), &METHODS).is_err());
        assert!(delegate(call("withdraw", "18820000000000000000000").as_bytes(), &["withdraw"]).is_err());
    }

    /// A grant as the Agent API wrote it live (2026-11-04): no «type», a domain; signed for at most 31 days, to the host
    /// the payload is addressed to.
    #[test]
    fn agent_grant_is_read_by_its_domain() {
        let grant = |days: f64, inner: &str| {
            let ms = chrono::Utc::now().timestamp_millis() + (days * 86_400_000.0) as i64;
            let message = format!(r#"{{"agent_id":"ab12cd34ef","credential":"00","domain":"near-intents-agent-api.agent-grant.v6","expires_at_ms":{ms},"label":"test","network":"mainnet","owner":{{"account_id":"owner.near","type":"near"}},"recipient":"{inner}"}}"#);
            serde_json::json!({"message": message, "nonce": STANDARD.encode([7u8; 32]), "recipient": "api.demo.agentsonintents.com"}).to_string().into_bytes()
        };
        let m = message(&grant(30.0, "api.demo.agentsonintents.com")).unwrap();
        assert!(m.doing.starts_with("let this PC run agent wallet ab12cd34"), "{}", m.doing);
        assert!(message(&grant(60.0, "api.demo.agentsonintents.com")).is_err());
        assert!(message(&grant(30.0, "evil.example")).is_err());
    }

    /// agent_freeze as the API asked it live (2026-11-05): the rules stored again, then freeze_wallet, no NEAR attached.
    #[test]
    fn agent_freeze_is_signed_and_said_but_costs_no_near() {
        let freeze = |deposit: &str| format!(r#"{{"actions":[{{"type":"FunctionCall","params":{{"gas":"50000000000000","args":{{"wallet_pubkey":"ed25519:00","encrypted_data":"AQ==","wallet_signature":"00","agent_policy_hash":"00"}},"deposit":"0","methodName":"store_wallet_policy"}}}},{{"type":"FunctionCall","params":{{"gas":"50000000000000","args":{{"wallet_pubkey":"ed25519:00"}},"deposit":"{deposit}","methodName":"freeze_wallet"}}}}],"receiverId":"outlayer.near"}}"#);
        let d = delegate(freeze("0").as_bytes(), &METHODS).unwrap();
        assert!(d.actions.len() == 2 && d.doing.contains("freeze the agent wallet") && !d.doing.contains(';'), "{}", d.doing);
        assert!(delegate(freeze("1000000000000000000000000").as_bytes(), &METHODS).is_err());
    }

    /// Swift's `AgentAPI.policyHash` of the policy the app asks for (JSONSerialization, sorted keys, 2026-11-07), keys
    /// given out of order and a «/» that stays unescaped.
    #[test]
    fn a_policy_hashes_as_swift_hashes_it() {
        let policy = br#"{"frozen":false,"actions":["swap","transfer","withdraw"],"confidential":false,"owner_approval":false,"assets":["nep141:sol-5ce3bf3a31af18be40ba30f721101b4341690186.omft.near"],"limits":{"per_transaction":{"nep141:sol-5ce3bf3a31af18be40ba30f721101b4341690186.omft.near":"20000000"},"hourly":{},"daily":{"nep141:sol-5ce3bf3a31af18be40ba30f721101b4341690186.omft.near":"20000000"},"monthly":{}},"max_actions_per_hour":10,"destinations":{"mode":"only","list":[{"action":"transfer","address":"owner.near"},{"action":"withdraw","chain":"sol","address":"So1/x"}]},"budget":{"daily_usd":"20","weekly_usd":null,"monthly_usd":null},"timelock_ms":0}"#;
        assert_eq!(policy_hash(policy).as_deref(), Some("aa44d7156859cbabba624e77312dc9bed390cd0a5eb6c2f5c0a2021c758b1944"));
        assert_eq!(policy_hash(b"not json"), None);
        assert_eq!(near(18_820_000_000_000_000_000_000), "0.01882");
        assert_eq!(near(1), "0.00000");
    }
}

#[cfg(test)]
mod bound {
    use super::*;

    fn call(method: &str) -> Action {
        // The custody's own hash, as a live agent_create carried it: unrelated to the rules' sha256.
        Action::FunctionCall { method: method.into(), args: br#"{"agent_policy_hash":"2021a6891e3ac1b50e111434f26419f981dab0ccef94b3d24977c495d8d3f112"}"#.to_vec(), gas: 1, deposit: 0 }
    }

    /// Rules are stored only beside the rules the signer checked, whatever hash the custody names them by.
    #[test]
    fn stored_rules_were_shown() {
        assert!(rules_shown(&[call("store_wallet_policy")], Some(br#"{"frozen":true}"#)).is_ok());
        assert!(rules_shown(&[call("store_wallet_policy"), call("freeze_wallet")], None).is_err());
        assert!(rules_shown(&[call("freeze_wallet")], None).is_ok());
    }
}
