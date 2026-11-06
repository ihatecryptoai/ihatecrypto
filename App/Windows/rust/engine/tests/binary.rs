//! The binary `ihatecrypto-engine` as the window runs it (main.swift): a `hello` line in, its answer and a state out, and
//! the end of its input ends it with 0. Offline: a data folder of the test's own, no signer beside the copy that runs,
//! every request through a proxy that isn't there. Unix only: on Windows the window's own tests start it.
#![cfg(not(windows))]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

#[test]
fn hello_is_answered_with_a_state_and_closed_input_ends_it() {
    let dir = std::env::temp_dir().join(format!("ihatecrypto-engine-bin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A copy alone in its folder: the signer that may sit beside the built one isn't started.
    let exe = dir.join("ihatecrypto-engine");
    std::fs::copy(env!("CARGO_BIN_EXE_ihatecrypto-engine"), &exe).unwrap();
    let mut child = Command::new(&exe)
        .env("IHATECRYPTO_ENGINE_DIR", dir.join("data"))
        .env("IHATECRYPTO_SIGNER_DIR", dir.join("signer"))
        .env("WINSEC_FAKE_HELLO", "1")
        .envs(["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "https_proxy", "http_proxy", "all_proxy"].map(|k| (k, "http://127.0.0.1:9")))
        .env_remove("NO_PROXY").env_remove("no_proxy")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || for line in BufReader::new(output).lines().map_while(Result::ok) { if tx.send(line).is_err() { break } });

    writeln!(input, "not a request").unwrap();
    writeln!(input, r#"{{"id":1,"op":"hello"}}"#).unwrap();
    let (mut answer, mut state) = (None, None);
    while answer.is_none() || state.is_none() {
        let line: Value = serde_json::from_str(&rx.recv_timeout(Duration::from_secs(60)).expect("a line from the engine")).unwrap();
        if line["id"] == 1 { answer = Some(line) } else if line["event"] == "state" && answer.is_some() { state = Some(line["data"].clone()) }
    }
    assert_eq!(answer.unwrap(), serde_json::json!({"id": 1, "ok": true, "result": {}}));
    let state = state.unwrap();
    assert!(state["chat"].is_array() && state["canvases"].is_array() && state["lock"]["locked"] == false, "{state}");
    assert!(state["signer"]["error"].as_str().is_some_and(|e| e.contains("signer isn't installed")), "{}", state["signer"]);

    drop(input);
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}
