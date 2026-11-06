//! The engine of the desktop app (main.swift), started by its window (Tauri) with pipes on standard input and output:
//! one JSON request a line in (`Request`), one JSON answer or event a line out (`Outbox`). The window closing its end
//! ends it.

use std::io::BufRead;
use std::sync::Arc;

use engine::browser::PhishingList;
use engine::signer_link::SignerLink;
use engine::{Engine, Outbox, Request};

fn main() {
    // Windows gives the main thread 1 MiB, which a debug build's deepest moves can outgrow: the engine runs on a thread
    // of a Mac's main-thread size.
    let engine = std::thread::Builder::new().name("engine".into()).stack_size(8 << 20).spawn(run).expect("the engine's thread");
    // `run` ends the process itself: here only when the engine's thread panicked.
    let _ = engine.join();
    std::process::exit(1);
}

fn run() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("the engine's runtime");
    // Swift's MainActor: one thread, and a request's task runs between another's awaits.
    tokio::task::LocalSet::new().block_on(&runtime, async {
        let engine = Engine::new(Outbox::stdout(), Arc::new(SignerLink::beside_engine()), Engine::snapshot);
        // The phishing list read (and the day's copy fetched) before any site asks, as the Mac's browser does at launch.
        tokio::task::spawn_local(async { PhishingList::shared().warm().await });

        let (lines, mut requests) = tokio::sync::mpsc::unbounded_channel();
        std::thread::spawn(move || {
            let mut input = std::io::stdin().lock();
            let mut line = Vec::new();
            // A line that doesn't read as a request is skipped; the end of input ends the loop.
            while input.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
                if let Some(request) = Request::read(&String::from_utf8_lossy(&line)) && lines.send(request).is_err() { break }
                line.clear();
            }
        });
        // Swift's `Task { @MainActor in await engine.handle(request) }`, one per line.
        while let Some(request) = requests.recv().await {
            let engine = engine.clone();
            tokio::task::spawn_local(async move { engine.handle(request).await });
        }
    });
    // The window closed its end: gone at once, as Swift's exit(0), whatever still runs (a signer's prompt included,
    // which dropping the runtime would wait for).
    std::process::exit(0);
}
