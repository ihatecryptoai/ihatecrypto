//! NEAR AI Cloud's evidence that its gateway and the model run inside Intel TDX enclaves (the model on attested NVIDIA
//! GPUs), checked here before a chat goes out (`NearAI/NearAttestation.swift`, docs.near.ai/cloud/verification,
//! 2026-11-01). For every report:
//! - the quote is signed by its attestation key; Intel's Quoting Enclave vouches for that key in a report signed by the
//!   platform's PCK certificate, and that certificate chains to Intel's SGX root, pinned here by its SHA-256;
//! - the quote's report data is the enclave's signing key followed by our fresh nonce: that key lives in that enclave,
//!   and the evidence was made for this check;
//! - not a debug enclave; its configuration (MRCONFIGID) is the hash of the app_compose it shows, and RTMR3 is the
//!   replay of its event log;
//! - the model's GPUs: NVIDIA's attestation service passes their evidence, bound to the same nonce.
//!
//! ponytail: Intel's TCB level (collateral from Intel PCS) and the image's provenance aren't checked, so a genuine but
//! out-of-date platform still passes; add the dcap collateral when that matters.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256, Sha384};

use crate::{Result, fail, http, now, oauth, usage};

/// Intel SGX Provisioning Certification Root CA, as certificates.trustedservices.intel.com serves it.
pub const INTEL_ROOT_SHA256: &str = "44a0196b2b99f889b8e149e95b807a350e7424964399e885a7cbb8ccfab674d3";

#[derive(Debug, Clone, PartialEq)]
pub struct Verified {
    pub model: String,
    /// The ed25519 keys that sign inside the enclaves, hex.
    pub gateway_key: String,
    pub model_keys: Vec<String>,
    /// Unix seconds.
    pub at: f64,
}

/// `report`: GET /v1/attestation/report with `model`, provider=near, signing_algo=ed25519 and `nonce` (32 random
/// bytes, hex). `gpu`: whether NVIDIA passes one GPU evidence payload (`nvidia_verdict`).
pub async fn verify(report: &[u8], nonce: &str, model: &str, gpu: impl AsyncFn(String) -> Result<bool>) -> Result<Verified> {
    let root: Option<Value> = serde_json::from_slice(report).ok();
    let Some(gateway) = root.as_ref().and_then(|r| r.get("gateway_attestation")?.as_object()) else {
        return fail("NEAR AI sent no gateway attestation.");
    };
    let ours = bytes(nonce)?;
    if ours.len() != 32 { return fail("The nonce must be 32 bytes.") }
    let gateway_key = check(gateway, &ours, "NEAR AI's gateway")?;
    let reports: Vec<&Map<String, Value>> = objects(root.as_ref().and_then(|r| r.get("model_attestations")))
        .into_iter().filter(|r| r.get("model_name").and_then(Value::as_str) == Some(model)).collect();
    if reports.is_empty() { return fail(format!("NEAR AI sent no enclave evidence for {model}.")) }
    let mut keys = Vec::new();
    for r in reports {
        keys.push(check(r, &ours, model)?);
        let payload = r.get("nvidia_payload").and_then(Value::as_str).filter(|p| {
            serde_json::from_str::<Value>(p).ok().and_then(|p| Some(p.get("nonce")?.as_str()?.to_lowercase())) == Some(nonce.to_lowercase())
        });
        let Some(payload) = payload else { return fail(format!("{model}'s GPU evidence isn't bound to this check.")) };
        if !gpu(payload.to_string()).await? { return fail(format!("NVIDIA didn't pass {model}'s GPUs.")) }
    }
    Ok(Verified { model: model.into(), gateway_key, model_keys: keys, at: now() })
}

/// One report: its quote checked down to Intel's root and bound to its key and `nonce`. Returns the key.
fn check(r: &Map<String, Value>, nonce: &[u8], what: &str) -> Result<String> {
    let text = |k: &str| r.get(k).and_then(Value::as_str);
    let (Some("ed25519"), Some(address), Some(quote)) = (text("signing_algo"), text("signing_address"), text("intel_quote")) else {
        return fail(format!("{what}: the report is incomplete."));
    };
    let key = bytes(address.strip_prefix("0x").unwrap_or(address))?;
    if key.len() != 32 { return fail(format!("{what}: the signing key isn't an ed25519 key.")) }
    if text("request_nonce").map(str::to_lowercase) != Some(hex(nonce)) { return fail(format!("{what}: the report answers another check.")) }
    let tcb = json(r.get("info").and_then(|i| i.get("tcb_info")));
    let Some(compose) = tcb.as_ref().and_then(|t| t.get("app_compose")?.as_str()) else {
        return fail(format!("{what}: the report shows no configuration."));
    };
    let log = json(r.get("event_log"));
    verify_quote(&bytes(quote)?, &[key.as_slice(), nonce].concat(), compose, &objects(log.as_ref()), what)?;
    Ok(hex(&key))
}

/// An Intel TDX v4 quote: its report data, configuration and RTMR3 as expected, and its signatures down to Intel.
fn verify_quote(q: &[u8], report_data: &[u8], app_compose: &str, events: &[&Map<String, Value>], what: &str) -> Result<()> {
    const BODY: usize = 48;
    const SIGNED: usize = 48 + 584;
    let failure = |m: &str| crate::Error::Llm(format!("{what}: {m}"));
    let data = |from: usize, n: usize| q.get(from..from.checked_add(n).ok_or_else(|| failure("the quote is cut short."))?)
        .ok_or_else(|| failure("the quote is cut short."));
    let u16_at = |o: usize| data(o, 2).map(|d| u16::from_le_bytes([d[0], d[1]]) as usize);
    let u32_at = |o: usize| data(o, 4).map(|d| u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize);
    if u16_at(0)? != 4 || u32_at(4)? != 0x81 { return Err(failure("not a TDX quote.")) }
    if data(BODY + 520, 64)? != report_data { return Err(failure("the quote doesn't bind its key and our nonce.")) }
    if data(BODY + 120, 1)?[0] & 1 != 0 { return Err(failure("a debug enclave, which its host can read.")) }
    let config = [&[1u8][..], &Sha256::digest(app_compose.as_bytes())[..], &[0u8; 15]].concat();
    if data(BODY + 184, 48)? != config { return Err(failure("the enclave runs another configuration than the one shown.")) }
    let mut rtmr3 = vec![0u8; 48];
    for e in events.iter().filter(|e| usage::int(e.get("imr")) == Some(3)) {
        let Some(kind) = usage::int(e.get("event_type")) else { return Err(failure("an unreadable event.")) };
        let event = e.get("event").and_then(Value::as_str).unwrap_or("");
        let payload = bytes(e.get("event_payload").and_then(Value::as_str).unwrap_or(""))?;
        let digest = Sha384::digest([&(kind as u32).to_le_bytes()[..], b":", event.as_bytes(), b":", &payload].concat());
        rtmr3 = Sha384::digest([&rtmr3[..], &digest[..]].concat()).to_vec();
    }
    if data(BODY + 472, 48)? != rtmr3 { return Err(failure("the event log doesn't add up to RTMR3.")) }

    // The signature data: the quote's ECDSA signature and attestation key, then Intel's QE report certifying the key.
    let mut o = SIGNED + 4;
    let (signature, attestation_key) = (data(o, 64)?, data(o + 64, 64)?);
    o += 128;
    if u16_at(o)? != 6 { return Err(failure("the quote carries no QE report.")) }
    o += 6;
    let (qe, qe_signature) = (data(o, 384)?, data(o + 384, 64)?);
    o += 448;
    let auth_length = u16_at(o)?;
    let auth = data(o + 2, auth_length)?;
    o += 2 + auth_length;
    if u16_at(o)? != 5 { return Err(failure("the quote carries no PCK certificates.")) }
    let pem = String::from_utf8_lossy(data(o + 6, u32_at(o + 2)?)?).into_owned();
    let (key, signed) = (VerifyingKey::from_sec1_bytes(&[&[4u8][..], attestation_key].concat()), data(0, SIGNED)?);
    if !key.is_ok_and(|k| p256_valid(&k, signature, signed)) {
        return Err(failure("the quote's signature doesn't verify."));
    }
    if qe[320..352] != Sha256::digest([attestation_key, auth].concat())[..] {
        return Err(failure("Intel's Quoting Enclave didn't vouch for this quote's key."));
    }
    if !p256_valid(&pck_key(&pem, what)?, qe_signature, qe) {
        return Err(failure("the QE report isn't signed by the platform's Intel certificate."));
    }
    Ok(())
}

/// A raw (r ‖ s) P-256 signature over SHA-256 of `message`.
fn p256_valid(key: &VerifyingKey, signature: &[u8], message: &[u8]) -> bool {
    Signature::from_slice(signature).is_ok_and(|s| key.verify(message, &s).is_ok())
}

/// The platform's PCK key, once its certificates chain to Intel's pinned root (dates checked).
fn pck_key(pem: &str, what: &str) -> Result<VerifyingKey> {
    let ders: Vec<Vec<u8>> = pem.split("-----END CERTIFICATE-----").filter_map(|part| {
        let (_, body) = part.split_once("-----BEGIN CERTIFICATE-----")?;
        STANDARD.decode(body.chars().filter(|c| !c.is_whitespace()).collect::<String>()).ok()
    }).collect();
    if ders.len() < 2 || ders.last().map(|root| hex(&Sha256::digest(root))).as_deref() != Some(INTEL_ROOT_SHA256) {
        return fail(format!("{what}: the certificates don't end at Intel's root."));
    }
    chain(&ders).ok_or_else(|| crate::Error::Llm(format!("{what}: the Intel certificate chain doesn't verify.")))
}

/// What SecTrust's basic X.509 policy checks with the last certificate as the only anchor: each one signed by the next
/// (names matching), every one within its dates, every issuer a CA within its path length. Returns the first's key.
/// ponytail: no key usage, policies or revocation; the root is pinned by its hash, so its own signature isn't checked.
fn chain(ders: &[Vec<u8>]) -> Option<VerifyingKey> {
    let certs = ders.iter().map(|d| Cert::parse(d)).collect::<Option<Vec<_>>>()?;
    let today = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    for (i, c) in certs.iter().enumerate() {
        if !(c.not_before <= today && today <= c.not_after) { return None }
        let Some(issuer) = certs.get(i + 1) else { break };
        if c.issuer != issuer.subject || !issuer.ca || issuer.path_len.is_some_and(|n| i > n) { return None }
        let key = VerifyingKey::from_sec1_bytes(issuer.key).ok()?;
        key.verify(c.tbs, &Signature::from_der(c.signature).ok()?).ok()?;
    }
    VerifyingKey::from_sec1_bytes(certs[0].key).ok()
}

/// One X.509 certificate signed with ECDSA and SHA-256, read as far as `chain` needs it.
struct Cert<'a> {
    tbs: &'a [u8],
    signature: &'a [u8],
    issuer: &'a [u8],
    subject: &'a [u8],
    /// YYYYMMDDHHMMSS, UTC.
    not_before: String,
    not_after: String,
    key: &'a [u8],
    ca: bool,
    path_len: Option<usize>,
}

/// ecdsa-with-SHA256, 1.2.840.10045.4.3.2.
const ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
/// basicConstraints, 2.5.29.19.
const BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x13];

impl<'a> Cert<'a> {
    fn parse(der: &'a [u8]) -> Option<Self> {
        let [(0x30, cert, _)] = Der(der).all::<1>()? else { return None };
        let [(0x30, tbs_body, tbs), (0x30, algorithm, _), (0x03, signature, _)] = Der(cert).all::<3>()? else { return None };
        if Der(algorithm).next()?.1 != ECDSA_SHA256 { return None }
        let mut f = Der(tbs_body);
        if f.0.first() == Some(&0xa0) { f.next()?; } // the version
        f.next()?; // the serial number
        let (Some(_algorithm), Some((0x30, _, issuer)), Some((0x30, validity, _)), Some((0x30, _, subject)), Some((0x30, spki, _))) =
            (f.next(), f.next(), f.next(), f.next(), f.next()) else { return None };
        let [(t1, from, _), (t2, until, _)] = Der(validity).all::<2>()?;
        let [_, (0x03, key, _)] = Der(spki).all::<2>()? else { return None };
        let (mut ca, mut path_len) = (false, None);
        for (tag, contents, _) in f {
            if tag != 0xa3 { continue }
            let (0x30, extensions, _) = Der(contents).next()? else { return None };
            let mut list = Der(extensions);
            while let Some((0x30, extension, _)) = list.next() {
                let mut e = Der(extension);
                if e.next()?.1 != BASIC_CONSTRAINTS { continue }
                let (mut tag, mut value, _) = e.next()?;
                if tag == 0x01 { (tag, value, _) = e.next()? } // critical
                if tag != 0x04 { return None }
                let (0x30, constraints, _) = Der(value).next()? else { return None };
                for (tag, v, _) in Der(constraints) {
                    match tag { 0x01 => ca = v.first().is_some_and(|b| *b != 0), 0x02 if v.len() <= 2 => path_len = Some(v.iter().fold(0, |a, b| a << 8 | *b as usize)), _ => return None }
                }
            }
        }
        Some(Self { tbs, signature: bits(signature)?, issuer, subject, not_before: time(t1, from)?, not_after: time(t2, until)?, key: bits(key)?, ca, path_len })
    }
}

/// A BIT STRING's bytes, when they're whole.
fn bits(v: &[u8]) -> Option<&[u8]> { v.strip_prefix(&[0]) }

/// A UTCTime (0x17, years before 2050) or GeneralizedTime (0x18) as YYYYMMDDHHMMSS.
fn time(tag: u8, v: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(v).ok()?.strip_suffix('Z')?;
    if !s.bytes().all(|b| b.is_ascii_digit()) { return None }
    match (tag, s.len()) {
        (0x17, 12) => Some(format!("{}{s}", if s < "50" { "20" } else { "19" })),
        (0x18, 14) => Some(s.into()),
        _ => None,
    }
}

/// DER elements one after another.
struct Der<'a>(&'a [u8]);

/// One DER element: its tag, its contents and the whole element.
type Element<'a> = (u8, &'a [u8], &'a [u8]);

impl<'a> Der<'a> {
    /// Exactly `N` elements and nothing after them.
    fn all<const N: usize>(mut self) -> Option<[Element<'a>; N]> {
        let out: [_; N] = std::array::from_fn(|_| self.next());
        if !self.0.is_empty() { return None }
        out.iter().all(Option::is_some).then(|| out.map(|e| e.expect("checked")))
    }
}

impl<'a> Iterator for Der<'a> {
    type Item = Element<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let d = self.0;
        let (&tag, rest) = d.split_first()?;
        let (&first, rest) = rest.split_first()?;
        let (length, rest) = if first < 0x80 { (first as usize, rest) } else {
            let n = (first & 0x7f) as usize;
            if n == 0 || n > 4 || rest.len() < n { return None }
            (rest[..n].iter().fold(0usize, |a, b| a << 8 | *b as usize), &rest[n..])
        };
        if rest.len() < length { return None }
        let whole = d.len() - rest.len() + length;
        self.0 = &d[whole..];
        Some((tag, &rest[..length], &d[..whole]))
    }
}

/// NVIDIA's remote attestation service on one payload: its token's overall result.
/// ponytail: the token's signature isn't checked against NVIDIA's keys; it's trusted for coming over TLS from
/// nras.attestation.nvidia.com. Check it with NVIDIA's JWKS if the service is ever reached through anyone else.
pub async fn nvidia_verdict(payload: String) -> Result<bool> {
    let r = net::HTTP.post("https://nras.attestation.nvidia.com/v3/attest/gpu").header("Content-Type", "application/json").body(payload);
    let (_, data) = http::read(http::send(r, 60).await?, 60).await?;
    let list: Option<Value> = serde_json::from_slice(&data).ok();
    let first = list.as_ref().and_then(|l| l.as_array()?.first()?.as_array());
    let Some(token) = first.filter(|f| f.first().and_then(Value::as_str) == Some("JWT")).and_then(|f| f.last()?.as_str()) else {
        return Ok(false);
    };
    Ok(oauth::claims(token).and_then(|c| c.get("x-nvidia-overall-att-result")?.as_bool()) == Some(true))
}

/// A field that is JSON or JSON written as a string.
fn json(v: Option<&Value>) -> Option<Value> {
    match v? { Value::String(s) => serde_json::from_str(s).ok(), other => Some(other.clone()) }
}

/// An array of objects, as Swift's `as? [[String: Any]] ?? []` reads it: empty when it's anything else.
fn objects(v: Option<&Value>) -> Vec<&Map<String, Value>> {
    v.and_then(Value::as_array).and_then(|a| a.iter().map(Value::as_object).collect()).unwrap_or_default()
}

pub(crate) fn bytes(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) { return fail("Odd hex.") }
    hex.as_bytes().chunks(2).map(|pair| {
        std::str::from_utf8(pair).ok().and_then(|p| u8::from_str_radix(p, 16).ok()).ok_or_else(|| crate::Error::Llm("Not hex.".into()))
    }).collect()
}

pub(crate) fn hex(d: &[u8]) -> String { d.iter().map(|b| format!("{b:02x}")).collect() }
