//! A Relay bridge order as its quote gives it (protocol.v2.orderData, version v1), read and checked offline
//! (`RelayOrder.swift`). Its id is the order's EIP-712 hashStruct with no domain — the 32 bytes a deposit into Relay's
//! depository names — so the signer recomputes it from the order and checks the order before any deposit naming it
//! signs.

use serde_json::{Map, Value as Json};
use wallet::evm::keccak;
use wallet::keys::base58;
use wallet::typed_data::word;
use wallet::from_hex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayOrder {
    pub id: [u8; 32],
    pub input_chain: String,
    pub input_amount: String,
    pub output_chain: String,
    /// The output's one payee, as its chain writes it (an EVM address lowercased, a Solana address in Base58).
    pub recipient: String,
    pub minimum_amount: String,
    pub deadline: u64,
}

/// Why an order isn't signed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub reason: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.reason) }
}

impl std::error::Error for Refusal {}

type R<T> = std::result::Result<T, Refusal>;

fn no<T>(reason: impl Into<String>) -> R<T> { Err(Refusal { reason: reason.into() }) }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Vm { Evm, Svm, HypEvm }

/// The Relay networks ihatecrypto bridges between, by Relay's own name for them.
fn vm(chain: &str) -> Option<Vm> {
    Some(match chain {
        "solana" => Vm::Svm,
        "hyperliquid" => Vm::HypEvm,
        "ethereum" | "base" | "arbitrum" | "optimism" | "polygon" | "bnb" | "hyperevm" | "robinhood" | "tempo" => Vm::Evm,
        _ => return None,
    })
}

impl RelayOrder {
    /// For the prompt that allows its payee: what the order does.
    pub fn summary(&self) -> String {
        format!("bridge {} units from {} through Relay: at least {} units to {} on {}",
                self.input_amount, self.input_chain, self.minimum_amount, self.recipient, self.output_chain)
    }

    /// The order read and checked: one input, one payee on the output network, every refund to `evm`/`solana` (or the
    /// zero placeholder, which means the depositor), no calls run for it, no fees taken out of it, not yet expired.
    /// The payee may be someone else's address: the signer asks about it in person like any other.
    pub fn read(json: &[u8], evm: Option<&str>, solana: Option<&str>, now: u64) -> R<Self> {
        let refuse = |why: &str| Refusal { reason: format!("The Relay order {why}. Nothing was signed.") };
        let Ok(Json::Object(o)) = serde_json::from_slice::<Json>(json) else { return Err(refuse("can't be read")) };
        if o.get("version").and_then(Json::as_str) != Some("v1") { return Err(refuse("is of a version ihatecrypto doesn't read")) }
        let inputs = o.get("inputs").and_then(dicts).filter(|i| i.len() == 1);
        let payment = inputs.as_ref().and_then(|i| i[0].get("payment")).and_then(Json::as_object);
        let in_chain = payment.and_then(|p| p.get("chainId")).and_then(Json::as_str).filter(|c| vm(c).is_some());
        let (Some(inputs), Some(payment), Some(in_chain)) = (inputs, payment, in_chain) else {
            return Err(refuse("isn't one input from a network ihatecrypto knows"));
        };
        let output = o.get("output").and_then(Json::as_object);
        let out_chain = output.and_then(|x| x.get("chainId")).and_then(Json::as_str).filter(|c| vm(c).is_some());
        let payments = output.and_then(|x| x.get("payments")).and_then(dicts).filter(|p| p.len() == 1);
        let pay = payments.as_ref().and_then(|p| p[0].get("recipient")).and_then(Json::as_str);
        let (Some(output), Some(out_chain), Some(payments), Some(pay)) = (output, out_chain, payments.clone(), pay) else {
            return Err(refuse("doesn't pay one address on a network ihatecrypto knows"));
        };
        if !output.get("calls").and_then(Json::as_array).is_some_and(Vec::is_empty) { return Err(refuse("runs calls of its own on the other network")) }
        if !o.get("fees").and_then(Json::as_array).is_some_and(Vec::is_empty) { return Err(refuse("takes fees out for someone")) }
        let deadline = output.get("deadline").and_then(Json::as_u64).unwrap_or(0);
        if deadline <= now { return Err(refuse("has expired")) }
        let ours = |address: &str, chain: &str| -> bool {
            let Ok(raw) = bytes(address, chain) else { return false };
            if raw.iter().all(|&b| b == 0) { return true } // the native-currency placeholder: «the depositor»
            let own = if vm(chain) == Some(Vm::Svm) { solana.and_then(|s| base58::decode(s).ok()) } else { evm.and_then(from_hex) };
            own.as_deref() == Some(raw.as_slice())
        };
        for refund in inputs[0].get("refunds").and_then(dicts).unwrap_or_default() {
            let to = refund.get("recipient").and_then(Json::as_str);
            let chain = refund.get("chainId").and_then(Json::as_str);
            if !matches!((chain, to), (Some(c), Some(t)) if ours(t, c)) { return Err(refuse("refunds someone else")) }
        }
        let recipient = if vm(out_chain) == Some(Vm::Svm) { pay.to_string() } else { pay.to_lowercase() };
        let text = |m: &Map<String, Json>, k: &str| m.get(k).and_then(Json::as_str).unwrap_or("").to_string();
        Ok(RelayOrder {
            id: hash_order(&o)?, input_chain: in_chain.into(), input_amount: text(payment, "amount"), output_chain: out_chain.into(),
            recipient, minimum_amount: text(payments[0], "minimumAmount"), deadline,
        })
    }
}

/// A JSON array whose every item is an object (Swift's `as? [[String: Any]]`).
fn dicts(v: &Json) -> Option<Vec<&Map<String, Json>>> { v.as_array()?.iter().map(Json::as_object).collect() }

/// An address or a currency as its network's raw bytes: Base58 on Solana, 20 bytes on an EVM network, 1–20 on Hyperliquid.
fn bytes(s: &str, chain: &str) -> R<Vec<u8>> {
    match vm(chain) {
        Some(Vm::Svm) => match base58::decode(s) {
            Ok(b) if b.len() == 32 => Ok(b),
            _ => no(format!("Not a Solana address: {s}.")),
        },
        Some(Vm::Evm) => match from_hex(s) {
            Some(b) if b.len() == 20 => Ok(b),
            _ => no(format!("Not an EVM address: {s}.")),
        },
        Some(Vm::HypEvm) => match from_hex(s) {
            Some(b) if (1..=20).contains(&b.len()) => Ok(b),
            _ => no(format!("Not a Hyperliquid address: {s}.")),
        },
        None => no(format!("A network ihatecrypto doesn't bridge to: {chain}.")),
    }
}

/// The Order's EIP-712 types.
fn fields(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        "Order" => &[("version", "string"), ("solverChainId", "string"), ("solver", "address"), ("salt", "uint256"),
                     ("inputs", "Input[]"), ("output", "Output"), ("fees", "Fee[]")],
        "Input" => &[("payment", "InputPayment"), ("refunds", "InputRefund[]")],
        "InputPayment" => &[("chainId", "string"), ("currency", "bytes"), ("amount", "uint256"), ("weight", "uint256")],
        "InputRefund" => &[("chainId", "string"), ("recipient", "bytes"), ("currency", "bytes"), ("minimumAmount", "uint256"),
                           ("deadline", "uint32"), ("extraData", "bytes")],
        "Output" => &[("chainId", "string"), ("payments", "OutputPayment[]"), ("deadline", "uint32"), ("calls", "bytes[]"), ("extraData", "bytes")],
        "OutputPayment" => &[("recipient", "bytes"), ("currency", "bytes"), ("minimumAmount", "uint256"), ("expectedAmount", "uint256")],
        "Fee" => &[("recipientChainId", "string"), ("recipient", "bytes"), ("currencyChainId", "string"), ("currency", "bytes"), ("amount", "uint256")],
        _ => &[],
    }
}

fn type_string(name: &str) -> String {
    let one = |n: &str| format!("{n}({})", fields(n).iter().map(|(f, t)| format!("{t} {f}")).collect::<Vec<_>>().join(","));
    let mut deps = std::collections::BTreeSet::new();
    fn walk(n: &str, deps: &mut std::collections::BTreeSet<&'static str>) {
        for (_, t) in fields(n) {
            let base = t.strip_suffix("[]").unwrap_or(t);
            if !fields(base).is_empty() && deps.insert(base) { walk(base, deps) }
        }
    }
    walk(name, &mut deps);
    deps.remove(name);
    one(name) + &deps.iter().map(|d| one(d)).collect::<String>()
}

/// The id: the order's fields hashed the way relay-settlement's normalizeOrder hands them over — addresses and
/// currencies as their network's bytes (which network: the payment's, the refund's, the output's, the fee's own), hex as bytes.
fn hash_order(o: &Map<String, Json>) -> R<[u8; 32]> {
    let k = |d: &[u8]| keccak(d);
    let hex = |v: Option<&Json>| -> R<Vec<u8>> {
        match v.and_then(Json::as_str).and_then(from_hex) { Some(d) => Ok(d), None => no("Not hex in the Relay order.") }
    };
    let string = |v: Option<&Json>| -> R<[u8; 32]> {
        match v.and_then(Json::as_str) { Some(s) => Ok(keccak(s.as_bytes())), None => no("Not text in the Relay order.") }
    };
    let uint = |v: Option<&Json>| -> R<[u8; 32]> { word(v).map_err(|e| Refusal { reason: e.0 }) };
    let addr = |v: Option<&Json>, chain: Option<&Json>| -> R<[u8; 32]> {
        match (v.and_then(Json::as_str), chain.and_then(Json::as_str)) {
            (Some(s), Some(c)) => Ok(keccak(&bytes(s, c)?)),
            _ => no("An address without its network in the Relay order."),
        }
    };
    let type_hash = |n: &str| keccak(type_string(n).as_bytes());
    let list = |v: Option<&Json>, each: &dyn Fn(&Map<String, Json>) -> R<[u8; 32]>| -> R<[u8; 32]> {
        let Some(items) = v.and_then(dicts) else { return no("Not a list in the Relay order.") };
        let mut all = Vec::new();
        for item in items { all.extend(each(item)?) }
        Ok(k(&all))
    };
    let payment = |p: &Map<String, Json>| -> R<[u8; 32]> {
        Ok(k(&[type_hash("InputPayment"), string(p.get("chainId"))?, addr(p.get("currency"), p.get("chainId"))?, uint(p.get("amount"))?,
               uint(p.get("weight"))?].concat()))
    };
    let refund = |r: &Map<String, Json>| -> R<[u8; 32]> {
        Ok(k(&[type_hash("InputRefund"), string(r.get("chainId"))?, addr(r.get("recipient"), r.get("chainId"))?, addr(r.get("currency"), r.get("chainId"))?,
               uint(r.get("minimumAmount"))?, uint(r.get("deadline"))?, k(&hex(r.get("extraData"))?)].concat()))
    };
    let input = |i: &Map<String, Json>| -> R<[u8; 32]> {
        let Some(p) = i.get("payment").and_then(Json::as_object) else { return no("An input without its payment in the Relay order.") };
        Ok(k(&[type_hash("Input"), payment(p)?, list(i.get("refunds"), &refund)?].concat()))
    };
    let Some((out, out_chain)) = o.get("output").and_then(Json::as_object).and_then(|x| Some((x, x.get("chainId")?))) else {
        return no("The Relay order has no output.");
    };
    let output_payment = |p: &Map<String, Json>| -> R<[u8; 32]> {
        Ok(k(&[type_hash("OutputPayment"), addr(p.get("recipient"), Some(out_chain))?, addr(p.get("currency"), Some(out_chain))?,
               uint(p.get("minimumAmount"))?, uint(p.get("expectedAmount"))?].concat()))
    };
    let Some(calls) = out.get("calls").and_then(Json::as_array).and_then(|c| c.iter().map(|x| x.as_str()).collect::<Option<Vec<_>>>()) else {
        return no("The Relay order's calls can't be read.");
    };
    let mut call_hashes = Vec::new();
    for c in calls { call_hashes.extend(k(&hex(Some(&Json::from(c)))?)) }
    let output = k(&[type_hash("Output"), string(Some(out_chain))?, list(out.get("payments"), &output_payment)?, uint(out.get("deadline"))?,
                     k(&call_hashes), k(&hex(out.get("extraData"))?)].concat());
    let fee = |f: &Map<String, Json>| -> R<[u8; 32]> {
        Ok(k(&[type_hash("Fee"), string(f.get("recipientChainId"))?, addr(f.get("recipient"), f.get("recipientChainId"))?,
               string(f.get("currencyChainId"))?, addr(f.get("currency"), f.get("currencyChainId"))?, uint(f.get("amount"))?].concat()))
    };
    let Some(solver) = o.get("solver").and_then(Json::as_str).and_then(from_hex).filter(|s| s.len() == 20) else {
        return no("The Relay order's solver isn't an address.");
    };
    Ok(k(&[&type_hash("Order")[..], &string(o.get("version"))?, &string(o.get("solverChainId"))?, &[0; 12], &solver, &uint(o.get("salt"))?,
           &list(o.get("inputs"), &input)?, &output, &list(o.get("fees"), &fee)?].concat()))
}
