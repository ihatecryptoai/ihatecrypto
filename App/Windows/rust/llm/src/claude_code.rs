//! Claude Pro/Max through the Claude Code the person already installed and signed in to, the way OpenClaw does it
//! (`ClaudeCode/ClaudeCodeSubscription.swift`). Anthropic's terms don't allow third-party Claude.ai login, so this stays
//! an opt-in gray-zone option.

use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::catalog::{Info, Kind};
use crate::usage::{self, Usage};
use crate::{Credential, Error, Partial, Report, Request, Result, fail};

pub fn info() -> Info {
    let mut info = Info::new("claude-code", "Claude (subscription via Claude Code)", Kind::Subscription, "claude-opus-5-5",
                             "Runs the installed Claude Code with no tools. Anthropic doesn't officially allow third-party sign-in — a gray area.")
        // All answer through `claude -p`. Fable isn't in a Pro plan's limits: Claude Code says with each answer when usage
        // credits paid for it (Usage.overage).
        .models(&["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1", "claude-haiku-5-5"]);
    info.existing_login_label = Some("Use the installed Claude Code".into());
    info
}

/// The installed `claude`: on Windows Anthropic's installer puts claude.exe in %USERPROFILE%\.local\bin, and PATH is
/// split by «;». ponytail: npm's claude.cmd isn't taken — a .cmd needs cmd.exe to run.
pub(crate) fn existing_login() -> Option<Credential> {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let found = if cfg!(windows) {
        let home = env("USERPROFILE");
        std::iter::once(format!("{home}\\.local\\bin\\claude.exe"))
            .chain(env("PATH").split(';').filter(|d| !d.is_empty()).map(|d| format!("{d}\\claude.exe")).collect::<Vec<_>>())
            .find(|p| Path::new(p).is_file())
    } else {
        let home = env("HOME");
        ["~/.local/bin/claude", "/opt/homebrew/bin/claude", "/usr/local/bin/claude", "~/.claude/local/claude"].iter()
            .map(|c| c.replacen('~', &home, 1))
            .chain(env("PATH").split(':').filter(|d| !d.is_empty()).map(|d| format!("{d}/claude")).collect::<Vec<_>>())
            .find(|p| executable(p))
    };
    found.map(|path| Credential::External { path })
}

#[cfg(unix)]
fn executable(p: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(p: &str) -> bool { Path::new(p).is_file() }

#[derive(Debug, Clone)]
pub struct ClaudeCodeClient {
    pub path: String,
}

impl ClaudeCodeClient {
    /// Headless, no tools, no MCP, nothing saved to session history; our system prompt replaces Claude Code's own (a
    /// coding agent's). The prompt goes through stdin so it isn't in the process list. One JSON event a line, the
    /// answer's words as they come, the result last: only these events say whether usage credits paid for the answer.
    pub fn arguments(r: &Request) -> Vec<String> {
        let system = r.system();
        let mut args: Vec<String> = ["-p", "--restricted", "--strict-mcp-config", "--tools", "", "--no-session-persistence",
                                     "--output-format", "stream-json", "--verbose", "--include-partial-messages", "--model"]
            .iter().map(|s| s.to_string()).collect();
        args.push(r.model.clone());
        args.push("--system-prompt".into());
        args.push(if system.is_empty() { "You are an assistant. Answer briefly and to the point.".into() } else { system });
        if r.thinking { args.extend(["--effort".into(), "high".into()]) }
        args
    }

    /// The answer, its words told to `partial` as Claude Code writes them (text_delta events).
    pub(crate) async fn stream(&self, r: &Request, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        let prompt = r.dialogue().iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n\n");
        let text = Mutex::new(String::new());
        let tell = |line: &str| {
            let Ok(j) = serde_json::from_str::<Value>(line) else { return };
            let event = j.get("event").filter(|_| j.get("type").and_then(Value::as_str) == Some("stream_event"));
            let delta = event.filter(|e| e.get("type").and_then(Value::as_str) == Some("content_block_delta")).and_then(|e| e.get("delta"));
            let Some(words) = delta.filter(|d| d.get("type").and_then(Value::as_str) == Some("text_delta")).and_then(|d| d.get("text")).and_then(Value::as_str)
            else { return };
            let mut t = text.lock().expect("text");
            t.push_str(words);
            partial(&t);
        };
        let output = run(&self.path, &Self::arguments(r), &prompt, Duration::from_secs(120), Some(&tell)).await?;
        // The result is the last event; a run that wrote none (an old Claude Code) is read as one JSON answer.
        let lines: Vec<&[u8]> = output.split(|&b| b == b'\n').collect();
        let typed = |line: &[u8]| serde_json::from_slice::<Value>(line).ok().filter(|v| v.is_object());
        let result = lines.iter().rev().find(|l| typed(l).is_some_and(|v| v.get("type").and_then(Value::as_str) == Some("result")))
            .map_or(output.as_slice(), |l| *l);
        let mut used = usage_of(result);
        // Whether usage credits paid for the answer, said just before the result (read from Claude Code 2.1.318's output,
        // 2026-11-05). Said more than once (a helper model's request too): credits if any one did.
        let said: Vec<bool> = lines.iter().filter_map(|l| {
            let e = typed(l)?;
            if e.get("type").and_then(Value::as_str) != Some("rate_limit_event") { return None }
            e.get("rate_limit_info")?.get("isUsingOverage")?.as_bool()
        }).collect();
        if let Some(u) = used.as_mut() { u.overage = if said.is_empty() { None } else { Some(said.contains(&true)) } }
        if let Some(u) = used { report(u) }
        result_of(result)
    }
}

/// What the run used, from its result object: the tokens of its one answer, the price Claude Code puts on them (a
/// subscription isn't billed it) and the model's context window.
pub fn usage_of(output: &[u8]) -> Option<Usage> {
    let result: Value = serde_json::from_slice(output).ok()?;
    let mut u = Usage::reported(result.get("usage"))?;
    u.cost = usage::double(result.get("total_cost_usd"));
    // A run may count a small helper model beside the one asked: the window is the one that read the most.
    let read = |m: &Value| ["inputTokens", "cacheReadInputTokens", "cacheCreationInputTokens"].iter().map(|k| usage::int(m.get(*k)).unwrap_or(0)).sum::<i64>();
    u.window = result.get("modelUsage").and_then(Value::as_object)
        .and_then(|models| models.values().filter(|m| m.is_object()).max_by_key(|m| read(m)))
        .and_then(|m| usage::int(m.get("contextWindow")));
    Some(u)
}

/// The answer in Claude Code's result object, or why there's none.
pub fn result_of(output: &[u8]) -> Result<String> {
    let unexpected = || Error::Llm(format!("Claude Code replied unexpectedly: {}", String::from_utf8_lossy(&output[..output.len().min(200)])));
    let Ok(Value::Object(r)) = serde_json::from_slice::<Value>(output) else { return Err(unexpected()) };
    let is_error = match r.get("is_error") { None | Some(Value::Null) => false, Some(Value::Bool(b)) => *b, _ => return Err(unexpected()) };
    let result = match r.get("result") { None | Some(Value::Null) => None, Some(Value::String(s)) => Some(s.clone()), _ => return Err(unexpected()) };
    if is_error { return fail(format!("Claude Code: {}", result.unwrap_or_else(|| "error".into()))) }
    Ok(result.unwrap_or_default())
}

/// Runs `path` with the prompt on stdin. A `claude -p` that hangs (a login prompt, a stalled network) is stopped after
/// `timeout`, or as soon as the future waiting for it is dropped (the chat's task cancelled). `lines`: each line of the
/// output as it comes, besides the whole at the end.
pub async fn run(path: &str, arguments: &[String], input: &str, timeout: Duration, lines: Option<&(dyn Fn(&str) + Send + Sync)>) -> Result<Vec<u8>> {
    let mut command = tokio::process::Command::new(path);
    command.args(arguments).current_dir(std::env::temp_dir())
        .env_remove("CLAUDECODE").env_remove("CLAUDE_CODE_ENTRYPOINT")
        // A chat message never reinstalls Claude Code: while it reinstalls, every run fails for a minute or two.
        .env("DISABLE_AUTOUPDATER", "1")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true);
    // No console window for each chat message from a program that has none.
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command.spawn().map_err(|e| Error::Io(format!("Claude Code didn't start: {e}")))?;
    // The prompt and stderr on their own, so a run that stops reading or fills stderr can't block the wait.
    let mut stdin = child.stdin.take().expect("piped");
    let prompt = input.as_bytes().to_vec();
    tokio::spawn(async move { let _ = stdin.write_all(&prompt).await; });
    let mut stderr = child.stderr.take().expect("piped");
    let errors = tokio::spawn(async move { let mut e = Vec::new(); let _ = stderr.read_to_end(&mut e).await; e });
    let mut stdout = BufReader::new(child.stdout.take().expect("piped"));
    let read = async {
        let mut all = Vec::new();
        loop {
            let mut line = Vec::new();
            if stdout.read_until(b'\n', &mut line).await.unwrap_or(0) == 0 { break }
            all.extend_from_slice(&line);
            if let (Some(tell), Some(b'\n')) = (lines, line.last()) { tell(&String::from_utf8_lossy(&line[..line.len() - 1])) }
        }
        let status = child.wait().await;
        (all, status)
    };
    let Ok((data, status)) = tokio::time::timeout(timeout, read).await else {
        return fail(format!("Claude Code didn't answer in {} s, so it was stopped. Send it again, or pick another model.", timeout.as_secs()));
    };
    let status = status.map_err(|e| Error::Io(e.to_string()))?;
    if data.is_empty() && !status.success() {
        let e = errors.await.unwrap_or_default();
        let said = String::from_utf8_lossy(&e[e.len().saturating_sub(300)..]).trim().to_string();
        return fail(format!("Claude Code stopped with code {}{}", code(&status), if said.is_empty() { ".".into() } else { format!(": {said}") }));
    }
    Ok(data)
}

/// The exit code, or on Unix the signal that ended it (Foundation's terminationStatus).
fn code(status: &std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    { use std::os::unix::process::ExitStatusExt; status.code().or(status.signal()).unwrap_or(-1) }
    #[cfg(not(unix))]
    { status.code().unwrap_or(-1) }
}

pub(crate) fn client(credential: Option<&Credential>) -> Result<ClaudeCodeClient> {
    let Some(Credential::External { path }) = credential else { return fail("Claude Code isn't connected.") };
    Ok(ClaudeCodeClient { path: path.clone() })
}
