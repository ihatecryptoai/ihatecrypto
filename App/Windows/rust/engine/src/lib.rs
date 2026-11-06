//! ihatecrypto's engine for Windows: the Rust port of `App/Sources/Desktop/Engine`, one module per Swift file
//! (Canvas.swift → canvas.rs, TopUp.swift → top_up.rs…). The window (Tauri) sends one JSON request a line on standard
//! input and reads one JSON answer or event a line on standard output (`Outbox`); money is signed only by the signer
//! process, after Confirm (`SignerLink`).
//!
//! Built in two layers. Layer 1 (here): the platform calls, the link to the signer, and everything that is data or
//! state — the engine's stored fields (`State`), its files (`store`, `task_seal`, `nodes`, `history_tabs::HistoryStore`),
//! the lock, the window's contract (`Snapshot`, `Request`) and every file's types with their JSON shapes. Layer 2: each
//! Swift file's `extension Engine` as an `impl Engine` block in that file's module (each module's `//!` names it).
//!
//! How Swift's pieces are carried over, for every module:
//! - Swift's `Engine` is a `@MainActor` class whose Tasks interleave only at an `await`. Here it is `Engine`, made by
//!   `Engine::new` as an `Rc<Engine>` on a single-threaded tokio runtime inside a `LocalSet`: `Task { @MainActor in … }`
//!   is `tokio::task::spawn_local`, so nothing here needs `Send`, and two tasks never run at once.
//! - Methods take `&self`. `self.rc()` is an owned handle for a task that outlives the call (`spawn_local(async move {
//!   me.… })`), as trade's `self.arc()`.
//! - The stored fields are `State` (engine.rs), grouped as Swift groups them, behind a `RefCell`: `self.st()` reads
//!   them, `self.st_mut()` changes them. A borrow is brief and never held across an `.await` (clippy's
//!   `await_holding_refcell_ref` is denied below): take what you need into locals, `drop` the borrow, then await. A
//!   method that calls another `&self` method that borrows drops its own borrow first, or the second borrow panics.
//! - Swift's `didSet` side effects are setters taking the change as a closure, saving the file (and pushing a state
//!   where Swift calls `changed()`) after it: `set_settings`, `set_app_costs`, `set_seen_costs`, `set_added_skills`,
//!   `set_skills_off`, `set_memory`, `set_favorites`, `set_starred`, `set_folder_store`, `set_live_models`, `set_ledger`,
//!   `set_alerts`, `set_tasks`, `set_schedule_runs`, `set_jupiter_shown`, `set_way_quotes`, and canvas.rs's
//!   `set_boards` (Swift's `boards` setter). A field without one is assigned through `st_mut()` as Swift assigns it.
//!   `options` (didSet `pinOptions()`) gets its setter with pin_options in chat_turns.rs.
//! - Swift's `static var`s on Engine and its helpers (`Schedule.composing`, `WalletScope`, `HistoryStore.saved`…) are
//!   fields of `State` too, in its last group: one engine, one copy. `@TaskLocal`s are tokio task-locals
//!   (automation.rs `TASK_RUN`), set with `.scope(v, future)`; they don't follow into `spawn_local`.
//! - `changed()` queues one state push after the current step, however many changes it made; while locked the window
//!   gets the lock alone. What the push draws is `Engine::new`'s `draw` (layer 2's `Engine::snapshot`).
//! - Files go through `store` (`store::load`, `store::save`, `store::url`): the data folder a person switching from the
//!   Swift engine already has, read and written in Swift's JSONEncoder shapes (dates as seconds since 2001, Data as
//!   base64, optionals left out when nil). The window's JSON (`Outbox`) writes dates in ISO 8601, as Swift's
//!   `JSONEncoder.lines`: a type that goes both ways writes its dates with `json::date`, which follows the writer.
//!   Decimals go with ihatecrypto-core's `foundation::number`. A test never touches the real folder: `store::test_folder`.
//! - Thrown errors are `wallet::WalletError` (Swift's WalletError and LocalizedError texts): `?` and `.map_err`.
//! - Money math on outside numbers uses ihatecrypto-core's `Num` (docs/windows-rust.md «For the engine»): an answer from
//!   an API must not be able to panic the engine.
#![deny(clippy::await_holding_refcell_ref)]

pub mod engine;
pub mod from_core;
pub mod json;
pub mod link_signer;
pub mod outbox;
pub mod platform;
pub mod signer_link;
pub mod snapshot;
pub mod store;

// One module per Swift file of Desktop/Engine.
pub mod agent;
pub mod alerts;
pub mod automation;
pub mod blocks;
pub mod browser;
pub mod canvas;
pub mod chains;
pub mod charts;
pub mod chat;
pub mod chat_commands;
pub mod chat_turns;
pub mod consent;
pub mod contract_calls;
pub mod costs;
pub mod dispatch;
pub mod earn;
pub mod favorites;
pub mod folders;
pub mod gas_ways;
pub mod hidden_tokens;
pub mod history_moves;
pub mod history_tabs;
pub mod imports;
pub mod ledger;
pub mod local_models;
pub mod lock;
pub mod models;
pub mod modules;
pub mod move_checks;
pub mod move_routes;
pub mod moves;
pub mod multisig;
pub mod nodes;
pub mod notices;
pub mod payments;
pub mod place_facts;
pub mod portfolio;
pub mod position_guard;
pub mod profit;
pub mod quotes;
pub mod route_edit;
pub mod schedule;
pub mod site_open;
pub mod skills;
pub mod task_seal;
pub mod top_up;
pub mod trade_plan;
pub mod trading_agent;
pub mod trading_key;
pub mod wallet_ops;

pub use engine::{Card, ChainStep, ChartLine, Engine, Message, Pending, Request, State};
pub use outbox::Outbox;
pub use snapshot::Snapshot;

#[cfg(test)]
mod tests;
