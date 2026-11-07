//! The Mac's IntentsJourney.swift and TEEChip.swift: N5, a move through NEAR Intents after Confirm as the three places
//! its money passes; P3, Regular | Private over Confirm for a USDC send or a NEAR Intents swap; T4, the private
//! assistant's proof while the chat runs on an attested NEAR AI model. The window draws them (chat.js); here is what
//! they read.

use std::sync::LazyLock;

use chrono::Utc;
use execution::Kind;
use fancy_regex::Regex;
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::WalletCommand as W;
use protocols::one_click;
use serde::Serialize;
use trade::{EvmNetwork, Executor};

use crate::automation::task_run;
use crate::engine::{Engine, Pending};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage { Paying, Paid, Filling, Landed, Refunded }

/// N5 «Live journey» (owner's pick, 2026-11-06), read from 1Click's own states as the executor reports them. Times are
/// Unix milliseconds: the window counts the time left from them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Journey {
    pub title: String,
    /// Places: «solana», «base», «bitcoin».
    pub from: String,
    pub to: String,
    pub privately: bool,
    pub started: i64,
    /// 1Click's own estimate, from the deposit to the payout, in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate: Option<i64>,
    pub stage: Stage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paid_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
    /// It ended without 1Click's last word: it settles or refunds on its own.
    pub stuck: bool,
    /// 1Click's deposit address: NEAR Intents' explorer page for this move.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deposit: Option<String>,
    /// 1Click said REFUNDED: the money went back. FAILED alone doesn't say where it is.
    pub refunded: bool,
}

static ESTIMATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"of ≈ (\d+)").expect("a regex"));
/// «≈ 30 s», «≈ 3 min» (and the Russian «с», «мин») in a quote's line.
static SECONDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"≈ (\d+) (s|с|min|мин)\b").expect("a regex"));

impl Journey {
    pub fn new(title: String, from: &str, to: &str, privately: bool, estimate: Option<i64>, started: i64) -> Self {
        Self { title, from: from.into(), to: to.into(), privately, started, estimate, stage: Stage::Paying, paid_at: None, ended_at: None,
               stuck: false, deposit: None, refunded: false }
    }

    /// The seconds a quote's line says it takes.
    pub fn seconds_in(text: &str) -> Option<i64> {
        let m = SECONDS.captures(text).ok().flatten()?;
        let n: i64 = m[1].parse().ok()?;
        Some(if m[2].starts_with('m') || m[2].starts_with('м') { n * 60 } else { n })
    }

    /// A report line from the executor: «1Click · deposit received, swapping · 12 s of ≈ 30».
    pub fn read(&mut self, line: &str, now: i64) {
        let parts: Vec<&str> = line.split(" · ").collect();
        if parts.len() < 2 || parts[0] != "1Click" { return }
        if let Ok(Some(m)) = ESTIMATE.captures(line) && let Ok(e) = m[1].parse() { self.estimate = Some(e) }
        let Some(status) = Executor::ONE_CLICK_WORDS.iter().find(|(_, w)| *w == parts[1]).map(|(k, _)| *k) else { return };
        let next = match status {
            "PROCESSING" => Stage::Filling,
            "SUCCESS" => Stage::Landed,
            "REFUNDED" | "FAILED" => Stage::Refunded,
            // INCOMPLETE_DEPOSIT isn't final: 1Click refunds it, and its next word says so.
            _ => Stage::Paid,
        };
        if status == "REFUNDED" { self.refunded = true }
        if next >= Stage::Paid && self.paid_at.is_none() { self.paid_at = Some(now) }
        if next > self.stage { self.stage = next }
        if next >= Stage::Landed { self.ended_at = Some(now) }
    }

    /// The move came back: landed, or it failed. One that failed before anything was paid isn't a journey: None.
    /// `unknown`: the journal holds a deposit sent with no outcome — paid, and still on its way as far as anyone knows.
    pub fn ended(&self, ok: bool, unknown: bool, now: i64) -> Option<Self> {
        let mut j = self.clone();
        if j.ended_at.is_none() { j.ended_at = Some(now) }
        if ok { j.stage = Stage::Landed; return Some(j) }
        if j.stage == Stage::Paying && unknown { j.stage = Stage::Paid }
        if j.stage == Stage::Paying { return None }
        if j.stage < Stage::Landed { j.stuck = true }
        Some(j)
    }
}

/// P3's rows: what it pays from where, the fees, when it arrives, what it goes through.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyRow {
    pub label: String,
    pub value: String,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyChoice {
    pub is_private: bool,
    pub rows: Vec<PrivacyRow>,
}

/// T4: the last enclave check that passed, while the chat runs on NEAR AI Cloud.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeeView {
    pub model: String,
    pub gateway_key: String,
    /// Unix milliseconds of the check.
    pub at: i64,
    /// The last reply's signature: «model», «gateway» or «unverified»; absent before the first reply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// Where a private send pays USDC (one_click::asset::payout), and the networks a plain transfer goes on.
const PRIVATE_USDC: [&str; 4] = ["solana", "base", "arbitrum", "ethereum"];

fn now_ms() -> i64 { Utc::now().timestamp_millis() }

impl Engine {
    /// N5's journey for a move through NEAR Intents that waits for its solvers: a send paid out there, private or not, a
    /// private swap, and USDC moved «via NEAR Intents». None for any other, and for a scheduled run in the background.
    pub fn journey_for(&self, c: &W) -> Option<Journey> {
        if task_run().is_some() { return None }
        let quoted = self.quotes(c).and_then(|w| w.first().cloned());
        let now = now_ms();
        match c {
            W::SendPrivately { name, amount, .. } | W::SendToken { name, amount: Some(amount), .. } => {
                let (symbol, to) = match c {
                    W::SendPrivately { name, token, network, .. } => self.private_recipient(name, token, network.as_deref()).ok().map(|r| (r.1, r.3))?,
                    W::SendToken { name, token, network, .. } => self.across_recipient(name, token, network.as_deref()).ok().flatten().map(|r| (r.1, r.0.network))?,
                    _ => return None,
                };
                // The send's own quote is its receipt: without it (a chain's step) no journey.
                let q = quoted?;
                let from = EvmNetwork::all().iter().find(|n| n.name == q.way).map_or("solana", |n| n.key);
                Some(Journey::new(format!("{} {symbol} → {name}", Fmt::num(*amount, 9)), from, &to, matches!(c, W::SendPrivately { .. }),
                                  Journey::seconds_in(&q.gets), now))
            }
            W::SwapPrivately { from: a, on, to: b, at, amount } => {
                Some(Journey::new(format!("{} {} → {}", Fmt::num(*amount, 8), a.to_uppercase(), b.to_uppercase()), on, at, true,
                                  quoted.as_ref().and_then(|q| Journey::seconds_in(&q.way)), now))
            }
            W::Intents { from, to, amount } => {
                // All of it reads as its number: «all · 3.8 USDC».
                let shown = amount.map(|x| Fmt::num(x, 6)).or_else(|| self.usdc_free(from).map(|x| self.say(&format!("all · {}", Fmt::num(x, 6)), &format!("всё · {}", Fmt::num(x, 6)))))
                    .unwrap_or_else(|| self.say("all", "всё"));
                Some(Journey::new(format!("{shown} USDC → {}", W::place_name(to)), from, to, false, quoted.as_ref().and_then(|q| Journey::seconds_in(&q.gets)), now))
            }
            _ => None,
        }
    }

    /// The journey starts with the move (true when it did).
    pub fn start_journey(&self, c: &W) -> bool {
        let Some(j) = self.journey_for(c) else { return false };
        self.st_mut().journey = Some(j);
        self.changed();
        true
    }

    /// A report line during the move: its stage, and 1Click's deposit from the journal once paid.
    pub fn journey_read(&self, line: &str) {
        let Some(mut j) = self.st().journey.clone().filter(|j| j.ended_at.is_none()) else { return };
        j.read(line, now_ms());
        if j.stage >= Stage::Paid && j.deposit.is_none() { j.deposit = self.journey_deposit(j.started) }
        self.st_mut().journey = Some(j);
    }

    /// The 1Click deposit this move signed, from the journal: the last one since it started.
    fn journey_deposit(&self, started: i64) -> Option<String> {
        let since = chrono::DateTime::from_timestamp_millis(started - 1000)?;
        self.journal().entries().ok()?.into_iter().rev().find(|e| e.chain == "1click" && e.kind == Kind::Signed && e.at >= since).map(|e| e.id)
    }

    /// The move came back. It stays in the field as it ended until the next message.
    pub fn end_journey(&self, ok: bool) {
        let Some(j) = self.st().journey.clone().filter(|j| j.ended_at.is_none()) else { return };
        let deposit = j.deposit.clone().or_else(|| self.journey_deposit(j.started));
        let ended = Journey { deposit: deposit.clone(), ..j }.ended(ok, deposit.is_some(), now_ms());
        self.st_mut().journey = ended;
        self.changed();
    }

    /// A new message clears a journey that ended.
    pub fn clear_ended_journey(&self) {
        let ended = self.st().journey.as_ref().is_some_and(|j| j.ended_at.is_some());
        if ended { self.st_mut().journey = None }
    }

    // MARK: P3 · regular or private

    /// The move waiting for Confirm and the same move the other way, when both can go: USDC to a saved address on
    /// Solana, Base, Arbitrum or Ethereum with a NEAR Intents key, or a NEAR Intents swap and its private twin. `true`:
    /// the one waiting is the private one.
    pub fn privacy_twin(&self) -> Option<(bool, W)> {
        let pending = self.st().pending.clone()?;
        match pending {
            Pending::Transfer(entry, amount) => {
                let net = entry.network.clone();
                if !PRIVATE_USDC.contains(&net.as_str()) || self.private_recipient(&entry.name, "USDC", Some(&net)).is_err() { return None }
                Some((false, W::SendPrivately { name: entry.name, token: "USDC".into(), amount, network: Some(net) }))
            }
            Pending::Wallet(W::SendPrivately { name, token, amount, network }) => {
                if !token.eq_ignore_ascii_case("USDC") { return None }
                let r = self.private_recipient(&name, &token, network.as_deref()).ok()?;
                PRIVATE_USDC.contains(&r.3.as_str()).then_some((true, W::SendTo { name, amount, network: Some(r.3) }))
            }
            Pending::Wallet(c @ W::SwapPrivately { .. }) => c.made_public().map(|p| (true, p)),
            Pending::Wallet(c) => {
                one_click::key()?;
                c.made_private().filter(|p| matches!(p, W::SwapPrivately { .. })).map(|p| (false, p))
            }
            _ => None,
        }
    }

    /// The other way picked over Confirm: its card takes this one's place. Nothing is signed.
    pub async fn switch_privacy(&self) {
        let busy = { let st = self.st(); st.thinking || st.running_board.is_some() };
        if busy { return }
        let Some((_, other)) = self.privacy_twin() else { return };
        Box::pin(self.handle_wallet(&other, None)).await;
    }

    /// P3's rows for the move waiting for Confirm.
    pub fn privacy_choice(&self) -> Option<PrivacyChoice> {
        let (is_private, other) = self.privacy_twin()?;
        let say = |a: &str, b: &str| self.say(a, b);
        let row = |label: &str, value: String, note: String, logo: Option<&str>| PrivacyRow { label: label.into(), value, note, logo: logo.map(Into::into) };
        let (pay, get, fees, arrives, through) = (say("You pay", "Платишь"), say("You get", "Получишь"), say("Blockchain fees", "Комиссии сетей"),
                                                  say("Arrives", "Придёт"), say("Goes through", "Идёт через"));
        let time = |s: i64| if s >= 120 { say(&format!("≈ {} min", s / 60), &format!("≈ {} мин", s / 60)) } else { say(&format!("≈ {s} s"), &format!("≈ {s} с")) };
        let pending = self.st().pending.clone()?;
        match pending {
            Pending::Transfer(entry, amount) => {
                let (net, place) = (entry.network.clone(), W::place_name(&entry.network));
                let quick = net == "solana" || self.usdc_free(&net).unwrap_or(amount) >= amount;
                Some(PrivacyChoice { is_private: false, rows: vec![
                    row(&pay, format!("{} USDC", Fmt::num(amount, 6)), if quick { say(&format!("from {place}"), &format!("из {place}")) } else { say("the rest from Solana first", "остаток сначала из Solana") }, None),
                    row(&fees, say("network gas", "газ сети"), say("none to us", "нам — ничего"), None),
                    row(&arrives, if quick { say("in seconds", "за секунды") } else { say("after a bridge", "после моста") }, String::new(), None),
                    row(&through, say(&format!("{place} directly"), &format!("{place} напрямую")), say("anyone can see it's from you", "любой увидит, что это от тебя"), Some(&net)),
                ] })
            }
            Pending::Wallet(c) if matches!(c, W::SwapPrivately { .. }) || matches!(other, W::SwapPrivately { .. }) => {
                let W::SwapPrivately { from: a, on, amount, .. } = (if is_private { c.clone() } else { other.clone() }) else { return None };
                let own = self.quotes(&c).and_then(|w| w.first().cloned());
                let failed = self.quotes(&c).is_some_and(|w| w.is_empty());
                let gets = own.as_ref().map_or_else(|| "…".to_string(), |w| format!("≈ {}", w.gets));
                let pays = row(&pay, format!("{} {}", Fmt::num(amount, 8), a.to_uppercase()), say(&format!("on {}", W::place_name(&on)), &format!("в {}", W::place_name(&on))), None);
                Some(if is_private {
                    PrivacyChoice { is_private: true, rows: vec![
                        pays,
                        row(&get, gets, if failed { say("no quote now", "котировки сейчас нет") } else { String::new() }, None),
                        row(&arrives, own.as_ref().and_then(|w| Journey::seconds_in(&w.way)).map_or_else(|| "…".into(), |s| say("in ", "через ") + &time(s)), String::new(), None),
                        row(&through, "Confidential Intents".into(), say("nobody can match the two ends", "концы никто не сопоставит"), Some("near")),
                    ] }
                } else {
                    PrivacyChoice { is_private: false, rows: vec![
                        pays,
                        row(&get, gets, String::new(), None),
                        row(&through, own.map_or_else(|| "…".into(), |w| w.way), say("anyone can see the swap", "обмен виден всем"), None),
                    ] }
                })
            }
            Pending::Wallet(c) => {
                let q = self.quotes(&c);
                let first = q.as_ref().and_then(|w| w.first().cloned());
                let failed = q.as_ref().is_some_and(|w| w.is_empty());
                let pays = first.as_ref().and_then(|w| Self::quoted_amount(Some(&w.gets)));
                // «≈ 1.02 USDC for $1.00 of it · fees ≈ $0.02 (2%) · ≈ 30 s»: the fee part as the quote says it.
                let fee = first.as_ref().and_then(|w| w.gets.split(" · ").find(|p| p.starts_with("fees") || p.starts_with("комиссии"))
                    .map(|p| p.trim_start_matches("fees ").trim_start_matches("комиссии ").to_string()));
                Some(PrivacyChoice { is_private: true, rows: vec![
                    row(&pay, pays.map_or_else(|| "…".into(), |p| format!("≈ {} USDC", Fmt::num(p, 2))),
                        first.as_ref().map_or_else(|| if failed { say("no quote now", "котировки сейчас нет") } else { say("getting the price", "узнаю цену") },
                                                   |w| say(&format!("from {}", w.way), &format!("из {}", w.way))), None),
                    row(&fees, fee.unwrap_or_else(|| "…".into()), say("regular: network gas · none to us", "обычный: газ сети · нам — ничего"), None),
                    row(&arrives, first.as_ref().and_then(|w| Journey::seconds_in(&w.gets)).map_or_else(|| "…".into(), |s| say("in ", "через ") + &time(s)),
                        say("regular in seconds", "обычный — за секунды"), None),
                    row(&through, "NEAR Intents".into(), say("nobody can link it to you", "никто не свяжет это с тобой"), Some("near")),
                ] })
            }
            _ => None,
        }
    }

    // MARK: T4 · the private assistant

    /// The last enclave check that passed while the chat runs on NEAR AI Cloud; None on any other model.
    pub fn tee_view(&self) -> Option<TeeView> {
        if self.provider_id() != llm::near_ai::info().id { return None }
        let model = self.model();
        let v = llm::near_ai::last_verified(&model)?;
        let (reply, why) = match llm::near_ai::last_reply(&model) {
            Some(llm::near_ai::ReplyProof::Model) => (Some("model".to_string()), None),
            Some(llm::near_ai::ReplyProof::Gateway) => (Some("gateway".into()), None),
            Some(llm::near_ai::ReplyProof::Unverified(why)) => (Some("unverified".into()), Some(why)),
            None => (None, None),
        };
        Some(TeeView { model, gateway_key: v.gateway_key, at: (v.at * 1000.0) as i64, reply, why })
    }
}

#[cfg(test)]
#[path = "journey/tests.rs"]
mod tests;
