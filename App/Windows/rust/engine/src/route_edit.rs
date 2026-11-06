//! A route step's menu and details (RouteEdit.swift, the Mac's RouteCards: «Why this way?», «Edit step…»): what of the
//! waiting move can change — its amount, and for a trade on an EVM network the way (Relay's swap, or Relay's bridge +
//! Uniswap) — and the answer from the step's own facts, which goes in the chat without the model. And a trade plan as
//! its coin's chart draws it (TradeView).
//! Layer 2: step_edit, quote_ways, quote_stock, edit_step, take_way, why_step, draw_trade, note (each move's footer),
//! note_route.

use std::sync::LazyLock;
use std::time::Duration;

use fancy_regex::Regex;
use ihatecrypto_core::foundation::{self, Num, Rounding, number};
use ihatecrypto_core::intent::{Fmt, IntentParser, Side, TradeIntent};
use ihatecrypto_core::plan::{Market, Sizing};
use ihatecrypto_core::position_command::WalletCommand;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use trade::EvmNetwork;
use wallet::WalletError;

use crate::canvas::Step;
use crate::engine::{ChainStep, Engine, Pending, RouteWay, WayQuotes};

/// What the window may change in the move waiting for Confirm: the amount as the command holds it and what it's in, and
/// whether the way can be picked (and which one is).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Edit {
    pub board: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    pub unit: String,
    pub ways: bool,
    /// A Robinhood Chain trade: its ways are compared and the best is taken, nothing to pick (ROUTES · BEST IS TAKEN).
    pub stock: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub way: Option<String>,
    /// Both ways as quoted now, the better first; None while they're still being asked, empty when no way quotes it now
    /// (the Mac's empty swapQuote).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quotes: Option<Vec<Way>>,
}

/// One way of an EVM trade as quoted: «relay» or «uniswap» (or «here» on the network's own USDC), what arrives, and its
/// worth in dollars after gas.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Way {
    pub way: String,
    pub gets: String,
    pub worth: String,
    /// How much less than the best this way leaves, for a Robinhood Chain trade's list; None for the best.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub less: Option<String>,
    /// The worth as a number, for the fee warning on the chat's quote line (the Mac's way.worth).
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usd: Option<Decimal>,
}

fn matches(text: &str, pattern: &'static LazyLock<Regex>) -> bool { pattern.is_match(text).unwrap_or(false) }

macro_rules! regex {
    ($name:ident, $p:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($p).expect("a regex"));
    };
}

regex!(SPACED, r"^\d{1,3}(?:\s\d{3})+(?:[.,]\d+)?$");
regex!(COMMAS, r"^\d{1,3}(?:,\d{3})+(?:\.\d+)?$");
regex!(DOT_OR_COMMAS, r"\.|,.*,");
regex!(DOTS, r"^\d{1,3}(?:\.\d{3})+(?:,\d+)?$");
regex!(COMMA_OR_DOTS, r",|\..*\.");
regex!(PLAIN, r"^(\d+)(?:([.,])(\d+))?$");

/// The field as one amount: «2», «0,5», «1 000», «1,000.50», «1.000,50». Anything else is None, never the number it
/// starts with («5abc» isn't 5), and so is «1,000» alone: a thousand, or one with three decimals. (RouteCards.amount)
pub fn amount(text: &str) -> Option<Decimal> {
    let mut s = text.trim_matches([' ', '\t']).to_string();
    // Thousands apart by spaces: «1 000», «12 500,5».
    if matches(&s, &SPACED) { s.retain(|c| !c.is_whitespace()) }
    // Thousands apart by the other mark: with a decimal mark after them, or more than one of them.
    if matches(&s, &COMMAS) && matches(&s, &DOT_OR_COMMAS) { s.retain(|c| c != ',') }
    else if matches(&s, &DOTS) && matches(&s, &COMMA_OR_DOTS) { s.retain(|c| c != '.') }
    let m = PLAIN.captures(&s).ok().flatten()?;
    let (int, mark, fraction) = (m.get(1).map_or("", |g| g.as_str()), m.get(2).map(|g| g.as_str()), m.get(3).map(|g| g.as_str()));
    if mark == Some(",") && fraction.is_some_and(|f| f.chars().count() == 3) && int.chars().any(|c| c != '0') { return None }
    foundation::decimal(&s.replace(',', "."))
}

/// The way key of a trade on an EVM network (Workspace.wayKey): «base:aero»; None for any other move.
pub fn way_key(command: &WalletCommand) -> Option<String> {
    match command {
        WalletCommand::BuyOn { place, token, .. } | WalletCommand::SellOn { place, token, .. } => Some(format!("{place}:{}", token.to_lowercase())),
        _ => None,
    }
}

/// What the amount is in: USDC for most moves, SOL to stake, the token for a deposit.
pub fn unit(command: &WalletCommand) -> String {
    match command {
        WalletCommand::Stake(_) => "SOL".into(),
        WalletCommand::LendDeposit(symbol, _) | WalletCommand::Kamino { symbol, .. } => symbol.clone(),
        _ => "USDC".into(),
    }
}

/// `String.capitalized`: a letter after anything but a letter (or an apostrophe) upper case, every other letter lower.
fn capitalized(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev: Option<char> = None;
    for c in s.chars() {
        let starts = c.is_alphabetic() && prev.is_none_or(|p| !p.is_alphabetic() && p != '\'');
        if starts { out.extend(c.to_uppercase()) } else { out.extend(c.to_lowercase()) }
        prev = Some(c);
    }
    out
}

/// «BRIDGE · RELAY» → «Bridge · Relay» (StepDetails.pretty).
pub fn pretty(label: &str) -> String {
    capitalized(&label.to_lowercase()).replace(" Or ", " or ").replace("Usdc", "USDC").replace("Jitosol", "JitoSOL").replace("Hlp", "HLP")
        .replace("V3", "v3").replace("Layerzero", "LayerZero").replace("Cctp", "CCTP")
}

/// The step's facts as «Why this way?» quotes them: its title, the balance it changes, its lines.
pub fn facts(step: &Step) -> String {
    std::iter::once(step.title.clone())
        .chain(step.money.iter().map(|m| format!("{} {} → {} {}", m.place, m.was, m.after, m.symbol)))
        .chain(step.lines.iter().filter(|l| !l.is_empty()).map(|l| l.replace('\t', " ")))
        .collect::<Vec<_>>().join(" · ")
}

/// A way as trade's swap_quotes names it («relay», «bridge:Uniswap», «here:KyberSwap», main's) in the words the window's
/// way picker and the route go by (the Swift engine's executor's): «relay», «uniswap» (Relay's bridge and an exchange
/// there), «here» (the network's own USDC).
fn window_way(way: &str) -> String {
    match way.split(':').next().unwrap_or(way) { "bridge" => "uniswap".into(), "here" => "here".into(), _ => way.into() }
}

/// A Robinhood Chain trade.
pub fn is_stock_trade(command: &WalletCommand) -> bool { matches!(command, WalletCommand::BuyStock { .. } | WalletCommand::SellStock { .. }) }

/// Which waiting moves the window gets prices for: a Base or Arbitrum trade's two ways, a Robinhood Chain trade's.
pub fn quoted(command: &WalletCommand) -> bool { way_key(command).is_some() || is_stock_trade(command) }

/// A Robinhood Chain trade's way by name (Workspace.stockWayName).
pub fn stock_way_name(way: &str) -> &'static str {
    match way { "v4" => "Uniswap v4", "v3" => "Uniswap v3", _ => "Relay swap" }
}

/// A trade plan as its coin's chart draws it (the Mac's PlanChartBoard, the owner's pick C): the entry, stop and take
/// across the line with the loss and the gain between them, and the trade's numbers under it. Prices as numbers for the
/// drawing, the rest as the card says it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeView {
    pub coin: String,
    pub side: String,
    pub leverage: String,
    /// A limit order's price rather than the market's now («Limit» against «Entry ≈»).
    pub limit: bool,
    pub entry: f64,
    pub stop: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take: Option<f64>,
    pub notional: String,
    pub size: String,
    pub margin: String,
    pub liquidation: String,
    pub loss_at_stop: String,
    /// The loss at the stop as a share of the portfolio, «1.2»; None without a portfolio read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss_pct: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profit_at_take: Option<String>,
    /// The coin's price alerts, drawn as dashed lines.
    pub alerts: Vec<f64>,
    /// When a card's numbers were read, seconds since 1970: «numbers refreshed N s ago» (the Mac's Execute sheet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared: Option<f64>,
}

impl Engine {
    /// The waiting move's step that can change, as the window shows it: a single move's, or a chain's first (the Mac's
    /// chain card is its first move's RouteCards, the later steps pale); None when nothing can (no move waits, a site's
    /// request, or the command has no amount and no way).
    pub fn step_edit(&self) -> Option<Edit> {
        let command = self.quoted_move()?;
        let quotes = self.quotes(&command);
        let st = self.st();
        let id = st.card.as_ref()?.id.clone();
        let key = way_key(&command);
        let stock = is_stock_trade(&command);
        if command.editable_amount().is_none() && key.is_none() && !stock { return None }
        Some(Edit {
            board: id, amount: command.editable_amount().map(foundation::text), unit: unit(&command), ways: key.is_some(), stock,
            way: key.and_then(|k| st.route_way.as_ref().filter(|r| r.key == k).map(|r| r.way.clone())), quotes,
        })
    }

    /// Both ways of a Base or Arbitrum trade quoted while its card waits (the Mac's quoteSwap): once, asked again every
    /// 15 s while it fails, until the card goes or is asked anew (quote_card's round). Nothing is signed; the executor
    /// quotes again right before signing.
    pub async fn quote_ways(&self, command: &WalletCommand, board: &str, round: i64) {
        if is_stock_trade(command) {
            self.quote_stock(command, board, round).await;
            return;
        }
        let (place, token, usdc, sell, from, keep) = match command {
            WalletCommand::BuyOn { place, token, usdc: Some(u), from } => (place, token, Some(*u), None, from.as_deref(), false),
            WalletCommand::SellOn { place, token, amount, keep } => (place, token, None, *amount, None, *keep),
            _ => return,
        };
        let Some(net) = EvmNetwork::named(place) else { return };
        let Ok(e) = self.executor() else { return };
        while self.quote_live(command, board, round) {
            let ways = e.swap_quotes(net, token, usdc, sell, from, keep).await;
            if !self.quote_live(command, board, round) { return }
            if let Ok(ways) = ways && !ways.is_empty() {
                let ways = ways.into_iter().map(|(way, gets, worth)| Way { way: window_way(&way), gets, worth: Fmt::usd(worth), less: None, usd: Some(worth) }).collect();
                self.set_way_quotes(Some(WayQuotes { command: command.clone(), ways }));
                // The best way and what it gets, on the step it names (move_routes.rs).
                self.redraw_route(board, command);
                return;
            }
            self.quote_failed(command);
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    }

    /// A Robinhood Chain trade's ways while its card waits (the Mac's quoteStock): Uniswap v4, v3 and Relay's swap, the
    /// best first, which Confirm takes; its step says the best and what the others give (stockBuyStep).
    pub async fn quote_stock(&self, command: &WalletCommand, board: &str, round: i64) {
        let (symbol, usdc, sell) = match command {
            WalletCommand::BuyStock { symbol, usdc: Some(u) } => (symbol, Some(*u), None),
            WalletCommand::SellStock { symbol, amount } => (symbol, None, *amount),
            _ => return,
        };
        let Ok(e) = self.executor() else { return };
        while self.quote_live(command, board, round) {
            let ways = e.stock_quotes(symbol, usdc, sell).await;
            if !self.quote_live(command, board, round) { return }
            if let Ok(ways) = ways && let Some(best) = ways.first().map(|w| w.2) {
                let ways = ways.into_iter().enumerate().map(|(i, (way, gets, worth))| Way {
                    way, gets, worth: Fmt::usd(worth), less: if i == 0 { None } else { best.checked_sub(worth).map(Fmt::usd) }, usd: Some(worth),
                }).collect();
                self.set_way_quotes(Some(WayQuotes { command: command.clone(), ways }));
                // The best names the BUY (or SELL) step, the others are its line (move_routes.rs, the Mac's stockBuyStep).
                self.redraw_route(board, command);
                return;
            }
            self.quote_failed(command);
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    }

    /// Save in a step's details (StepDetails.save): the way is kept for this move; a new amount is a new plan, through
    /// the same checks and the same Confirm as if typed.
    pub async fn edit_step(&self, board: Option<&str>, text: Option<&str>, way: Option<&str>) -> Result<(), WalletError> {
        let id = self.st().card.as_ref().map(|c| c.id.clone());
        let (Some(command), Some(id)) = (self.quoted_move(), id) else { return Err(self.not_waiting()) };
        if board.is_some_and(|b| b != id) { return Err(self.not_waiting()) }
        let mut edited = None;
        if let (Some(old), Some(text)) = (command.editable_amount(), text) {
            let Some(value) = amount(text).filter(|v| *v > Decimal::ZERO) else {
                return Err(WalletError(self.say("Type the amount as one number, for example 2.5 or 1000.", "Напиши сумму одним числом, например 2,5 или 1000.")));
            };
            if value != old { edited = Some(command.with_amount(value)) }
        }
        if let Some(key) = way_key(&command) {
            self.st_mut().route_way = way.filter(|w| *w == "relay" || *w == "uniswap").map(|w| RouteWay { key, way: w.into() });
        }
        // The route shows the way at once (its swap step, the approve Uniswap needs), as the Mac's observed routeWays
        // redraws — whatever the amount then does: a new amount refused before its card leaves this card waiting, and
        // its Confirm takes the picked way (take_way), so the card must say it.
        self.redraw_route(&id, &command);
        let Some(edited) = edited else { return Ok(()) };
        let chain = match self.st().pending.as_ref() { Some(Pending::Chain(list)) => Some(list.clone()), _ => None };
        let Some(list) = chain else {
            self.handle_wallet(&edited, None).await;
            return Ok(());
        };
        // A chain's first move edited: checked as that move would be on its own, then the chain drawn again with the rest
        // still behind it (the Mac's Save keeps the task queue).
        self.freshen_source(&edited).await;
        if let Some(why) = self.refusal_before_card(&edited) { return Err(WalletError(why)) }
        let steps: Vec<ChainStep> = std::iter::once(ChainStep::Wallet(edited.clone())).chain(list.into_iter().skip(1)).collect();
        let (title, mut lines) = self.st().card.as_ref().map_or_else(|| (String::new(), vec![]), |c| (c.title.clone(), c.lines.clone()));
        if !lines.is_empty() { lines[0] = format!("1. {}", self.chain_line(&ChainStep::Wallet(edited.clone()))) }
        let cautions = self.send_cautions(&edited);
        self.show(Pending::Chain(steps), &title, lines, vec![], cautions);
        Ok(())
    }

    fn not_waiting(&self) -> WalletError {
        WalletError(self.say("That move isn't waiting for Confirm any more.", "Этот перевод уже не ждёт подтверждения."))
    }

    /// The way picked on the route's step for this trade, taken once: Confirm runs with it, the next move without.
    /// In trade's words: the window's «uniswap» (Relay's bridge and an exchange there) is trade's «bridge», «relay» is
    /// «relay» — buy_on and sell_on refuse a way they don't name.
    pub fn take_way(&self, place: &str, token: &str) -> Option<String> {
        let mut st = self.st_mut();
        let picked = st.route_way.as_ref().filter(|r| r.key == format!("{place}:{}", token.to_lowercase()))?.way.clone();
        st.route_way = None;
        Some(if picked == "uniswap" { "bridge".into() } else { picked })
    }

    /// «Why this way?»: the step's own facts in the chat, without the model; a move waiting for Confirm keeps waiting.
    pub fn why_step(&self, board: Option<&str>, index: Option<i64>) -> Result<(), WalletError> {
        let found = {
            let st = self.st();
            st.boards().iter().find(|b| Some(b.id.as_str()) == board).and_then(|b| {
                let s = b.steps.as_ref()?.get(usize::try_from(index?).ok()?)?.clone();
                // A trade plan keeps its footer in its plan view (the Mac's footer from the plan's route, RouteCards.swift:247).
                Some((s, b.note.clone().or_else(|| b.plan.as_ref().filter(|p| !p.footer.is_empty()).map(|p| p.footer.clone()))))
            })
        };
        let Some((s, footer)) = found else { return Err(WalletError(self.say("That step isn't on the canvas any more.", "Этого шага на холсте уже нет."))) };
        let (name, facts) = (pretty(&s.label), facts(&s));
        let text = self.say(&format!("Why this way — {name}: {facts}."), &format!("Почему так — {name}: {facts}.")) + &footer.map(|f| format!(" {f}")).unwrap_or_default();
        self.reply(&text, Some("from the route".into()));
        Ok(())
    }

    /// The trade plan's numbers on its board, for its chart (Moves.plan calls this after its card is shown).
    pub fn draw_trade(&self, board: Option<&str>, intent: &TradeIntent, sizing: Option<&Sizing>, market: Option<&Market>) {
        let (Some(board), Some(s)) = (board, sizing) else { return };
        let d = foundation::double;
        let pct = self.rule_equity()
            .and_then(|e| if e > Decimal::ZERO { (Num::of(s.loss_at_stop_usd) / e * 100).rounded(1, Rounding::Plain).0.map(|x| Fmt::num(x, 2)) } else { None });
        let places = market.map_or(4, |m| u32::try_from(m.sz_decimals).unwrap_or(0));
        let mut view = {
            let st = self.st();
            TradeView {
                coin: intent.asset.clone(), side: if intent.side == Side::Long { "long" } else { "short" }.into(),
                leverage: Fmt::num(intent.leverage.unwrap_or(Decimal::ONE), 2), limit: intent.entry_px.is_some(), entry: d(s.entry), stop: d(s.stop_price),
                take: s.take_price.map(d), notional: Fmt::usd(s.notional), size: format!("{} {}", Fmt::num(s.size, places), IntentParser::pretty(&intent.asset)),
                margin: Fmt::usd(s.margin), liquidation: format!("≈ {}", Fmt::price(s.liquidation_price)), loss_at_stop: format!("−{}", Fmt::usd(s.loss_at_stop_usd)),
                loss_pct: pct, profit_at_take: s.profit_at_take_usd.map(|p| format!("+{}", Fmt::usd(p))),
                alerts: st.alerts.iter().filter(|a| a.coin == intent.asset).map(|a| d(a.price)).collect(), prepared: None,
            }
        };
        {
            let st = self.st();
            if let Some(Pending::Trade(p)) = st.pending.as_ref() && st.card.as_ref().map(|c| c.id.as_str()) == Some(board) {
                view.prepared = Some(p.prepared_at.timestamp_micros() as f64 / 1_000_000.0);
            }
        }
        self.update_board(Some(board), |b| b.trade = Some(view));
    }

    /// The route's note on its board when its card is shown (Moves.show calls this).
    pub fn note_route(&self, board: &str, pending: &Pending) {
        let note = match pending {
            Pending::Wallet(c) | Pending::Recurring { command: c, .. } => self.route_note(c),
            Pending::Transfer(e, amount) => Some(self.transfer_note(e, *amount)),
            // A chain's card reads as its first move's (the Mac draws the pending action's route and footer), then the
            // chain's own words from its card (chains.rs): which step asks how much, and that they run in order.
            Pending::Chain(list) => {
                let first = list.iter().find_map(|step| match step {
                    ChainStep::Wallet(c) => self.route_note(c),
                    ChainStep::Transfer(e, amount) => Some(self.transfer_note(e, *amount)),
                    ChainStep::Said(_) => None,
                });
                let own: Vec<String> = self.st().card.as_ref().filter(|c| c.id == board).map(|c| c.lines.clone()).unwrap_or_default()
                    .into_iter().filter(|l| !NUMBERED.is_match(l).unwrap_or(false)).collect();
                Some(first.into_iter().chain(own).collect::<Vec<_>>().join(" ")).filter(|n| !n.is_empty())
            }
            // An address to save from the chat (the Mac's PendingAddressBoard): where it came from and what Save asks.
            Pending::Address(e) => Some(self.address_note(e)),
            _ => None,
        };
        if matches!(pending, Pending::Address(_)) {
            let save = self.say("Save", "Сохранить");
            if let Some(card) = self.st_mut().card.as_mut().filter(|c| c.id == board) { card.confirm = save }
        }
        if let Pending::Wallet(c) = pending { self.name_tokens(board, c) }
        let Some(note) = note else { return };
        self.update_board(Some(board), |b| b.note = Some(note));
    }
}

regex!(NUMBERED, r"^\d+\. .*$");

/// What the route signs and how it's checked, as the Mac's MoveRoute footers say it (Canvases.swift): «Why this way?» and
/// a finished step's details quote it. None where the card says it itself (a payment, a call, a multisig).
pub fn note(c: &WalletCommand, say: impl Fn(&str, &str) -> String) -> Option<String> { note_by(c, &say) }

fn note_by(c: &WalletCommand, say: &dyn Fn(&str, &str) -> String) -> Option<String> {
    use WalletCommand as W;
    let name = WalletCommand::place_name;
    let confirm = say(" Press Confirm.", " Нажми «Подтвердить».");
    let drops = say(" Press Confirm; Cancel or “cancel” drops it.", " Нажми «Подтвердить»; «Отмена» или «отмена» в чате её снимет.");
    let out = match c {
        W::WithdrawToSolana(_) => say("Signed with the main key — the trading key can’t withdraw.", "Подписывает основной ключ — торговый ключ выводить не может.") + &drops,
        W::Aave { deposit: true, amount: Some(x), network: n } | W::Morpho { deposit: true, amount: Some(x), network: n } => {
            let morpho = matches!(c, W::Morpho { .. });
            let (n, x) = (name(n), Fmt::num(*x, 2));
            say(&format!("What's missing on {n} comes from Solana over Relay first; then two {n} transactions: approve exactly {x} USDC, then {}. {} re-read from {n} first.",
                         if morpho { "deposit; the shares come to this wallet only" } else { "supply" }, if morpho { "The vault's asset is" } else { "Pool and rate are" }),
                &format!("Если на {n} не хватает, недостающее сначала придёт с Solana через Relay; потом две транзакции на {n}: разрешение ровно на {x} USDC и {}. {} перечитываются с {n} перед этим.",
                         if morpho { "депозит — доли приходят только на этот кошелёк" } else { "вклад" }, if morpho { "Актив хранилища" } else { "Пул и ставка" })) + &confirm
        }
        W::Aave { deposit: false, network: n, .. } | W::Morpho { deposit: false, network: n, .. } => {
            let what = if matches!(c, W::Morpho { .. }) { "Morpho" } else { "Aave" };
            let n = name(n);
            say(&format!("Three {n} transactions: withdraw from {what}, an exact approve and Relay’s deposit — checked field by field before signing."),
                &format!("Три транзакции на {n}: вывод из {what}, точное разрешение и депозит Relay — каждое поле проверяется перед подписью.")) + &confirm
        }
        W::EvmToSolana(n, _) => {
            let n = name(n);
            say(&format!("Two {n} transactions: an exact approve and Relay’s deposit — checked field by field before signing."),
                &format!("Две транзакции на {n}: точное разрешение и депозит Relay — каждое поле проверяется перед подписью.")) + &confirm
        }
        W::BaseToSolana(_) | W::ArbitrumToSolana(_) | W::RobinhoodToSolana(_) => {
            let n = match c { W::BaseToSolana(_) => "base", W::ArbitrumToSolana(_) => "arbitrum", _ => "robinhood" };
            return note_by(&W::EvmToSolana(n.into(), None), say);
        }
        W::SolanaTo(n, _) => {
            let n = name(n);
            say(&format!("One Solana signature for the Relay bridge, checked and simulated before it’s signed; it pays only your own address on {n}."),
                &format!("Одна подпись в Solana для моста Relay, с проверкой и симуляцией перед подписью; платит только на твой же адрес в {n}.")) + &confirm
        }
        W::Bridge { to, .. } | W::Cctp { to, .. } => {
            let to = name(to);
            (if matches!(c, W::Cctp { .. }) {
                say(&format!("Circle's CCTP: an exact approve and the burn (one transaction where it batches); Circle mints on {to} itself, fast-transfer fee plus its gas, a minute or so. "),
                    &format!("CCTP от Circle: точное разрешение и сжигание (одной транзакцией, где сеть это умеет); Circle сам выпускает на {to}, комиссия быстрого перевода плюс газ, около минуты. "))
            } else {
                say("An exact approve and the bridge's deposit, checked field by field before signing: Relay, unless NEAR Intents' 1Click or Circle's CCTP lands more right then. ",
                    "Точное разрешение и депозит моста, каждое поле проверяется перед подписью: Relay, если только 1Click от NEAR Intents или CCTP от Circle не доставят больше прямо сейчас. ")
            }) + &say(&format!("It pays only your own address on {to}."), &format!("Платит только на твой же адрес в {to}.")) + &confirm
        }
        W::Usdt { to, layer_zero, .. } => {
            let to = name(to);
            (if *layer_zero {
                say(&format!("Tether's own bridge, USDT0 on LayerZero: one call sends the USDT with LayerZero's fee in the network's coin, and the same amount is minted on {to} later — in transit on the portfolio until then. "),
                    &format!("Собственный мост Tether, USDT0 на LayerZero: один вызов отправляет USDT с комиссией LayerZero в монете сети, а та же сумма выпускается на {to} позже — до тех пор она «в пути» в портфеле. "))
            } else {
                say("Relay's swap of USDT for USDT: an exact approve and its deposit, checked before signing, seconds. Tether's own bridge on LayerZero moves it for cents of gas but takes longer: say «via LayerZero» for it. ",
                    "Обмен USDT на USDT через Relay: точное разрешение и депозит, проверка перед подписью, секунды. Собственный мост Tether на LayerZero берёт центы газа, но идёт дольше: скажи «через LayerZero». ")
            }) + &say(&format!("It pays only your own address on {to}."), &format!("Платит только на твой же адрес в {to}.")) + &confirm
        }
        W::Gas { place, usd } => {
            let (n, usd) = (name(place), Fmt::usd(usd.unwrap_or(Decimal::ONE)));
            say(&format!("One Solana signature for the Relay bridge: {usd} of the gas coin and half a dollar of USDC arrive at your own address on {n}. When Solana is short, Relay swaps {n}'s own USDC for it there."),
                &format!("Одна подпись в Solana для моста Relay: {usd} монеты для газа и полдоллара USDC придут на твой же адрес в {n}. Если в Solana не хватает, Relay обменяет на неё USDC самой {n}.")) + &confirm
        }
        W::BuyStock { symbol, .. } => {
            // A token by its contract isn't Robinhood's registered one (the Mac's stockRoute footer).
            say("Two signatures on Robinhood Chain (with USDG short there, the bridge comes first): an exact USDG approve and the swap the best way takes (Uniswap v4, v3 or Relay’s swap, compared in the step), which may only pay this wallet. ",
                "Две подписи в Robinhood Chain (если USDG там не хватает, сначала мост): точное разрешение на USDG и обмен лучшим путём (Uniswap v4, v3 или обмен Relay — сравнение в шаге), который может платить только этому кошельку. ")
                + &if symbol.starts_with("0x") {
                    say("The token is taken by its contract: it must trade on Robinhood Chain with $50k+ of liquidity, and the price must sit within 5% of DexScreener's.",
                        "Токен взят по контракту: он должен торговаться в Robinhood Chain с ликвидностью от $50 тыс., а цена — не дальше 5% от DexScreener.")
                } else {
                    say("The token is Robinhood's registered contract, checked on-chain, and the price must sit within 5% of the market.",
                        "Токен — зарегистрированный контракт Robinhood, проверенный в сети, а цена не дальше 5% от рынка.")
                } + &confirm
        }
        W::SellStock { .. } => say("Two Robinhood Chain transactions: an exact approve and the Uniswap swap, which may only pay this wallet.",
                                   "Две транзакции в Robinhood Chain: точное разрешение и обмен в Uniswap, который может платить только этому кошельку.") + &drops,
        W::BuyOn { place, token, .. } | W::SellOn { place, token, .. } => {
            let n = name(place);
            (if matches!(c, W::BuyOn { .. }) {
                say("Relay’s swap is one Solana signature; the other way bridges USDC over Relay and swaps on Uniswap with an exact approve, the swap paying only this wallet. ",
                    "Обмен Relay — одна подпись в Solana; другой путь везёт USDC по мосту Relay и меняет в Uniswap с точным разрешением, обмен платит только этому кошельку. ")
            } else {
                say(&format!("Relay’s swap is an exact approve and its deposit on {n}; the other way swaps on Uniswap with an exact approve, then bridges the USDC over Relay, paying only this wallet. "),
                    &format!("Обмен Relay — точное разрешение и депозит на {n}; другой путь меняет в Uniswap с точным разрешением, потом везёт USDC по мосту Relay, платя только этому кошельку. "))
            }) + &if token.starts_with("0x") {
                say(&format!("A token by its contract must trade on {n} with $50k+ of liquidity."), &format!("Токен по контракту должен торговаться на {n} с ликвидностью от $50 тыс."))
            } else {
                say("The ticker is taken from Uniswap’s token list, its contract checked on-chain.", "Тикер берётся из списка токенов Uniswap, контракт проверяется в сети.")
            } + &confirm
        }
        W::SwapOn { place, .. } if place == "solana" => {
            say("One swap on Solana through Jupiter's best route, no USDC in the middle: simulated before signing, and at least Jupiter's minimum must arrive in this wallet. SOL keeps 0.01 for fees.",
                "Один обмен в Solana лучшим путём Jupiter, без USDC посередине: симуляция перед подписью, и на этот кошелёк должно прийти не меньше минимума Jupiter. На комиссии остаётся 0,01 SOL.") + &confirm
        }
        W::SwapOn { place, to, .. } if place == "near" => {
            let to = to.to_uppercase();
            say(&format!("One deposit to the 1Click address NEAR Intents binds to this quote — checked with 1Click before signing — and {to} comes back to your own NEAR account; a refund goes there too."),
                &format!("Один депозит на адрес 1Click, который NEAR Intents привязывает к этой котировке, — сверяется с 1Click перед подписью, — и {to} возвращается на твой аккаунт NEAR; туда же и возврат.")) + &confirm
        }
        W::SwapOn { place, from, to, .. } if place == "hyperliquid" => {
            let (from, to) = (from.to_uppercase(), to.to_uppercase());
            say(&format!("Two spot orders on Hyperliquid's book, each IOC within 1% of the mid: {from} sold for USDC, that USDC buys {to}. The book takes orders from 10 USDC."),
                &format!("Два спотовых ордера в стакане Hyperliquid, каждый IOC в пределах 1% от середины: {from} продаётся за USDC, на эти USDC покупается {to}. Ордера — от 10 USDC.")) + &confirm
        }
        W::SwapOn { place, .. } => {
            let n = name(place);
            say(&format!("One swap on {n}, no USDC in the middle: the best of Uniswap v2, v3 and v4, at most 0.5–3% under its quote by the price impact, paying only this wallet."),
                &format!("Один обмен на {n}, без USDC посередине: лучший из Uniswap v2, v3 и v4, не хуже котировки больше чем на 0,5–3% по влиянию на цену, платит только этому кошельку.")) + &confirm
        }
        W::CrossSwap { on, to, at, .. } => {
            let (from, b, to) = (name(on), to.to_uppercase(), name(at));
            say(&format!("One signature on {from}: Relay's solver swaps on the way and pays {b} to your own address on {to}, usually within seconds. If more than 3% (or 15 cents) of the value would be lost, nothing is signed."),
                &format!("Одна подпись на {from}: решатель Relay обменивает по пути и платит {b} на твой же адрес в {to}, обычно за секунды. Если потерялось бы больше 3% (или 15 центов) стоимости, ничего не подписывается.")) + &confirm
        }
        W::HlSpotBuy { .. } => say("One spot order on Hyperliquid's book, IOC within 1% of the mid; the spot USDC comes from your perp balance first when short (one more signature of yours). The exchange takes orders from 10 USDC.",
                                   "Один спотовый ордер в стакане Hyperliquid, IOC в пределах 1% от середины; если спотовых USDC не хватает, они сначала придут с баланса фьючерсов (ещё одна твоя подпись). Биржа берёт ордера от 10 USDC.") + &confirm,
        W::HlSpotSell { .. } => say("One spot order on Hyperliquid's book, IOC within 1% of the mid; the USDC lands on your spot balance. The exchange takes orders from 10 USDC.",
                                    "Один спотовый ордер в стакане Hyperliquid, IOC в пределах 1% от середины; USDC придут на спотовый баланс. Биржа берёт ордера от 10 USDC.") + &confirm,
        W::RevokeApproval { place, spender, permit2, .. } => {
            let n = name(place);
            say(&format!("One {n} transaction: {} set to zero for {spender}. Nothing is spent and nobody gains a right; only the gas is paid.", if *permit2 { "Permit2's own approve" } else { "the token's approve" }),
                &format!("Одна транзакция на {n}: {} для {spender} обнуляется. Ничего не тратится, и никто не получает прав; платится только газ.", if *permit2 { "разрешение Permit2" } else { "разрешение токена" })) + &confirm
        }
        W::SweepDust { under } => {
            let under = Fmt::usd(under.unwrap_or(Decimal::from(5)));
            say(&format!("Each verified token worth $0.20 to {under} on Base, Arbitrum and Polygon: quoted on Uniswap first and sold for USDC on its own network with an exact approve, only when the gas takes less than half of it — the rest stays, said by name."),
                &format!("Каждый проверенный токен стоимостью от $0,20 до {under} на Base, Arbitrum и Polygon: сначала котировка в Uniswap, потом продажа за USDC в своей сети с точным разрешением, только если газ съедает меньше половины — остальное остаётся, по названию.")) + &confirm
        }
        W::BuyNear { .. } => say("One transfer: exactly this USDC to the 1Click deposit address the quote names, checked before signing. Native NEAR arrives on your NEAR account in ≈ 30–50 s.",
                                 "Один перевод: ровно эти USDC на адрес депозита 1Click из котировки, с проверкой перед подписью. NEAR придёт на твой аккаунт NEAR за ≈ 30–50 с.") + &confirm,
        W::SellNear { to, .. } => {
            let to = name(to.as_deref().unwrap_or("solana"));
            say(&format!("One NEAR transaction: exactly this NEAR, wrapped and sent to the 1Click deposit address the quote names, checked before signing. USDC arrives at your own address on {to} in ≈ 30–40 s."),
                &format!("Одна транзакция NEAR: ровно этот NEAR, обёрнутый и отправленный на адрес депозита 1Click из котировки, с проверкой перед подписью. USDC придут на твой же адрес в {to} за ≈ 30–40 с.")) + &confirm
        }
        W::SendNear { .. } => say("One NEAR transfer, built and signed by the signer; only this saved address may receive it.",
                                  "Один перевод NEAR, его собирает и подписывает подписант; получить его может только этот сохранённый адрес.") + &confirm,
        W::CloseNear { name: n } => say(&format!("One NEAR transaction: your NEAR account is deleted and NEAR sends everything on it to {n}; buying NEAR later opens the account again."),
                                        &format!("Одна транзакция NEAR: твой аккаунт NEAR удаляется, и всё, что на нём, уходит на {n}; если потом купить NEAR, аккаунт откроется снова.")) + &confirm,
        W::SendSol { .. } => say("One Solana transfer: only this saved address may receive it, and the simulation must show exactly this SOL leaving plus the fee.",
                                 "Один перевод в Solana: получить его может только этот сохранённый адрес, а симуляция должна показать, что уходит ровно этот SOL плюс комиссия.") + &confirm,
        W::SendToken { network, .. } => {
            let n = name(network.as_deref().unwrap_or("solana"));
            say(&format!("One transfer on {n}, the token as it is: only this saved address may receive it."), &format!("Один перевод в {n}, токен как есть: получить его может только этот сохранённый адрес.")) + &confirm
        }
        W::AaveRepay { .. } => say("An approve of what's owed (a tenth of a percent over, for the interest of the next blocks), then the repay — of this wallet's own debt only.",
                                   "Разрешение на сумму долга (на десятую процента больше, на проценты следующих блоков), потом погашение — только собственного долга этого кошелька.") + &confirm,
        W::AaveCollateral { .. } => say("ETH wrapped into WETH, an exact approve, the supply to this wallet: it earns a little and can be borrowed against.",
                                        "ETH оборачивается в WETH, точное разрешение, вклад на этот кошелёк: приносит немного и может быть залогом.") + &confirm,
        W::AaveBorrow { .. } => say("One transaction: Aave lends USDC to this wallet against what it supplied there. If the health factor reaches 1, Aave sells the collateral at a penalty; ihatecrypto won't borrow below 1.5. Pay it back with “repay all USDC on Aave”.",
                                    "Одна транзакция: Aave даёт этому кошельку USDC в долг под то, что на нём лежит. Если показатель здоровья дойдёт до 1, Aave продаст залог со штрафом; ihatecrypto не берёт в долг ниже 1,5. Вернуть: «верни весь USDC на Aave».") + &confirm,
        W::LendDeposit(..) | W::LendWithdraw(..) | W::Kamino { .. } | W::Stake(_) | W::Unstake(_) | W::Hlp { .. } | W::Buy { .. } | W::Sell { .. }
        | W::JupiterDca { .. } | W::JupiterDcaCancel(_) => say("One Solana signature, checked and simulated before it’s signed.", "Одна подпись в Solana, с проверкой и симуляцией перед подписью.") + &drops,
        _ => return None,
    };
    Some(out)
}

#[cfg(test)]
#[path = "route_edit/tests.rs"]
mod tests;
