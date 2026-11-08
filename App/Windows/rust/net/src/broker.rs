//! The contained engine's way out. On Windows the window starts the engine in an AppContainer (winsec::sandbox) with two
//! pipes of their own, named in IHATECRYPTO_BROKER, and does for it what the container doesn't allow, one fixed thing per
//! request (the window's broker.rs): it starts the signer and Claude Code and hands over only their pipes, listens on
//! 127.0.0.1 for the engine's pages and sign-ins, reads the Codex login the person picked. A request is one JSON line
//! with an `id`; its answer is the line with the same `id`, an `error` when refused. A line with `page` is a browser's
//! request to one of the engine's loopback pages (`serve`). Without IHATECRYPTO_BROKER — tests, a run outside the
//! container — `on()` is false and the callers do the thing themselves, as before.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// A browser's request to a page the engine serves through the window: the connection's number and its bytes.
pub type PageRequests = tokio::sync::mpsc::UnboundedReceiver<(u64, Vec<u8>)>;

pub struct Broker {
    out: Mutex<Box<dyn Write + Send>>,
    waiting: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    pages: Mutex<HashMap<u64, tokio::sync::mpsc::UnboundedSender<(u64, Vec<u8>)>>>,
    next: AtomicU64,
    /// What `find` last heard, and when: the model menu asks for each provider on every state.
    found: Mutex<HashMap<String, (bool, Instant)>>,
}

impl Broker {
    /// The broker on these two pipes; a thread reads the answers until the window closes its end.
    pub fn new(answers: impl Read + Send + 'static, requests: impl Write + Send + 'static) -> &'static Broker {
        let broker: &'static Broker = Box::leak(Box::new(Broker {
            out: Mutex::new(Box::new(requests)), waiting: Mutex::new(HashMap::new()), pages: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1), found: Mutex::new(HashMap::new()),
        }));
        std::thread::spawn(move || {
            for line in BufReader::new(answers).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if let Some(port) = v["page"].as_u64() {
                    let (conn, data) = (v["conn"].as_u64().unwrap_or(0), unhex(v["data"].as_str().unwrap_or("")));
                    if let Some(page) = broker.pages.lock().expect("pages").get(&port) { let _ = page.send((conn, data)); }
                } else if let Some(id) = v["id"].as_u64() {
                    let waiter = broker.waiting.lock().expect("waiting").remove(&id);
                    match waiter {
                        Some(waiter) => { let _ = waiter.send(v); }
                        // Its asker gave up: the pipes and processes the window handed over are closed, so a signer or a
                        // Claude Code run started for it ends instead of waiting on a pipe no one reads.
                        None => for key in v["handles"].as_array().into_iter().flatten().filter_map(Value::as_str) { drop(handle(&v, key)) },
                    }
                }
            }
            // The window is gone: every request still waiting gets no answer.
            broker.waiting.lock().expect("waiting").clear();
            broker.pages.lock().expect("pages").clear();
        });
        broker
    }

    /// Asks the window and waits at most `wait` for its answer: the answer, or why there's none.
    pub fn call(&self, mut request: Value, wait: Duration) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        request["id"] = id.into();
        let (tx, rx) = mpsc::channel();
        self.waiting.lock().expect("waiting").insert(id, tx);
        let answer = self.write(&request).and_then(|_| rx.recv_timeout(wait).map_err(|_| "The window didn't answer.".to_string()));
        self.waiting.lock().expect("waiting").remove(&id);
        let answer = answer?;
        match answer["error"].as_str() { Some(e) => Err(e.to_string()), None => Ok(answer) }
    }

    /// Tells the window something that has no answer (a page's reply).
    pub fn tell(&self, request: Value) { let _ = self.write(&request); }

    fn write(&self, request: &Value) -> Result<(), String> {
        let mut out = self.out.lock().expect("out");
        writeln!(out, "{request}").and_then(|_| out.flush()).map_err(|_| "The window is gone.".to_string())
    }

    /// Whether the window finds `what` on this PC («claude», «codex», «ollama»): the container can't look itself.
    pub fn find(&self, what: &str) -> bool {
        if let Some((found, at)) = self.found.lock().expect("found").get(what) && at.elapsed() < Duration::from_secs(3) { return *found }
        let found = self.call(json!({ "op": "find", "what": what }), Duration::from_secs(5)).is_ok_and(|v| v["found"] == true);
        self.found.lock().expect("found").insert(what.into(), (found, Instant::now()));
        found
    }

    /// A page on 127.0.0.1:`port` (0: any free one), its listener in the window: the port, and the browser's requests.
    pub fn serve(&self, port: u16) -> Result<(u16, PageRequests), String> {
        let port = self.call(json!({ "op": "serve", "port": port }), Duration::from_secs(5))?["port"].as_u64().ok_or("No port.")?;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.pages.lock().expect("pages").insert(port, tx);
        Ok((port as u16, rx))
    }

    /// The answer to the browser's request `conn`.
    pub fn reply(&self, conn: u64, data: &[u8]) { self.tell(json!({ "op": "pageReply", "conn": conn, "data": hex(data) })) }

    /// The page's listener closed.
    pub fn unserve(&self, port: u16) {
        self.pages.lock().expect("pages").remove(&(port as u64));
        self.tell(json!({ "op": "unserve", "port": port }));
    }
}

static GLOBAL: OnceLock<Option<&'static Broker>> = OnceLock::new();

/// The engine's broker, when the window started it in its container.
pub fn get() -> Option<&'static Broker> {
    *GLOBAL.get_or_init(|| {
        let text = std::env::var("IHATECRYPTO_BROKER").ok()?;
        let (requests, answers) = text.split_once(',')?;
        let (requests, answers) = (requests.parse::<usize>().ok()?, answers.parse::<usize>().ok()?);
        let (requests, answers) = unsafe { (pipe_end(requests)?, pipe_end(answers)?) };
        Some(Broker::new(answers, requests))
    })
}

/// Whether the engine runs in its container, with the window as its broker.
pub fn on() -> bool { get().is_some() }

#[cfg(windows)]
unsafe fn pipe_end(handle: usize) -> Option<std::fs::File> {
    use std::os::windows::io::FromRawHandle;
    (handle != 0).then(|| unsafe { std::fs::File::from_raw_handle(handle as _) })
}

#[cfg(unix)]
unsafe fn pipe_end(fd: usize) -> Option<std::fs::File> {
    use std::os::unix::io::FromRawFd;
    i32::try_from(fd).ok().map(|fd| unsafe { std::fs::File::from_raw_fd(fd) })
}

/// A pipe end or process handle the window handed over, by its number in an answer.
pub fn handle(answer: &Value, key: &str) -> Option<std::fs::File> {
    let n = answer[key].as_u64()? as usize;
    unsafe { pipe_end(n) }
}

/// A process the window started for the engine (Claude Code): only waited for, asked how it ended, or ended.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Process(std::fs::File);

#[cfg(windows)]
mod process {
    use std::os::windows::io::AsRawHandle;
    unsafe extern "system" {
        fn WaitForSingleObject(handle: *mut core::ffi::c_void, ms: u32) -> u32;
        fn GetExitCodeProcess(handle: *mut core::ffi::c_void, code: *mut u32) -> i32;
        fn TerminateProcess(handle: *mut core::ffi::c_void, code: u32) -> i32;
    }

    impl super::Process {
        /// Waits for it to end: its exit code.
        pub fn wait(&self) -> i32 {
            let h = self.0.as_raw_handle();
            let mut code = 0u32;
            unsafe { WaitForSingleObject(h, u32::MAX); GetExitCodeProcess(h, &mut code); }
            code as i32
        }
        pub fn kill(&self) { unsafe { TerminateProcess(self.0.as_raw_handle(), 1); } }
    }
}

#[cfg(not(windows))]
mod process {
    impl super::Process {
        pub fn wait(&self) -> i32 { -1 }
        pub fn kill(&self) {}
    }
}

impl Process {
    pub fn from_answer(answer: &Value, key: &str) -> Option<Process> { handle(answer, key).map(Process) }
}

pub fn hex(data: &[u8]) -> String { data.iter().map(|b| format!("{b:02x}")).collect() }

pub fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2).filter_map(|i| text.get(2 * i..2 * i + 2).and_then(|p| u8::from_str_radix(p, 16).ok())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window's side, played here: it answers `find` and `serve`, and sends one browser request to the page.
    #[test]
    fn requests_meet_their_answers_and_pages_their_requests() {
        let (answers_read, mut answers_write) = std::io::pipe().unwrap();
        let (requests_read, requests_write) = std::io::pipe().unwrap();
        let broker = Broker::new(answers_read, requests_write);
        // A browser comes once the page listens: after serve() has its port and its page. Sent right after the port, the
        // request could be read first, find no page and be dropped, and the test waited forever (CI, 2026-11-08).
        let (listening, browser) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            for line in BufReader::new(requests_read).lines() {
                let v: Value = serde_json::from_str(&line.unwrap()).unwrap();
                let id = v["id"].clone();
                let answer = match v["op"].as_str() {
                    Some("find") => json!({ "id": id, "found": v["what"] == "claude" }),
                    Some("serve") => {
                        writeln!(answers_write, "{}", json!({ "id": id, "port": 4242 })).unwrap();
                        browser.recv().unwrap();
                        json!({ "page": 4242, "conn": 7, "data": hex(b"GET / HTTP/1.1\r\n\r\n") })
                    }
                    Some("pageReply") => json!({ "id": 99, "echo": String::from_utf8(unhex(v["data"].as_str().unwrap())).unwrap() }),
                    _ => json!({ "id": id, "error": "unknown" }),
                };
                writeln!(answers_write, "{answer}").unwrap();
            }
        });
        assert!(broker.find("claude"));
        assert!(!broker.find("ollama"));
        assert_eq!(broker.call(json!({ "op": "nope" }), Duration::from_secs(2)).unwrap_err(), "unknown");
        let (port, mut pages) = broker.serve(0).unwrap();
        listening.send(()).unwrap();
        assert_eq!(port, 4242);
        let (conn, data) = pages.blocking_recv().unwrap();
        assert_eq!((conn, data.as_slice()), (7, &b"GET / HTTP/1.1\r\n\r\n"[..]));
        assert_eq!(unhex(&hex(&[0, 1, 0xfe, 0xff])), vec![0, 1, 0xfe, 0xff]);
    }
}
