//! `ModelExam.swift`: a short exam of how a model reads the chat — requests people send, each with what it must lead
//! to, read through the same parsers as in the app. A model that misreads them makes the chat feel dumb — people took
//! that for the product — so a model, a local one above all, is checked before it's trusted (Settings › AI model ›
//! Check).

use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Instant;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::charts::{ChartCommand, Range};
use crate::intent::{IntentParser, Side, TradeIntent};
use crate::normalize::{ChatRouter, Context, Model, Route};
use crate::position_command::{SpotAsset, WalletCommand};

pub struct ModelExam;

type Judge = Box<dyn Fn(&Route, &HashSet<String>) -> bool + Send + Sync>;

pub struct Question {
    pub message: &'static str,
    /// What a right reading writes, for the test that the judge takes it.
    pub ideal: Route,
    judge: Judge,
}

impl Question {
    /// Whether a reading of `message` is right, with these markets.
    pub fn passes(&self, route: &Route, markets: &HashSet<String>) -> bool { (self.judge)(route, markets) }
}

/// Swift's `ModelExam.Result`, in its JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub passed: usize,
    pub total: usize,
    /// Seconds a reply took on average.
    pub seconds: f64,
    /// The messages it misread or didn't answer.
    pub misses: Vec<String>,
}

impl Score {
    /// Good enough for the chat: 8 of 10 and more.
    pub fn good(&self) -> bool { self.passed * 10 >= self.total * 8 }

    pub fn short(&self) -> String { format!("{}/{} · {:.1} s", self.passed, self.total, self.seconds) }
}

static MARKETS: LazyLock<HashSet<String>> = LazyLock::new(|| ["BTC", "ETH", "SOL", "xyz:CL"].iter().map(|s| s.to_string()).collect());

static QUESTIONS: LazyLock<Vec<Question>> = LazyLock::new(|| {
    let sol = SpotAsset::all()[0].clone();
    let n = |v: i64| Some(Decimal::from(v));
    let mv = |message: &'static str, ideal: &str, want: WalletCommand| Question {
        message,
        ideal: Route::Command(ideal.into()),
        judge: Box::new(move |r, _| ModelExam::wallet(r).as_ref() == Some(&want)),
    };
    let ask = |message: &'static str, ideal: Route, judge: Judge| Question { message, ideal, judge };
    vec![
        mv("купи SOL на 5 долларов", "buy SOL for 5 USDC", WalletCommand::Buy { asset: sol.clone(), usdc: n(5) }),
        mv("send 5 USDC to Mom", "send 5 USDC to Mom", WalletCommand::SendTo { name: "Mom".into(), amount: Decimal::from(5), network: None }),
        mv("положи 10 usdc в jupiter", "lend 10 USDC", WalletCommand::LendDeposit("USDC".into(), n(10))),
        ask("покажи график эфира за неделю", Route::Command("chart ETH week".into()), Box::new(|r, m| {
            ModelExam::written(r).and_then(|c| ChartCommand::parse(c, m)) == Some(ChartCommand::Chart { coin: "ETH".into(), range: Range::Week })
        })),
        // A trade may go to the plan editor instead of a command: either reads it.
        ask("лонг биткоин, риск 5 долларов", Route::Command("long BTC risk $5".into()), Box::new(|r, m| {
            matches!(r, Route::Plan)
                || ModelExam::written(r).and_then(|c| IntentParser::apply(c, &TradeIntent::default(), m)).is_some_and(|i| i.asset == "BTC" && i.side == Side::Long)
        })),
        ask("сколько у меня денег?", Route::Question, Box::new(|r, _| ModelExam::talks(r))),
        mv("выведи всё из jupiter lend", "withdraw all USDC from the vault", WalletCommand::LendWithdraw("USDC".into(), None)),
        mv("Jupiter DCA 100 USDC в SOL на 2 недели", "Jupiter DCA 100 USDC into SOL over 2 weeks",
           WalletCommand::JupiterDca { asset: sol, usdc: n(100), orders: Some(2), every: 604_800 }),
        mv("закинь 3 доллара газа на base", "add gas on base for 3 USDC", WalletCommand::Gas { place: "base".into(), usd: n(3) }),
        ask("какая завтра погода в Москве?", Route::Chat("I can't see the weather; I handle your crypto.".into()), Box::new(|r, _| ModelExam::talks(r))),
    ]
});

impl ModelExam {
    /// The markets every question is read with.
    pub fn markets() -> &'static HashSet<String> { &MARKETS }

    pub fn questions() -> &'static [Question] { &QUESTIONS }

    fn written(r: &Route) -> Option<&str> { if let Route::Command(c) = r { Some(c) } else { None } }

    fn wallet(r: &Route) -> Option<WalletCommand> {
        Self::written(r).and_then(|c| WalletCommand::parse(&ChatRouter::plan_change(c, &MARKETS).unwrap_or_else(|| c.to_string())))
    }

    /// Not a move: an answer, a question back, or a read-only command.
    fn talks(r: &Route) -> bool {
        match r {
            Route::Question | Route::Chat(_) | Route::Clarify(_) => true,
            Route::Command(c) => WalletCommand::parse(c)
                .map_or_else(|| IntentParser::apply(c, &TradeIntent::default(), &MARKETS).is_none(), |w| matches!(w, WalletCommand::Yields(_))),
            _ => false,
        }
    }

    /// The context every question is asked in: a small wallet with Mom and Bybit saved.
    pub fn context() -> Context {
        Context {
            address_book: vec!["Bybit".into(), "Mom".into()],
            balances: vec!["Solana 120 USDC, 0.05 SOL".into(), "Hyperliquid $10".into(), "Base 2 USDC".into()],
            capital: Some("$132".into()),
            reply_language: "Russian".into(),
            ..Context::default()
        }
    }

    /// Asks every question; `progress` is told how many are done. A question the model can't answer counts as a miss.
    pub async fn run(client: &impl Model, model: &str, progress: impl Fn(usize)) -> Score {
        let (mut passed, mut misses, mut total) = (0, Vec::new(), 0.0);
        let context = Self::context();
        for (i, q) in Self::questions().iter().enumerate() {
            let started = Instant::now();
            let route = ChatRouter::route(q.message, &context, client, model, Some(&MARKETS), None, None, None).await.map(|r| r.route);
            total += started.elapsed().as_secs_f64();
            if route.is_ok_and(|r| q.passes(&r, &MARKETS)) { passed += 1 } else { misses.push(q.message.to_string()) }
            progress(i + 1);
        }
        let count = Self::questions().len();
        Score { passed, total: count, seconds: total / count as f64, misses }
    }
}
