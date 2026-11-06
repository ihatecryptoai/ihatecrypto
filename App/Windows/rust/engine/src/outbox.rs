//! The engine's output (Engine.swift's Outbox): one JSON line per answer or event on standard output, which the window
//! reads. Written whole under a lock, so the signer's crash callback (another thread) never splits a line.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::json;

#[derive(Clone)]
pub struct Outbox {
    /// None: an engine inside a test writes nothing (on Windows `swift test` stalled on the states, and every test with it).
    sink: Option<Arc<Mutex<Box<dyn Write + Send>>>>,
}

impl Outbox {
    pub fn stdout() -> Self { Self::writer(std::io::stdout()) }

    pub fn silent() -> Self { Outbox { sink: None } }

    /// Lines to `w`: a test reads what the window would.
    pub fn writer(w: impl Write + Send + 'static) -> Self { Outbox { sink: Some(Arc::new(Mutex::new(Box::new(w)))) } }

    fn write<T: Serialize + ?Sized>(&self, object: &T) {
        let Some(sink) = &self.sink else { return };
        let Ok(mut line) = json::window(object) else { return };
        line.push(b'\n');
        let mut w = sink.lock().unwrap_or_else(|e| e.into_inner());
        let _ = w.write_all(&line).and_then(|_| w.flush());
    }

    pub fn event<T: Serialize + ?Sized>(&self, name: &str, body: &T) {
        #[derive(Serialize)]
        struct OutEvent<'a, T: ?Sized> {
            event: &'a str,
            data: &'a T,
        }
        self.write(&OutEvent { event: name, data: body });
    }

    pub fn answer(&self, id: Option<i64>, ok: bool, result: &HashMap<String, String>, error: Option<&str>) {
        #[derive(Serialize)]
        struct Answer<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            id: Option<i64>,
            ok: bool,
            result: &'a HashMap<String, String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            error: Option<&'a str>,
        }
        self.write(&Answer { id, ok, result, error });
    }
}
