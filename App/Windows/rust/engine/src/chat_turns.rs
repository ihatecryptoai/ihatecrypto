//! The chat's turn (ChatTurns.swift): the model call Stop cuts off, the cards a reply offered (the `options` setter and
//! pin_options, Swift's didSet), the blocks a message went with, the model's next step on the waiting card, «Try again»,
//! a priced swap's bar, the context meter, and the model menu's facts with the models on this PC (Ollama: OllamaPull).

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::Duration;

use fancy_regex::Regex;
use ihatecrypto_core::blocks;
use ihatecrypto_core::chat_recall::ChatRecall;
use ihatecrypto_core::foundation::{self, Date, Num};
use ihatecrypto_core::intent::{Fmt, IntentParser, TradeIntent};
use ihatecrypto_core::normalize::{ChatRouter, Lookup};
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::stablecoins::DOLLARS;
use ihatecrypto_core::tokens::TokenLookup;
use llm::catalog::{self, Kind};
use llm::usage::{self, ModelFacts};
use llm::{Request, Usage};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use trade::EvmNetwork;
use unicode_segmentation::UnicodeSegmentation;

use crate::canvas::{Board, Step};
use crate::chat::{prefix, trim_spaces};
use crate::engine::{Engine, Message, Pending, Retry};
use crate::from_core::RouterOption;
use crate::{json, nodes, route_edit, store};

/// One block a message went with, as its chip over the message says it (the Mac's SentContextChip): its title, what the
/// model was told, and whether only its name went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentContext {
    pub title: String,
    pub about: String,
    pub bare: bool,
}

/// A card a reply offered, kept with that reply (the Mac's Message.options): live while it's the last reply's and under
/// 30 minutes old; an older set folds, its pick keeping the check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferView {
    pub title: String,
    pub comment: String,
    pub recommended: bool,
    /// What the card runs, shown on hover as on the Mac («Answer with this» when it's a reply); None in older saved chats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// A priced swap's card, for the field's waiting bar (the Mac's quoteState): quoting, fresh until 15 s after its quote,
/// then stale — Refresh asks again before Confirm comes back.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuoteView {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub at: Option<Date>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

impl QuoteView {
    fn only(state: &str) -> Self { QuoteView { state: state.into(), at: None, line: None } }
}

/// The model menu and the connect bar's facts beyond the catalog: the lists a subscription or a model on this PC gives
/// now, whether Ollama answers and is installed, what each model takes, downloads under way, this PC's memory.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsView {
    pub live: std::collections::HashMap<String, Vec<String>>,
    pub servers: std::collections::HashMap<String, bool>,
    pub installed: bool,
    pub sizes: std::collections::HashMap<String, f64>,
    pub downloads: std::collections::HashMap<String, f64>,
    #[serde(rename = "memoryGB")]
    pub memory_gb: i64,
    pub recommended: std::collections::HashMap<String, String>,
    pub refusals: std::collections::HashMap<String, String>,
}

/// What Stop says: nothing ran (English, Russian).
pub const STOPPED_LINE: (&str, &str) = ("Stopped — nothing ran. Write what you'd like instead.", "Остановил — ничего не запускал. Напиши, что сделать вместо этого.");
/// Seconds a swap's quote stays fresh.
pub const QUOTE_LIFE: f64 = 15.0;
/// Ollama on this PC.
pub const OLLAMA: &str = "http://localhost:11434";

macro_rules! regex {
    ($name:ident, $p:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($p).expect("a regex"));
    };
}

regex!(GETS, r"([\d,]+(?:\.\d+)?)\s+([^\s·≈(]+)");
regex!(WORTH, r"≈\s*\$([\d,]+(?:\.\d+)?)");
regex!(ALL_OF, r"\ball ([A-Za-z][A-Za-z0-9₮.]*)");
regex!(AN_ADDRESS, r"^(?:0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44})$");

/// `Decimal(string:)` of a number written with thousands commas.
fn number(s: &str) -> Option<Decimal> { foundation::decimal(&s.replace(',', "")) }

impl Engine {
    // MARK: Stop

    /// A model call the Stop button can cancel, as the Mac's modelCall: the call is dropped when Stop is pressed, so
    /// nothing it would have led to runs, never a signature (a move runs outside it). None: Stop cut it off.
    pub async fn model_call<T>(&self, work: impl Future<Output = T>) -> Option<T> {
        let stop = Rc::new(tokio::sync::Notify::new());
        {
            let told = stop.clone();
            let mut st = self.st_mut();
            st.cancel_model = Some(Box::new(move || told.notify_one()));
            st.model_running = true;
        }
        self.changed();
        let out = tokio::select! {
            done = work => Some(done),
            _ = stop.notified() => None,
        };
        {
            let mut st = self.st_mut();
            st.cancel_model = None;
            st.model_running = false;
        }
        self.changed();
        out
    }

    /// The Stop button and Esc: the model stops answering and looking things up, nothing it would have led to runs, and the
    /// request it carried on with is dropped. A move already signing goes on: it can't be called back.
    pub fn stop_model(&self) {
        let mut st = self.st_mut();
        if st.running_board.is_some() || st.cancel_model.is_none() { return }
        st.stop_requested = true;
        st.agent_goal = None;
        if let Some(cancel) = &st.cancel_model { cancel() }
    }

    // MARK: What the model looks at

    /// The line under the thinking dots while a lookup runs (the Mac's agentStep).
    pub fn lookup_step(&self, l: &Lookup) -> Option<String> {
        Some(match l.tool.as_str() {
            "portfolio" => self.say("Checking your portfolio", "Смотрю портфель"),
            "market" => {
                let markets: HashSet<String> = self.st().markets.keys().cloned().collect();
                let coin = IntentParser::named_asset(&l.arg, &markets).map_or_else(|| l.arg.to_uppercase(), |c| IntentParser::pretty(&c));
                self.say(&format!("Looking at {coin}"), &format!("Смотрю {coin}"))
            }
            "token" => self.say("Looking the token up", "Ищу токен"),
            "rates" => self.say("Comparing rates", "Сравниваю ставки"),
            "gas" => self.say("Checking gas", "Смотрю газ"),
            "services" => self.say("Looking for a service that takes payment", "Ищу сервис с оплатой"),
            "service" => self.say("Reading the service's docs", "Читаю документацию сервиса"),
            "skill" | "journal" | "study" => match self.skills_on().into_iter().find(|s| s.id == l.arg) {
                Some(s) => self.say(&format!("Reading the skill «{}»", s.name), &format!("Читаю скилл «{}»", s.name)),
                None => self.say("Reading the skill", "Читаю скилл"),
            },
            "trades" => self.say("Rebuilding the trades on Hyperliquid", "Восстанавливаю сделки на Hyperliquid"),
            tool if trade::agent_reads::AgentReads::TOOLS.contains(&tool) => {
                let host = if tool == "fetch" { nodes::host(l.arg.split(' ').next().unwrap_or("")) } else { None };
                match host {
                    Some(h) => self.say(&format!("Reading {h}"), &format!("Читаю {h}")),
                    None => self.say("Reading the chain", "Читаю блокчейн"),
                }
            }
            _ => return None,
        })
    }

    // MARK: Cards kept with their reply

    /// The last reply's cards (Swift's `options` setter): set, then pinned to that reply (its didSet).
    pub fn set_options(&self, options: Vec<RouterOption>) {
        self.st_mut().options = options;
        self.pin_options();
    }

    /// The cards just offered go with the reply they follow, so an older reply keeps its own, folded (Engine.options).
    pub fn pin_options(&self) {
        {
            let mut st = self.st_mut();
            let last = st.messages().len().checked_sub(1).filter(|&i| !st.messages()[i].from_user);
            let (false, Some(i)) = (st.options.is_empty(), last) else {
                st.options_message = None;
                return;
            };
            let shown: Vec<OfferView> = st.options.iter()
                .map(|o| OfferView { title: o.title.clone(), comment: o.comment.clone(), recommended: o.recommended, command: Some(o.command.clone()) })
                .collect();
            let m = &mut st.messages_mut()[i];
            if m.options.as_ref() != Some(&shown) {
                m.options = Some(shown);
                m.picked = None;
            }
            let id = m.id.clone();
            st.options_message = Some(id);
        }
        self.save_chat();
    }

    /// A card picked: the check stays on it once the set folds.
    pub fn mark_picked(&self, index: usize) {
        {
            let mut st = self.st_mut();
            let Some(id) = st.options_message.clone() else { return };
            let Some(m) = st.messages_mut().iter_mut().find(|m| m.id == id) else { return };
            m.picked = Some(index as i64);
        }
        self.save_chat();
    }

    // MARK: What a message went with

    /// The chips over a message: the block it was about, and the others the person attached with it.
    pub fn sent_contexts(&self) -> Option<Vec<SentContext>> {
        let all: Vec<Board> = {
            let st = self.st();
            st.attached.iter().chain(&st.also_attached).cloned().collect()
        };
        if all.is_empty() { return None }
        Some(all.iter().map(|b| {
            let facts = self.block_facts(b);
            // Swift's split(separator: ":", maxSplits: 1): empty pieces left out, the last one is what the block shows.
            let shows = facts.splitn(2, ':').filter(|p| !p.is_empty()).last().map_or("", trim_spaces);
            let title = if ["move", "result", "chart"].contains(&b.kind.as_str()) { b.title.clone() } else { b.kind.clone() };
            SentContext { title, bare: shows.is_empty(), about: facts }
        }).collect())
    }

    // MARK: NEXT · AFTER THIS

    /// The canvas's blocks as the window draws them: the card waiting for Confirm gets the step the model plans after it
    /// (the Mac's agentNextSteps), pale, until the move goes or the request is dropped.
    pub fn boards_shown(&self) -> Vec<Board> {
        let st = self.st();
        let (Some(next), Some(id)) = (st.agent_goal.as_ref().and_then(|g| g.next.clone()), st.card.as_ref().map(|c| c.id.clone())) else {
            return st.boards().clone();
        };
        st.boards().iter().map(|b| {
            let mut b = b.clone();
            if b.id != id { return b }
            let step = Step {
                label: st.say("NEXT · AFTER THIS", "ДАЛЬШЕ · ПОСЛЕ ЭТОГО"), title: next.clone(),
                lines: vec![st.say("the chat goes on once this is through · write a change before Confirm", "чат продолжит, когда это пройдёт · изменение напиши до Confirm")],
                money: None, status: Some("then".into()),
            };
            b.steps.get_or_insert_with(Vec::new).push(step);
            b
        }).collect()
    }

    // MARK: Try again, and the steps a move took

    /// A move that failed before anything went out can be laid out again, with fresh quotes (the Mac's retryCommand).
    pub fn note_retry(&self, waiting: &Pending) {
        let mut st = self.st_mut();
        if let Pending::Wallet(c) = waiting && let Some(board) = st.running_board.clone() { st.retry = Some(Retry { board, command: c.clone() }) }
    }

    pub async fn retry_move(&self) {
        let Some(r) = self.st_mut().retry.take() else { return };
        Box::pin(self.handle_wallet(&r.command, None)).await;
    }

    /// The move's lines as it ran stay in the chat as one «Executed · N steps» card, before what it ended with.
    pub fn keep_steps(&self, index: usize) {
        {
            let mut st = self.st_mut();
            let Some(lines) = st.running.as_ref().map(|r| r.lines.clone()).filter(|l| !l.is_empty()) else { return };
            if index > st.messages().len() { return }
            let mut m = Message::new(false, &lines.join("\n"), Some("execution".into()));
            m.steps = Some(lines);
            st.messages_mut().insert(index, m);
        }
        self.save_chat();
    }

    // MARK: The price on a swap's card

    /// A swap whose card shows a price: Jupiter's on Solana, the two ways of a Base or Arbitrum trade, a stock's.
    pub fn priced(&self, c: &WalletCommand) -> bool {
        if self.st().unpriced.as_ref() == Some(c) { return false }
        match c {
            WalletCommand::Buy { usdc: Some(_), .. } | WalletCommand::Sell { amount: Some(_), .. } | WalletCommand::CrossSwap { .. } => true,
            WalletCommand::SwapOn { place, amount: Some(_), .. } if place == "solana" => true,
            // The Mac's quoteSwap prices every place's swap and a coin sent through NEAR Intents (pricedCommand) — but not
            // Hyperliquid's spot book: its quote shows on the card and Confirm isn't held for it.
            WalletCommand::HlSpotBuy { .. } | WalletCommand::HlSpotSell { .. } => false,
            WalletCommand::SwapOn { place, .. } if place == "hyperliquid" => false,
            WalletCommand::SwapOn { place, amount, .. } if place == "near" => amount.is_some(),
            WalletCommand::SwapOn { place, .. } => EvmNetwork::named(place).is_some(),
            WalletCommand::SendToken { name, token, amount: Some(_), network } => {
                matches!(self.across_recipient(name, token, network.as_deref()), Ok(Some(_)))
            }
            _ => route_edit::quoted(c),
        }
    }

    /// The open position this canvas's plan is on, or the coin just traded while its position is open: the chat's position
    /// chips (breakeven, trail, close half, close) show only then and act on it (the Mac's managesPosition and
    /// positions.contains(intent.asset)); targetCoin picks the same coin when the phrase names none.
    pub fn chips_position(&self) -> Option<String> {
        let st = self.st();
        let plan = if st.intent != TradeIntent::default() { Some(st.intent.asset.clone()) } else { st.traded_plan.as_ref().map(|p| p.asset.clone()) }?;
        st.positions.iter().any(|p| p.coin == plan).then_some(plan)
    }

    pub fn quote_view(&self) -> Option<QuoteView> {
        let c = self.quoted_move()?;
        if !self.priced(&c) { return None }
        let (shown, ways) = {
            let st = self.st();
            (st.jupiter_shown.clone(), st.way_quotes.clone())
        };
        if let Some(j) = shown && j.command == c {
            let gets: String = j.title.graphemes(true).skip(1).collect();
            return Some(self.quote_state(&c, trim_spaces(&gets), None));
        }
        if let Some(w) = ways && w.command == c {
            // Asked and not answered yet: Refresh and «over 15 s old» while it keeps asking (the Mac's empty swapQuote).
            let Some(best) = w.ways.first() else { return Some(QuoteView::only("stale")) };
            return Some(self.quote_state(&c, &best.gets, best.usd));
        }
        Some(QuoteView::only("quoting"))
    }

    /// Swift's private `state(_:gets:worth:)`.
    fn quote_state(&self, c: &WalletCommand, gets: &str, worth: Option<Decimal>) -> QuoteView {
        let Some(at) = self.st().quoted_at else { return QuoteView::only("quoting") };
        // A sale by contract names its ticker once known (the Mac's tokenSymbol).
        let sold = match c {
            WalletCommand::SellOn { place, token, .. } if token.starts_with("0x") => self.token_symbol(token, place),
            _ => None,
        };
        let age = (chrono::Utc::now() - at).as_seconds_f64();
        QuoteView { state: if age < QUOTE_LIFE { "fresh" } else { "stale" }.into(), at: Some(at), line: Some(Self::swap_line(c, gets, worth, sold.as_deref())) }
    }

    /// «You get ≈ 23.01 SPX · 1 SPX ≈ $0.4346», from what the quote gets (the Mac's swapLine), and «⚠️ fees take ≈ $0.18
    /// (18%): a bigger amount loses less» when dollars go in and fees keep more than a tenth. `worth`: what arrives in
    /// dollars after gas (an EVM way's, a stock's); without it, a cross quote's own «≈ $X». A price past the arithmetic
    /// (an amount from the chat against a quote's) is left out, never a panic.
    pub fn swap_line(c: &WalletCommand, gets: &str, worth: Option<Decimal>, sold: Option<&str>) -> String {
        // A coin bought on the way to a NEAR Intents address: the quote's own words — what it costs in USDC, the fees, the
        // time — never «You get» (that's what the send costs, not what arrives).
        if let WalletCommand::SendToken { .. } = c { return gets.into() }
        let caps = GETS.captures(gets).ok().flatten();
        let parsed = caps.as_ref().and_then(|m| Some((number(m.get(1)?.as_str())?, m.get(2)?.as_str().to_string())));
        let Some((out, got)) = parsed.filter(|(out, _)| *out > Decimal::ZERO) else { return format!("You get ≈ {gets}") };
        let pays: Option<(Decimal, String)> = match c {
            WalletCommand::Buy { usdc: Some(u), .. } | WalletCommand::BuyOn { usdc: Some(u), .. } | WalletCommand::BuyStock { usdc: Some(u), .. } => Some((*u, "USDC".into())),
            WalletCommand::Sell { asset, amount: Some(x) } => Some((*x, asset.symbol.clone())),
            WalletCommand::SellOn { token, amount: Some(x), .. } => Some((*x, sold.map(String::from).unwrap_or_else(|| {
                if token.starts_with("0x") { format!("{}…", prefix(token, 6)) } else { token.to_uppercase() }
            }))),
            WalletCommand::SellStock { symbol, amount: Some(x) } => Some((*x, symbol.clone())),
            WalletCommand::SwapOn { from, amount: Some(x), .. } | WalletCommand::CrossSwap { from, amount: Some(x), .. } => Some((*x, from.to_uppercase())),
            _ => None,
        };
        let mut line = format!("You get ≈ {} {got}", Fmt::num(out, 6));
        if let Some((amount, symbol)) = pays && amount > Decimal::ZERO {
            let dollars_in = DOLLARS.contains(&symbol.to_uppercase());
            if dollars_in {
                if let Some(p) = (Num::of(amount) / out).0 { line += &format!(" · 1 {got} ≈ ${}", Fmt::price(p)) }
            } else if DOLLARS.contains(&got.to_uppercase()) {
                if let Some(p) = (Num::of(out) / amount).0 { line += &format!(" · 1 {symbol} ≈ ${}", Fmt::price(p)) }
            } else if let Some(p) = (Num::of(out) / amount).0 {
                line += &format!(" · 1 {symbol} ≈ {} {got}", Fmt::price(p));
            }
            let value = worth.filter(|w| *w > Decimal::ZERO)
                .or_else(|| WORTH.captures(gets).ok().flatten().and_then(|m| number(m.get(1)?.as_str())));
            if dollars_in && let Some(value) = value && let Some(warning) = Fmt::fee_warning(amount, value) { line += &format!(" · {warning}") }
        }
        line
    }

    /// «Sell all USDT» with the amount held for each «all X» the wallet holds in one place only: «Sell 0.999604 USDT»
    /// (the Mac's withAmounts, for the chat bar's head). Words stay where it's held in several places or not read yet,
    /// and for USDC, which is on every network.
    pub fn with_amounts(&self, text: &str) -> String {
        let st = self.st();
        ALL_OF.replace_all(text, |m: &fancy_regex::Captures<'_, str>| {
            let whole = m.get(0).map_or("", |g| g.as_str()).to_string();
            let symbol = m.get(1).map_or("", |g| g.as_str());
            let want = symbol.to_uppercase();
            let held: Vec<(&str, &crate::portfolio::Row)> = st.portfolio.networks.iter()
                .flat_map(|n| n.rows.iter().filter(|r| r.section == "tokens" && r.symbol.to_uppercase() == want).map(move |r| (n.key.as_str(), r)))
                .collect();
            match held.first() {
                Some(first) if want != "USDC" && first.1.value > Decimal::ZERO && held.iter().all(|h| h.0 == first.0 && h.1.contract == first.1.contract) => {
                    format!("{} {symbol}", Fmt::num(first.1.value, 8))
                }
                _ => whole,
            }
        }).into_owned()
    }

    /// Refresh on a stale price: quoted again, and Confirm back for 15 s once it is.
    pub fn requote(&self) {
        let Some(c) = self.quoted_move() else { return };
        let Some(id) = self.st().card.as_ref().map(|c| c.id.clone()) else { return };
        {
            let mut st = self.st_mut();
            st.quoted_at = None;
            st.jupiter_shown = None;
            st.way_quotes = None;
        }
        self.changed();
        let ways = route_edit::quoted(&c);
        let (me, command) = (self.rc(), c.clone());
        tokio::task::spawn_local(async move { me.quote_card(&command, &id).await });
        if ways {
            let me = self.rc();
            tokio::task::spawn_local(async move { me.quote_ways(&c).await });
        }
    }

    // MARK: Lookups the field asks for

    /// A contract or mint typed after «@» or «$»: what DexScreener knows of it, one row per network, for the list over the
    /// field (the Mac's lookUpContract). The person typed it there to look it up, so it goes out.
    pub async fn look_up_contract(&self, address: &str) -> HashMap<String, String> {
        if !AN_ADDRESS.is_match(address).unwrap_or(false) { return HashMap::from([("list".into(), "[]".into())]) }
        let found = TokenLookup::lookup(address).await.unwrap_or_default();
        let mut chains = HashSet::new();
        let rows: Vec<Value> = found.into_iter().filter(|t| chains.insert(t.chain.clone())).map(|t| {
            let mut row = json!({"symbol": t.symbol, "name": t.name, "chain": t.chain, "address": t.address});
            if let Some(image) = t.image_url { row["image"] = image.into() }
            if let Some(price) = t.price_usd { row["price"] = Fmt::usd(price).into() }
            if let Some(liquidity) = t.liquidity_usd { row["liquidity"] = Fmt::usd(liquidity).into() }
            row
        }).collect();
        HashMap::from([("list".into(), Value::Array(rows).to_string())])
    }

    /// The blocks «/» lists: the ones a pick puts an example of in the field (BlockCatalog, the Mac's list).
    pub fn block_catalog(&self) -> HashMap<String, String> {
        let rows: Vec<Value> = blocks::ALL.iter().filter(|b| !b.automatic && !b.example.is_empty())
            .map(|b| json!({"title": b.title, "group": b.group, "example": b.example, "logo": b.logo.unwrap_or("")}))
            .collect();
        HashMap::from([("list".into(), Value::Array(rows).to_string())])
    }

    // MARK: The context meter

    /// The lines of the chat that go to the model with a message (ChatRecall): all of them to a wide window, the last 12
    /// at most to a model on this PC, as on the Mac.
    pub fn sent_chat(&self, lines: &[String]) -> Vec<String> {
        let id = self.provider_id();
        let window = catalog::window(&id).or_else(|| {
            let st = self.st();
            st.meters.get(&st.canvas_id).and_then(|m| m.window)
        });
        let small = self.provider(&id).is_some_and(|p| p.info().kind == Kind::Local);
        ChatRecall::sent(lines, window, small, ChatRouter::instruction_tokens(), ChatRecall::MOST)
    }

    /// What one answer read and cost, for the chip over the field (the Mac's ContextMeter.count): the window's parts when it
    /// was the chat reading a message; dollars for every answer an API key pays for. The counts are a service's: kept
    /// within i64 whatever it says.
    pub async fn counted(&self, usage: Usage, request: Request, id: &str) {
        let paid = self.provider(id).is_some_and(|p| p.info().kind == Kind::ApiKey);
        if paid && self.st().model_facts.is_empty() { self.load_model_facts().await }
        let facts = if paid { self.st().model_facts.get(&ModelFacts::key(&request.model)).cloned() } else { None };
        let canvas = self.st().canvas_id.clone();
        let mut m = self.st().meters.get(&canvas).cloned().unwrap_or_default();
        if ChatRouter::reads(&request) {
            self.canvas_talked(id);
            let bytes = (request.messages.iter().map(|m| m.text.len() as i64).sum::<i64>()).max(1);
            let brief = request.system().len() as i64;
            let all = self.chat_lines();
            let sent = self.sent_chat(&all);
            let sent_bytes = sent.iter().map(|l| l.len() as i64).sum::<i64>();
            m.used = usage.input;
            m.window = usage.window.or_else(|| catalog::window(id)).or(facts.as_ref().map(|f| f.window)).or(m.window);
            m.instructions = usage.input.saturating_mul(brief.min(bytes)) / bytes;
            m.chat = usage.input.saturating_mul((bytes - brief).max(0).min(sent_bytes)) / bytes;
            m.sent = sent.len() as i64;
            m.lines = all.len() as i64;
        }
        if paid && let Some(cost) = usage.cost.or_else(|| facts.as_ref().map(|f| f.cost(&usage))) {
            m.spent += cost;
            m.last = Some(cost);
            m.billed = usage.cost.is_some();
        }
        let canvas = self.st().canvas_id.clone();
        self.st_mut().meters.insert(canvas, m);
        self.changed();
    }

    /// Windows and prices of the models a key pays for, read once a launch (asked again after a minute when it failed).
    pub async fn load_model_facts(&self) {
        {
            let mut st = self.st_mut();
            if (chrono::Utc::now() - st.model_facts_asked).as_seconds_f64() <= 60.0 { return }
            st.model_facts_asked = chrono::Utc::now();
        }
        if let Ok((data, _)) = net::get(usage::SOURCE, &[]).await { self.st_mut().model_facts = ModelFacts::table(&data) }
    }

    // MARK: The model menu's lists, and models on this PC

    /// Ollama's app or its server on this PC.
    pub fn ollama_installed() -> bool {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let program = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        [format!("{local}\\Programs\\Ollama\\ollama app.exe"), format!("{local}\\Programs\\Ollama\\ollama.exe"), format!("{program}\\Ollama\\ollama.exe")]
            .iter().any(|p| std::path::Path::new(p).exists())
    }

    pub fn memory_gb() -> f64 { physical_memory() as f64 / 1_073_741_824.0 }

    /// A model fits when it and its working memory take at most ~70% of the PC's (the Mac's OllamaModels.fits).
    pub fn fits(size_gb: f64) -> bool { size_gb * 1.2 <= Self::memory_gb() * 1.073_741_824 * 0.7 }

    /// The biggest suggested model that fits this PC's memory; None until the sizes are read.
    pub fn local_pick(&self) -> Option<String> {
        let models = self.provider("ollama").map(|p| p.info().models).unwrap_or_default();
        let st = self.st();
        models.into_iter().find(|m| st.model_sizes.get(m).is_some_and(|gb| Self::fits(*gb)))
    }

    pub fn models_view(&self) -> ModelsView {
        let all = catalog::all();
        let pick = self.local_pick();
        let recommended = all.iter().map(|p| {
            let info = p.info();
            let model = if info.id == "ollama" { pick.clone().unwrap_or(info.default_model) } else { info.default_model };
            (info.id, model)
        }).collect();
        let st = self.st();
        let listed: HashSet<String> = all.iter().flat_map(|p| p.info().models).chain(st.live_models.values().flatten().cloned()).collect();
        let refusals = listed.into_iter().filter_map(|m| catalog::chat_refusal(&m).map(|why| (m, why))).collect();
        ModelsView {
            live: st.live_models.clone(), servers: st.local_servers.clone(), installed: Self::ollama_installed(), sizes: st.model_sizes.clone(),
            downloads: st.model_downloads.clone(), memory_gb: Self::memory_gb().round() as i64, recommended, refusals,
        }
    }

    /// Read when the chat's model chip appears and after its menu closes, never while it's open (the rows would move under
    /// the pointer): a subscription's live list, and what the servers on this PC have downloaded.
    pub async fn refresh_models(&self) {
        let id = self.provider_id();
        if let Some(p) = self.provider(&id) && p.info().kind == Kind::Subscription
            && let Ok(list) = p.models(store::credentials().get(&id)).await && !list.is_empty()
        {
            self.set_live_models(|l| { l.insert(id.clone(), list.into_iter().take(4).collect()); });
        }
        for p in catalog::all() {
            let info = p.info();
            if info.kind != Kind::Local || info.unavailable.is_some() || info.id == "apple" { continue }
            if info.id == "ollama" && let Some(tags) = Self::ollama_downloaded().await {
                self.st_mut().model_sizes.extend(tags);
                // Its version, as the Mac reads it here: an Ollama updated outside the app takes the model again.
                self.read_ollama_versions().await;
            }
            let Ok(list) = p.models(None).await else {
                self.st_mut().local_servers.insert(info.id.clone(), false);
                continue;
            };
            self.st_mut().local_servers.insert(info.id.clone(), true);
            self.set_live_models(|l| { l.insert(info.id.clone(), list.into_iter().filter(|m| !m.to_lowercase().contains("embed")).collect()); });
            if info.id == "ollama" {
                for m in &info.models {
                    let missing = {
                        let st = self.st();
                        !st.live_models.get("ollama").is_some_and(|l| l.contains(m)) && !st.model_sizes.contains_key(m)
                    };
                    if !missing { continue }
                    match Self::ollama_size(m).await {
                        Some(gb) => { self.st_mut().model_sizes.insert(m.clone(), gb); }
                        None => { self.st_mut().model_sizes.remove(m); }
                    }
                }
            }
        }
        self.changed();
    }

    /// The downloaded models and their sizes, GB, from Ollama's /api/tags; None when Ollama doesn't answer.
    pub async fn ollama_downloaded() -> Option<HashMap<String, f64>> {
        let response = net::HTTP.get(format!("{OLLAMA}/api/tags")).timeout(Duration::from_secs(3)).send().await.ok()?;
        let j: Value = serde_json::from_slice(&response.bytes().await.ok()?).ok()?;
        let mut out = HashMap::new();
        for m in j.get("models")?.as_array()? {
            let Some(name) = m.get("name").and_then(Value::as_str) else { continue };
            // The first of a name stays, as Dictionary(uniquingKeysWith:) keeps it.
            out.entry(name.to_string()).or_insert(m.get("size").and_then(Value::as_f64).unwrap_or(0.0) / 1_000_000_000.0);
        }
        Some(out)
    }

    /// A model's download size, GB, from the registry's manifest; None when the registry doesn't know it.
    pub async fn ollama_size(model: &str) -> Option<f64> {
        let (base, tag) = model.split_once(':').unwrap_or((model, "latest"));
        let name = if base.contains('/') { base.to_string() } else { format!("library/{base}") };
        let response = net::HTTP.get(format!("https://registry.ollama.ai/v2/{name}/manifests/{tag}"))
            .header("Accept", "application/vnd.docker.distribution.manifest.v2+json").timeout(Duration::from_secs(10)).send().await.ok()?;
        if response.status().as_u16() != 200 { return None }
        let j: Value = serde_json::from_slice(&response.bytes().await.ok()?).ok()?;
        Some(j.get("layers")?.as_array()?.iter().map(|l| l.get("size").and_then(Value::as_f64).unwrap_or(0.0)).sum::<f64>() / 1_000_000_000.0)
    }

    /// Downloads a model into Ollama from the model menu or the connect bar, with its progress; the chat says when it's ready.
    pub async fn download_model(&self, model: &str) {
        if self.st().model_downloads.contains_key(model) { return }
        // An Ollama older than its newest release is updated first: a new model wants a new Ollama (the Mac's pull).
        if self.st().local_setup.ollama_version.is_none() { self.read_ollama_versions().await }
        if self.st().local_servers.get("ollama") == Some(&true) && self.needs_new_ollama() { self.install_ollama().await }
        self.st_mut().model_downloads.insert(model.into(), 0.0);
        self.changed();
        let shown = |p: f64| {
            self.st_mut().model_downloads.insert(model.into(), p);
            self.changed();
        };
        let name = catalog::display_name(model);
        match OllamaPull::run(model, &shown).await {
            Ok(()) => {
                self.refresh_models().await;
                self.reply(&self.say(&format!("{name} is downloaded to this PC — pick it in the model menu."),
                                     &format!("{name} скачана на этот ПК — выбери её в меню моделей.")), None);
            }
            // «pull model manifest: 412: The model you are attempting to pull requires a newer version of Ollama.»: the bar
            // and the menu offer the update from then on (the Mac's ollamaOutdated).
            Err(e) if e.message().contains("newer version of Ollama") => {
                let version = {
                    let mut st = self.st_mut();
                    st.local_setup.ollama_outdated = true;
                    st.local_setup.ollama_version.as_ref().map(|v| format!(" {v}")).unwrap_or_default()
                };
                self.reply(&self.say(&format!("Ollama{version} is too old for {name}. Update it (free, ollama.com/download), then press Download again."),
                                     &format!("Ollama{version} слишком старая для {name}. Обнови её (бесплатно, ollama.com/download) и нажми «Скачать» ещё раз.")), None);
            }
            Err(e) => self.reply(&(self.say(&format!("Couldn't download {model}: "), &format!("Не удалось скачать {model}: ")) + e.message()), None),
        }
        self.st_mut().model_downloads.remove(model);
        self.changed();
    }

    /// «Open Ollama»: its app, when it's installed but not running. Only on Windows, as the Swift's `#if os(Windows)`.
    pub fn start_ollama(&self) {
        #[cfg(windows)]
        {
            use std::process::{Command, Stdio};
            // Every place ollama_installed counts: the tray app first, else the server alone.
            let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
            let program = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
            let exists = |p: &String| std::path::Path::new(p).exists();
            let app = [format!("{local}\\Programs\\Ollama\\ollama app.exe"), format!("{program}\\Ollama\\ollama app.exe")].into_iter().find(exists);
            let server = [format!("{local}\\Programs\\Ollama\\ollama.exe"), format!("{program}\\Ollama\\ollama.exe")].into_iter().find(exists);
            let _ = match (app, server) {
                (Some(app), _) => Command::new(app).spawn(),
                (None, Some(server)) => Command::new(server).arg("serve").stdout(Stdio::null()).stderr(Stdio::null()).spawn(),
                (None, None) => return,
            };
        }
    }

    /// «Get Ollama»: its download page, in the person's browser (the Mac installs its signed app itself).
    pub fn get_ollama(&self) { self.out.event("openURL", &HashMap::from([("url", "https://ollama.com/download/windows")])) }
}

/// This PC's memory in bytes (`ProcessInfo.physicalMemory`).
#[cfg(windows)]
fn physical_memory() -> u64 {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut status = MemoryStatusEx {
        length: size_of::<MemoryStatusEx>() as u32, memory_load: 0, total_phys: 0, avail_phys: 0, total_page_file: 0, avail_page_file: 0,
        total_virtual: 0, avail_virtual: 0, avail_extended_virtual: 0,
    };
    // SAFETY: a zeroed MEMORYSTATUSEX with its length set, as the call asks; it only writes into it.
    if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 { status.total_phys } else { 0 }
}

/// Off Windows (tests, a dev run): a 16 GB stand-in, as platform.rs stands in for winsec.
// ponytail: a fixed size; read the system's when a non-Windows build ever ships.
#[cfg(not(windows))]
fn physical_memory() -> u64 { 16 * 1_073_741_824 }

/// Ollama's /api/pull streamed, read line by line as its JSON comes, reporting 0…1 over all the layers (the Mac's
/// OllamaModels.pull).
#[derive(Default)]
pub struct OllamaPull {
    buffer: Vec<u8>,
    layers: HashMap<String, (f64, f64)>,
    shown: Option<f64>,
    failed: Option<String>,
    succeeded: bool,
}

impl OllamaPull {
    pub async fn run(model: &str, progress: &dyn Fn(f64)) -> llm::Result<()> {
        let io = |e: reqwest::Error| llm::Error::Io(e.to_string());
        let mut response = net::HTTP.post(format!("{OLLAMA}/api/pull")).header("Content-Type", "application/json")
            .body(json!({"model": model, "stream": true}).to_string()).timeout(Duration::from_secs(3600)).send().await.map_err(io)?;
        let status = response.status().as_u16();
        let mut pull = OllamaPull::default();
        while let Some(chunk) = response.chunk().await.map_err(io)? { pull.heard(&chunk, progress) }
        pull.finish(status, progress)
    }

    /// Bytes as they come: each whole line read.
    pub fn heard(&mut self, data: &[u8], progress: &dyn Fn(f64)) {
        self.buffer.extend_from_slice(data);
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            self.read(&line[..end], progress);
        }
    }

    /// The stream's end: what's left read, then how it went.
    pub fn finish(mut self, status: u16, progress: &dyn Fn(f64)) -> llm::Result<()> {
        if !self.buffer.is_empty() {
            let rest = std::mem::take(&mut self.buffer);
            self.read(&rest, progress);
        }
        if let Some(failed) = self.failed { return Err(llm::Error::Llm(failed)) }
        if status != 200 { return Err(llm::Error::Llm(format!("Ollama didn't start the download (HTTP {status})."))) }
        if self.succeeded { Ok(()) } else { Err(llm::Error::Llm("Ollama stopped before the download finished.".into())) }
    }

    fn read(&mut self, line: &[u8], progress: &dyn Fn(f64)) {
        let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(line) else { return };
        if let Some(error) = j.get("error").and_then(Value::as_str) {
            self.failed = Some(error.into());
            return;
        }
        if let (Some(digest), Some(total)) = (j.get("digest").and_then(Value::as_str), j.get("total").and_then(Value::as_f64)) && total > 0.0 {
            self.layers.insert(digest.into(), (j.get("completed").and_then(Value::as_f64).unwrap_or(0.0), total));
            let (done, all) = self.layers.values().fold((0.0, 0.0), |(d, t), (ld, lt)| (d + ld, t + lt));
            let p = (done / all).min(1.0);
            if self.shown.is_none_or(|s| (p - s).abs() >= 0.01) {
                self.shown = Some(p);
                progress(p);
            }
        }
        if j.get("status").and_then(Value::as_str) == Some("success") { self.succeeded = true }
    }
}
