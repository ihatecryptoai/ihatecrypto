//! The engine in its AppContainer, and its broker (the Mac's Helper.xpc). The window starts the engine in a container
//! (winsec::sandbox) where it reaches the internet but not the person's files, the signer's folder, the autostart keys
//! or Task Scheduler, may start no program and takes no connection from the person's browser. What it still needs it
//! asks here, on two pipes only it and this window hold — no other program can write to them — and each request does
//! one fixed thing:
//!
//! - `signer`: the signer beside the engine, started hardened as the engine started it; its pipes handed over.
//! - `claude`: the installed Claude Code with no tools, its arguments built here from a checked model name, the system
//!   prompt and the effort; the prompt goes through the pipe the engine gets.
//! - `find`: whether Claude Code, Codex CLI's login or Ollama is on this PC — yes or no.
//! - `codex`: the tokens of the Codex CLI login the person picked once in this window's Open dialog, without the
//!   refresh token.
//! - `serve`, `unserve`, `pageReply`: a listener on 127.0.0.1 for the engine's Top up page and its sign-in redirects,
//!   each request handed over whole and the engine's answer given back.
//! - `claudeInstall`, `claudeSignIn`, `ollamaStart`, `reveal`, `locked`, `hello`, `ledger`: Anthropic's installer,
//!   `claude auth login` in a terminal, Ollama's app, the study notes or journal in Explorer, whether the session is
//!   locked, the app lock's Windows Hello, a Ledger's USB handle.
//!
//! The engine's data folder is in the container (its LOCALAPPDATA there); the folder from before it is copied in once.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::io::{AsHandle, AsRawHandle, OwnedHandle, RawHandle};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::Manager;
use winsec::sandbox;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE: a process the engine may only wait for or end.
const WAIT_AND_END: u32 = 0x0001 | 0x1000 | 0x0010_0000;

/// The engine started in its container.
pub struct Started {
    pub input: File,
    pub output: File,
    pub process: Arc<OwnedHandle>,
}

struct Broker {
    app: tauri::AppHandle,
    engine: Arc<OwnedHandle>,
    answers: Mutex<File>,
    /// The engine's folder and its own program's folder, where the signer is.
    data: PathBuf,
    bin: PathBuf,
    /// Pages' listeners by port (cleared to stop one), and the browser requests waiting for the engine's answer.
    serving: Mutex<HashMap<u16, Arc<AtomicBool>>>,
    replies: Mutex<HashMap<u64, mpsc::Sender<Vec<u8>>>>,
    next: AtomicU64,
    /// One Open dialog at a time: a second ask for the Codex login waits for the first's pick.
    picking: Mutex<()>,
    /// What runs for the engine now — requests, Claude Code runs, Anthropic's installer — each with a ceiling, so a
    /// broken engine can't fill the PC with them.
    working: AtomicUsize,
    claudes: Arc<AtomicUsize>,
    installing: Mutex<()>,
}

/// At most this many requests at once, Claude Code runs at once, pages listened for at once, and bytes in one request.
const REQUESTS: usize = 32;
const CLAUDES: usize = 3;
const PAGES: usize = 4;
const LINE: u64 = 4 << 20;

/// The container's name: the app's identifier, so a development copy (its own identifier) has its own container.
fn container_name(app: &tauri::AppHandle) -> String { format!("{}.engine", app.config().identifier) }

/// The engine's product folder name, as its store names it.
const PRODUCT: &str = if cfg!(debug_assertions) { "ihatecrypto-dev" } else { "ihatecrypto" };

pub fn start(app: &tauri::AppHandle, exe: &Path) -> std::io::Result<Started> {
    let refused = |what: &str| std::io::Error::other(format!("the engine's container: {what}"));
    let sid = sandbox::container(&container_name(app)).ok_or_else(|| refused("Windows didn't make it"))?;
    let home = sandbox::folder(&sid).ok_or_else(|| refused("no folder"))?;
    // A development run keeps the engine's files where IHATECRYPTO_ENGINE_DIR says (the engine reads it in debug
    // builds only), which the container is let into; else they're in the container's own folder.
    let data = match std::env::var_os("IHATECRYPTO_ENGINE_DIR").filter(|_| cfg!(debug_assertions)) {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&dir);
            sandbox::grant(&dir, &sid, true);
            dir
        }
        None => {
            let data = home.join(PRODUCT).join("engine");
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
            // Not started on an empty folder while the old one is still there: it would be taken as moved already.
            migrate(&local.join(PRODUCT).join("engine"), &data)
                .map_err(|e| refused(&format!("the engine's files didn't move into it ({e}); they're where they were")))?;
            data
        }
    };
    let bin = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    // Program Files lets every container read and run already; a development build's folder is let in here.
    sandbox::grant(&bin, &sid, false);
    // Windows gives the engine `home` as its LOCALAPPDATA and its Temp as TEMP, made from this window's own LOCALAPPDATA:
    // named here they'd be made from `home`, «…\AC\Packages\<name>\AC», and the engine would start empty beside its files.
    let engine = sandbox::spawn(exe, &[], &sid)?;
    let process = Arc::new(engine.process);
    let broker = Arc::new(Broker {
        app: app.clone(), engine: process.clone(), answers: Mutex::new(engine.answers), data, bin,
        serving: Mutex::new(HashMap::new()), replies: Mutex::new(HashMap::new()), next: AtomicU64::new(1), picking: Mutex::new(()),
        working: AtomicUsize::new(0), claudes: Arc::new(AtomicUsize::new(0)), installing: Mutex::new(()),
    });
    let mut requests = BufReader::new(engine.requests);
    std::thread::spawn(move || loop {
        // One line, 4 MB at most: a longer one ends the broker (the engine is broken), as its pipe closing does.
        let mut line = Vec::new();
        match (&mut requests).take(LINE).read_until(b'\n', &mut line) { Ok(n) if n > 0 && line.ends_with(b"\n") => {}, _ => break }
        let Ok(request) = serde_json::from_slice::<Value>(&line) else { continue };
        let broker = broker.clone();
        if broker.working.fetch_add(1, Ordering::SeqCst) >= REQUESTS {
            broker.working.fetch_sub(1, Ordering::SeqCst);
            if let Some(id) = request["id"].as_u64() { broker.send(&json!({ "id": id, "error": "The window is busy with the engine's other requests." })) }
            continue;
        }
        // Each on its own thread: Claude Code's installer takes minutes, a sign-in waits for the browser.
        std::thread::spawn(move || {
            let me = broker.clone();
            broker.handle(request);
            me.working.fetch_sub(1, Ordering::SeqCst);
        });
    });
    Ok(Started { input: engine.stdin, output: engine.stdout, process })
}

/// The engine's folder from before its container copied into it once; the old one is then renamed «engine-before-
/// container» and kept. Copied file by file, not moved: a moved file keeps the rules it had, which don't let the
/// container in; a new one takes its folder's. An error leaves the old folder as it was, for the next start to try again.
fn migrate(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.exists() || !from.is_dir() { return Ok(()) }
    let moving = to.with_extension("moving");
    let _ = std::fs::remove_dir_all(&moving);
    copy(from, &moving)?;
    std::fs::rename(&moving, to)?;
    let _ = std::fs::rename(from, from.with_file_name("engine-before-container"));
    Ok(())
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() { copy(&entry.path(), &to.join(entry.file_name()))? }
        else if kind.is_file() { std::fs::write(to.join(entry.file_name()), std::fs::read(entry.path())?)? }
    }
    Ok(())
}

impl Broker {
    fn handle(self: &Arc<Self>, request: Value) {
        let op = request["op"].as_str().unwrap_or("");
        let answer = match op {
            "signer" => self.signer(),
            "claude" => self.claude(&request),
            "find" => Ok(json!({ "found": find(request["what"].as_str().unwrap_or(""), &self.app) })),
            "codex" => self.codex(),
            "serve" => self.serve(request["port"].as_u64().unwrap_or(0)),
            "unserve" => {
                if let Some(stop) = self.serving.lock().expect("serving").remove(&(request["port"].as_u64().unwrap_or(0) as u16)) { stop.store(true, Ordering::SeqCst) }
                return;
            }
            "pageReply" => {
                let conn = request["conn"].as_u64().unwrap_or(0);
                if let Some(reply) = self.replies.lock().expect("replies").remove(&conn) { let _ = reply.send(unhex(request["data"].as_str().unwrap_or(""))); }
                return;
            }
            "claudeInstall" => match self.installing.try_lock() { Ok(_one) => claude_install(), Err(_) => Err("Claude Code is being installed already.".into()) },
            "claudeSignIn" => claude_sign_in(),
            "ollamaStart" => ollama_start(),
            "reveal" => self.reveal(request["what"].as_str().unwrap_or("")),
            "locked" => Ok(json!({ "locked": winsec::winsec_session_locked() == 1 })),
            "hello" => Ok(json!({ "hello": hello(request["reason"].as_str().unwrap_or("")) })),
            "ledger" => self.ledger(request["open"] == true),
            _ => Err(format!("The window doesn't do «{op}».")),
        };
        // A request without an id wants no answer.
        let Some(id) = request["id"].as_u64() else { return };
        let mut line = match answer { Ok(mut v) => { v["id"] = id.into(); v } Err(e) => json!({ "id": id, "error": e }) };
        if line.is_null() { line = json!({ "id": id }) }
        self.send(&line);
    }

    fn send(&self, line: &Value) {
        let mut answers = self.answers.lock().expect("answers");
        let _ = writeln!(answers, "{line}").and_then(|_| answers.flush());
    }

    /// `handle` to the engine, closed here: its number there.
    fn give(&self, handle: OwnedHandle, access: Option<u32>) -> Result<u64, String> {
        sandbox::give(self.engine.as_raw_handle(), handle, access).map(|n| n as u64).ok_or_else(|| "Windows refused to hand it over.".to_string())
    }

    fn signer(&self) -> Result<Value, String> {
        let exe = self.bin.join("ihatecrypto-signer.exe");
        let path = exe.to_string_lossy();
        let (mut pid, mut process, mut to_child, mut from_child) = (0u32, 0isize, 0isize, 0isize);
        let code = unsafe { winsec::winsec_spawn_signer(path.as_ptr(), path.len(), &mut pid, &mut process, &mut to_child, &mut from_child) };
        if code != 0 { return Err(format!("The signer didn't start: {} is missing or Windows refused it.", exe.display())) }
        // SAFETY: winsec hands out three fresh handles nothing else owns; all three owned before any goes, so none leaks.
        let owned = |h: isize| unsafe { <OwnedHandle as std::os::windows::io::FromRawHandle>::from_raw_handle(h as RawHandle) };
        let (stdin, stdout, process) = (owned(to_child), owned(from_child), owned(process));
        Ok(json!({ "pid": pid, "stdin": self.give(stdin, None)?, "stdout": self.give(stdout, None)?, "process": self.give(process, None)?,
                   "handles": ["stdin", "stdout", "process"] }))
    }

    fn claude(&self, request: &Value) -> Result<Value, String> {
        let model = request["model"].as_str().unwrap_or("");
        if !model_name(model) { return Err(format!("Not a Claude model: {model}")) }
        let path = claude_path().ok_or("Claude Code isn't installed.")?;
        if self.claudes.fetch_add(1, Ordering::SeqCst) >= CLAUDES {
            self.claudes.fetch_sub(1, Ordering::SeqCst);
            return Err("Claude Code is answering the engine's other requests: try again in a moment.".into());
        }
        let claudes = self.claudes.clone();
        let done = move |system: &Path| { let _ = std::fs::remove_file(system); claudes.fetch_sub(1, Ordering::SeqCst); };
        // The system prompt in a file of this window's (the container can't write its Temp): the chat's is longer than
        // Windows' 32,767-character command line.
        let system = std::env::temp_dir().join(format!("ihatecrypto-system-{}-{}.txt", std::process::id(), self.next.fetch_add(1, Ordering::SeqCst)));
        if let Err(e) = std::fs::write(&system, request["system"].as_str().unwrap_or("")) { done(&system); return Err(e.to_string()) }
        let started = Command::new(path).args(claude_arguments(model, &system, request["effort"] == true))
            .current_dir(std::env::temp_dir()).env_remove("CLAUDECODE").env_remove("CLAUDE_CODE_ENTRYPOINT").env("DISABLE_AUTOUPDATER", "1")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).creation_flags(CREATE_NO_WINDOW)
            .spawn();
        let mut child = match started {
            Ok(child) => child,
            Err(e) => { done(&system); return Err(format!("Claude Code didn't start: {e}")) }
        };
        let handed = (|| -> Result<Value, String> {
            let process = child.as_handle().try_clone_to_owned().map_err(|e| e.to_string())?;
            let (stdin, stdout, stderr) = (child.stdin.take().ok_or("no pipe")?, child.stdout.take().ok_or("no pipe")?, child.stderr.take().ok_or("no pipe")?);
            let (stdin, stdout, stderr): (OwnedHandle, OwnedHandle, OwnedHandle) = (stdin.into(), stdout.into(), stderr.into());
            Ok(json!({ "pid": child.id(), "stdin": self.give(stdin, None)?, "stdout": self.give(stdout, None)?, "stderr": self.give(stderr, None)?,
                       "process": self.give(process, Some(WAIT_AND_END))?, "handles": ["stdin", "stdout", "stderr", "process"] }))
        })();
        // Not handed over whole: ended here, as the engine couldn't.
        if handed.is_err() { let _ = child.kill(); }
        // Its file goes, and its place, once it has ended.
        std::thread::spawn(move || { let _ = child.wait(); done(&system); });
        handed
    }

    fn codex(&self) -> Result<Value, String> {
        let _one = self.picking.lock().unwrap_or_else(|e| e.into_inner());
        let kept = picked_file(&self.app);
        let path = match std::fs::read_to_string(&kept).ok().map(|p| PathBuf::from(p.trim())).filter(|p| p.is_file()) {
            Some(p) => p,
            None => {
                let owner = self.app.get_webview_window("main").filter(|w| w.is_visible().unwrap_or(false)).and_then(|w| w.hwnd().ok());
                let picked = pick_codex_login(owner).ok_or("Pick Codex CLI's login file (auth.json) in the dialog to use it here.")?;
                if let Some(dir) = kept.parent() { let _ = std::fs::create_dir_all(dir); }
                let _ = std::fs::write(&kept, picked.to_string_lossy().as_bytes());
                picked
            }
        };
        let file: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?).map_err(|_| "Codex CLI isn't signed in with ChatGPT.")?;
        Ok(json!({ "text": codex_tokens(&file).to_string() }))
    }

    fn serve(self: &Arc<Self>, port: u64) -> Result<Value, String> {
        // Any free port, or 1455: Codex CLI's sign-in sends the browser back to localhost:1455 only.
        if port != 0 && port != 1455 { return Err(format!("Not a port the window listens on: {port}")) }
        if self.serving.lock().expect("serving").len() >= PAGES { return Err("The window listens for enough pages already.".into()) }
        let listener = std::net::TcpListener::bind(("127.0.0.1", port as u16)).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let stop = Arc::new(AtomicBool::new(false));
        self.serving.lock().expect("serving").insert(port, stop.clone());
        let me = self.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((socket, _)) => { let me = me.clone(); std::thread::spawn(move || me.relay(port, socket)); }
                    Err(_) => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        });
        Ok(json!({ "port": port }))
    }

    /// One browser request, read whole (64 KB, 5 s at most), to the engine; its answer back, and the connection closed.
    fn relay(&self, port: u16, mut socket: std::net::TcpStream) {
        let _ = socket.set_nonblocking(false);
        let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
        let (mut data, mut buffer) = (Vec::new(), [0u8; 16_384]);
        let started = Instant::now();
        while !whole(&data) {
            if data.len() > 65_536 || started.elapsed() > Duration::from_secs(5) { return }
            match socket.read(&mut buffer) { Ok(n) if n > 0 => data.extend_from_slice(&buffer[..n]), _ => return }
        }
        let conn = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.replies.lock().expect("replies").insert(conn, tx);
        self.send(&json!({ "page": port, "conn": conn, "data": hex(&data) }));
        let reply = rx.recv_timeout(Duration::from_secs(60));
        self.replies.lock().expect("replies").remove(&conn);
        if let Ok(reply) = reply { let _ = socket.write_all(&reply); }
        let _ = socket.shutdown(std::net::Shutdown::Both);
    }

    /// Settings › Skills' two places, named here from the engine's folder, never a path the engine sends. Explorer
    /// reads a folder's desktop.ini, a shortcut's icon and other files' thumbnails as the person, outside the container,
    /// so it's shown nothing else the engine wrote: the journal is copied into this window's folder and shown selected
    /// there; the notes folder opens only while it holds nothing but notes (.md, .txt) and no link to elsewhere.
    fn reveal(&self, what: &str) -> Result<Value, String> {
        let explorer = format!("{}\\explorer.exe", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));
        let spawned = match what {
            "journal" => {
                let journal = self.data.join("journal.json");
                if !plain_file(&journal) { return Err("No journal entries yet.".into()) }
                let shown = self.app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("Journal");
                std::fs::create_dir_all(&shown).map_err(|e| e.to_string())?;
                let copy = shown.join("journal.json");
                std::fs::copy(&journal, &copy).map_err(|e| e.to_string())?;
                Command::new(explorer).raw_arg(format!("/select,\"{}\"", copy.display())).spawn()
            }
            "study" => {
                let study = self.data.join("study");
                if !notes_only(&study) { return Err("The study folder holds something other than notes, so it wasn't opened.".into()) }
                Command::new(explorer).arg(study).spawn()
            }
            _ => return Err(format!("Nothing to show for «{what}».")),
        };
        spawned.map(|_| Value::Null).map_err(|e| e.to_string())
    }

    fn ledger(&self, open: bool) -> Result<Value, String> {
        let product = winsec::ledger::product();
        if !open { return Ok(json!({ "product": product })) }
        let Some(handle) = winsec::ledger::open_handle() else { return Ok(json!({ "product": product })) };
        Ok(json!({ "product": product, "handle": self.give(handle, None)?, "handles": ["handle"] }))
    }
}

/// The app lock's Windows Hello (the engine's lock.rs): a signature with the unlock key named here, the engine's words
/// shown beside the dialog (cut to 200 characters). Only how it went goes back: the signature says only that Windows
/// Hello saw the person, and nothing is kept from it, so the challenge needn't change.
fn hello(reason: &str) -> &'static str {
    const UNLOCK_KEY: &str = if cfg!(debug_assertions) { "ihatecrypto-dev-unlock" } else { "ihatecrypto-unlock" };
    let reason: String = reason.chars().take(200).collect();
    let challenge = b"ihatecrypto: open the app";
    let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
    let code = unsafe {
        winsec::winsec_hello_sign(UNLOCK_KEY.as_ptr(), UNLOCK_KEY.len(), challenge.as_ptr(), challenge.len(), reason.as_ptr(), reason.len(), 1, &mut out, &mut len)
    };
    if !out.is_null() { unsafe { winsec::winsec_free(out, len) } }
    match code { 0 => "signed", 1 => "cancelled", _ => "failed" }
}

/// A file, not a link (a symlink, a junction or any other reparse point) and not a folder.
fn plain_file(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
}

/// A real folder (no reparse point) holding only plain .md and .txt files.
fn notes_only(folder: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    let real = std::fs::symlink_metadata(folder).is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0);
    real && std::fs::read_dir(folder).is_ok_and(|entries| entries.into_iter().all(|e| e.is_ok_and(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        plain_file(&e.path()) && (name.ends_with(".md") || name.ends_with(".txt"))
    })))
}

/// A Claude model's name as the model menu has them: «claude-» and lowercase letters, digits, dots and dashes.
fn model_name(model: &str) -> bool {
    model.len() <= 64 && model.starts_with("claude-") && model.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// Headless, no tools, no MCP, nothing saved to session history, the system prompt replacing Claude Code's own; one JSON
/// event a line (llm's ClaudeCodeClient::arguments, which the engine uses outside a container).
fn claude_arguments(model: &str, system: &Path, effort: bool) -> Vec<String> {
    let mut args: Vec<String> = ["-p", "--restricted", "--strict-mcp-config", "--tools", "", "--no-session-persistence", "--output-format", "stream-json",
                                 "--verbose", "--include-partial-messages", "--model", model, "--system-prompt-file"]
        .iter().map(|s| s.to_string()).collect();
    args.push(system.display().to_string());
    if effort { args.extend(["--effort".into(), "high".into()]) }
    args
}

/// The installed claude.exe: Anthropic's installer puts it in %USERPROFILE%\.local\bin; else on PATH.
fn claude_path() -> Option<PathBuf> {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    std::iter::once(PathBuf::from(format!("{}\\.local\\bin\\claude.exe", env("USERPROFILE"))))
        .chain(env("PATH").split(';').filter(|d| !d.is_empty()).map(|d| Path::new(d).join("claude.exe")).collect::<Vec<_>>())
        .find(|p| p.is_file())
}

fn ollama_places() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let program = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    (vec![format!("{local}\\Programs\\Ollama\\ollama app.exe").into(), format!("{program}\\Ollama\\ollama app.exe").into()],
     vec![format!("{local}\\Programs\\Ollama\\ollama.exe").into(), format!("{program}\\Ollama\\ollama.exe").into()])
}

fn codex_default() -> PathBuf { Path::new(&std::env::var("USERPROFILE").unwrap_or_default()).join(".codex").join("auth.json") }

/// Where the window keeps which Codex login the person picked.
fn picked_file(app: &tauri::AppHandle) -> PathBuf {
    app.path().app_local_data_dir().unwrap_or_default().join("codex-login.txt")
}

fn find(what: &str, app: &tauri::AppHandle) -> bool {
    match what {
        "claude" => claude_path().is_some(),
        "codex" => codex_default().is_file() || std::fs::read_to_string(picked_file(app)).is_ok_and(|p| Path::new(p.trim()).is_file()),
        "ollama" => { let (apps, servers) = ollama_places(); apps.iter().chain(&servers).any(|p| p.exists()) }
        _ => false,
    }
}

/// What the engine needs of a Codex CLI login: the access token, the account and the id token. The refresh token stays
/// in the file — the engine never refreshes Codex CLI's tokens, and a refresh would log Codex CLI out.
fn codex_tokens(file: &Value) -> Value {
    let t = &file["tokens"];
    json!({ "tokens": { "access_token": t["access_token"], "account_id": t["account_id"], "id_token": t["id_token"] } })
}

/// The Open dialog for Codex CLI's login, in ~/.codex: the file the person picks, or None.
fn pick_codex_login(owner: Option<windows::Win32::Foundation::HWND>) -> Option<PathBuf> {
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::*;
    use windows::core::{HSTRING, w};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = dialog.SetTitle(w!("Pick Codex CLI's login (auth.json) for ihatecrypto"));
        let _ = dialog.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: w!("Codex CLI login (auth.json)"), pszSpec: w!("auth.json") }]);
        if let Some(dir) = codex_default().parent()
            && let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None) { let _ = dialog.SetFolder(&folder); }
        dialog.Show(owner).ok()?;
        let name = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const _));
        path.map(PathBuf::from)
    }
}

/// Anthropic's installer, the way claude.ai/install.ps1 is meant to run — only if the download stayed on claude.ai.
fn claude_install() -> Result<Value, String> {
    const SCRIPT: &str = "$ErrorActionPreference='Stop'; $r = Invoke-WebRequest -UseBasicParsing https://claude.ai/install.ps1; \
        $h = $r.BaseResponse.ResponseUri.Host; if (-not ($h -eq 'claude.ai' -or $h.EndsWith('.claude.ai'))) { throw 'The installer did not come from claude.ai.' }; \
        Invoke-Expression ([Text.Encoding]::UTF8.GetString($r.Content))";
    let powershell = format!("{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));
    let out = Command::new(powershell).args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])
        .stdin(Stdio::null()).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
    if out.status.success() { return Ok(Value::Null) }
    let text = String::from_utf8_lossy(&[out.stdout, out.stderr].concat()).into_owned();
    Err(text.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("The installer failed.").trim().to_string())
}

/// `claude auth login` in a terminal of its own.
fn claude_sign_in() -> Result<Value, String> {
    let claude = claude_path().ok_or("Install Claude Code first.")?;
    let cmd = format!("{}\\System32\\cmd.exe", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));
    Command::new(&cmd).args(["/c", "start", "Claude Code", &cmd, "/k"]).arg(claude).args(["auth", "login"]).spawn().map(|_| Value::Null).map_err(|e| e.to_string())
}

/// Ollama's tray app, else its server alone.
fn ollama_start() -> Result<Value, String> {
    let (apps, servers) = ollama_places();
    let started = match (apps.into_iter().find(|p| p.exists()), servers.into_iter().find(|p| p.exists())) {
        (Some(app), _) => Command::new(app).spawn(),
        (None, Some(server)) => Command::new(server).arg("serve").stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW).spawn(),
        (None, None) => return Err("Ollama isn't installed.".into()),
    };
    started.map(|_| Value::Null).map_err(|e| e.to_string())
}

/// An HTTP request's head and its Content-Length body are in.
fn whole(data: &[u8]) -> bool {
    let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") else { return false };
    let head = String::from_utf8_lossy(&data[..end]).to_lowercase();
    let length = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
    data.len() >= end + 4 + length
}

fn hex(data: &[u8]) -> String { data.iter().map(|b| format!("{b:02x}")).collect() }

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2).filter_map(|i| text.get(2 * i..2 * i + 2).and_then(|p| u8::from_str_radix(p, 16).ok())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_claude_models_and_fixed_arguments() {
        for good in ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "claude-fable-5-1"] { assert!(model_name(good), "{good}") }
        for bad in ["opus", "claude-opus 5", "claude-x\" --tools Bash", "claude-x;calc", "Claude-Opus", "--model", &format!("claude-{}", "a".repeat(64))] {
            assert!(!model_name(bad), "{bad}")
        }
        let args = claude_arguments("claude-opus-5-5", Path::new("C:\\t\\s.txt"), true);
        assert_eq!(&args[..6], ["-p", "--restricted", "--strict-mcp-config", "--tools", "", "--no-session-persistence"]);
        assert_eq!(&args[args.len() - 6..], ["--model", "claude-opus-5-5", "--system-prompt-file", "C:\\t\\s.txt", "--effort", "high"]);
    }

    #[test]
    fn the_codex_login_goes_without_its_refresh_token() {
        let file = json!({ "tokens": { "access_token": "a", "refresh_token": "r", "account_id": "acc", "id_token": "i" }, "OPENAI_API_KEY": "k" });
        let text = codex_tokens(&file).to_string();
        assert!(text.contains("\"access_token\":\"a\"") && text.contains("acc") && !text.contains("\"r\"") && !text.contains("refresh") && !text.contains("\"k\""));
    }

    #[test]
    fn a_request_is_whole_with_its_body() {
        assert!(!whole(b"GET / HTTP/1.1\r\nHost: x\r\n"));
        assert!(whole(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"));
        assert!(!whole(b"POST /a HTTP/1.1\r\nContent-Length: 4\r\n\r\nab"));
        assert!(whole(b"POST /a HTTP/1.1\r\nContent-Length: 4\r\n\r\nabcd"));
        assert_eq!(unhex(&hex(b"\x00\xffok")), b"\x00\xffok");
    }
}

#[cfg(test)]
mod container_tests {
    /// The installer's loopback rule names the release engine's container by the SID Windows derives from its name.
    #[test]
    fn the_installer_names_the_engine_container() {
        let sid = winsec::sandbox::container_sid("app.ihatecrypto.windows.engine").and_then(|s| s.text()).expect("a SID");
        assert!(include_str!("../windows/hooks.nsh").contains(&format!("\"{sid}\"")), "{sid}");
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], "app.ihatecrypto.windows");
    }
}
