//! `MPP.swift`: paying for an API on Tempo — MPP, the «Payment» HTTP auth scheme (paymentauth.org, the x402 flow with
//! receipts). The service answers 402 with a challenge: this much USDC on Tempo to this payee. ihatecrypto pays it itself
//! (push mode: its own transfer, signed after Confirm) and asks again with the transaction's hash as the credential.
//! Pull mode (a signed Tempo 0x76 transaction the service sends) and sessions aren't supported: the signer signs only
//! its own EIP-1559 transfers, so a service that takes only those is refused with the reason.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, URL_SAFE_NO_PAD};
use llm::{Client, Message, Request, Role};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use unicode_segmentation::UnicodeSegmentation;
use wallet::WalletError;

use crate::foundation::{Caps, Date, adding, all, chars, count, decimal, first, format_g, group, has, is_letter, is_number, iso8601, replacing,
                        split_chars, trim_set, whole};
use crate::intent::Fmt;

/// The one token ihatecrypto pays in: USDC on Tempo, 6 decimals.
pub const USDC: &str = "0x20c000000000000000000000b9537d11c60e8b50";
pub const CHAIN_ID: u64 = 4217;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// The auth-params as the service sent them: the credential echoes them unchanged.
    pub params: HashMap<String, String>,
    /// USDC units.
    pub amount: u64,
    pub payee: String,
    pub memo: Option<Vec<u8>>,
    pub expires: Option<Date>,
}

impl Challenge {
    pub fn realm(&self) -> &str { self.params.get("realm").map_or("", String::as_str) }
    pub fn purpose(&self) -> Option<&str> { self.params.get("description").map(String::as_str) }
    /// «Payment-Authorization» when the service keeps Authorization for its own login.
    pub fn field(&self) -> &str { self.params.get("header").map_or("Authorization", String::as_str) }
    pub fn usdc(&self) -> Decimal { Decimal::from_i128_with_scale(self.amount.into(), 6).normalize() }
}

/// The Payment challenges in a WWW-Authenticate value, each as its auth-params (a value may list several schemes).
pub(crate) fn challenges(header: &str) -> Vec<HashMap<String, String>> {
    let (mut out, mut current, mut scheme) = (Vec::new(), None::<HashMap<String, String>>, String::new());
    for m in all(header, r#"(?:([A-Za-z][\w-]*)\s+)?([A-Za-z][\w-]*)=("(?:[^"\\]|\\.)*"|[^\s,]+)"#) {
        if let Some(s) = m.get(1) {
            if let Some(c) = current.take() && scheme == "payment" { out.push(c) }
            scheme = s.as_str().to_lowercase();
            current = Some(HashMap::new());
        }
        let mut value = group(&m, 3).to_string();
        if let Some(quoted) = value.strip_prefix('"') {
            // `dropFirst().dropLast()`: the quotes, or the opening one and the last Character of a value never closed.
            let inner = quoted.grapheme_indices(true).next_back().map_or(quoted, |(i, _)| &quoted[..i]);
            value = replacing(inner, r"\\(.)", |c| group(c, 1).to_string());
        }
        if let Some(c) = current.as_mut() { c.insert(group(&m, 2).to_lowercase(), value); }
    }
    if let Some(c) = current && scheme == "payment" { out.push(c) }
    out
}

/// The challenge ihatecrypto can pay, or why none: USDC on Tempo, a charge of more than zero, push mode allowed. `now`:
/// Swift's `Date()`; a quote that has expired by then is never paid.
pub fn challenge(header: &str, now: Date) -> Result<Challenge, Refusal> {
    let all = challenges(header);
    let Some(p) = all.iter().find(|p| p.get("method").is_some_and(|m| m == "tempo") && p.get("intent").is_some_and(|i| i == "charge")) else {
        let offered = all.iter().map(|p| format!("{} {}", p.get("method").map_or("?", String::as_str), p.get("intent").map_or("?", String::as_str)))
            .collect::<Vec<_>>().join(", ");
        return Err(Refusal::new(if offered.is_empty() { "The service asked for payment without an MPP challenge.".into() } else {
            format!("The service takes {offered}, but ihatecrypto pays only a one-time charge in USDC on Tempo.")
        }));
    };
    let request = p.get("id").filter(|id| !id.is_empty()).and(p.get("realm")).and(p.get("request")).and_then(|r| base64url(r))
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok());
    let Some(request) = request.as_ref().and_then(Value::as_object) else { return Err(Refusal::new("The service's payment request can't be read.")) };
    let empty = Map::new();
    let details = request.get("methodDetails").and_then(Value::as_object).unwrap_or(&empty);
    let Some(amount) = request.get("amount").and_then(Value::as_str).and_then(|a| a.parse::<u64>().ok()).filter(|a| *a > 0) else {
        return Err(Refusal::new("The service asks for a zero-amount proof or an amount that can't be read, and ihatecrypto only pays real charges."));
    };
    let currency = request.get("currency").and_then(Value::as_str);
    if currency.map(str::to_lowercase).as_deref() != Some(USDC) {
        return Err(Refusal::new(format!("The service charges in {}, but ihatecrypto pays in USDC on Tempo.", currency.unwrap_or("an unknown token"))));
    }
    if let Some(chain) = details.get("chainId").and_then(uint64) && chain != CHAIN_ID {
        return Err(Refusal::new(format!("The service wants payment on network {chain}, not Tempo mainnet.")));
    }
    let Some(payee) = request.get("recipient").and_then(Value::as_str).filter(|r| whole(r, "0x[0-9a-fA-F]{40}").is_some()) else {
        return Err(Refusal::new("The service's payee isn't an address."));
    };
    if details.get("splits").and_then(Value::as_array).is_some_and(|s| !s.is_empty()) {
        return Err(Refusal::new("The service splits the payment between several payees, but ihatecrypto pays one."));
    }
    if let Some(modes) = strings(details.get("supportedModes")) && !modes.contains(&"push") {
        return Err(Refusal::new("The service takes only a transaction it sends itself (pull mode), but ihatecrypto sends its own and proves it with the hash."));
    }
    if details.get("feePayer").is_some_and(is_true) {
        return Err(Refusal::new("The service pays the fee itself (sponsored mode), which needs a transaction it sends, but ihatecrypto sends its own."));
    }
    let mut memo = None;
    if let Some(m) = details.get("memo").and_then(Value::as_str) {
        let Some(bytes) = whole(m, "0x[0-9a-fA-F]{64}").and_then(|_| wallet::from_hex(m)) else {
            return Err(Refusal::new("The service's memo isn't 32 bytes."));
        };
        memo = Some(bytes);
    }
    let expires = p.get("expires").and_then(|e| date(e));
    if expires.is_some_and(|e| e <= now) { return Err(Refusal::new("The service's price quote has expired. Ask again.")) }
    Ok(Challenge { params: p.clone(), amount, payee: payee.to_string(), memo, expires })
}

/// `(x as? NSNumber)?.uint64Value`: a JSON number or true/false, a fraction cut, a negative one wrapped as in C.
fn uint64(v: &Value) -> Option<u64> {
    match v {
        Value::Bool(b) => Some(u64::from(*b)),
        Value::Number(n) => n.as_u64().or_else(|| n.as_i64().map(|i| i as u64)).or_else(|| n.as_f64().map(|f| f as u64)),
        _ => None,
    }
}

/// `x as? Bool == true`: true, or a number that is exactly 1 (NSNumber bridges to Bool when it's 0 or 1).
fn is_true(v: &Value) -> bool { v.as_bool() == Some(true) || v.as_f64() == Some(1.0) }

/// `x as? [String]`: a list of strings only.
fn strings(v: Option<&Value>) -> Option<Vec<&str>> { v?.as_array()?.iter().map(Value::as_str).collect() }

/// `x as? [[String: Any]]`: a list of objects only.
fn objects(v: Option<&Value>) -> Option<Vec<&Map<String, Value>>> { v?.as_array()?.iter().map(Value::as_object).collect() }

/// Why MPP stopped: before paying (nothing paid) or, from `paid`, after it (the reason names the payment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub reason: String,
}

impl Refusal {
    pub(crate) fn new(reason: impl Into<String>) -> Self { Self { reason: reason.into() } }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.reason) }
}

impl std::error::Error for Refusal {}

/// The header value that proves the payment: «Payment <base64url JSON>» with the challenge echoed, the transaction's
/// hash and the payer as a did:pkh.
pub fn credential(c: &Challenge, hash: &str, payer: &str) -> String {
    const ECHOED: [&str; 10] = ["id", "realm", "method", "intent", "request", "description", "opaque", "digest", "expires", "header"];
    let echoed: Map<String, Value> = c.params.iter().filter(|(k, _)| ECHOED.contains(&k.as_str())).map(|(k, v)| (k.clone(), json!(v))).collect();
    // serde_json's objects keep their keys sorted, as `.sortedKeys` writes them; «/» stays unescaped.
    let body = json!({"challenge": echoed, "payload": {"type": "hash", "hash": hash}, "source": format!("did:pkh:eip155:{CHAIN_ID}:{payer}")});
    format!("Payment {}", URL_SAFE_NO_PAD.encode(body.to_string()))
}

/// `Data(base64Encoded:)` of base64url: Foundation's decoder keeps a last character's spare bits.
pub(crate) fn base64url(s: &str) -> Option<Vec<u8>> {
    const LENIENT: GeneralPurpose = GeneralPurpose::new(&base64::alphabet::STANDARD, GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true));
    let mut b = s.replace('-', "+").replace('_', "/");
    b += &"=".repeat((4 - count(&b) % 4) % 4);
    LENIENT.decode(b).ok()
}

/// ISO 8601 with or without a fraction of a second («2026-09-29T19:32:58.380Z»): ISO8601DateFormatter's
/// `.withFractionalSeconds` first, which reads milliseconds and drops the digits after them, then the plain form.
pub(crate) fn date(s: &str) -> Option<Date> {
    let Some(c) = whole(s, r"(.*T\d\d:\d\d:\d\d)\.(\d+)(Z|[+-]\d\d:?\d\d)") else { return iso8601(s) };
    let ms: u32 = format!("{:0<3}", &group(&c, 2)[..group(&c, 2).len().min(3)]).parse().ok()?;
    Some(adding(iso8601(&format!("{}{}", group(&c, 1), group(&c, 3)))?, f64::from(ms) / 1000.0))
}

/// The request as the service documents it; `credential` goes in the field its challenge named. The status, the headers
/// (names in lower case, repeated ones joined by «, » as URLSession joins them) and the body; the error is URLSession's
/// words.
pub async fn call(method: &str, url: &str, body: Option<&str>, credential: Option<(&str, &str)>)
    -> Result<(u16, HashMap<String, String>, Vec<u8>), WalletError> {
    // The tests' stand-in service is plain HTTP on this machine; every other build asks over https only.
    if !(url.starts_with("https:") || (cfg!(test) && url.starts_with("http://127.0.0.1:"))) { return Err(WalletError("unsupported URL".into())) }
    let method = method.parse().map_err(|e: _| WalletError(format!("{e}")))?;
    // URLRequest's timeoutInterval 60; here for the whole answer, which a service sends in one piece.
    let mut request = net::HTTP.request(method, url).timeout(Duration::from_secs(60));
    if let Some(body) = body { request = request.header("Content-Type", "application/json").body(body.to_string()) }
    if let Some((field, value)) = credential { request = request.header(field, value) }
    let response = request.send().await.map_err(|e| unreached(e.is_timeout(), e.is_connect(), e.is_builder(), e.to_string()))?;
    let status = response.status().as_u16();
    let mut headers = HashMap::<String, String>::new();
    for (name, value) in response.headers() {
        let value = String::from_utf8_lossy(value.as_bytes());
        headers.entry(name.as_str().to_string()).and_modify(|h| *h = format!("{h}, {value}")).or_insert_with(|| value.into_owned());
    }
    let data = response.bytes().await.map_err(|e| unreached(e.is_timeout(), e.is_connect(), e.is_builder(), e.to_string()))?;
    Ok((status, headers, data.to_vec()))
}

/// What URLSession says when a request got no answer (its errors' localizedDescription).
// ponytail: the three it says most; any other failure is reqwest's own words.
fn unreached(timeout: bool, connect: bool, builder: bool, words: String) -> WalletError {
    WalletError(if timeout { "The request timed out.".into() } else if connect { "Could not connect to the server.".into() } else if builder {
        "unsupported URL".into()
    } else { words })
}

/// The request again with the payment's proof. The money is gone by now: whatever goes wrong — no answer, a refusal —
/// names the payment, so the user can show it to the service.
pub async fn paid(method: &str, url: &str, body: Option<&str>, credential: (&str, &str), usdc: Decimal, service: &str, proof: &str)
    -> Result<Vec<u8>, Refusal> {
    let paid = format!("Paid {} USDC, but {service}", Fmt::num(usdc, 6));
    match call(method, url, body, Some(credential)).await {
        Err(e) => Err(Refusal::new(format!("{paid} didn't answer: {}. The payment is on Tempo: {proof}", trim_set(&e.0, ". ")))),
        Ok((status, _, data)) if !(200..300).contains(&status) => {
            Err(Refusal::new(format!("{paid} answered {status}: {}. The payment is on Tempo: {proof}", shown(&data, 300))))
        }
        Ok((_, _, data)) => Ok(data),
    }
}

/// A service's answer for the chat: JSON indented as JSONSerialization's `.prettyPrinted` writes it, cut at `limit`
/// characters (Swift's default is 1,500).
// ponytail: an object's keys come out sorted; Swift's come in NSDictionary's order (the service's for a few keys, a
// hash's for more).
pub fn shown(data: &[u8], limit: usize) -> String {
    let text = match serde_json::from_slice::<Value>(data) {
        Ok(v @ (Value::Object(_) | Value::Array(_))) => {
            let mut out = String::new();
            pretty(&v, 0, &mut out);
            out
        }
        _ => String::from_utf8_lossy(data).into_owned(),
    };
    let head = cut(&text, limit);
    if head.len() < text.len() { format!("{head}…") } else { text }
}

/// JSONSerialization's pretty printing: two spaces a level, « : » after a key, an empty list or object on three lines.
fn pretty(v: &Value, indent: usize, out: &mut String) {
    let (open, close, len) = match v {
        Value::Array(a) => ('[', ']', a.len()),
        Value::Object(o) => ('{', '}', o.len()),
        Value::Number(n) => return out.push_str(&number(n, 17)),
        other => return out.push_str(&other.to_string()),
    };
    out.push(open);
    out.push('\n');
    if len == 0 { out.push('\n') }
    let item = |i: usize, out: &mut String| {
        if i > 0 { out.push_str(",\n") }
        out.push_str(&" ".repeat(indent + 2));
    };
    match v {
        Value::Array(a) => a.iter().enumerate().for_each(|(i, x)| { item(i, out); pretty(x, indent + 2, out) }),
        Value::Object(o) => o.iter().enumerate().for_each(|(i, (k, x))| {
            item(i, out);
            out.push_str(&Value::String(k.clone()).to_string());
            out.push_str(" : ");
            pretty(x, indent + 2, out)
        }),
        _ => {}
    }
    if len > 0 { out.push('\n') }
    out.push_str(&" ".repeat(indent));
    out.push(close);
}

/// A number JSONSerialization read, as Swift writes it: a whole one as it is, a fraction with `digits` significant
/// digits (16 in NSNumber's `stringValue`, 17 in the JSON it writes).
// ponytail: a whole number past 64 bits, or a fraction of 18+ digits, is a Double here; Foundation keeps those as
// NSDecimalNumber and writes every digit.
fn number(n: &serde_json::Number, digits: usize) -> String {
    match (n.as_i64(), n.as_u64()) {
        (Some(i), _) => i.to_string(),
        (_, Some(u)) => u.to_string(),
        _ => format_g(n.as_f64().unwrap_or(0.0), digits),
    }
}

/// `String(s.prefix(n))`: the first `n` Characters.
fn cut(s: &str, n: usize) -> &str { s.grapheme_indices(true).nth(n).map_or(s, |(i, _)| &s[..i]) }

// MARK: The service's answer

/// What a service answered, for the canvas: its list of results (search hits, flights, images) when it has one, else
/// its plain values, else its text. Saved with its move in the canvas file, in Swift's synthesized Codable shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Answer {
    pub items: Vec<Item>,
    pub facts: Vec<Fact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// What an AI service wrote (Grok, Perplexity: the answer itself), and the links it cites. Its JSON puts the
    /// answer after its tool calls, so a cut of the start showed only «x_keyword_search» rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writing: Option<String>,
    pub links: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Item {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Item {
    pub fn new(title: &str, url: Option<&str>, detail: Option<&str>) -> Self {
        Self { title: title.into(), url: url.map(Into::into), detail: detail.map(Into::into) }
    }

    pub fn is_image(&self) -> bool { self.url.as_ref().is_some_and(|u| has(&u.to_lowercase(), r"\.(?:png|jpe?g|webp|gif)(?:\?|$)")) }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fact {
    pub key: String,
    pub value: String,
}

const TITLE_KEYS: [&str; 8] = ["title", "name", "headline", "label", "subject", "display_name", "full_name", "question"];
/// Where services put the words they wrote: xAI's and OpenAI's output_text, a chat reply's content, an answer.
const WRITING_KEYS: [&str; 6] = ["output_text", "text", "content", "answer", "response", "completion"];
const DETAIL_KEYS: [&str; 8] = ["description", "snippet", "summary", "content", "text", "body", "answer", "extra_snippets"];
const LINK_KEYS: [&str; 6] = ["url", "link", "href", "permalink", "profile_url", "image_url"];

type Row = Map<String, Value>;

impl Answer {
    pub fn new(data: &[u8]) -> Self {
        let mut a = Self::default();
        let Ok(json) = serde_json::from_slice::<Value>(data) else {
            let t = String::from_utf8_lossy(&data[..data.len().min(1500)]).trim().to_string();
            a.text = (!t.is_empty()).then_some(t);
            return a;
        };
        let mut texts = Vec::new();
        collect(&json, "", 0, &mut texts, &mut a.links);
        if !texts.is_empty() {
            a.writing = Some(cut(&texts.join("\n\n"), 6000).to_string());
            return a;
        }
        // The list is the array of objects with the most titles and links in it, wherever it sits («data.web.results»).
        let (mut best, mut score) = (Vec::new(), 0);
        walk(&json, 0, &mut best, &mut score);
        a.items = best.iter().take(10).filter_map(|row| item(row)).collect();
        if !a.items.is_empty() { return a }
        flatten(&json, "", 0, &mut a.facts);
        if a.facts.is_empty() { a.text = Some(shown(data, 1500)) }
        a
    }

    /// Search engines mark matches with <strong>: the canvas shows the words.
    pub fn plain(s: &str) -> String {
        replacing(s, "<[^>]+>", |_| String::new()).replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<")
            .replace("&gt;", ">").trim().to_string()
    }
}

fn collect(x: &Value, key: &str, depth: usize, texts: &mut Vec<String>, links: &mut Vec<String>) {
    if depth >= 14 { return }
    match x {
        Value::String(s) => {
            if s.starts_with("http") && count(s) < 400 {
                if !links.contains(s) && links.len() < 15 { links.push(s.clone()) }
            } else if WRITING_KEYS.contains(&key) && count(s) >= 80 && chars(s).iter().filter(|g| **g == " ").count() >= 8 && !texts.contains(s) {
                texts.push(s.clone());
            }
        }
        Value::Array(list) => list.iter().for_each(|v| collect(v, key, depth + 1, texts, links)),
        Value::Object(d) => d.iter().for_each(|(k, v)| collect(v, k, depth + 1, texts, links)),
        _ => {}
    }
}

fn walk<'a>(x: &'a Value, depth: usize, best: &mut Vec<&'a Row>, best_score: &mut usize) {
    if depth >= 7 { return }
    match x {
        Value::Array(list) => {
            let rows: Vec<&Row> = list.iter().filter_map(Value::as_object).collect();
            let score = rows.iter().filter(|r| pick(r, &TITLE_KEYS).is_some() || link(r).is_some()).count();
            if score > *best_score { (*best, *best_score) = (rows, score) }
            list.iter().for_each(|v| walk(v, depth + 1, best, best_score));
        }
        Value::Object(d) => d.values().for_each(|v| walk(v, depth + 1, best, best_score)),
        _ => {}
    }
}

fn item(row: &Row) -> Option<Item> {
    let url = link(row);
    // A row without a title or text (a flight: price, airline, times) shows its plain values instead.
    let shown = |k: &str| TITLE_KEYS.contains(&k) || DETAIL_KEYS.contains(&k) || LINK_KEYS.contains(&k);
    let facts: Vec<String> = row.iter().filter(|(k, _)| !shown(k))
        .filter_map(|(k, v)| scalar(v).map(|s| format!("{k} {s}"))).collect();
    let named = pick(row, &TITLE_KEYS).or_else(|| url.as_deref().and_then(host));
    let title = named.clone().or_else(|| facts.first().cloned())?;
    let rest = if named.is_none() { &facts[1..] } else { &facts[..] };
    let detail = pick(row, &DETAIL_KEYS).or_else(|| (!rest.is_empty()).then(|| rest.iter().take(3).cloned().collect::<Vec<_>>().join(" · ")));
    Some(Item { title, url, detail })
}

fn flatten(x: &Value, path: &str, depth: usize, facts: &mut Vec<Fact>) {
    if facts.len() >= 12 || depth >= 4 { return }
    if let Value::Object(d) = x {
        for (k, v) in d.iter().filter(|(k, _)| !["success", "type", "object", "status"].contains(&k.as_str())) {
            flatten(v, &if path.is_empty() { k.clone() } else { format!("{path}.{k}") }, depth + 1, facts);
        }
    } else if let Some(v) = scalar(x) {
        facts.push(Fact { key: path.into(), value: v });
    }
}

fn pick(row: &Row, keys: &[&str]) -> Option<String> {
    keys.iter().filter_map(|k| match row.get(*k)? {
        Value::String(s) => Some(Answer::plain(s)),
        v => strings(Some(v))?.first().map(|s| Answer::plain(s)),
    }).find(|s| !s.is_empty())
}

fn link(row: &Row) -> Option<String> { LINK_KEYS.iter().filter_map(|k| row.get(*k)?.as_str()).find(|s| s.starts_with("http")).map(Into::into) }

/// A number, a short string or a yes/no; None for objects, lists and long text.
fn scalar(x: &Value) -> Option<String> {
    match x {
        Value::Bool(b) => Some(if *b { "yes" } else { "no" }.into()),
        Value::Number(n) => Some(number(n, 16)),
        Value::String(s) if !s.is_empty() && count(s) <= 120 => Some(Answer::plain(s)),
        _ => None,
    }
}

/// `URL(string:)?.host()`: the host as written (its case and percent escapes kept); None when the address has no
/// authority or can't be a URL.
// ponytail: RFC 3986's authority read by hand, not Foundation's parser (no IDNA for a non-ASCII host).
fn host(url: &str) -> Option<String> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control() || "\"<>\\^`{|}".contains(c)) { return None }
    let (scheme, rest) = url.split_once("://")?;
    let mut s = scheme.chars();
    if !s.next()?.is_ascii_alphabetic() || !s.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) { return None }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let at = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if let Some(v6) = at.strip_prefix('[') { return v6.split_once(']').map(|(h, _)| h.to_string()) }
    Some(at.split(':').next().unwrap_or("").to_string())
}

/// «Paid … ↗\n\n{ "success": true, … }» → the words, and the JSON after them (whole or cut short).
pub fn split_data(text: &str) -> (String, Option<String>) {
    let Some(m) = first(text, r#"\n\s*\n(?=\s*[\{\[]\s*\n?\s*")"#).and_then(|c| c.get(0)) else { return (text.into(), None) };
    let data = text[m.end()..].trim();
    if count(data) <= 40 || !(data.contains("\":") || data.contains("\" :")) { return (text.into(), None) }
    (text[..m.start()].trim().into(), Some(data.into()))
}

/// What the folded card lists: the results' titles with their links when the JSON has them, else its first
/// non-empty text values («original: site:x.com …»). At most four.
pub fn rows(json: &str) -> Vec<(String, Option<String>)> {
    fn values(found: Vec<Caps<'_>>) -> Vec<String> { found.iter().map(|m| Answer::plain(group(m, 1))).filter(|s| !s.is_empty()).collect() }
    let titles = values(all(json, r#""(?:title|name|headline)"\s*:\s*"((?:[^"\\]|\\.)*)""#));
    let urls: Vec<String> = values(all(json, r#""(?:url|link)"\s*:\s*"((?:[^"\\]|\\.)*)""#)).into_iter().filter(|u| u.starts_with("http")).collect();
    if !titles.is_empty() { return titles.into_iter().take(4).enumerate().map(|(i, t)| (t, urls.get(i).cloned())).collect() }
    all(json, r#""(\w+)"\s*:\s*"((?:[^"\\]|\\.)+)""#).iter().take(4).map(|m| (format!("{}: {}", group(m, 1), Answer::plain(group(m, 2))), None)).collect()
}

/// What `reply` asks: the user's question answered from what the service sent, 1–3 sentences in their language, the
/// best results as links. The model sees only the service's data, cut to 6,000 characters.
pub fn reply_request(ask: &str, service: &str, data: &[u8], language: &str, model: &str) -> Request {
    let prompt = format!(
        "The user asked their crypto app: «{}»\nTo answer it, the app paid a web service, {service}, which sent back:\n{}\nAnswer the user in {language} in 1–3 short sentences, from this data only: what they wanted to know, naming the one to three most relevant results as markdown links [title](url) when there are any. If the data doesn't answer it (no results, an error), say so plainly and what to ask instead. Plain text: no JSON, no headings, no preamble.",
        cut(ask, 600), digest(data),
    );
    Request::new(model, vec![Message::new(Role::User, prompt)])
}

pub async fn reply(ask: &str, service: &str, data: &[u8], language: &str, client: &Client, model: &str) -> llm::Result<String> {
    Ok(client.complete(&reply_request(ask, service, data, language, model)).await?.trim().to_string())
}

/// What the chat's model reads of an answer: an AI service's own words and its links; else the JSON, cut.
pub(crate) fn digest(data: &[u8]) -> String {
    let a = Answer::new(data);
    let Some(w) = a.writing else { return shown(data, 6000) };
    format!("Its answer:\n{w}{}", if a.links.is_empty() { String::new() } else { format!("\n\nLinks it cites:\n{}", a.links.join("\n")) })
}

/// All of an answer the chat's model may read, for the guard: reply's digest and, for an AI service (whose digest is
/// only its words), the canvas's first 2,000 characters of the JSON (canvasBlocks), where its other fields sit.
pub fn for_model(data: &[u8]) -> String {
    if Answer::new(data).writing.is_none() { digest(data) } else { format!("{}\n\n{}", digest(data), shown(data, 2000)) }
}

/// The service's name in the directory («Brave Search») for its address; None when it isn't listed.
pub async fn service_name(url: &str) -> Option<String> {
    let host = host(url)?.to_lowercase();
    let services = directory().await?;
    let listed = services.iter().find(|s| s.get("serviceUrl").and_then(Value::as_str).and_then(self::host).map(|h| h.to_lowercase()).as_ref() == Some(&host))?;
    listed.get("name")?.as_str().map(Into::into)
}

/// Whether the user's text names this host whole («api.example.com», «https://api.example.com/v1», «api.example.com.»),
/// not inside a longer one: a substring let «notevil.com» name «evil.com» and «x.com» name «x.co».
/// An empty host («https://:8443/x») is never named: a lone «-» in the text trims to nothing.
pub fn named(host: &str, text: &str) -> bool {
    let host = host.to_lowercase();
    !host.is_empty()
        && split_chars(&text.to_lowercase(), |g| !is_letter(g) && !is_number(g) && !matches!(g, "." | "-" | "_")).iter().any(|w| trim_set(w, ".-_") == host)
}

// MARK: Directory

/// One service in mpp.dev's directory, as its JSON has it.
pub type Service = Map<String, Value>;

const DIRECTORY_DOWN: &str = "The MPP directory (mpp.dev) didn't answer.";

/// The services that take MPP payments (mpp.dev's directory) matching the words, with their endpoints and prices,
/// for the model's «services» lookup.
pub async fn search(query: &str) -> String {
    match directory().await {
        Some(services) => listing(&services, query),
        None => DIRECTORY_DOWN.into(),
    }
}

/// `search`'s words for the directory it read.
pub(crate) fn listing(services: &[Service], query: &str) -> String {
    let hits = rank(services, query);
    if hits.is_empty() { return format!("No MPP service matches «{query}». Try broader words (email, search, image, flights, phone, mail).") }
    hits.iter().map(|s| describe(s, 6)).collect::<Vec<_>>().join("\n\n")
        + "\n\nTo pay one: the form «pay <METHOD> <service URL + path> <JSON body>». For the exact body fields look up «service <id> <path>»."
}

/// The six services that fit the words best.
pub(crate) fn rank<'a>(services: &'a [Service], query: &str) -> Vec<&'a Service> {
    const SKIP: [&str; 25] = ["api", "apis", "service", "services", "pay", "paid", "request", "the", "for", "via", "tempo", "mpp", "on", "about",
                              "of", "in", "to", "and", "or", "what", "how", "is", "are", "with", "from"];
    let said = tokens(query);
    let mut words: Vec<String> = said.iter().filter(|w| count(w) > 1 && !SKIP.contains(&w.as_str())).cloned().collect();
    // A coin's name is the word «crypto» to the directory: «BTC price» is CoinGecko's.
    // ponytail: a short list; the model usually writes «crypto» itself.
    let coins = ["bitcoin", "btc", "ethereum", "eth", "solana", "sol", "coin", "coins", "token", "tokens"];
    if words.iter().any(|w| coins.contains(&w.as_str())) { words.push("crypto".into()) }
    // Twitter is «X» there («Chat with X Search»); a one-letter word matches only itself, not «x402» or «xai».
    let twitter = ["twitter", "tweet", "tweets", "твиттер", "твит", "твиты"];
    if words.iter().any(|w| twitter.contains(&w.as_str())) || said.iter().any(|t| t == "x") { words.push("x".into()) }
    // A word in the name counts most, then in the tags, then anywhere in what it does; a word matches the start of
    // one there («price» — «prices»). Plain substrings ranked a fax service «priced per page» over CoinGecko.
    let score = |s: &Service| -> usize {
        let name = tokens(&format!("{} {}", any_text(s.get("id")), any_text(s.get("name"))));
        let mut tags = strings(s.get("tags")).unwrap_or_default();
        tags.extend(strings(s.get("categories")).unwrap_or_default());
        let tags = tokens(&tags.join(" "));
        let endpoints = objects(s.get("endpoints")).map(|e| e.iter().filter_map(|e| e.get("description")?.as_str()).collect::<Vec<_>>().join(" "));
        let rest = tokens(&format!("{} {}", any_text(s.get("description")), endpoints.unwrap_or_default()));
        words.iter().map(|w| {
            let hit = |t: &String| if count(w) == 1 { t == w } else { t.starts_with(w.as_str()) };
            if name.iter().any(hit) { 5 } else if tags.iter().any(hit) { 2 } else if rest.iter().any(hit) { 1 } else { 0 }
        }).sum()
    };
    let mut scored: Vec<(&Service, usize)> = services.iter().filter(|s| !unpayable(s)).map(|s| (s, score(s))).filter(|(_, n)| words.is_empty() || *n > 0).collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().take(6).map(|(s, _)| s).collect()
}

/// `"\(s["id"] ?? "")"`: a value as Swift's string interpolation prints it.
fn any_text(v: Option<&Value>) -> String {
    match v {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => number(n, 16),
        Some(Value::Bool(b)) => if *b { "1" } else { "0" }.into(),
        Some(Value::Null) => "<null>".into(),
        Some(other) => other.to_string(),
    }
}

/// Services whose 402 ihatecrypto can't pay, measured 2026-09-30: Tempo's own proxies and Orthogonal's take only a
/// transaction they send and pay the fee for (fal, OpenAI, Anthropic, Fiber, ScrapeCreators), Exa only a transaction
/// it sends (pull), Nansen only x402 on Base — a signed spending authorization the signer refuses as a drainer's tool.
/// Left out of the search so the model never offers what can't be paid.
// ponytail: by host, as measured; a service's 402 is still checked before any card (`challenge`).
pub(crate) fn unpayable(s: &Service) -> bool {
    let host = s.get("serviceUrl").and_then(Value::as_str).and_then(host).map(|h| h.to_lowercase()).unwrap_or_default();
    host.ends_with(".mpp.tempo.xyz") || ["mpp.orthogonal.com", "api.exa.ai", "api.nansen.ai"].contains(&host.as_str()) || REFUSES_PROOF.contains(&host.as_str())
}

/// Services that take the payment and then refuse its proof, measured live 2026-10-23 on the Dev test wallet: AIsa
/// answers «credential request does not match this route's requirements» to every proof (GET and POST), Alchemy's
/// own receipt check fails (403 from its node), StableEnrich answers 402 again to a push proof. Their 402 looks
/// payable, so they're never paid, even when the chat names the address.
pub const REFUSES_PROOF: [&str; 3] = ["api.aisa.one", "mpp.alchemy.com", "stableenrich.dev"];

pub(crate) fn tokens(text: &str) -> Vec<String> {
    split_chars(&text.to_lowercase(), |g| !is_letter(g) && !is_number(g)).into_iter().map(Into::into).collect()
}

/// One service in full: every endpoint and price, and the start of its docs for the request fields.
/// `arg`: «grok» or «grok /grok/x-search»: with an endpoint the docs start at that endpoint's section — a long doc
/// cut at its start never reached Grok's X search, and the model fell back to a web search.
pub async fn service(arg: &str) -> String {
    let Some(services) = directory().await else { return DIRECTORY_DOWN.into() };
    let words: Vec<&str> = arg.split(' ').filter(|w| !w.is_empty()).collect();
    let id = words.first().map(|w| w.to_lowercase());
    let Some(s) = id.and_then(|id| services.iter().find(|s| s.get("id").and_then(Value::as_str).is_some_and(|i| i.to_lowercase() == id))) else {
        return format!("No MPP service «{arg}». Search with «services <words>» first.");
    };
    let mut text = describe(s, 25);
    if let Some(docs) = s.get("docs").and_then(Value::as_object).and_then(|d| d.get("llmsTxt")?.as_str())
        && let Ok(data) = llm::http::get(docs, &[], 10).await {
        text += &format!("\nDocs ({docs}):\n{}", section(&String::from_utf8_lossy(&data), &words[1..]));
    }
    text
}

/// 2,500 characters of the docs from the heading of the section that names the endpoint (its path's last part,
/// «x-search»); the start when none is named or found.
pub(crate) fn section(docs: &str, hint: &[&str]) -> String {
    let keys: Vec<&str> = hint.iter().filter_map(|h| h.split('/').filter(|p| !p.is_empty()).next_back()).filter(|k| count(k) > 2).collect();
    let Some(at) = keys.iter().find_map(|k| find_caseless(docs, k)) else { return cut(docs, 2500).into() };
    let head = docs[..at].rfind("\n#").unwrap_or(at);
    cut(&docs[head..], 2500).into()
}

/// `range(of:options: .caseInsensitive)`'s start.
// ponytail: letter by letter in lower case; a letter whose case changes its length («ß» — «SS») isn't matched.
fn find_caseless(text: &str, word: &str) -> Option<usize> {
    let word: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
    text.char_indices().map(|(i, _)| i).find(|&i| {
        let mut t = text[i..].chars().flat_map(char::to_lowercase);
        word.iter().all(|c| t.next() == Some(*c))
    })
}

pub(crate) fn describe(s: &Service, limit: usize) -> String {
    let text = |m: &Map<String, Value>, key: &str, or: &'static str| m.get(key).and_then(Value::as_str).unwrap_or(or).to_string();
    let base = text(s, "serviceUrl", "");
    let endpoints: Vec<String> = objects(s.get("endpoints")).unwrap_or_default().into_iter().take(limit).map(|e| {
        let pay = e.get("payment").and_then(Value::as_object);
        let said = |key: &str| pay.and_then(|p| p.get(key)?.as_str());
        let amount = said("amount").and_then(decimal).filter(|a| *a > Decimal::ZERO).and_then(|a| a.checked_div(Decimal::from(1_000_000)));
        let price = if let Some(a) = amount {
            format!("${}", Fmt::num(a, 6))
        } else if let Some(hint) = said("amountHint") {
            format!("priced by use ({hint}); the 402 names the most it charges")
        } else if said("intent") == Some("session") {
            "per use (session: ihatecrypto can't pay these yet)".into()
        } else if pay.is_none() { "free".into() } else { "price in its 402 answer".into() };
        format!("  {} {base}{} — {}, {price}", text(e, "method", "GET"), text(e, "path", ""), text(e, "description", ""))
    }).collect();
    format!("{} (id {}): {}\n{}", text(s, "name", "?"), text(s, "id", "?"), text(s, "description", ""), endpoints.join("\n"))
}

// ponytail: the directory is read on every lookup (≈ 400 KB); cache it if lookups get frequent.
pub(crate) async fn directory() -> Option<Vec<Service>> { services(&llm::http::get("https://mpp.dev/api/services", &[], 15).await.ok()?) }

/// The directory's answer read: its `services`, every one an object, or None.
pub(crate) fn services(data: &[u8]) -> Option<Vec<Service>> {
    let j: Value = serde_json::from_slice(data).ok()?;
    objects(j.as_object()?.get("services")).map(|s| s.into_iter().cloned().collect())
}
