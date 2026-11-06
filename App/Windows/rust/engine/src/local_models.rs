//! Settings › AI model, as the Mac's ModelSection and LocalModels.swift: «Think before answering», a model's Check (the
//! chat's exam) and its score, Claude Code installed and signed in from here, Ollama installed or updated from here and a
//! model set up in one go; and Settings › Background › Keep always running. The installers (OllamaSetup,
//! ClaudeCodeSetup) and Shell are this file's free functions; off Windows they say so, as Swift's `#if os(Windows)`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use ihatecrypto_core::foundation::Uuid;
use ihatecrypto_core::model_exam::ModelExam;
use llm::Credential;
use llm::catalog;
use serde::Serialize;
use serde_json::Value;
use wallet::WalletError;

use crate::chat_turns::OLLAMA;
use crate::engine::{Engine, Request};
use crate::from_core::ExamResult;

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

impl Engine {
    pub fn local_setup_view(&self) -> LocalSetupView {
        let claude_found = self.provider("claude-code").and_then(|p| p.existing_login()).is_some();
        let ollama_state = self.ollama_state();
        let st = self.st();
        let scores = st.settings.model_scores.iter().flatten().map(|(k, r)| {
            (k.clone(), Score { passed: r.passed as i64, total: r.total as i64, seconds: r.seconds, good: r.good(), short: r.short(), misses: r.misses.clone() })
        }).collect();
        let s = &st.local_setup;
        LocalSetupView {
            thinking: st.settings.thinking == Some(true), exam_progress: s.exam_progress, exam_total: ModelExam::questions().len() as i64,
            exam_for: s.exam_for.clone(), status: s.status.clone(), scores, ollama_version: s.ollama_version.clone(), ollama_latest: s.ollama_latest.clone(),
            ollama_state, ollama_outdated: s.ollama_outdated, ollama_install: s.ollama_install.clone(), claude_install: s.claude_install.clone(),
            claude_found, keep_running: st.settings.keep_running,
        }
    }

    pub async fn local_op(&self, r: &Request) -> Result<HashMap<String, String>, WalletError> {
        let on = r.test == Some(true);
        match r.op.as_str() {
            "thinking" => self.set_settings(|s| s.thinking = Some(on)),
            "checkModel" => {
                let (me, provider, model) = (self.rc(), r.provider.clone(), r.model.clone());
                tokio::task::spawn_local(async move {
                    let id = provider.unwrap_or_else(|| me.provider_id());
                    me.check_model(&id, model.as_deref()).await
                });
            }
            "installClaudeCode" => { let me = self.rc(); tokio::task::spawn_local(async move { me.install_claude_code().await }); }
            "claudeSignIn" => {
                let Some(Credential::External { path }) = self.provider("claude-code").and_then(|p| p.existing_login()) else {
                    return Err(WalletError(self.say("Install Claude Code first.", "Сначала установи Claude Code.")));
                };
                claude_code_sign_in(&path)?;
            }
            "installOllama" => { let me = self.rc(); tokio::task::spawn_local(async move { me.install_ollama().await }); }
            "ollamaStatus" => self.read_ollama_versions().await,
            "setUpLocal" => { let me = self.rc(); tokio::task::spawn_local(async move { me.set_up_local().await }); }
            // The wish goes to the app, which keeps Windows' scheduled task and says back how Windows has it.
            "keepRunning" => self.out.event("keepRunning", &HashMap::from([("on", if on { "1" } else { "0" })])),
            "keepRunningState" => self.set_settings(|s| s.keep_running = Some(on)),
            _ => {}
        }
        Ok(HashMap::new())
    }

    // MARK: Check

    /// The chat's exam: the requests people send, read by this account's model; 8 of 10 or more is good for the chat
    /// (the Mac's checkModel). It spends a few of the account's requests, and only when Check is pressed.
    pub async fn check_model(&self, id: &str, asked: Option<&str>) {
        if self.st().local_setup.exam_progress.is_some() { return }
        let name = self.provider(id).map_or_else(|| id.to_string(), |p| p.info().name);
        let Some(client) = self.llm_client(Some(id)) else {
            let line = self.say(&format!("Connect {name} first."), &format!("Сначала подключи {name}."));
            self.st_mut().local_setup.status = Some(line);
            self.changed();
            return;
        };
        let model = match asked.filter(|m| !m.is_empty()) {
            Some(m) => m.to_string(),
            None if id == self.provider_id() => self.model(),
            None => self.provider(id).map(|p| p.info().default_model).unwrap_or_default(),
        };
        {
            let mut st = self.st_mut();
            st.local_setup.exam_progress = Some(0);
            st.local_setup.exam_for = Some(format!("{id}/{model}"));
            st.local_setup.status = None;
        }
        self.changed();
        let result = ModelExam::run(&client, &model, |n| {
            self.st_mut().local_setup.exam_progress = Some(n as i64);
            self.changed();
        }).await;
        let key = format!("{id}/{model}");
        let kept = result.clone();
        self.set_settings(|s| { s.model_scores.get_or_insert_default().insert(key, kept); });
        let line = Self::exam_line(&result, &catalog::display_name(&model), |e, r| self.say(e, r));
        {
            let mut st = self.st_mut();
            st.local_setup.exam_progress = None;
            st.local_setup.exam_for = None;
            st.local_setup.status = Some(line);
        }
        self.changed();
    }

    pub fn exam_line(r: &ExamResult, model: &str, say: impl Fn(&str, &str) -> String) -> String {
        let seconds = format!("{:.1}", r.seconds);
        let (passed, total) = (r.passed, r.total);
        if r.good() {
            return say(&format!("{model} understood {passed} of {total} requests, {seconds} s a reply — good for the chat."),
                       &format!("{model} понял {passed} из {total} запросов, {seconds} с на ответ — для чата подходит."));
        }
        say(&format!("{model} understood only {passed} of {total} — it will misread requests. A bigger model or a subscription reads better. Missed: "),
            &format!("{model} понял только {passed} из {total} — он будет путать запросы. Модель побольше или подписка читают лучше. Не понял: "))
            + &r.misses.iter().take(3).cloned().collect::<Vec<_>>().join(" · ")
    }

    // MARK: Claude Code

    /// Anthropic's own Windows installer, then the account connects to it; its sign-in runs in a terminal.
    pub async fn install_claude_code(&self) {
        if self.st().local_setup.claude_install.is_some() { return }
        self.st_mut().local_setup.claude_install = Some("…".into());
        self.changed();
        let step = |text: &str| {
            self.st_mut().local_setup.claude_install = Some(text.into());
            self.changed();
        };
        let line = match claude_code_install(&step).await {
            Ok(()) => self.say("Claude Code is installed. Sign in to your Claude account with «Sign in in a terminal» in Settings › AI model.",
                               "Claude Code установлен. Войди в аккаунт Claude кнопкой «Войти в терминале» в Настройках › Модель ИИ."),
            Err(e) => self.say("Claude Code wasn't installed: ", "Claude Code не установился: ") + &e.0,
        };
        self.reply(&line, None);
        self.st_mut().local_setup.claude_install = None;
        self.changed();
    }

    // MARK: Ollama

    /// Its version here and its newest release, for «Update» and the outdated state. GitHub is asked only once Ollama
    /// answers, and only until it has said (the Mac's refreshLocalModels): no Ollama, no request.
    pub async fn read_ollama_versions(&self) {
        let Some(version) = ollama_version().await else { return };
        let asked = {
            let mut st = self.st_mut();
            // An updated Ollama may take the model now.
            if st.local_setup.ollama_version.as_ref() != Some(&version) {
                st.local_setup.ollama_version = Some(version);
                st.local_setup.ollama_outdated = false;
            }
            st.local_setup.ollama_latest.is_some()
        };
        if !asked {
            let latest = ollama_latest().await;
            self.st_mut().local_setup.ollama_latest = latest;
        }
    }

    /// Older than its newest release (and not just updated), or a model said it's too old.
    pub fn needs_new_ollama(&self) -> bool {
        let st = self.st();
        let s = &st.local_setup;
        if s.ollama_outdated { return true }
        match (&s.ollama_version, &s.ollama_latest) {
            (Some(version), Some(latest)) if !s.ollama_updated => ollama_older(version, latest),
            _ => st.local_servers.get("ollama") == Some(&false) && !Engine::ollama_installed(),
        }
    }

    pub fn ollama_state(&self) -> String {
        if self.st().local_servers.get("ollama") == Some(&false) { return if Engine::ollama_installed() { "notRunning" } else { "noOllama" }.into() }
        let model = self.model();
        let (missing, outdated) = {
            let st = self.st();
            (st.live_models.get("ollama").map(|l| l.contains(&model)) == Some(false), st.local_setup.ollama_outdated)
        };
        // Too old for a model it tried, or older than its newest release while the model still has to be downloaded.
        if outdated || (missing && self.needs_new_ollama()) { return "outdated".into() }
        if missing || self.provider_id() != "ollama" { "missing" } else { "ready" }.into()
    }

    /// Ollama's newest release, checked by its signature before it runs, installed for this user; the chat says how it went.
    pub async fn install_ollama(&self) {
        if self.st().local_setup.ollama_install.is_some() { return }
        self.st_mut().local_setup.ollama_install = Some("…".into());
        self.changed();
        let step = |text: &str| {
            self.st_mut().local_setup.ollama_install = Some(text.into());
            self.changed();
        };
        let line = match ollama_install(&step).await {
            Ok(()) => {
                {
                    let mut st = self.st_mut();
                    st.local_setup.ollama_updated = true;
                    st.local_setup.ollama_outdated = false;
                }
                // The installer starts its app; the server takes a moment to answer after that.
                for _ in 0..10 {
                    let up = self.st().local_servers.get("ollama") == Some(&true);
                    if up { continue }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    self.refresh_models().await;
                }
                self.read_ollama_versions().await;
                let version = self.st().local_setup.ollama_version.clone().unwrap_or_default();
                self.say(&format!("Ollama {version} is ready. Download a model in Settings › AI model, or pick one in the model menu."),
                         &format!("Ollama {version} готова. Скачай модель в Настройках › Модель ИИ или выбери в меню моделей."))
            }
            Err(e) => self.say("Ollama wasn't installed: ", "Ollama не установилась: ") + &e.0,
        };
        self.reply(&line, None);
        self.st_mut().local_setup.ollama_install = None;
        self.changed();
    }

    /// «Set up on this PC» in one go: Ollama installed or updated and started, then the chosen model — or one this PC has,
    /// else the biggest suggested one that fits its memory — downloaded and picked (the Mac's setUpLocal).
    pub async fn set_up_local(&self) {
        self.refresh_models().await;
        self.read_ollama_versions().await;
        let up = || self.st().local_servers.get("ollama") == Some(&true);
        if !up() || self.needs_new_ollama() { self.install_ollama().await }
        if !up() { return }
        let have: Vec<String> = self.st().live_models.get("ollama").into_iter().flatten().filter(|m| catalog::chat_refusal(m).is_none()).cloned().collect();
        let current = (self.provider_id() == "ollama").then(|| self.model());
        if let Some(pick) = current.filter(|m| have.contains(m)).or_else(|| have.first().cloned()) {
            let _ = self.connect("ollama", None, Some(&pick)).await;
            return;
        }
        let Some(pick) = self.local_pick().or_else(|| self.provider("ollama").and_then(|p| p.info().models.last().cloned())) else { return };
        self.download_model(&pick).await;
        let there = self.st().live_models.get("ollama").is_some_and(|l| l.contains(&pick));
        if there { let _ = self.connect("ollama", None, Some(&pick)).await; }
    }
}

// MARK: The installers (Windows; elsewhere they say so)

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

/// `OllamaSetup.latest`: the newest release («0.35.1»), from GitHub.
pub async fn ollama_latest() -> Option<String> {
    let tag = json_field("https://api.github.com/repos/ollama/ollama/releases/latest", 10, "tag_name").await?;
    Some(tag.strip_prefix('v').unwrap_or(&tag).to_string())
}

/// `OllamaSetup.version`: the running server's version; None when it doesn't answer (nothing is started to ask).
pub async fn ollama_version() -> Option<String> { json_field(&format!("{OLLAMA}/api/version"), 3, "version").await }

/// A text field of a JSON answer, whatever its status (URLSession's data(for:)).
async fn json_field(url: &str, seconds: u64, field: &str) -> Option<String> {
    let answer = net::HTTP.get(url).timeout(Duration::from_secs(seconds)).send().await.ok()?.bytes().await.ok()?;
    serde_json::from_slice::<Value>(&answer).ok()?.get(field)?.as_str().map(str::to_string)
}

/// Get-AuthenticodeSignature's «Status|Subject»: a valid signature whose signer is Ollama's organization. Anything else
/// is refused before the installer runs, as the Mac checks its Developer ID team.
pub fn signed_by_ollama(line: &str) -> bool {
    let Some((status, subject)) = line.trim().split_once('|') else { return false };
    status == "Valid" && subject.split(',').map(str::trim).any(|f| {
        f.strip_prefix("O=").map(|o| o.trim_matches(|c| c == '"' || c == ' ')).is_some_and(|o| o == "Ollama" || o == "Ollama Inc" || o == "Ollama Inc.")
    })
}

/// `OllamaSetup.install`: its newest OllamaSetup.exe, its signature checked, its own Inno Setup for this user.
pub async fn ollama_install(step: &dyn Fn(&str)) -> Result<(), WalletError> {
    if !cfg!(windows) { return Err(WalletError("Ollama installs from the Windows app.".into())) }
    step("Downloading Ollama…");
    let setup = std::env::temp_dir().join(format!("OllamaSetup-{}.exe", Uuid::new()));
    download("https://github.com/ollama/ollama/releases/latest/download/OllamaSetup.exe", &setup, false).await?;
    let done = async {
        step("Checking Ollama's signature…");
        let path = setup.to_string_lossy();
        let signature = powershell(&format!("$s = Get-AuthenticodeSignature -LiteralPath '{}'; $s.Status.ToString() + '|' + $s.SignerCertificate.Subject",
                                            path.replace('\'', "''"))).await?;
        if !signed_by_ollama(&signature) { return Err(WalletError("The downloaded Ollama isn't signed by Ollama — nothing was installed.".into())) }
        step("Installing Ollama…");
        // Its own Inno Setup, for this user (no administrator), with no window of its own.
        run(&path, &["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"]).await.map(drop)
    }.await;
    let _ = std::fs::remove_file(&setup);
    done
}

/// Claude Code's installer only from Anthropic's: the download must stay on claude.ai's hosts after its redirects.
pub fn from_claude(host: Option<&str>) -> bool { host.is_some_and(|h| h == "claude.ai" || h.ends_with(".claude.ai")) }

/// `ClaudeCodeSetup.install`: Anthropic's own installer (claude.ai/install.ps1, into %USERPROFILE%\.local\bin, no
/// administrator), run by PowerShell (the Mac's ClaudeCodeInstaller).
pub async fn claude_code_install(step: &dyn Fn(&str)) -> Result<(), WalletError> {
    if !cfg!(windows) { return Err(WalletError("Claude Code installs from the Windows app here.".into())) }
    step("Downloading Claude Code's installer…");
    // PowerShell runs a file only with its extension.
    let script = std::env::temp_dir().join(format!("claude-install-{}.ps1", Uuid::new()));
    download("https://claude.ai/install.ps1", &script, true).await?;
    step("Installing Claude Code…");
    let done = run(&powershell_path(), &["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &script.to_string_lossy()]).await.map(drop);
    let _ = std::fs::remove_file(&script);
    done
}

/// `ClaudeCodeSetup.signIn`: a terminal on `claude auth login`, the browser sign-in Anthropic's CLI runs itself.
pub fn claude_code_sign_in(path: &str) -> Result<(), WalletError> {
    if !cfg!(windows) { return Err(WalletError("Claude Code signs in from the Windows app here.".into())) }
    std::process::Command::new(format!("{}\\System32\\cmd.exe", system_root()))
        .args(["/c", "start", "Claude Code", "cmd", "/k", path, "auth", "login"]).spawn().map(drop).map_err(|e| WalletError(e.to_string()))
}

/// `url` saved to `to`; `claude_only`: refused, with nothing written, unless its redirects ended on claude.ai's hosts.
async fn download(url: &str, to: &Path, claude_only: bool) -> Result<(), WalletError> {
    use std::io::Write;
    let io = |e: &dyn std::fmt::Display| WalletError(e.to_string());
    // An installer is big: the client's 30 s stands for a whole hour here.
    let mut answer = net::HTTP.get(url).timeout(Duration::from_secs(3600)).send().await.map_err(|e| io(&e))?;
    if claude_only && !from_claude(answer.url().host_str()) {
        return Err(WalletError("The installer didn't come from claude.ai — nothing was installed.".into()));
    }
    let mut file = std::fs::File::create(to).map_err(|e| io(&e))?;
    let written = async {
        while let Some(chunk) = answer.chunk().await.map_err(|e| io(&e))? { file.write_all(&chunk).map_err(|e| io(&e))? }
        Ok(())
    }.await;
    if written.is_err() { let _ = std::fs::remove_file(to); }
    written
}

// MARK: Shell

fn system_root() -> String { std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()) }

pub fn powershell_path() -> String { format!("{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe", system_root()) }

pub async fn powershell(command: &str) -> Result<String, WalletError> {
    run(&powershell_path(), &["-NoProfile", "-NonInteractive", "-Command", command]).await
}

/// `Shell.run`: a tool to its end: what it printed, or its last line as the error. Its window stays hidden, and it reads
/// nothing from the engine's input (the window's requests).
pub async fn run(tool: &str, args: &[&str]) -> Result<String, WalletError> {
    let mut command = tokio::process::Command::new(tool);
    command.args(args).stdin(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = command.output().await.map_err(|e| WalletError(e.to_string()))?;
    let text = String::from_utf8_lossy(&[out.stdout, out.stderr].concat()).into_owned();
    if out.status.success() { return Ok(text) }
    Err(WalletError(text.lines().rfind(|l| !l.is_empty()).map_or_else(|| format!("{tool} failed"), str::to_string)))
}
