//! The offline cases of LLMTests.swift with their vectors, and the clients end to end against a stand-in server at
//! 127.0.0.1: no real model, login, Keychain or Ollama is ever touched.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::anthropic::{self, AnthropicClient, AnthropicStream};
use crate::api_key::{self, OpenAiChatClient};
use crate::catalog::{self, Kind, Provider};
use crate::chatgpt::{self, ChatGptClient, CodexStream, Source};
use crate::claude_code::{self, ClaudeCodeClient};
use crate::oauth::{self, LoopbackServer, Pkce};
use crate::usage::{ModelFacts, Usage};
use crate::*;

fn msg(role: Role, text: &str) -> Message { Message::new(role, text) }
fn req(model: &str, messages: Vec<Message>) -> Request { Request::new(model, messages) }
fn save_nothing() -> Save { Arc::new(|_| {}) }

/// What the stand-in server saw of one request.
#[derive(Debug, Clone)]
struct Asked {
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

impl Asked {
    fn json(&self) -> Value { serde_json::from_str(&self.body).unwrap_or(Value::Null) }
}

/// A server at 127.0.0.1 that answers each request with the next (status, body) and keeps what it was asked.
async fn server(answers: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<Asked>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    tokio::spawn(async move {
        for (status, body) in answers {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = socket.read(&mut chunk).await.unwrap_or(0);
                buf.extend(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let head = &text[..end];
                    let headers: HashMap<String, String> = head.lines().skip(1)
                        .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))).collect();
                    let length = headers.get("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                    if buf.len() >= end + 4 + length {
                        let path = head.split(' ').nth(1).unwrap_or("").to_string();
                        kept.lock().unwrap().push(Asked { path, headers, body: String::from_utf8_lossy(&buf[end + 4..end + 4 + length]).into_owned() });
                        break;
                    }
                }
                if n == 0 { break }
            }
            let reply = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(reply.as_bytes()).await.ok();
            socket.shutdown().await.ok();
        }
    });
    (url, seen)
}

fn sse(events: &[&str]) -> String { events.join("\n\n") + "\n\n" }

// MARK: Core

#[test]
fn pkce_matches_rfc7636_vector() {
    assert_eq!(Pkce::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    assert_eq!(Pkce::make().verifier.len(), 43);
}

#[test]
fn json_text_finds_object_in_fences_and_prose() {
    for reply in ["{\"a\":1}", "```json\n{\"a\": 1}\n```", "Вот ответ: {\"a\": 1}. Готово"] {
        let object: Value = serde_json::from_str(json_object(reply).unwrap()).unwrap();
        assert_eq!(object["a"], 1);
    }
    assert_eq!(json_object("нет json"), None);
}

#[tokio::test]
async fn loopback_server_returns_callback_query() {
    let server = LoopbackServer::start(0, "/callback").await.unwrap();
    assert!(server.port > 0);
    let port = server.port;
    let wait = tokio::spawn(server.wait_for_callback(Duration::from_secs(10)));
    // A wrong path is told «Not found» and doesn't end the wait.
    let (body, status) = net::get(&format!("http://127.0.0.1:{port}/favicon.ico"), &[]).await.unwrap();
    assert_eq!((status, String::from_utf8_lossy(&body).contains("Not found")), (404, true));
    let (body, status) = net::get(&format!("http://127.0.0.1:{port}/callback?code=abc&state=x%20y"), &[]).await.unwrap();
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("Signed in"));
    let query = wait.await.unwrap().unwrap();
    assert_eq!(query, HashMap::from([("code".to_string(), "abc".to_string()), ("state".to_string(), "x y".to_string())]));
    let late = LoopbackServer::start(0, "/callback").await.unwrap();
    assert_eq!(late.wait_for_callback(Duration::from_millis(50)).await.unwrap_err().message(), "Sign-in didn't finish within 0 min.");
}

/// The engine keeps credentials in the Swift engine's file: the same JSON both ways, a date as seconds since 2001.
#[test]
fn credentials_keep_swifts_shape() {
    assert_eq!(serde_json::to_string(&Credential::ApiKey("k".into())).unwrap(), r#"{"apiKey":{"_0":"k"}}"#);
    assert_eq!(serde_json::to_string(&Credential::External { path: "/c".into() }).unwrap(), r#"{"external":{"path":"/c"}}"#);
    let swift = r#"{"oauth":{"_0":{"access":"a","refresh":"r","expires":800000000,"accountID":"acc"}}}"#;
    let Credential::OAuth(t) = serde_json::from_str(swift).unwrap() else { panic!() };
    assert_eq!((t.expires, t.account_id.as_deref()), (1_778_307_200.0, Some("acc")));
    let back: Value = serde_json::to_value(Credential::OAuth(t.clone())).unwrap();
    assert_eq!(back, serde_json::from_str::<Value>(swift).unwrap());
    let no_account = Credential::OAuth(OAuthTokens { account_id: None, ..t });
    assert!(!serde_json::to_string(&no_account).unwrap().contains("accountID"));
    assert!(serde_json::from_str::<Credential>(r#"{"other":{}}"#).is_err());
}

// MARK: ChatGPT subscription

fn jwt(claims: Value) -> String { format!("eyJhbGciOiJub25lIn0.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string())) }

#[test]
fn chatgpt_tokens_read_account_and_expiry() {
    let id = jwt(json!({"https://api.openai.com/auth": {"chatgpt_account_id": "acc-1"}}));
    let access = jwt(json!({"exp": 2_000_000_000}));
    let tokens = chatgpt::tokens(json!({"access_token": access, "refresh_token": "r1", "id_token": id}).to_string().as_bytes(), None).unwrap();
    assert_eq!((tokens.account_id.as_deref(), tokens.refresh.as_str(), tokens.expires), (Some("acc-1"), "r1", 2_000_000_000.0));
    // A refresh response without a new refresh token or id_token keeps the old ones.
    let refreshed = chatgpt::tokens(json!({"access_token": access}).to_string().as_bytes(), Some(&tokens)).unwrap();
    assert_eq!((refreshed.refresh.as_str(), refreshed.account_id.as_deref()), ("r1", Some("acc-1")));
    assert_eq!(chatgpt::tokens(json!({"access_token": access}).to_string().as_bytes(), None).unwrap_err().message(),
               "ChatGPT didn't return a refresh token.");
    assert_eq!(oauth::expiry("not-a-jwt"), None);
}

#[test]
fn codex_body_puts_system_into_instructions() {
    let body = ChatGptClient::body(&req("gpt-6-sol", vec![msg(Role::System, "Отвечай JSON."), msg(Role::User, "привет"), msg(Role::Assistant, "{}")]));
    assert_eq!((body["instructions"].as_str(), body["stream"].as_bool(), body["store"].as_bool()), (Some("Отвечай JSON."), Some(true), Some(false)));
    let input = body["input"].as_array().unwrap();
    assert_eq!(input.len(), 2);
    assert_eq!(input[1]["content"][0]["type"], "output_text");
    assert_eq!(input[0]["content"][0]["type"], "input_text");
    assert!(body["prompt_cache_key"].as_str().unwrap().starts_with("ihatecrypto-"));
    assert_eq!(body.get("reasoning"), None);
    let mut thinking = req("m", vec![]);
    thinking.thinking = true;
    assert_eq!(ChatGptClient::body(&thinking)["reasoning"], json!({"effort": "high"}));
    // FNV-1a in base 36, the same across launches.
    assert_eq!(chatgpt::cache_key(""), "33niihzj4ux45");
}

#[test]
fn codex_stream_collects_deltas_and_stops_on_completed() {
    let mut stream = CodexStream::default();
    assert!(!stream.feed("event: response.output_text.delta").unwrap());
    assert!(!stream.feed(r#"data: {"type":"response.output_text.delta","delta":"{\"ok\""}"#).unwrap());
    assert!(!stream.feed(r#"data: {"type":"response.output_text.delta","delta":":true}"}"#).unwrap());
    assert!(stream.feed(r#"data: {"type":"response.completed","response":{}}"#).unwrap());
    assert_eq!(stream.text, r#"{"ok":true}"#);
    let mut failing = CodexStream::default();
    assert_eq!(failing.feed(r#"data: {"type":"response.failed","response":{"error":{"message":"usage limit"}}}"#).unwrap_err().message(),
               "ChatGPT: usage limit");
}

#[test]
fn codex_stream_keeps_what_the_response_used_and_the_backends_list_says_each_window() {
    let mut stream = CodexStream::default();
    stream.feed(r#"data: {"type":"response.output_text.delta","delta":"ok"}"#).unwrap();
    assert!(stream.feed(r#"data: {"type":"response.completed","response":{"usage":{"input_tokens":9000,"input_tokens_details":{"cached_tokens":8000},"output_tokens":25,"total_tokens":9025}}}"#).unwrap());
    assert_eq!(stream.usage, Some(Usage { cached: 8_000, ..Usage::new(9_000, 25) }));
    let list = r#"{"models":[{"slug":"gpt-6-luna","visibility":"list","priority":4,"context_window":272000,"max_context_window":872000},{"slug":"old"}]}"#;
    assert_eq!(chatgpt::windows(list.as_bytes()), vec![("gpt-6-luna".to_string(), 272_000)]);
}

#[test]
fn the_codex_backends_list_reads_its_pickable_models_in_order() {
    let json = r#"{"models":[{"slug":"gpt-6-sol","visibility":"list","priority":3},{"slug":"gpt-reserve","visibility":"hide","priority":4},{"slug":"gpt-6.1-sol","visibility":"list","priority":1},{"slug":"gpt-6-astra","visibility":"list","priority":2}]}"#;
    assert_eq!(chatgpt::listed(json.as_bytes()), ["gpt-6.1-sol", "gpt-6-astra", "gpt-6-sol"]);
}

/// Own tokens close to expiry are refreshed before the request, the new ones handed back to be kept; the answer is
/// read from the Responses API's events with the window the backend told.
#[tokio::test]
async fn chatgpt_refreshes_its_own_tokens_and_streams() {
    let fresh = jwt(json!({"exp": 4_000_000_000u64, "https://api.openai.com/auth": {"chatgpt_account_id": "acc-2"}}));
    let (issuer, issued) = server(vec![(200, json!({"access_token": fresh, "refresh_token": "r2"}).to_string())]).await;
    let (backend, asked) = server(vec![(200, sse(&[
        r#"data: {"type":"response.output_text.delta","delta":"o"}"#, r#"data: {"type":"response.output_text.delta","delta":"k"}"#,
        r#"data: {"type":"response.completed","response":{"usage":{"input_tokens":10,"output_tokens":2}}}"#,
    ]))]).await;
    let kept = Arc::new(Mutex::new(Vec::new()));
    let k = kept.clone();
    let stale = OAuthTokens { access: "old".into(), refresh: "r1".into(), expires: now() + 10.0, account_id: Some("acc-1".into()) };
    let mut client = ChatGptClient::new(Source::Own(stale), Arc::new(move |c| k.lock().unwrap().push(c)));
    client.issuer = issuer;
    client.backend = format!("{backend}/responses");
    usage::tell_window("gpt-test-window", 272_000);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let h = heard.clone();
    let parts = Mutex::new(Vec::new());
    let c = Client::new(Inner::ChatGpt(client)).metered(Arc::new(move |u, _| h.lock().unwrap().push(u.clone())));
    let answer = c.stream(&req("gpt-test-window", vec![msg(Role::User, "x")]), &|t| parts.lock().unwrap().push(t.to_string())).await.unwrap();
    assert_eq!(answer, "ok");
    assert_eq!(*parts.lock().unwrap(), ["o", "ok"]);
    assert_eq!(heard.lock().unwrap()[0], Usage { window: Some(272_000), ..Usage::new(10, 2) });
    let form = &issued.lock().unwrap()[0];
    assert!(form.body.contains("grant_type=refresh_token") && form.body.contains("refresh_token=r1") && form.body.contains("scope=openid%20profile%20email"));
    let Credential::OAuth(saved) = &kept.lock().unwrap()[0] else { panic!() };
    assert_eq!((saved.refresh.as_str(), saved.account_id.as_deref()), ("r2", Some("acc-2")));
    let sent = &asked.lock().unwrap()[0];
    assert_eq!(sent.headers["authorization"], format!("Bearer {fresh}"));
    assert_eq!((sent.headers["chatgpt-account-id"].as_str(), sent.headers["originator"].as_str()), ("acc-2", "ihatecrypto"));
}

#[test]
fn codex_cli_login_is_read_not_refreshed() {
    let dir = std::env::temp_dir().join(format!("ihc-codex-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("auth.json");
    let id = jwt(json!({"https://api.openai.com/auth": {"chatgpt_account_id": "acc-9"}}));
    std::fs::write(&file, json!({"tokens": {"access_token": "t", "id_token": id}}).to_string()).unwrap();
    assert_eq!(chatgpt::read_codex_file(file.to_str().unwrap()).unwrap(), ("t".to_string(), "acc-9".to_string()));
    std::fs::write(&file, "{}").unwrap();
    assert_eq!(chatgpt::read_codex_file(file.to_str().unwrap()).unwrap_err().message(), "Codex CLI isn't signed in with ChatGPT.");
    std::fs::remove_dir_all(&dir).ok();
}

// MARK: Claude Code, API keys, catalog

#[test]
fn claude_code_runs_without_tools_and_keeps_the_prompt_off_the_command_line() {
    let args = ClaudeCodeClient::arguments(&req("claude-haiku-4-5", vec![msg(Role::System, "Отвечай JSON."), msg(Role::User, "секретный текст")]));
    assert!(args.contains(&"--restricted".to_string()));
    let tools = args.iter().position(|a| a == "--tools").unwrap();
    assert_eq!(args[tools + 1], "");
    // `--tools` takes several values; the next argument must be a flag so nothing is swallowed.
    assert!(args[tools + 2].starts_with("--"));
    assert!(!args.contains(&"секретный текст".to_string()));
    assert_eq!(args[args.iter().position(|a| a == "--system-prompt").unwrap() + 1], "Отвечай JSON.");
    assert!(ClaudeCodeClient::arguments(&req("m", vec![msg(Role::User, "x")])).contains(&"--system-prompt".to_string()));
    assert!(ClaudeCodeClient::arguments(&req("m", vec![])).contains(&"--include-partial-messages".to_string()));
}

/// A hung `claude -p` is stopped after its timeout, or when the chat's task is dropped; it never keeps the chat busy.
#[cfg(unix)]
#[tokio::test]
async fn claude_code_run_stops_a_hung_process() {
    let start = std::time::Instant::now();
    let e = claude_code::run("/bin/sleep", &["30".into()], "x", Duration::from_millis(300), None).await.unwrap_err();
    assert_eq!(e.message(), "Claude Code didn't answer in 0 s, so it was stopped. Send it again, or pick another model.");
    let cancelled = tokio::time::timeout(Duration::from_millis(200), claude_code::run("/bin/sleep", &["30".into()], "x", Duration::from_secs(120), None)).await;
    assert!(cancelled.is_err());
    assert!(start.elapsed() < Duration::from_secs(10));
    // A normal run still returns its output; a failed one says what it printed.
    assert_eq!(claude_code::run("/bin/cat", &[], "hello", Duration::from_secs(10), None).await.unwrap(), b"hello");
    let failed = claude_code::run("/bin/ls", &["/nonexistent-path".into()], "", Duration::from_secs(10), None).await.unwrap_err();
    assert!(failed.message().starts_with("Claude Code stopped with code") && failed.message().contains("nonexistent-path"));
}

/// `claude` closes its output a moment before it exits, many runs at once: each wait still ends.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn claude_code_run_returns_when_the_process_exits_after_its_output() {
    let runs = (0..40).map(|i| tokio::spawn(async move {
        claude_code::run("/bin/sh", &["-c".into(), "cat; exec 1>&-; sleep 0.3".into()], &i.to_string(), Duration::from_secs(30), None).await
    }));
    let mut outputs = 0;
    for run in runs { if !run.await.unwrap().unwrap().is_empty() { outputs += 1 } }
    assert_eq!(outputs, 40);
}

#[cfg(unix)]
fn fake_claude(lines: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let path = std::env::temp_dir().join(format!("fake-claude-{}.sh", u64::from_le_bytes(b)));
    std::fs::write(&path, format!("#!/bin/sh\ncat > /dev/null\n{lines}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.to_string_lossy().into_owned()
}

/// Claude Code's stream-json from a stand-in that writes it the way `claude -p` does: the words reach `partial` as
/// they come, and the answer is the result event's.
#[cfg(unix)]
#[tokio::test]
async fn claude_code_stream_tells_the_words_as_they_come() {
    let script = fake_claude(r#"echo '{"type":"system","subtype":"init"}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"hm"}}}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"one "}}}'
sleep 0.2
echo '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"two"}}}'
echo '{"type":"result","is_error":false,"result":"one two"}'"#);
    let seen = Mutex::new(Vec::new());
    let client = Client::new(Inner::ClaudeCode(ClaudeCodeClient { path: script.clone() }));
    let answer = client.stream(&req("m", vec![msg(Role::User, "x")]), &|t| seen.lock().unwrap().push(t.to_string())).await.unwrap();
    assert_eq!(answer, "one two");
    assert_eq!(*seen.lock().unwrap(), ["one ", "one two"]);
    std::fs::remove_file(script).ok();
}

/// Claude Code says before its result whether the answer went past the plan's limits onto usage credits: the
/// rate_limit_event of a real Fable 5.1 run on a Max plan (2026-11-05).
#[cfg(unix)]
#[tokio::test]
async fn claude_code_says_when_an_answer_is_paid_from_credits() {
    let max = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","rateLimitType":"seven_day","overageStatus":"rejected","overageDisabledReason":"org_level_disabled","isUsingOverage":false,"unifiedWindows":{"seven_day":{"utilization":0.15},"seven_day_overage_included":{"utilization":0.03}}}}"#;
    async fn usage(event: Option<&str>, streaming: bool) -> Option<Usage> {
        let script = fake_claude(&format!("echo '{{\"type\":\"system\",\"subtype\":\"init\"}}'\n{}\necho '{{\"type\":\"result\",\"is_error\":false,\"result\":\"ok\",\"total_cost_usd\":0.013,\"usage\":{{\"input_tokens\":5,\"output_tokens\":4}}}}'",
                                          event.map(|e| format!("echo '{e}'")).unwrap_or_default()));
        let heard = Arc::new(Mutex::new(None));
        let h = heard.clone();
        let client = Client::new(Inner::ClaudeCode(ClaudeCodeClient { path: script.clone() })).metered(Arc::new(move |u, _| *h.lock().unwrap() = Some(u.clone())));
        let r = req("claude-fable-5-1", vec![msg(Role::User, "x")]);
        let answer = if streaming { client.stream(&r, &|_| {}).await } else { client.complete(&r).await };
        assert_eq!(answer.unwrap(), "ok");
        std::fs::remove_file(script).ok();
        heard.lock().unwrap().clone()
    }
    assert_eq!(usage(Some(max), true).await.unwrap().overage, Some(false));
    let credits = max.replace(r#""isUsingOverage":false"#, r#""isUsingOverage":true"#);
    assert_eq!(usage(Some(&credits), true).await.unwrap().overage, Some(true));
    // Said twice in one run (a helper model's request): credits if either answer took them.
    assert_eq!(usage(Some(&format!("{credits}'\necho '{max}")), true).await.unwrap().overage, Some(true));
    let older = usage(None, true).await.unwrap();
    assert_eq!((older.overage, older.cost), (None, Some(0.013)));
    assert_eq!(usage(Some(max), false).await.unwrap().overage, Some(false));
}

/// The result object of a real `claude -p` run (2026-10-13), cut to what is read.
#[test]
fn claude_code_result_tells_what_the_run_used() {
    let result = r#"{"type":"result","is_error":false,"result":"works","total_cost_usd":0.000701,"usage":{"input_tokens":521,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":36},"modelUsage":{"claude-haiku-4-5":{"inputTokens":521,"outputTokens":36,"cacheReadInputTokens":0,"cacheCreationInputTokens":0,"costUSD":0.000701,"contextWindow":200000}}}"#;
    assert_eq!(claude_code::usage_of(result.as_bytes()), Some(Usage { window: Some(200_000), cost: Some(0.000701), ..Usage::new(521, 36) }));
    assert_eq!(claude_code::usage_of(b"not json"), None);
    assert_eq!(claude_code::result_of(result.as_bytes()).unwrap(), "works");
    assert_eq!(claude_code::result_of(br#"{"is_error":true,"result":"Invalid API key"}"#).unwrap_err().message(), "Claude Code: Invalid API key");
    assert_eq!(claude_code::result_of(b"oops").unwrap_err().message(), "Claude Code replied unexpectedly: oops");
}

#[test]
fn json_mode_only_where_the_api_supports_it() {
    let mut r = req("m", vec![msg(Role::User, "x")]);
    r.wants_json = true;
    assert!(OpenAiChatClient::new("https://api.deepseek.com/", Some("k"), true).body(&r).get("response_format").is_some());
    assert!(OpenAiChatClient::new("http://localhost:1234/v1", None, false).body(&r).get("response_format").is_none());
    assert_eq!(OpenAiChatClient::new("https://api.deepseek.com/", None, true).base_url, "https://api.deepseek.com");
}

#[test]
fn api_key_provider_refuses_to_run_without_key() {
    let deepseek = catalog::provider("deepseek").unwrap();
    assert_eq!(deepseek.client(None, save_nothing()).err().unwrap().message(), "DeepSeek API: add a key in Settings.");
    assert!(catalog::provider("ollama").unwrap().client(None, save_nothing()).is_ok());
}

/// Ollama answers through its own API, with a window that holds ihatecrypto's instructions.
#[test]
fn ollama_gets_a_wide_context_window() {
    let client = catalog::provider("ollama").unwrap().client(None, save_nothing()).unwrap();
    let Inner::Ollama(ollama) = client.inner else { panic!("not Ollama's own API") };
    assert_eq!(ollama.base, "http://localhost:11434");
    let mut r = req("gemma4:26b", vec![msg(Role::System, "s"), msg(Role::User, "u")]);
    r.wants_json = true;
    let body = ollama.body(&r, true, false);
    assert_eq!((body["options"]["num_ctx"].as_i64(), body["format"].as_str(), body["stream"].as_bool()), (Some(16_384), Some("json"), Some(true)));
    // No thinking before the JSON: Gemma 4 12B took 117 s an answer with it, 8.4 s without.
    assert_eq!(body["think"], json!(false));
    assert!(ollama.body(&req("gpt-oss:20b", vec![]), true, true).get("think").is_none());
}

/// A model that can't stop thinking is asked again with thinking the other way; the last line counts.
#[tokio::test]
async fn ollama_streams_json_lines_and_asks_again_about_thinking() {
    let (url, asked) = server(vec![
        (400, r#"{"error":"\"gpt-oss\" does not support think false"}"#.into()),
        (200, [r#"{"message":{"content":"he"},"done":false}"#, r#"{"message":{"content":"y"},"done":false}"#,
               r#"{"message":{"content":""},"done":true,"prompt_eval_count":30,"eval_count":2}"#].join("\n")),
    ]).await;
    let heard = Arc::new(Mutex::new(Vec::new()));
    let h = heard.clone();
    let client = Client::new(Inner::Ollama(api_key::OllamaChatClient { base: url })).metered(Arc::new(move |u, _| h.lock().unwrap().push(u.clone())));
    assert_eq!(client.complete(&req("gpt-oss:20b", vec![msg(Role::User, "x")])).await.unwrap(), "hey");
    assert_eq!(heard.lock().unwrap()[0], Usage { window: Some(16_384), ..Usage::new(30, 2) });
    let a = asked.lock().unwrap();
    assert_eq!((a[0].path.as_str(), a[0].json()["think"].clone(), a[1].json().get("think").cloned()), ("/api/chat", json!(false), None));
}

/// Server-sent events from /chat/completions: the words told as they come and the count from the last event. A
/// server that refuses `stream_options` is asked again without it, and one that refuses `reasoning_effort` too.
#[tokio::test]
async fn openai_compatible_streams_and_drops_what_a_server_refuses() {
    let words = sse(&[": OPENROUTER PROCESSING", r#"data: {"choices":[{"delta":{"content":"o"}}]}"#, r#"data: {"choices":[{"delta":{"content":"k"}}]}"#,
                      r#"data: {"choices":[],"usage":{"prompt_tokens":11000,"completion_tokens":40,"prompt_tokens_details":{"cached_tokens":10240},"cost":0.0123}}"#,
                      "data: [DONE]"]);
    let (url, asked) = server(vec![
        (400, r#"{"error":{"message":"Unrecognized request argument supplied: reasoning_effort"}}"#.into()),
        (422, r#"{"detail":"stream_options: extra fields not permitted"}"#.into()),
        (200, words),
    ]).await;
    let heard = Arc::new(Mutex::new(Vec::new()));
    let h = heard.clone();
    let client = Client::new(Inner::OpenAi(OpenAiChatClient::new(&url, Some("sk"), true))).thinking()
        .metered(Arc::new(move |u, r| h.lock().unwrap().push((u.clone(), r.thinking))));
    let parts = Mutex::new(Vec::new());
    assert_eq!(client.stream(&req("m", vec![msg(Role::User, "x")]), &|t| parts.lock().unwrap().push(t.to_string())).await.unwrap(), "ok");
    assert_eq!(*parts.lock().unwrap(), ["o", "ok"]);
    // The meter hears the request as it was asked, before «Think before answering» was added.
    assert_eq!(heard.lock().unwrap()[0], (Usage { cached: 10_240, cost: Some(0.0123), ..Usage::new(11_000, 40) }, false));
    let a = asked.lock().unwrap();
    assert_eq!(a[0].json()["reasoning_effort"], "medium");
    assert!(a[1].json().get("reasoning_effort").is_none() && a[1].json().get("stream_options").is_some());
    assert!(a[2].json().get("stream_options").is_none());
    assert_eq!(a[0].headers["authorization"], "Bearer sk");
    // A server that ignores «stream» and answers with one body still works; a whole answer through `complete` too.
    let whole = json!({"choices": [{"message": {"content": "plain"}}], "usage": {"prompt_tokens": 3, "completion_tokens": 1}}).to_string();
    let (url, _) = server(vec![(200, whole.clone()), (200, whole), (200, r#"{"data":[{"id":"b"},{"id":"a"}]}"#.into())]).await;
    let c = OpenAiChatClient::new(&url, None, true);
    let plain = Client::new(Inner::OpenAi(c.clone()));
    assert_eq!(plain.stream(&req("m", vec![]), &|_| {}).await.unwrap(), "plain");
    assert_eq!(plain.complete(&req("m", vec![])).await.unwrap(), "plain");
    assert_eq!(c.models().await.unwrap(), ["a", "b"]);
}

// MARK: Anthropic's Messages API

/// An Anthropic key goes to Anthropic's own API, in its old place in the list.
#[test]
fn an_anthropic_key_uses_the_messages_api() {
    let ids: Vec<String> = catalog::all().iter().map(|p| p.info().id).collect();
    assert_eq!(ids.iter().position(|i| i == "anthropic"), ids.iter().position(|i| i == "openai").map(|i| i + 1));
    let anthropic = catalog::provider("anthropic").unwrap();
    let info = anthropic.info();
    assert!(matches!(anthropic, Provider::Anthropic) && info.kind == Kind::ApiKey && info.takes_key);
    assert_eq!(anthropic.client(None, save_nothing()).err().unwrap().message(), "Anthropic API: add a key in Settings.");
    assert!(matches!(anthropic.client(Some(&Credential::ApiKey("k".into())), save_nothing()).unwrap().inner, Inner::Anthropic(_)));
    assert!(!api_key::presets().iter().any(|p| p.info.id == "anthropic"));
    assert_eq!(AnthropicClient::new("k").headers(), [("x-api-key", "k".to_string()), ("anthropic-version", "2026-10-21".to_string())]);
    assert!(info.note.contains("platform.claude.com/settings/keys"));
}

/// The instructions go as one cached block, the dialogue as it is; «Think before answering» is adaptive thinking at
/// high effort, and without it the model keeps its own.
#[test]
fn anthropic_request_caches_the_instructions_and_thinks_when_asked() {
    let mut r = req("claude-opus-5-5", vec![msg(Role::System, "Answer as JSON."), msg(Role::User, "купи SOL на $5"), msg(Role::Assistant, "{}"), msg(Role::User, "ещё раз")]);
    r.wants_json = true;
    let body = AnthropicClient::body(&r, false, 32_000, true);
    assert_eq!((body["model"].as_str(), body["stream"].as_bool(), body["max_tokens"].as_i64()), (Some("claude-opus-5-5"), Some(true), Some(32_000)));
    assert_eq!(body["system"], json!([{"type": "text", "text": "Answer as JSON.", "cache_control": {"type": "ephemeral"}}]));
    let roles: Vec<&str> = body["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["user", "assistant", "user"]);
    assert_eq!(body["messages"][0]["content"], "купи SOL на $5");
    assert!(body.get("response_format").is_none() && body.get("thinking").is_none() && body.get("output_config").is_none());
    let thinking = AnthropicClient::body(&r, true, 32_000, true);
    assert_eq!((thinking["thinking"].clone(), thinking["output_config"].clone()), (json!({"type": "adaptive"}), json!({"effort": "high"})));
    assert!(AnthropicClient::body(&req("m", vec![msg(Role::User, "x")]), false, 32_000, true).get("system").is_none());
}

/// The event stream as the Messages API sends it (a thinking block before the text, a cache hit): the text, the stop
/// reason and the counts, cache parts included.
#[test]
fn anthropic_stream_reads_the_text_and_what_the_answer_used() {
    let events = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_01","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"stop_reason":null,"usage":{"input_tokens":412,"cache_creation_input_tokens":0,"cache_read_input_tokens":18210,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"EqQB"}}

event: ping
data: {"type": "ping"}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"{\"reply\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"\"ok\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":57,"cache_creation_input_tokens":null}}

event: message_stop
data: {"type":"message_stop"}"#;
    let (mut stream, mut texts, mut ended) = (AnthropicStream::default(), Vec::new(), false);
    for line in events.split('\n') {
        if ended { break }
        let before = stream.text.clone();
        ended = stream.feed(line).unwrap();
        if stream.text != before { texts.push(stream.text.clone()) }
    }
    assert!(ended);
    assert_eq!((stream.text.as_str(), texts), (r#"{"reply":"ok"}"#, vec![r#"{"reply":"#.to_string(), r#"{"reply":"ok"}"#.to_string()]));
    assert_eq!(stream.stop.as_deref(), Some("end_turn"));
    assert_eq!(stream.usage(), Some(Usage { cached: 18_210, ..Usage::new(18_622, 57) }));
    let mut refused = AnthropicStream::default();
    refused.feed(r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal"},"usage":{"output_tokens":0}}"#).unwrap();
    assert_eq!((refused.stop.as_deref(), refused.text.as_str()), (Some("refusal"), ""));
    let mut failing = AnthropicStream::default();
    assert_eq!(failing.feed(r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#).unwrap_err().message(), "Anthropic: Overloaded");
}

/// A model whose safety check can decline a request asks for Anthropic's recommended fallback model; Haiku 5.5, Sonnet
/// 5 and the 4.x models have none.
#[test]
fn anthropic_asks_for_a_fallback_on_the_models_that_have_one() {
    let asks = |model: &str| AnthropicClient::body(&req(model, vec![msg(Role::User, "x")]), false, 32_000, true)["fallbacks"] == "default";
    for model in ["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1", "claude-opus-5", "claude-fable-5"] { assert!(asks(model), "{model}") }
    for model in ["claude-haiku-5-5", "claude-opus-4-8", "claude-haiku-4-5", "claude-sonnet-5"] { assert!(!asks(model), "{model}") }
    assert!(AnthropicClient::body(&req("claude-opus-5-5", vec![msg(Role::User, "x")]), false, 32_000, false).get("fallbacks").is_none());
}

/// A refusal midway: the declined model's words stay, a fallback block marks the switch, and the fallback model goes
/// on from them. Read as one answer.
#[test]
fn anthropic_stream_reads_a_fallback_as_one_answer() {
    let events = [
        r#"data: {"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":40,"output_tokens":1}}}"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"{\"route\":\"chat\",\"re"}}"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-opus-4-8"}}}"#,
        r#"data: {"type":"content_block_stop","index":1}"#,
        r#"data: {"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}}"#,
        r#"data: {"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"ply\":\"ok\"}"}}"#,
        r#"data: {"type":"content_block_stop","index":2}"#,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":12}}"#,
        r#"data: {"type":"message_stop"}"#,
    ];
    let (mut stream, mut ended) = (AnthropicStream::default(), false);
    for line in events { if !ended { ended = stream.feed(line).unwrap() } }
    assert!(ended && stream.text == r#"{"route":"chat","reply":"ok"}"# && stream.stop.as_deref() == Some("end_turn"));
}

fn anthropic(base: &str, key: &str) -> AnthropicClient { AnthropicClient { key: key.into(), base: base.into() } }

/// The whole way through the client: the key and version sent, the words told as they come, what the answer used told
/// to the meter. A model that refuses adaptive thinking (Haiku 4.5) is asked again without it, and a refused fallback
/// field is dropped.
#[tokio::test]
async fn anthropic_client_streams_the_answer_and_counts_it() {
    let (url, asked) = server(vec![
        (400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"adaptive thinking is not supported on this model"}}"#.into()),
        (200, sse(&[r#"data: {"type":"message_start","message":{"usage":{"input_tokens":30,"cache_creation_input_tokens":5000,"cache_read_input_tokens":0,"output_tokens":1}}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"wor"}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"king"}}"#,
                    r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}"#,
                    r#"data: {"type":"message_stop"}"#])),
    ]).await;
    let heard = Arc::new(Mutex::new(Vec::new()));
    let h = heard.clone();
    let client = Client::new(Inner::Anthropic(anthropic(&url, "sk-ant-test"))).metered(Arc::new(move |u, _| h.lock().unwrap().push(u.clone())));
    let partial = Mutex::new(Vec::new());
    let mut r = req("claude-haiku-4-5", vec![msg(Role::System, "s"), msg(Role::User, "x")]);
    r.thinking = true;
    assert_eq!(client.stream(&r, &|t| partial.lock().unwrap().push(t.to_string())).await.unwrap(), "working");
    assert_eq!(*partial.lock().unwrap(), ["wor", "working"]);
    assert_eq!(*heard.lock().unwrap(), [Usage { written: 5_000, ..Usage::new(5_030, 3) }]);
    {
        let a = asked.lock().unwrap();
        assert!(a.len() == 2 && a.iter().all(|x| x.headers["x-api-key"] == "sk-ant-test" && x.headers["anthropic-version"] == "2026-10-21"));
        assert!(a[0].json().get("thinking").is_some() && a[1].json().get("thinking").is_none() && a[1].json().get("output_config").is_none());
        assert_eq!((a[0].json()["max_tokens"].as_i64(), a[1].json()["max_tokens"].as_i64()), (Some(32_000), Some(4_096)));
        assert_eq!(a[0].path, "/messages");
    }

    // A stream that ends before message_stop is half an answer: said, not taken for the whole.
    let (url, _) = server(vec![(200, sse(&[r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"{\"rep"}}"#]))]).await;
    let cut = Client::new(Inner::Anthropic(anthropic(&url, "k"))).complete(&req("claude-opus-5-5", vec![msg(Role::User, "x")])).await.unwrap_err();
    assert!(cut.message().contains("cut off"));

    // A key that isn't scoped to a workspace is said as what to do, in the chat and in Settings' model list alike, and
    // not asked again without thinking.
    let no_workspace = r#"{"type":"error","error":{"type":"invalid_request_error","message":"This API key is not scoped to a workspace, so this request must include the anthropic-workspace-id header with the ID of the workspace to use. Add the header, or use an API key that is scoped to a workspace."},"request_id":"req_011CSHoEeqs5C35K2UUqR7Fy"}"#;
    let (url, asked) = server(vec![(400, no_workspace.into()), (400, no_workspace.into())]).await;
    let mut thinking = req("claude-opus-5-5", vec![msg(Role::User, "x")]);
    thinking.thinking = true;
    let chat = Client::new(Inner::Anthropic(anthropic(&url, "k"))).complete(&thinking).await.unwrap_err();
    assert!(chat.message().contains("platform.claude.com/settings/keys › Create key"));
    assert_eq!(asked.lock().unwrap().len(), 1);
    assert_eq!(anthropic(&url, "k").models().await.unwrap_err().message(), anthropic::NO_WORKSPACE);

    // The fallback field goes with its beta header; refused, the request is asked again without them, thinking and
    // length as before, even when the refusal names max_tokens.
    let (url, asked) = server(vec![
        (400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"fallbacks: max_tokens 32000 is not valid for claude-opus-4-8"}}"#.into()),
        (200, sse(&[r#"data: {"type":"message_start","message":{"usage":{"input_tokens":30,"output_tokens":1}}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#,
                    r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}"#,
                    r#"data: {"type":"message_stop"}"#])),
    ]).await;
    assert_eq!(Client::new(Inner::Anthropic(anthropic(&url, "k"))).complete(&thinking).await.unwrap(), "ok");
    {
        let a = asked.lock().unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!((a[0].json()["fallbacks"].as_str(), a[0].headers.get("anthropic-beta").map(String::as_str)), (Some("default"), Some("server-side-fallback-2026-07-01")));
        assert!(a[1].json().get("fallbacks").is_none() && !a[1].headers.contains_key("anthropic-beta"));
        assert_eq!((a[1].json()["thinking"].clone(), a[1].json()["max_tokens"].as_i64()), (json!({"type": "adaptive"}), Some(32_000)));
    }

    // A model without a fallback sends neither; a declined answer is counted, then said.
    let (url, asked) = server(vec![(200, sse(&[r#"data: {"type":"message_start","message":{"usage":{"input_tokens":9,"output_tokens":0}}}"#,
                                               r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal"},"usage":{"output_tokens":0}}"#,
                                               r#"data: {"type":"message_stop"}"#]))]).await;
    let heard = Arc::new(Mutex::new(Vec::new()));
    let h = heard.clone();
    let declined = Client::new(Inner::Anthropic(anthropic(&url, "k"))).metered(Arc::new(move |u, _| h.lock().unwrap().push(u.clone())))
        .complete(&req("claude-haiku-5-5", vec![msg(Role::User, "x")])).await.unwrap_err();
    assert!(declined.message().starts_with("Claude declined this request"));
    assert_eq!(heard.lock().unwrap().len(), 1);
    let a = asked.lock().unwrap();
    assert!(!a[0].headers.contains_key("anthropic-beta") && a[0].json().get("fallbacks").is_none());
}

/// The starter lists before a sign-in carry the newest models (2026-11-05).
#[test]
fn starter_lists_carry_the_newest_models() {
    let models = |id: &str| catalog::provider(id).map(|p| p.info().models).unwrap_or_default();
    assert!(models("anthropic").contains(&"claude-haiku-5-5".into()) && models("claude-code").contains(&"claude-haiku-5-5".into()));
    assert!(![models("anthropic"), models("claude-code")].concat().contains(&"claude-haiku-4-5".into()));
    assert!(models("openai").contains(&"gpt-6.1-terra".into()) && models("chatgpt").contains(&"gpt-6.1-terra".into()));
    assert!(models("gemini").contains(&"gemini-4-argon".into()));
    for m in ["openai/gpt-6.1-terra", "google/gemini-4-argon", "anthropic/claude-haiku-5.5"] { assert!(models("openrouter").contains(&m.into())) }
}

#[test]
fn catalog_ids_are_unique_and_usable_entries_have_a_model_or_are_local() {
    let all = catalog::all();
    let ids: std::collections::HashSet<String> = all.iter().map(|p| p.info().id).collect();
    assert_eq!(ids.len(), all.len());
    for p in &all {
        let i = p.info();
        if i.unavailable.is_none() && i.kind != Kind::Local { assert!(!i.default_model.is_empty(), "{}", i.id) }
    }
    assert!(catalog::provider("grok-subscription").unwrap().info().unavailable.is_some());
    assert_eq!(catalog::provider("grok-subscription").unwrap().client(None, save_nothing()).err().unwrap().message(),
               "xAI opens subscription sign-in only to partners (Hermes, OpenClaw). For now, connect Grok with an xAI API key.");
    assert_eq!(all.iter().filter(|p| p.info().kind == Kind::Subscription).count(), 4);
    // No Apple model on Windows; the local servers run on this PC.
    assert!(catalog::provider("apple").is_none() && catalog::chat_refusal("gpt-6.1-sol").is_none());
    assert_eq!(catalog::provider("ollama").unwrap().info().name, "Ollama on this PC");
}

#[test]
fn model_ids_read_as_names() {
    for (id, name) in [("gpt-6-sol", "GPT 6 SOL"), ("gpt-6.1-sol", "GPT 6.1 SOL"), ("gpt-6-astra", "GPT 6 Astra"), ("claude-opus-5-5", "Claude Opus 5.5"),
                       ("claude-fable-5-1", "Claude Fable 5.1"), ("grok-4.7", "Grok 4.7"), ("claude-haiku-4-5", "Claude Haiku 4.5"),
                       ("deepseek-flash", "DeepSeek Flash"), ("deepseek/deepseek-v4.1-flash", "DeepSeek V4.1 Flash"), ("gemini-3.8-flash", "Gemini 3.8 Flash"),
                       ("grok-4.3", "Grok 4.3"), ("qwen3.5:9b", "Qwen3.5 9B"), ("gpt-6-pro", "GPT 6 Pro")] {
        assert_eq!(catalog::display_name(id), name);
    }
}

// MARK: What an answer used (the chat's context chip)

#[test]
fn usage_reads_each_services_shape() {
    let open_router = json!({"prompt_tokens": 11_000, "completion_tokens": 40, "prompt_tokens_details": {"cached_tokens": 10_240}, "cost": 0.0123});
    assert_eq!(Usage::reported(Some(&open_router)), Some(Usage { cached: 10_240, cost: Some(0.0123), ..Usage::new(11_000, 40) }));
    let responses = json!({"input_tokens": 9_000, "input_tokens_details": {"cached_tokens": 8_000}, "output_tokens": 25});
    assert_eq!(Usage::reported(Some(&responses)), Some(Usage { cached: 8_000, ..Usage::new(9_000, 25) }));
    let anthropic = json!({"input_tokens": 521, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 10_000, "output_tokens": 36});
    assert_eq!(Usage::reported(Some(&anthropic)), Some(Usage { cached: 10_000, written: 100, ..Usage::new(10_621, 36) }));
    assert_eq!(Usage::reported(Some(&json!({"total_tokens": 5}))), None);
    assert_eq!(Usage::reported(None), None);
}

#[test]
fn model_facts_read_open_routers_list_and_price_an_answer() {
    let list = r#"{"data":[{"id":"anthropic/claude-opus-5.5","context_length":1000000,"pricing":{"prompt":"0.000004","completion":"0.00002","input_cache_read":"0.0000002","input_cache_write":"0.000005"}},{"id":"deepseek/deepseek-v4.1-flash:free","context_length":1048576,"pricing":{"prompt":"0","completion":"0"}},{"id":"~deepseek/deepseek-flash-latest","context_length":1048576,"pricing":{"prompt":"0.00000015","completion":"0.0000006"}},{"id":"no-window"}]}"#;
    let table = ModelFacts::table(list.as_bytes());
    let opus = table[&ModelFacts::key("claude-opus-5-5")].clone();
    assert!(opus.window == 1_000_000 && table.len() == 3 && !table.contains_key(&ModelFacts::key("deepseek-v4.1-flash")));
    assert!(!table.contains_key(&ModelFacts::key("deepseek-flash")));
    assert_eq!(table[&ModelFacts::key("deepseek-flash-latest")], ModelFacts::new(1_048_576, 0.00000015, 0.00000015, 0.0000006, None));
    assert!((opus.cost(&Usage { cached: 10_000, ..Usage::new(11_000, 100) }) - 0.008).abs() < 1e-12);
    assert!((opus.cost(&Usage { written: 18_000, ..Usage::new(18_500, 100) }) - 0.094).abs() < 1e-12);
    let haiku_row = r#"{"data":[{"id":"anthropic/claude-haiku-5.5","context_length":1000000,"pricing":{"prompt":"0.0000001","completion":"0.0000005","input_cache_read":"0.00000001","input_cache_write":"0.000000125","overrides":[{"min_prompt_tokens":100000,"prompt":"0.0000005","completion":"0.0000025","input_cache_read":"0.00000005","input_cache_write":"0.000000625"}]}}]}"#;
    let haiku = ModelFacts::table(haiku_row.as_bytes())[&ModelFacts::key("claude-haiku-5-5")].clone();
    assert_eq!(haiku.longer.iter().map(|t| t.over).collect::<Vec<_>>(), [100_000]);
    assert!((haiku.cost(&Usage::new(100_000, 1_000)) - 0.0105).abs() < 1e-12);
    assert!((haiku.cost(&Usage::new(100_001, 1_000)) - 0.0525005).abs() < 1e-12);
    let partial = r#"{"data":[{"id":"x/m","context_length":10,"pricing":{"prompt":"1","completion":"2","overrides":[{"min_prompt_tokens":5,"prompt":"3"}]}}]}"#;
    assert_eq!(ModelFacts::table(partial.as_bytes())[&ModelFacts::key("m")].cost(&Usage::new(6, 1)), 20.0);
    assert_eq!((catalog::window("ollama"), catalog::window("openai")), (Some(16_384), None));
}
