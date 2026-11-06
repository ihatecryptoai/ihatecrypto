//! Settings › AI model, as the Mac's ModelSection and LocalModels.swift: «Think before answering», a model's Check (the
//! chat's exam) and its score, Claude Code installed and signed in from here, Ollama installed or updated from here and a
//! model set up in one go; and Settings › Background › Keep always running.
//! Layer 2 adds here: local_op (thinking, checkModel, installClaudeCode, claudeSignIn, installOllama, ollamaStatus,
//! setUpLocal, keepRunning), local_setup_view, the installers (OllamaSetup.latest/version/install, ClaudeCodeSetup.install
//! and signIn) and Shell.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LocalSetup {
    /// The Check while it runs: how many of the exam's requests are done, for «provider/model».
    pub exam_progress: Option<i64>,
    pub exam_for: Option<String>,
    /// The last Check's or install's line, shown under the account.
    pub status: Option<String>,
    pub ollama_version: Option<String>,
    pub ollama_latest: Option<String>,
    /// What the Ollama or Claude Code install is doing; None when none runs.
    pub ollama_install: Option<String>,
    pub claude_install: Option<String>,
    /// Ollama was just updated here: its newest is in, whatever GitHub said a moment ago.
    pub ollama_updated: bool,
    /// A download said this Ollama is too old for its model («requires a newer version of Ollama»), until its version
    /// changes (the Mac's ollamaOutdated).
    pub ollama_outdated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSetupView {
    pub thinking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exam_progress: Option<i64>,
    pub exam_total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exam_for: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub scores: HashMap<String, Score>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_latest: Option<String>,
    /// The Mac's localState: noOllama, notRunning, outdated, missing, ready.
    pub ollama_state: String,
    /// A download said this Ollama is too old (the Mac's ollamaOutdated): the chat's bar says so only then, not when
    /// Ollama is merely behind its newest release.
    pub ollama_outdated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_install: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_install: Option<String>,
    /// Claude Code found on this PC: «Sign in in a terminal» runs its own sign-in.
    pub claude_found: bool,
    /// As Windows has it; None until the app has said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_running: Option<bool>,
}

/// `LocalSetupView.Score`: a model's Check as Settings shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Score {
    pub passed: i64,
    pub total: i64,
    pub seconds: f64,
    pub good: bool,
    pub short: String,
    pub misses: Vec<String>,
}

/// `OllamaSetup.older`: «0.4.0» before «0.35.1», as `compare(_:options: .numeric)` orders them — runs of digits by their
/// value, the rest by its characters.
pub fn ollama_older(a: &str, than: &str) -> bool { numeric_order(a, than) == Ordering::Less }

fn numeric_order(a: &str, b: &str) -> Ordering {
    fn pieces(s: &str) -> Vec<(bool, &str)> {
        let mut out = vec![];
        let mut start = 0;
        let bytes: Vec<(usize, char)> = s.char_indices().collect();
        for i in 1..=bytes.len() {
            let end = i == bytes.len() || bytes[i].1.is_ascii_digit() != bytes[i - 1].1.is_ascii_digit();
            if end {
                let stop = if i == bytes.len() { s.len() } else { bytes[i].0 };
                out.push((bytes[start].1.is_ascii_digit(), &s[bytes[start].0..stop]));
                start = i;
            }
        }
        out
    }
    let (pa, pb) = (pieces(a), pieces(b));
    for (x, y) in pa.iter().zip(&pb) {
        let o = match (x, y) {
            ((true, x), (true, y)) => {
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                x.len().cmp(&y.len()).then_with(|| x.cmp(y))
            }
            ((_, x), (_, y)) => x.cmp(y),
        };
        if o != Ordering::Equal { return o }
    }
    pa.len().cmp(&pb.len())
}

/// Get-AuthenticodeSignature's «Status|Subject»: a valid signature whose signer is Ollama's organization. Anything else
/// is refused before the installer runs, as the Mac checks its Developer ID team.
pub fn signed_by_ollama(line: &str) -> bool {
    let Some((status, subject)) = line.trim().split_once('|') else { return false };
    status == "Valid" && subject.split(',').map(str::trim).any(|f| {
        f.strip_prefix("O=").map(|o| o.trim_matches(|c| c == '"' || c == ' ')).is_some_and(|o| o == "Ollama" || o == "Ollama Inc" || o == "Ollama Inc.")
    })
}

/// Claude Code's installer only from Anthropic's: the download must stay on claude.ai's hosts after its redirects.
pub fn from_claude(host: Option<&str>) -> bool { host.is_some_and(|h| h == "claude.ai" || h.ends_with(".claude.ai")) }
