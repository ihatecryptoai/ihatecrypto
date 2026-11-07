//! NEAR AI Cloud: open models that run inside Intel TDX enclaves on attested NVIDIA GPUs, behind an OpenAI-compatible
//! API (cloud-api.near.ai/v1) (`NearAI/NearAI.swift`). Before a chat goes out the enclaves' evidence is checked with a
//! fresh nonce (`near_attestation`), and again every ten minutes; nothing is sent while it fails. Only the models NEAR
//! itself runs attest (provider=near, 2026-11-01): Claude, Gemini, GLM and the others there are proxied, not private.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::catalog::{Info, Kind};
use crate::near_attestation::{self, Verified, bytes, hex};
use crate::usage::Usage;
use crate::{Credential, Error, Report, Request, Result, fail, http, now};

pub const BASE: &str = "https://cloud-api.near.ai/v1";

pub fn info() -> Info {
    Info::new("nearai", "NEAR AI Cloud (private)", Kind::ApiKey, "deepseek-ai/DeepSeek-V4.1-Flash-1017",
              "Key from cloud.near.ai. The model runs in a hardware enclave. ihatecrypto checks the enclave before every chat, so the provider can't read your balances.")
        .key().models(&["deepseek-ai/DeepSeek-V4.1-Flash-1017", "Qwen/Qwen3.8-27B", "Qwen/Qwen3.6-35B-A3B-FP8"])
}

pub(crate) fn client(credential: Option<&Credential>) -> Result<NearAiClient> {
    let Some(Credential::ApiKey(key)) = credential else { return fail(format!("{}: add a key in Settings.", info().name)) };
    Ok(NearAiClient { key: key.clone(), base: BASE.into() })
}

pub(crate) static CHECKS: LazyLock<Mutex<HashMap<String, Verified>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static REPLIES: LazyLock<Mutex<HashMap<String, ReplyProof>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The check that passed for `model` within the last ten minutes: what the chat shows as «TEE». Older, or after a check
/// that failed, None.
pub fn verified(model: &str) -> Option<Verified> { last_verified(model).filter(|v| now() - v.at < 600.0) }

/// The last check that passed for `model`, however old: the chat says it's private until a check fails (which clears
/// it) or another model is picked. Each message still waits for a check under ten minutes old.
pub fn last_verified(model: &str) -> Option<Verified> { CHECKS.lock().expect("checks").get(model).cloned() }

/// The enclaves behind `model` checked now, or within the last ten minutes.
pub async fn ensure_verified(model: &str) -> Result<Verified> {
    if let Some(v) = verified(model) { return Ok(v) }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("the system's randomness");
    let nonce = hex(&seed);
    let url = http::with_query(&format!("{BASE}/attestation/report"),
                               &[("model", model), ("provider", "near"), ("signing_algo", "ed25519"), ("nonce", &nonce)]);
    // Whatever goes wrong from here, the old check no longer stands for this model.
    CHECKS.lock().expect("checks").remove(model);
    let (status, data) = http::read(http::send(net::HTTP.get(url).header("x-no-aliasing", "true"), 60).await?, 60).await?;
    if status != 200 {
        return fail(format!("NEAR AI gave no enclave evidence for {model}, so nothing was sent. Pick another of its private models."));
    }
    match near_attestation::verify(&data, &nonce, model, near_attestation::nvidia_verdict).await {
        Ok(v) => {
            CHECKS.lock().expect("checks").insert(model.into(), v.clone());
            Ok(v)
        }
        Err(e) => fail(format!("NEAR AI's enclave didn't check out, so nothing was sent: {e}")),
    }
}

/// What the last reply of a model proved: what the chat's TEE chip shows beside the enclave check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyProof {
    /// Its exact bytes signed by the model's own enclave key, the one the attestation verified.
    Model,
    /// Signed by the attested gateway only: it passed through NEAR AI's enclave, not proven from the model's.
    Gateway,
    Unverified(String),
}

pub fn last_reply(model: &str) -> Option<ReplyProof> { REPLIES.lock().expect("replies").get(model).cloned() }

/// The chat, each request only after its model's enclaves check out, and each reply checked against them after: the
/// exact request and response bytes, signed by the model's enclave key (GET /v1/signature). Answers come whole, not
/// streamed: a streamed one is re-signed by the gateway, which proves less.
#[derive(Debug, Clone)]
pub struct NearAiClient {
    pub(crate) key: String,
    pub(crate) base: String,
}

pub(crate) fn body(r: &Request) -> Value {
    let mut body = json!({"model": r.model, "messages": r.messages_json()});
    if r.wants_json { body["response_format"] = json!({"type": "json_object"}) }
    body
}

impl NearAiClient {
    pub(crate) async fn complete(&self, r: &Request, report: Report<'_>) -> Result<String> {
        let verified = ensure_verified(&r.model).await?;
        let request = serde_json::to_vec(&body(r)).expect("JSON");
        let post = net::HTTP.post(format!("{}/chat/completions", self.base)).body(request.clone());
        let post = [("Authorization", format!("Bearer {}", self.key)), ("Content-Type", "application/json".into()),
                    ("x-no-aliasing", "true".into()), ("Accept-Encoding", "identity".into())]
            .into_iter().fold(post, |p, (k, v)| p.header(k, v));
        let (status, data) = http::read(http::send(post, if r.thinking { 180 } else { 90 }).await?, if r.thinking { 180 } else { 90 }).await?;
        let answer: Option<Value> = serde_json::from_slice(&data).ok();
        let Some(id) = answer.as_ref().filter(|_| status == 200).and_then(|o| o.get("id")?.as_str()) else {
            return Err(Error::Llm(format!("NEAR AI: {}", String::from_utf8_lossy(&data[..data.len().min(300)]))));
        };
        if let Some(u) = Usage::reported(answer.as_ref().and_then(|o| o.get("usage"))) { report(u) }
        let text = answer.as_ref().and_then(|o| o.get("choices")?.get(0)?.get("message")?.get("content")?.as_str()).unwrap_or("").to_string();
        let proof = self.proof(id, &r.model, &request, &data, &verified).await;
        REPLIES.lock().expect("replies").insert(r.model.clone(), proof);
        Ok(text)
    }

    /// The reply's signature, fetched (it can lag a moment) and checked: the text must be the hashes of these exact
    /// bytes, the signer the attested model key (or the gateway's), the ed25519 signature valid.
    async fn proof(&self, id: &str, model: &str, request: &[u8], response: &[u8], v: &Verified) -> ReplyProof {
        let url = http::with_query(&format!("{}/signature/{}", self.base, http::percent(id, b"-._~")), &[("model", model), ("signing_algo", "ed25519")]);
        for attempt in 0..4 {
            if attempt > 0 { tokio::time::sleep(std::time::Duration::from_secs(1)).await }
            let get = net::HTTP.get(&url).header("Authorization", format!("Bearer {}", self.key));
            let Ok(answer) = http::send(get, 20).await else { continue };
            let Ok((200, data)) = http::read(answer, 20).await else { continue };
            let Some(o) = serde_json::from_slice::<Value>(&data).ok().filter(|o| o.is_object() && o.get("error_code").is_none()) else { continue };
            return check(&o, model, request, response, v);
        }
        ReplyProof::Unverified("no signature for this reply".into())
    }
}

pub(crate) fn check(o: &Value, model: &str, request: &[u8], response: &[u8], v: &Verified) -> ReplyProof {
    let hashes = format!("{}:{}", hex(&Sha256::digest(request)), hex(&Sha256::digest(response)));
    let unhex = |s: &str| bytes(&s.replace("0x", "")).ok();
    let signed = || {
        if o.get("signing_algo")?.as_str()? != "ed25519" { return None }
        let text = o.get("text")?.as_str()?;
        let signer = o.get("signing_address")?.as_str()?.to_lowercase();
        let signature: [u8; 64] = unhex(o.get("signature")?.as_str()?)?.try_into().ok()?;
        let key: [u8; 32] = unhex(&signer)?.try_into().ok()?;
        VerifyingKey::from_bytes(&key).ok()?.verify(text.as_bytes(), &Signature::from_bytes(&signature)).ok()?;
        Some((text, signer))
    };
    let Some((text, signer)) = signed() else {
        return ReplyProof::Unverified("the reply's signature doesn't check out".into());
    };
    let unverified = |why: &str| ReplyProof::Unverified(why.into());
    match o.get("signature_kind").and_then(Value::as_str) {
        Some("provider_tee") if text != format!("{model}:{hashes}") => unverified("the signature is for other bytes"),
        Some("provider_tee") if v.model_keys.iter().any(|k| k.to_lowercase() == signer) => ReplyProof::Model,
        Some("gateway") if text != hashes => unverified("the signature is for other bytes"),
        Some("gateway") if signer == v.gateway_key.to_lowercase() => ReplyProof::Gateway,
        Some("provider_tee" | "gateway") => unverified("signed by a key the attestation didn't verify"),
        _ => unverified("an unknown kind of signature"),
    }
}
