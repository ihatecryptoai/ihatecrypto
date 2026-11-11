//! Runs an approved plan on mainnet: the Rust port of `App/Sources/Execution/Trade`, one module per Swift file
//! (Executor.swift → executor.rs, AaveBorrow.swift → aave_borrow.rs…), each file's `extension Executor` an
//! `impl Executor` in its module, its other types that module's. Same decisions and the same words as the Swift.
//!
//! How Swift's pieces are carried over, for every module:
//! - The actor is `Executor`, made by `Executor::new`/`with` as an `Arc`; methods take `&self`, and `self.arc()` is an
//!   owned handle for a task that outlives the call (a bridge's landing). Its mutable state sits behind a
//!   `std::sync::Mutex`, never held across an `.await`. Fields are `pub(crate)`: `self.wallet.evm`, `self.rpc`,
//!   `self.signer`; `self.report(line)` tells the chat; `self.record(chain, id, kind, note)?` writes the journal.
//! - What Swift throws is `crate::Error` (error.rs): `Execution` (ExecutionError), `Guard` (GuardStop), `Wallet`
//!   (WalletError, and what a network, a reader or the journal threw), `Device` (the Ledger's LedgerError), `Costly`
//!   (CostlyMove), `NoReply` (Executor.NoReply). `?` takes a signer's TradeError and a WalletError; `fail(…)` and
//!   `stop(…)` are `Err(Execution)` and `Err(Guard)`.
//! - Swift's `@TaskLocal`s are tokio task-locals: `LOSS_ACCEPTED`, `IMPACT_ACCEPTED`, `LANDINGS` and the crate's
//!   `TOPPING_UP`, read with `loss_accepted()`, `impact_accepted()`, `landings()`, `topping_up()` (their Swift defaults
//!   outside a scope), set with `LOSS_ACCEPTED.scope(v, future).await` or `sync_scope(v, || …)`. They don't follow
//!   into `tokio::spawn` as Swift's into a child task: a spawned task that needs one is handed it.
//! - The Dev copy's launch flags are atomics (`JUPITER_Z_FIRST.load(Relaxed)`…; `STOP_MOVE_FAIL_TEST` a count). What
//!   the Mac reads from UserDefaults the engine hands in: `set_modules` (Settings › Modules, read by `modules()`) and
//!   `BATCHES_ON` (Settings › Wallet › one transaction for approve and swap).
//! - Hyperliquid: every call this crate makes there goes through `executor::hl_post(path, body)` (or `hl_url(path)`), and
//!   its markets through `executor::hl_markets()`, so a test points them all at a stand-in exchange (tests/fake_hl.rs).
//!   Core's reads (balances, ATR) go to api.hyperliquid.xyz.
//! - Money math on numbers from an API, a node or a chat never uses rust_decimal's operators on two of them: `Num`
//!   (ihatecrypto-core's foundation), and `num(x)?` where a number past range must stop the move.
//! - A method on a call cycle (send_evm → the gas top-up → send_evm; ensure_evm → bridge → cctp_move → ensure_evm)
//!   returns `Boxed` instead of being an `async fn`: called and awaited the same, its future's type is named, so the
//!   cycle compiles and stays `Send`. `send_evm` and `bridge` are; a new cycle boxes one of its own methods.
//! - Token amounts past 64 bits are `U256` (evm's); `u256(d)` turns a whole Decimal into one, `token_balance` reads a
//!   balance as Swift's `EVMRPC.tokenBalance` does (0 when it doesn't fit 64 bits).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, LazyLock, RwLock};

use ihatecrypto_core::foundation::Num;
use ihatecrypto_core::modules::Modules;
use net::evm_rpc::EvmRpc;
use rust_decimal::Decimal;

mod error;
pub use error::{CostlyMove, Error, Result, fail, stop};

// G1: the hub.
pub mod aave;
pub mod aave_borrow;
pub mod cctp_bridge;
pub mod evm_places;
pub mod executor;
pub mod morpho;
pub mod transit;
// G2: wallet and sends.
pub mod approvals;
pub mod dust;
pub mod kamino_borrow;
pub mod rewards;
pub mod send;
pub mod wallet;
// G3: swaps.
pub mod cross_swap;
pub mod intent_swap;
pub mod limit_order;
pub mod stocks;
pub mod swap;
pub mod tempo_swap;
pub mod v4;
// G4: bridges, Hyperliquid spot, reads.
pub mod agent_reads;
pub mod contract_calls;
pub mod hl_spot;
pub mod multisig;
pub mod one_click_bridge;
pub mod rebalance;
pub mod usdt0_bridge;

pub use aave::EvmNetwork;
pub use executor::{Executor, HlReply, HyperliquidAccount};
pub use transit::Landings;

#[cfg(test)]
mod tests;

// MARK: Task-locals (Swift's @TaskLocal)

tokio::task_local! {
    /// Dollars the user agreed to lose on this run: «Send anyway» on a move the cap stopped (CostlyMove).
    pub static LOSS_ACCEPTED: Decimal;
    /// The price impact the user took with «Send anyway» on a swap that the impact cap stopped.
    pub static IMPACT_ACCEPTED: Decimal;
    /// Set by a chain of moves: a bridge leg signs and goes on, its landing waited for by the step that spends where it
    /// lands. `None` inside a scope that must wait itself.
    pub static LANDINGS: Option<Arc<transit::Landings>>;
    /// Set while a gas top-up runs: its own transactions don't start another.
    pub(crate) static TOPPING_UP: bool;
}

pub fn loss_accepted() -> Decimal { LOSS_ACCEPTED.try_with(|v| *v).unwrap_or_default() }
pub fn impact_accepted() -> Decimal { IMPACT_ACCEPTED.try_with(|v| *v).unwrap_or_default() }
pub fn landings() -> Option<Arc<transit::Landings>> { LANDINGS.try_with(Clone::clone).ok().flatten() }
pub(crate) fn topping_up() -> bool { TOPPING_UP.try_with(|v| *v).unwrap_or(false) }

// MARK: The Dev copy's flags and the user's settings

/// A JupiterZ fill is taken even when the pools give more (Dev's `-jupiterZFirst YES`).
pub static JUPITER_Z_FIRST: AtomicBool = AtomicBool::new(false);
/// A swap's impact that reaches checkImpact counts as at least 3% and its dollars don't matter (`-impactStopTest YES`).
pub static IMPACT_STOP_TEST: AtomicBool = AtomicBool::new(false);
/// A limit entry goes in through the market and its armed stop counts as unconfirmed (`-limitFillTest YES`).
pub static LIMIT_FILL_TEST: AtomicBool = AtomicBool::new(false);
/// A Jupiter swap that went through reads no balance after it (`-swapTailTest YES`).
pub static SWAP_TAIL_TEST: AtomicBool = AtomicBool::new(false);
/// This many stop moves fail before anything is sent (`-stopMoveFailTest N`).
pub static STOP_MOVE_FAIL_TEST: AtomicU32 = AtomicU32::new(0);
/// Settings › Wallet › «One transaction for approve and swap» (Swift's UserDefaults `Executor.batchKey`): off unless
/// the user turns it on.
pub static BATCHES_ON: AtomicBool = AtomicBool::new(false);

static MODULES: LazyLock<RwLock<Modules>> = LazyLock::new(Default::default);

thread_local! {
    /// The switches as the engine on this thread set them. A program has one engine, on one thread, so this is the
    /// same as MODULES; tests run an engine each on threads of their own, and one test's switches mustn't reach another's.
    static HERE: std::cell::RefCell<Option<Modules>> = const { std::cell::RefCell::new(None) };
}

/// Settings › Modules as the user left them, handed in by the engine whenever they change (Swift's `Modules.current`).
pub fn set_modules(m: Modules) {
    HERE.with(|h| *h.borrow_mut() = Some(m.clone()));
    *MODULES.write().unwrap_or_else(|e| e.into_inner()) = m
}
pub fn modules() -> Modules {
    HERE.with(|h| h.borrow().clone()).unwrap_or_else(|| MODULES.read().unwrap_or_else(|e| e.into_inner()).clone())
}

// MARK: Hyperliquid's address

static HL_API: RwLock<Option<String>> = RwLock::new(None);

/// `path` on Hyperliquid's API: «/info», «/exchange».
pub(crate) fn hl_url(path: &str) -> String {
    format!("{}{path}", HL_API.read().unwrap_or_else(|e| e.into_inner()).as_deref().unwrap_or("https://api.hyperliquid.xyz"))
}

/// A stand-in exchange's root for the tests; None: the real one.
#[cfg(test)]
pub(crate) fn set_hl_api(root: Option<String>) { *HL_API.write().unwrap_or_else(|e| e.into_inner()) = root }

/// The future of a method on a call cycle.
pub type Boxed<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

// MARK: Numbers

/// What a number past range says where Swift's Decimal would have gone on as NaN.
pub(crate) const OUT_OF_RANGE: &str = "A number in the answer is out of range.";

/// The checked result, or the move stops.
pub(crate) fn num(x: Num) -> Result<Decimal> { x.0.ok_or_else(|| Error::Execution(OUT_OF_RANGE.into())) }

/// A whole, non-negative amount as a 256-bit word's number (`ERC20.approve(units:)`'s Decimal).
pub(crate) fn u256(d: Decimal) -> Result<evm::U256> { Ok(evm::U256::from_be_bytes(evm::abi::word_decimal(d)?)) }

/// A 256-bit number as a Decimal (`ABI.decimal(in:word:)`); None past Decimal's 28 digits.
pub(crate) fn decimal(n: evm::U256) -> Option<Decimal> { n.to_string().parse().ok() }

/// Word `i` of a return value as a Decimal (`ABI.decimal(in:word:)`).
pub(crate) fn word(data: &[u8], i: usize) -> Option<Decimal> { evm::abi::u256_in(data, i).and_then(decimal) }

/// `EVMRPC.tokenBalance`: the balance in the token's units, 0 when it doesn't fit 64 bits.
pub(crate) async fn token_balance(rpc: &EvmRpc, token: &str, owner: &str) -> Result<u64> {
    Ok(u64::try_from(rpc.token_balance(token, owner).await?).unwrap_or(0))
}

/// A JSON number as `NSNumber.decimalValue` gives it: a whole number as is, a fraction as the shortest decimal that reads
/// back the same double.
pub(crate) fn json_decimal(v: &serde_json::Value) -> Option<Decimal> {
    let n = v.as_number()?;
    n.as_u64().map(Decimal::from).or_else(|| n.as_i64().map(Decimal::from)).or_else(|| ihatecrypto_core::foundation::decimal(&format!("{}", n.as_f64()?)))
}
