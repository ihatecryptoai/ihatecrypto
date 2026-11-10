//! What the Mac's app reads in the chat itself rather than through the shared parsers (ChatCommands.swift): exact
//! commands read before the model (Workspace.send, Wallets.walletSwitch), folders and notes (Canvases.folderCommand), the
//! model's lookups the core doesn't answer (Agent.agentLookup), answers from the portfolio and the closed trades
//! (Activity.answer), and what the chat still does when the model can't be reached. Its WalletScope (whose per-wallet
//! records are in memory) is `State::wallet_scope_costs` / `wallet_scope_tasks`; `Engine.scoped` is `store::scoped`.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use chrono::Utc;
use fancy_regex::Regex;
use ihatecrypto_core::charts::{ChartCommand, Range};
use ihatecrypto_core::folders::FolderCommand;
use ihatecrypto_core::foundation::{Num, Rounding, rounded};
use ihatecrypto_core::intent::{Fmt, IntentParser, Side, TradeIntent};
use ihatecrypto_core::moved_line::MovedLine;
use ihatecrypto_core::network::Hyperliquid;
use ihatecrypto_core::plan::{Market, Plan};
use ihatecrypto_core::pnl::{CostBasis, PnLCommand};
use ihatecrypto_core::risk::{Advice, Level, RiskRules};
use ihatecrypto_core::token_research;
use ihatecrypto_core::tokens::TokenCommand;
use ihatecrypto_core::trade_history::{self, Story};
use llm::catalog::{self, Kind};
use rust_decimal::Decimal;
use signer_api::{SignerRequest, SignerWallet};
use trade::{EvmNetwork, HyperliquidAccount};
use trade::executor::Position;
use unicode_segmentation::UnicodeSegmentation;
use wallet::WalletError;

use crate::canvas::{Board, CanvasDoc};
use crate::engine::{Engine, Pending};
use crate::ledger::LedgerAccount;
use crate::{store, task_seal};

/// Swift's `String(s.prefix(n))`: the first `n` characters as a person sees them.
pub(crate) fn prefix(s: &str, n: usize) -> String { s.graphemes(true).take(n).collect() }

/// Swift's `trimmingCharacters(in: .whitespaces)`: spaces and tabs, not line breaks.
pub(crate) fn trim_spaces(s: &str) -> &str { s.trim_matches(|c: char| c.is_whitespace() && !matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}')) }

macro_rules! regex {
    ($name:ident, $p:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($p).expect("a regex"));
    };
}

// Swift's wholeMatch: the pattern anchored at both ends.
regex!(NEW_CANVAS, r"\A(?:\s*(?:new\s+canvas|новый\s+(?:канвас|холст)|start\s+over|начн[иём]\s+заново)\s*[.!]?\s*)\z");
regex!(CLEAR_PLAN, r"\A(?:\s*(?:clear|reset|remove|drop)\s+(?:the\s+)?plan|(?:очисти|сбрось|убери|удали)\s+план\w*\s*[.!]?\s*)\z");
regex!(WHATS_NEW, r"\A(?:\s*(?:what'?s\s+new(?:\s+in\s+(?:the\s+)?(?:app|ihatecrypto))?|что\s+нового\s+в\s+(?:приложении|ihatecrypto|апп?\w*))\s*[.!?]?\s*)\z");
regex!(UPDATES, r"\A(?:\s*(?:check\s+for\s+updates?|update\s+(?:the\s+)?app|any\s+updates\??|(?:проверь|есть|нет\s+ли)\s+обновлени\w*|обнови(?:сь|\s+(?:приложение|ihatecrypto|апп?ку?)))\s*[.!?]?\s*)\z");
regex!(WALLET_SWITCH, r#"(?i)^\s*(?:переключ\w*|switch|use|используй)\s+(?:на\s+|to\s+)?(?:кошел[её]к\w*|wallet)\s+[«"“]?(.+?)[»"”]?\s*[.!]?\s*$"#);
regex!(SHOWN_WORD, r"(?:charts?|график\w*|чарт\w*|show|покажи)\s+(?:(?:the|token|токен\w*|чарт\w*|график\w*)\s+)*\$?([a-z][a-z0-9]{1,15})\b");
regex!(SELLS, r"short|шорт|прода|sell");
regex!(BUYS, r"\blong|лонг|купи|buy");

fn found(text: &str, pattern: &Regex) -> bool { pattern.is_match(text).unwrap_or(false) }

fn level(a: &Advice) -> &'static str { match a.level { Level::Ok => "ok", Level::Warn => "warning", Level::Block => "blocked" } }

fn side_word(side: Side) -> &'static str { if side == Side::Long { "long" } else { "short" } }

/// Swift's `capitalized`: «лонг» → «Лонг».
fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()).unwrap_or_default()
}

impl Engine {
    // MARK: Exact commands, before the model

    /// «new canvas», «clear the plan», «what's new», «check for updates», «switch to wallet Main»: read before the model so
    /// a wallet's or a folder's name can't be taken for something else. True when the message was one of them.
    pub async fn app_command(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        if found(&lower, &NEW_CANVAS) {
            // The command itself doesn't belong to the canvas being closed.
            let said = self.st_mut().messages_mut().pop();
            if let Err(e) = self.new_canvas(true) {
                if let Some(said) = said { self.st_mut().messages_mut().push(said) }
                self.reply(&e.0, None);
            }
            self.save_chat();
            return true;
        }
        if found(&lower, &CLEAR_PLAN) {
            self.clear_plan(None);
            return true;
        }
        // «what's new»: the versions by date and their stories, over the window (whatsnew.js).
        if found(&lower, &WHATS_NEW) {
            self.out.event("whatsNew", &HashMap::<String, String>::new());
            self.reply(&self.say("What's new is on screen. It's also in the Profile menu.", "Что нового — на экране, а ещё в меню профиля."), None);
            return true;
        }
        // «check for updates»: the window asks the signed update feed (main.rs update_check) and says what it finds.
        if found(&lower, &UPDATES) {
            self.out.event("checkUpdates", &HashMap::<String, String>::new());
            self.reply(&self.say("Checking for a new version: the window says what it finds. An update installs only when it's signed with ihatecrypto's key.",
                                 "Проверяю новую версию: окно скажет, что нашлось. Обновление ставится, только если оно подписано ключом ihatecrypto."), None);
            return true;
        }
        if let Some(target) = self.wallet_switch(text) {
            match target {
                Some(target) => self.switch_wallet(target).await,
                None => {
                    let yours = self.switchable_wallets().iter().map(|w| format!("«{}»", w.label)).collect::<Vec<_>>().join(", ");
                    self.reply(&self.say(&format!("No wallet has that name. Yours: {yours}."), &format!("Кошелька с таким именем нет. Твои: {yours}.")), None);
                }
            }
            return true;
        }
        false
    }

    /// The plan goes, with its card if one waited; a move's card stays (it isn't the plan).
    pub fn clear_plan(&self, note: Option<String>) {
        // The price alert that waited to prepare this plan goes with it (alerts.rs).
        let alert = self.plan_alert();
        if let Some(id) = alert.as_ref().map(|a| a.id) { self.set_alerts(|list| list.retain(|a| a.id != id)) }
        self.st_mut().intent = TradeIntent::default();
        let trade = matches!(self.st().pending, Some(Pending::Trade(_)));
        if trade { self.cancel_pending(None) }
        // Its board goes too, and the plan a trade came from is forgotten (trade_plan.rs), as Cancel does.
        self.drop_plan_board();
        let line = if alert.is_none() { self.say("The plan is cleared. Tell me what to do next.", "План очищен. Скажи, что дальше.") }
                   else { self.say("The plan is cleared, and its price alert with it. Tell me what to do next.", "План очищен, и его алерт тоже. Скажи, что дальше.") };
        self.reply(&line, note);
    }

    /// The wallets a name can switch to: this PC's own (never the automation wallet) and a connected Ledger.
    pub fn switchable_wallets(&self) -> Vec<SignerWallet> {
        let st = self.st();
        let mut list: Vec<SignerWallet> = st.status.as_ref().and_then(|s| s.wallets.as_ref()).into_iter().flatten().filter(|w| w.automation != Some(true)).cloned().collect();
        if let Some(ledger) = st.ledger.as_ref().filter(|l| l.evm.is_some()) { list.push(ledger.wallet()) }
        list
    }

    /// «переключись на кошелёк Main», «switch to wallet Trading»: Some(None) when the name matches none.
    pub fn wallet_switch(&self, text: &str) -> Option<Option<SignerWallet>> {
        let m = WALLET_SWITCH.captures(text).ok().flatten()?;
        let name = m.get(1).map_or("", |g| g.as_str()).to_lowercase();
        let list = self.switchable_wallets();
        Some(list.iter().find(|w| w.label.to_lowercase() == name).or_else(|| list.iter().find(|w| w.label.to_lowercase().starts_with(&name))).cloned())
    }

    pub async fn switch_wallet(&self, wallet: SignerWallet) {
        let (active, current) = {
            let st = self.st();
            let current = st.status.as_ref().and_then(|s| s.active_wallet.clone());
            (if st.ledger_active() { Some(LedgerAccount::ID.to_string()) } else { current.clone() }, current)
        };
        let label = &wallet.label;
        if active.as_ref() == Some(&wallet.id) {
            return self.reply(&self.say(&format!("«{label}» is already the wallet ihatecrypto signs with."), &format!("«{label}» и так подписывает в ihatecrypto.")), None);
        }
        let switched: Result<(), WalletError> = async {
            // Off the Ledger only once the signer said yes (the Mac's switchWallet): a cancelled Windows Hello leaves it on.
            if wallet.id == LedgerAccount::ID { self.use_ledger(true) } else {
                if current.as_ref() != Some(&wallet.id) { self.signer_call(SignerRequest::SetActiveWallet { id: wallet.id.clone() }).await?; }
                self.use_ledger(false);
            }
            Ok(())
        }.await;
        let line = match switched {
            Ok(()) => {
                let what = Self::describe_wallet(&wallet, |e, r| self.say(e, r));
                self.say(&format!("Now signing with «{label}»: {what}."), &format!("Теперь подписывает «{label}»: {what}."))
            }
            Err(e) => self.say("The wallet wasn't switched: ", "Кошелёк не переключён: ") + &e.0,
        };
        self.reply(&line, None);
    }

    // MARK: What's kept per wallet

    /// The wallet that signs now has its own record of what its buys cost (the Mac's walletChanged loads each wallet's
    /// own costbasis): another wallet's cost never reads as this one's P/L. Called with each word from the signer.
    pub fn load_wallet_state(&self) {
        let first = || SignerWallet::FIRST_ID.to_string();
        let (id, tasks_for, costs_for) = {
            let st = self.st();
            (st.status.as_ref().and_then(|s| s.active_wallet.clone()).unwrap_or_else(first),
             st.wallet_scope_tasks.clone().unwrap_or_else(first), st.wallet_scope_costs.clone().unwrap_or_else(first))
        };
        if id != tasks_for {
            self.st_mut().wallet_scope_tasks = Some(id.clone());
            let (tasks, tampered) = task_seal::load(&store::scoped(task_seal::TASKS, Some(&id)), task_seal::KEY);
            self.set_tasks(|t| *t = tasks);
            if tampered {
                self.reply(&self.say("This wallet's scheduled tasks were changed outside ihatecrypto, so every one is paused. Check them in the Schedule and switch on only the ones you made.",
                                     "Задачи этого кошелька изменили вне ihatecrypto, поэтому все они на паузе. Проверь их в «Расписании» и включи только свои."), None);
            }
        }
        if id == costs_for { return }
        self.st_mut().wallet_scope_costs = Some(id.clone());
        let costs: HashMap<String, CostBasis> = store::load(&store::scoped("costs.json", Some(&id))).unwrap_or_default();
        self.set_app_costs(|c| *c = costs);
        self.st_mut().chain_cost.clear();
    }

    /// «recovery phrase · Solana AN5b…e8cL, EVM 0x1523…F656»: what a wallet can sign for (the Mac's Wallets.describe).
    /// Swift's `describe(_:say:)`; favorites.rs has the other `describe`, a token's.
    pub fn describe_wallet(w: &SignerWallet, say: impl Fn(&str, &str) -> String) -> String {
        let short = |s: &str| {
            let c: Vec<char> = s.chars().collect();
            format!("{}…{}", c[..c.len().min(4)].iter().collect::<String>(), c[c.len().saturating_sub(4)..].iter().collect::<String>())
        };
        let kind = match w.kind.as_str() {
            "phrase" => say("recovery phrase", "фраза восстановления"),
            "ledger" => say("Ledger, confirm on the device", "Ledger, подтверждение на устройстве"),
            "solana" => say("Solana key only", "только ключ Solana"),
            _ => say("EVM key only", "только ключ EVM"),
        };
        let parts: Vec<String> = [("Solana", &w.solana), ("EVM", &w.evm), ("NEAR", &w.near)].into_iter()
            .filter_map(|(chain, a)| a.as_deref().map(|a| format!("{chain} {}", short(a)))).collect();
        format!("{kind} · {}", parts.join(", "))
    }

    // MARK: Folders and notes

    /// «save this canvas to folder X» moves the open canvas there; «write to folder X: …» keeps the text there as a note:
    /// a card on a canvas of its own, titled by it. Written on a new canvas, that canvas becomes the note; otherwise the
    /// open canvas stays open and shows the card with «Open». The next note to that folder from there is one more note on
    /// that note's canvas. A typo of a folder that's there is that folder; a new name makes one (Canvases.folderCommand).
    pub fn folder_command(&self, raw: FolderCommand, note: Option<String>) {
        let folders = self.canvas_folders();
        let command = match raw {
            FolderCommand::Save { folder } => FolderCommand::Save { folder: FolderCommand::existing(&folder, &folders).unwrap_or(folder) },
            FolderCommand::Note { folder, text } => FolderCommand::Note { folder: FolderCommand::existing(&folder, &folders).unwrap_or(folder), text },
        };
        match command {
            FolderCommand::Save { folder } => {
                let id = self.st().canvas_id.clone();
                self.move_canvas(&id, Some(&folder));
                self.reply(&self.say(&format!("Saved this canvas to «{folder}»."), &format!("Сохранил этот холст в папку «{folder}».")), note);
            }
            FolderCommand::Note { folder, text } => {
                self.create_folder(&folder);
                let title = if text.graphemes(true).count() > 48 { prefix(&text, 48) + "…" } else { text.clone() };
                let note_board = |opens: Option<String>| Board { lines: vec![text.clone()], opens, ..Board::new("note", &folder) };
                let (joins, fresh, here) = {
                    let st = self.st();
                    let shown: HashSet<&String> = st.boards().iter().filter(|b| b.kind == "note").filter_map(|b| b.opens.as_ref()).collect();
                    let joins = st.canvases.iter().position(|c| {
                        c.folder.as_deref() == Some(folder.as_str()) && c.boards.iter().any(Self::is_own_note) && (c.id == st.canvas_id || shown.contains(&c.id))
                    });
                    (joins, st.messages().iter().filter(|m| m.from_user).count() <= 1 && st.boards().is_empty(), st.canvas_id.clone())
                };
                if let Some(i) = joins {
                    // This canvas is a note in that folder, or shows one: the note joins it.
                    let (texts, opens) = {
                        let mut st = self.st_mut();
                        let c = &mut st.canvases[i];
                        if let Some(b) = c.boards.iter_mut().find(|b| Self::is_own_note(b)) { b.lines.push(text.clone()) }
                        (c.boards.iter().find(|b| Self::is_own_note(b)).map_or_else(|| vec![text.clone()], |b| b.lines.clone()), c.id.clone())
                    };
                    if opens != here { self.update_note_card(&opens, |b| b.lines = texts) }
                } else if fresh {
                    // A new canvas with only this message: it becomes the note, keeping the chat that wrote it.
                    {
                        let mut st = self.st_mut();
                        let i = st.canvas_index();
                        st.canvases[i].folder = Some(folder.clone());
                        st.canvases[i].title = title;
                    }
                    self.pin(note_board(None));
                } else {
                    // A canvas of its own in the folder; this one shows the note's card with «Open».
                    let mut c = CanvasDoc::new(&title);
                    c.folder = Some(folder.clone());
                    c.boards = vec![note_board(None)];
                    let opens = c.id.clone();
                    self.st_mut().canvases.insert(0, c);
                    self.pin(note_board(Some(opens)));
                }
                self.save_canvases();
                self.reply(&self.say(&format!("Saved the note to «{folder}» — it's on the canvas."), &format!("Записал заметку в «{folder}» — она на холсте.")), note);
            }
        }
    }

    /// A note canvas's own notes (not a card showing another canvas's).
    pub fn is_own_note(b: &Board) -> bool { b.kind == "note" && b.opens.is_none() }

    fn update_note_card(&self, opens: &str, change: impl FnOnce(&mut Board)) {
        let Some(i) = self.st().boards().iter().position(|b| b.kind == "note" && b.opens.as_deref() == Some(opens)) else { return };
        self.set_boards(|list| change(&mut list[i]));
    }

    // MARK: Lookups the model asks for

    /// A market's 4-hour ATR read once it's needed; a market not loaded asks too, and keeps nothing.
    async fn read_atr(&self, coin: &str) {
        let missing = self.st().markets.get(coin).is_none_or(|m| m.atr_pct.is_none());
        if !missing { return }
        if let Ok(v) = Hyperliquid::atr_pct(coin, Utc::now()).await && let Some(m) = self.st_mut().markets.get_mut(coin) { m.atr_pct = Some(v) }
    }

    /// «market ETH»: the price, the day, funding and 4h ATR; the open position there; a trade by the book now, each way,
    /// with what the rules say; the week's range (Agent.agentLookup «market»).
    pub async fn market_facts(&self, coin: &str) -> String {
        self.read_atr(coin).await;
        let atr = self.st().markets.get(coin).and_then(|m| m.atr_pct);
        let mut facts: Vec<String> = self.market_line(coin)
            .map(|l| l + &atr.map_or(String::new(), |a| format!(", 4h ATR {}%", Fmt::num(rounded(a, 2, Rounding::Plain), 2)))).into_iter().collect();
        let (position, orders) = { let st = self.st(); (st.positions.iter().find(|p| p.coin == coin).cloned(), st.hl_orders.clone()) };
        if let Some(p) = position {
            facts.push(format!("Open: {}, {}, unrealized {}.", if p.size > Decimal::ZERO { "long" } else { "short" },
                               Self::summary(&p.coin, p.size, p.entry, &orders, |e, _| e.to_string()), Fmt::usd(p.unrealized_pnl)));
        }
        for side in [Side::Long, Side::Short] {
            let Some((line, advice)) = self.book_plan_facts(coin, side) else { continue };
            facts.push(format!("By the book, a {} now: {line}.", side_word(side)));
            facts.extend(advice.iter().map(|a| format!("  rule check ({}): {}", level(a), a.text)));
        }
        if let Ok(candles) = Hyperliquid::candles(coin, Range::Week, Utc::now()).await && let (Some(first), Some(last)) = (candles.first(), candles.last())
            && first.close > Decimal::ZERO && let Some(change) = ((Num::of(last.close) - first.close) / first.close * 100).0 {
            let low = candles.iter().map(|c| c.low).min().unwrap_or_default();
            let high = candles.iter().map(|c| c.high).max().unwrap_or_default();
            facts.push(format!("Week: {} → {} ({}), low {}, high {}.", Fmt::price(first.close), Fmt::price(last.close), self.signed(change), Fmt::price(low), Fmt::price(high)));
        }
        facts.join("\n")
    }

    /// A trade of `asset` by the book now: its risk, stop and take from the rules, and what the rules say about it
    /// (the Mac's TradeAdvice.bookPlan).
    pub fn book_plan_facts(&self, asset: &str, side: Side) -> Option<(String, Vec<Advice>)> {
        let m: Market = self.st().markets.get(asset).cloned()?;
        let trial = TradeIntent { asset: asset.into(), side, auto_rules: Some(true), ..TradeIntent::default() };
        let (equity, policy) = (self.rule_equity(), self.policy());
        let (r, _) = Plan::resolve(&trial, Some(&m), equity, &policy, None);
        let s = Plan::evaluate(&r, Some(&m)).sizing?;
        let stop = r.stop_pct?;
        let mut line = format!("risk {}, stop {} at {}", Fmt::usd(s.loss_at_stop_usd), Fmt::pct(stop), Fmt::price(s.stop_price));
        if let (Some(take), Some(tp), Some(p)) = (r.take_pct, s.take_price, s.profit_at_take_usd) {
            line += &format!(", take {} at {} → +{}", Fmt::pct(take), Fmt::price(tp), Fmt::usd(p));
        }
        let open_risk = { let st = self.st(); HyperliquidAccount::risk_at_stops(&st.positions, &st.hl_orders, &HashMap::new()) };
        let advice = RiskRules::check(&r, &s, &m, equity, &policy, open_risk);
        Some((line, advice))
    }

    /// «rates USDC»: where the asset earns now, best first, with each place's form, what's free and what's already in.
    pub async fn rates_facts(&self, arg: &str) -> String {
        let asset = if arg.to_uppercase().contains("SOL") { "SOL" } else { "USDC" };
        let mut options = self.load_yields(Some(asset)).await;
        options.sort_by_key(|o| std::cmp::Reverse(o.apy));
        let (held, free) = {
            let st = self.st();
            let held: Vec<String> = st.portfolio.networks.iter().flat_map(|n| n.rows.iter().filter(|r| r.section == "defi").map(move |r| {
                format!("{} {}{} on {}", r.symbol, r.amount, r.note.as_ref().map_or(String::new(), |note| format!(" ({note}) ")), n.name)
            })).collect();
            let solana = st.portfolio.networks.iter().find(|n| n.key == "solana");
            let free: Vec<String> = ["USDC", "SOL"].iter().filter_map(|s| {
                solana?.rows.iter().find(|r| r.symbol == *s && r.section == "tokens").map(|r| format!("{} {s}", r.amount))
            }).collect();
            (held, free)
        };
        let lines: Vec<String> = if options.is_empty() { vec!["The rate sources didn't answer.".into()] } else {
            options.iter().take(16).map(|o| {
                format!("{} ({}): {}% a year", o.place, o.asset, Fmt::num(rounded(o.apy, 2, Rounding::Plain), 2))
                    + &o.tvl_usd.map_or(String::new(), |t| format!(", {}M deposited", Fmt::usd(rounded(t / Decimal::from(1_000_000), 1, Rounding::Plain))))
                    + &if o.note.is_empty() { String::new() } else { format!("; {}", o.note) }
                    + &o.how.as_ref().map_or_else(|| "; ihatecrypto can't deposit there".to_string(), |h| format!("; form: {h}"))
            }).collect()
        };
        format!("{}\nFree on Solana: {}.{}", lines.join("\n"), if free.is_empty() { "unknown".to_string() } else { free.join(", ") },
                if held.is_empty() { " Nothing in vaults now.".to_string() } else { format!(" Already in vaults: {}.", held.join(", ")) })
    }

    /// «gas base»: today's gas on an EVM network (all of them when none is named) and what ihatecrypto's calls cost there.
    pub async fn gas_facts(&self, arg: &str) -> String {
        let asked = EvmNetwork::named(trim_spaces(&arg.to_lowercase()));
        let empty = self.st().markets.is_empty();
        if empty {
            let markets = Hyperliquid::markets().await.unwrap_or_default();
            self.st_mut().markets = markets;
        }
        let mut lines = vec![];
        for net in asked.map_or_else(EvmNetwork::searched, |n| vec![n]).into_iter().filter(|n| n.fee_token.is_none()) {
            let price = self.st().markets.get(net.gas_coin).map(|m| m.mark_px);
            let fees = match price { Some(_) => net.rpc.fees().await.ok(), None => None };
            // fees() doubles the base fee for headroom.
            let (Some(price), Some(wei)) = (price, fees.and_then(|(tip, max)| max.checked_sub(tip).map(|d| d / 2 + tip))) else {
                lines.push(format!("{}: gas couldn't be read.", net.name));
                continue;
            };
            let wei = Decimal::from(wei);
            let usd = |units: i64| (Num::of(Decimal::from(units)) * wei / Decimal::from(1_000_000_000_000_000_000u64) * price).0.map_or_else(String::new, Fmt::usd);
            lines.push(format!("{}: {} gwei, {} {} → an exact approve + Relay's deposit (≈ 150k gas) {}; approve + Aave supply (≈ 250k) {}; \
                                approve + Morpho deposit (≈ 500k) {}; a Uniswap swap (≈ 200k) {}; a transfer (≈ 65k) {}.",
                               net.name, Fmt::num(wei / Decimal::from(1_000_000_000), 3), net.gas_coin, Fmt::price(price),
                               usd(150_000), usd(250_000), usd(500_000), usd(200_000), usd(65_000)));
        }
        let lines = if lines.is_empty() { format!("No EVM network by «{arg}».") } else { lines.join("\n") };
        lines + "\nRelay's own fee for a bridge is apart: it is on the move's card. Tempo has no gas coin: its fees come out of the USDC."
    }

    /// «movers»: the day's biggest moves on Hyperliquid, up and down.
    pub fn movers_facts(&self) -> String {
        let mut list: Vec<Market> = self.st().markets.values().filter(|m| m.change24h().is_some() && m.volume24h > Decimal::from(1_000_000)).cloned().collect();
        if list.is_empty() { return "Markets aren't loaded yet.".into() }
        list.sort_by_key(|m| std::cmp::Reverse(m.change24h().unwrap_or_default()));
        let line = |m: &Market| format!("{} {} {}", IntentParser::pretty(&m.name), Fmt::price(m.mark_px), self.signed(m.change24h().unwrap_or_default()));
        format!("Up the most today: {}.\nDown the most: {}.", list.iter().take(6).map(line).collect::<Vec<_>>().join(", "),
                list.iter().rev().take(6).map(line).collect::<Vec<_>>().join(", "))
    }

    // MARK: Questions

    /// What a question is answered from: balances, positions, the closed trades on Hyperliquid, the markets named, a trade
    /// by the book in the direction asked and what the rules say (Activity.answer). Swift's `(facts:, stories:)`.
    pub async fn answer_facts(&self, question: &str) -> (Vec<String>, Vec<Story>) {
        let stories = self.closed_stories().await;
        let mut facts = self.portfolio_facts();
        facts.extend(self.pnl_facts().await);
        for s in stories.iter().filter(|s| !s.is_open()).take(6) {
            let (open, close) = (s.fills.first(), s.fills.last());
            facts.push(format!("Closed: {} {}, opened {} at {}, closed {} at {}, realized {} after fees.", if s.long { "long" } else { "short" },
                               IntentParser::pretty(&s.coin), Fmt::date(s.opened()), open.map_or_else(|| "?".into(), |f| Fmt::price(f.px)),
                               s.closed().map_or_else(|| "?".into(), Fmt::date), close.map_or_else(|| "?".into(), |f| Fmt::price(f.px)), Fmt::usd(s.realized())));
        }
        // Where money went, as the History board says it: the latest day's moves summed, then each request with what was
        // done (the Mac's Activity.answerFacts).
        if let Some(day) = self.moves_day() {
            facts.push(format!("Money moved by ihatecrypto on {}: {}.", Fmt::date(day.day), day.lines.join("; ")));
            if !day.own.is_empty() { facts.push(format!("{} is the user's own wallet: what went there is still theirs, outside the portfolio total.", day.own.join(", "))) }
        }
        let activities = self.history_activities();
        for a in activities[activities.len().saturating_sub(12)..].iter().filter(|a| a.request.is_some() || a.title.is_some()) {
            let done: Vec<String> = a.lines.iter().flat_map(|l| l.split('\n').filter(|s| !s.is_empty()).filter_map(MovedLine::new)).map(|m| m.text).collect();
            let said = a.request.as_ref().map_or_else(|| a.title.clone().unwrap_or_default(), |r| format!("user said “{}”", prefix(r, 120)));
            let then = if done.is_empty() { format!(" → {}", prefix(a.lines.first().map_or("", String::as_str), 200)) } else { format!(" → done: {}", done.join("; ")) };
            facts.push(format!("Activity {}: {said}{then}", Fmt::date(a.at)));
        }
        let (names, intent, coins) = {
            let st = self.st();
            (st.markets.keys().cloned().collect::<HashSet<_>>(), st.intent.clone(), st.positions.iter().map(|p| p.coin.clone()).collect::<Vec<_>>())
        };
        let planned = intent != TradeIntent::default();
        let asked = IntentParser::named_asset(question, &names).or_else(|| planned.then(|| intent.asset.clone()));
        let mut seen = HashSet::new();
        for coin in asked.iter().chain(&coins) {
            if seen.insert(coin) && let Some(line) = self.market_line(coin) { facts.push(line) }
        }
        let q = question.to_lowercase();
        let side = if found(&q, &SELLS) { Some(Side::Short) } else if found(&q, &BUYS) { Some(Side::Long) } else { planned.then_some(intent.side) };
        if let (Some(coin), Some(side)) = (asked, side) {
            self.read_atr(&coin).await;
            if let Some((line, advice)) = self.book_plan_facts(&coin, side) {
                facts.push(format!("By the book, a {} {} now: {line}.", side_word(side), IntentParser::pretty(&coin)));
                facts.extend(advice.iter().map(|a| format!("Rule check ({}): {}", level(a), a.text)));
            }
        }
        (facts, stories)
    }

    /// The trades on Hyperliquid as stories, newest first; none when there's no EVM address or the exchange didn't answer.
    pub async fn closed_stories(&self) -> Vec<Story> {
        let me = self.st().evm_address();
        let Some(me) = me else { return vec![] };
        let Ok(fills) = trade_history::fills(&me).await else { return vec![] };
        let mut stories = trade_history::stories(&fills);
        stories.sort_by_key(|s| std::cmp::Reverse(s.opened()));
        stories
    }

    /// Without a model: the questions that matter most — where is a position, what happened to it, what is open.
    pub async fn answer_without_model(&self, question: &str) {
        let stories = self.closed_stories().await;
        let (positions, names) = { let st = self.st(); (st.positions.clone(), st.markets.keys().cloned().collect::<HashSet<_>>()) };
        let text = Self::local_answer(question, &positions, &stories, &names, |e, r| self.say(e, r));
        self.reply(&text, Some(self.say("from your portfolio and history", "по портфелю и истории")));
    }

    pub fn local_answer(question: &str, positions: &[Position], stories: &[Story], markets: &HashSet<String>, say: impl Fn(&str, &str) -> String) -> String {
        let sided = |long: bool| if long { say("long", "лонг") } else { say("short", "шорт") };
        if let Some(coin) = IntentParser::named_asset(question, markets) {
            let name = IntentParser::pretty(&coin);
            if let Some(p) = positions.iter().find(|p| p.coin == coin) {
                let (side, size, entry, pnl) = (sided(p.size > Decimal::ZERO), Fmt::num(p.size.abs(), 6), Fmt::price(p.entry), Fmt::usd(p.unrealized_pnl));
                return say(&format!("Your {side} {name} is open: {size} at {entry}, unrealized {pnl}."),
                           &format!("Твой {side} {name} открыт: {size} по {entry}, нереализованно {pnl}."));
            }
            if let Some(s) = stories.iter().find(|s| s.coin == coin && !s.is_open()) && let Some(close) = s.fills.last() {
                let (side, when, at, realized) = (sided(s.long), s.closed().map_or(String::new(), Fmt::date), Fmt::price(close.px), Fmt::usd(s.realized()));
                return say(&format!("The {side} {name} is closed: it closed {when} at {at}, realized {realized} after fees. The whole story is in History › Positions."),
                           &format!("{} {name} закрыт: закрылся {when} по {at}, итог {realized} после комиссий. Вся история — в «История › Позиции».", capitalized(&side)));
            }
            return say(&format!("There is no {name} position, open or closed, on Hyperliquid."), &format!("Позиции {name} на Hyperliquid нет — ни открытой, ни закрытой."));
        }
        if positions.is_empty() {
            let last = stories.iter().find(|s| !s.is_open()).map_or(String::new(), |s| {
                let (name, realized) = (IntentParser::pretty(&s.coin), Fmt::usd(s.realized()));
                say(&format!(" The last one: {} {name}, realized {realized}.", if s.long { "long" } else { "short" }),
                    &format!(" Последняя: {} {name}, итог {realized}.", if s.long { "лонг" } else { "шорт" }))
            });
            return say("No open positions.", "Открытых позиций нет.") + &last;
        }
        say("Open: ", "Открыты: ")
            + &positions.iter().map(|p| format!("{} {} {}", sided(p.size > Decimal::ZERO), IntentParser::pretty(&p.coin), Fmt::usd(p.unrealized_pnl))).collect::<Vec<_>>().join(", ")
            + &say(". For anything else, connect a model in Settings.", ". Для остального подключи модель в настройках.")
    }

    // MARK: When the model can't be reached

    /// The model didn't give a reading after two tries: say why in the words that help — not downloaded, refused, an
    /// answer ihatecrypto can't read — and still show what reads the same without a model (a chart, a token, P/L), never
    /// anything that moves money (Workspace.routeWithModel's nil cases).
    pub async fn model_unreachable(&self, text: &str, failure: &str, unreadable: bool, typed_by_user: bool) {
        let model = self.model();
        if unreadable {
            let line = self.say(&format!("{model} answered, but not in a form ihatecrypto can read, so nothing ran. Say it another way, or pick another model."),
                                &format!("{model} ответил, но не в том виде, который ihatecrypto может прочитать, поэтому ничего не запускал. Скажи иначе или выбери другую модель."));
            return self.reply(&line, Some(model));
        }
        if let Some(refusal) = catalog::chat_refusal(&model) {
            return self.reply(&(refusal + &self.say(" Nothing ran. Pick another model.", " Ничего не запускал. Выбери другую модель.")), Some(model));
        }
        // A model on this PC that isn't downloaded answers 404 «model … not found»: say how to get it.
        let id = self.provider_id();
        let local = self.provider(&id).is_some_and(|p| p.info().kind == Kind::Local);
        let why = if local && failure.to_lowercase().contains("not found") {
            if id == "ollama" { self.say(&format!("it isn't downloaded — run «ollama pull {model}» in a terminal"), &format!("она не скачана — выполни в терминале «ollama pull {model}»")) }
            else { self.say("it isn't downloaded", "она не скачана") }
        } else { llm::failure::words(failure) };
        // What happened in the person's words (no credits, too many requests, a key that didn't fit…), never the service's
        // JSON: «NEAR AI: {"error":{"message":"No spending limit configured…» said nothing to do.
        let missing = local && failure.to_lowercase().contains("not found");
        let known = if missing { None } else { self.model_failure_words(failure, &id, &model) };
        let line = if typed_by_user && self.read_displays(text).await {
            match &known {
                Some((why, fix)) => self.say3(&format!("{model} didn’t answer. {why} So I read this one myself. {fix}"),
                                              &format!("{model} не ответил. {why} Поэтому это я прочитал сам. {fix}"),
                                              &format!("{model} no respondió. {why} Así que esto lo leí yo. {fix}")),
                None => self.say(&format!("{model} isn't answering ({why}), so I read this one myself. Pick another model for the rest."),
                                 &format!("{model} не отвечает ({why}), поэтому это я прочитал сам. Для остального выбери другую модель.")),
            }
        } else if let Some((why, fix)) = &known {
            self.say3(&format!("{model} didn’t answer. {why} {fix} I didn’t act on this — nothing moved."),
                      &format!("{model} не ответил. {why} {fix} Ничего не сделал — ничего не ушло."),
                      &format!("{model} no respondió. {why} {fix} No hice nada: no se movió nada."))
        } else {
            self.say(&format!("I couldn’t reach {model} ({why}), so I didn’t act on this — nothing moved. Send it again, or pick another model."),
                     &format!("Не достучался до {model} ({why}), поэтому ничего не сделал — ничего не ушло. Отправь ещё раз или выбери другую модель."))
        };
        self.reply(&line, Some(model));
    }

    /// A provider's name as a sentence takes it: «Claude (subscription via Claude Code)» → «Claude», «Ollama on this PC» →
    /// «Ollama», «OpenAI API» → «OpenAI».
    pub fn service_name(name: &str) -> String {
        let mut n = name.split(" (").next().unwrap_or(name);
        for tail in [" on this PC", " API"] { n = n.strip_suffix(tail).unwrap_or(n) }
        n.to_string()
    }

    /// A model that didn't answer, in the person's words: what happened and what to do, a sentence each, in the chat's
    /// language — never the service's JSON (the Mac's modelFailureWords). None for a failure the chat has no words for:
    /// the service's own sentence is shown then. The service's own page is named where the fix is there.
    fn model_failure_words(&self, failure: &str, provider: &str, model: &str) -> Option<(String, String)> {
        use llm::failure::Failure as F;
        let mut kind = llm::failure::read(failure)?;
        let info = self.provider(provider).map(|x| (x.info().name.to_string(), x.info().kind));
        let local = info.as_ref().is_some_and(|(_, k)| *k == Kind::Local);
        // A program on this PC says in its own words why it failed («model requires more system memory»): «overloaded,
        // send it again later» would never help. A subscription has no key: its 401 is a sign-in that ran out.
        if local && kind == F::Busy { return None }
        if info.as_ref().is_some_and(|(_, k)| *k == Kind::Subscription) && kind == F::Key { kind = F::SignIn }
        // The service by its short name: «Ollama», not «Ollama on this PC isn’t running on this PC».
        let p = Self::service_name(&info.map_or_else(|| provider.to_string(), |(name, _)| name));
        let at = llm::failure::site(provider).map(|u| self.say3(&format!(" at {u}"), &format!(" на {u}"), &format!(" en {u}"))).unwrap_or_default();
        let (why, fix) = match kind {
            F::Credits => (self.say3(&format!("{p} says this account has no credits, or no spending limit is set for the key."),
                                     &format!("{p} пишет, что на счёте нет денег или для ключа не задан лимит расходов."),
                                     &format!("{p} dice que la cuenta no tiene saldo o que la clave no tiene límite de gasto.")),
                           self.say3(&format!("Add credits or set the limit in your account{at}."), &format!("Пополни баланс или задай лимит в кабинете{at}."),
                                     &format!("Añade saldo o fija el límite en tu cuenta{at}."))),
            F::PlanLimit => (self.say3(&format!("The limit of your {p} plan is used up until it resets."), &format!("Лимит {p} исчерпан, он обновится позже."),
                                       &format!("Se agotó el límite de tu plan de {p} hasta que se renueve.")),
                             self.say3("Wait for the reset.", "Подожди, пока он обновится.", "Espera a que se renueve.")),
            F::RateLimit => (self.say3(&format!("{p} got too many requests and asks to wait."), &format!("{p} получил слишком много запросов и просит подождать."),
                                       &format!("{p} recibió demasiadas solicitudes y pide esperar.")),
                             self.say3("Send it again in a minute.", "Отправь ещё раз через минуту.", "Envíalo de nuevo en un minuto.")),
            F::Key => (self.say3(&format!("{p} didn’t take the key. It is wrong, revoked or expired."), &format!("{p} не принял ключ. Он неверный, отозван или истёк."),
                                 &format!("{p} no aceptó la clave. Es incorrecta, fue revocada o caducó.")),
                       self.say3(&format!("Make a new one{at} and paste it in Settings › AI model."), &format!("Создай новый{at} и вставь в Settings › AI model."),
                                 &format!("Crea una nueva{at} y pégala en Settings › AI model."))),
            F::SignIn => (self.say3(&format!("The sign-in to {p} has expired."), &format!("Сессия {p} истекла."), &format!("La sesión de {p} caducó.")),
                          self.say3("Sign in again in Settings › AI model.", "Войди заново в Settings › AI model.", "Inicia sesión de nuevo en Settings › AI model.")),
            F::Access => (self.say3(&format!("This key or account may not use {model} at {p}."), &format!("Этому ключу или аккаунту {p} модель {model} недоступна."),
                                    &format!("Esta clave o cuenta de {p} no puede usar {model}.")), String::new()),
            F::NoModel => (self.say3(&format!("{p} has no model {model}."), &format!("У {p} нет модели {model}."), &format!("{p} no tiene el modelo {model}.")), String::new()),
            F::TooLong => (self.say3(&format!("This conversation is longer than {model} can read."), &format!("Разговор стал длиннее, чем {model} может прочитать."),
                                     &format!("La conversación es más larga de lo que {model} puede leer.")),
                           self.say3("Start a new canvas.", "Начни новый холст.", "Empieza un lienzo nuevo.")),
            F::Busy => (self.say3(&format!("{p} is overloaded or down right now."), &format!("{p} сейчас перегружен или недоступен."), &format!("{p} está sobrecargado o caído ahora.")),
                        self.say3("Send it again a little later.", "Отправь ещё раз чуть позже.", "Envíalo de nuevo un poco más tarde.")),
            F::Offline if local => (self.say3(&format!("{p} isn’t running on this PC."), &format!("{p} не запущен на этом компьютере."), &format!("{p} no está abierto en este PC.")),
                                    self.say3("Open it and send this again.", "Открой его и отправь ещё раз.", "Ábrelo y envíalo de nuevo.")),
            F::Offline => (self.say3(&format!("There’s no connection to {p} from this PC."), &format!("С этого компьютера нет связи с {p}."), &format!("No hay conexión con {p} desde este PC.")),
                           self.say3("Check the internet and send it again.", "Проверь интернет и отправь ещё раз.", "Revisa internet y envíalo de nuevo.")),
            F::Timeout => (self.say3(&format!("{p} took too long to answer."), &format!("{p} отвечал слишком долго."), &format!("{p} tardó demasiado en responder.")),
                           self.say3("Send it again.", "Отправь ещё раз.", "Envíalo de nuevo.")),
            F::Updating => (self.say3("Claude Code is being updated on this PC right now.", "Claude Code сейчас обновляется на этом компьютере.",
                                      "Claude Code se está actualizando en este PC ahora."),
                            self.say3("Send it again in a minute or two.", "Отправь ещё раз через минуту-две.", "Envíalo de nuevo en un minuto o dos.")),
        };
        let another = if fix.is_empty() { self.say3("Pick another model.", "Выбери другую модель.", "Elige otro modelo.") }
                      else { format!("{fix} {}", self.say3("Or pick another model.", "Или выбери другую модель.", "O elige otro modelo.")) };
        Some((why, another))
    }

    /// What shows something without moving money: a token, a chart, P/L, a Solana token by its ticker.
    pub async fn read_displays(&self, text: &str) -> bool {
        let names: HashSet<String> = self.st().markets.keys().cloned().collect();
        if let Some(command) = TokenCommand::parse(text) { self.token_command(command, None).await; return true }
        if let Some(command) = ChartCommand::parse(text, &names) { self.chart(command, None, false).await; return true }
        if let Some(command) = PnLCommand::parse(text, &names) { self.show_pnl(command, None).await; return true }
        let lower = text.to_lowercase();
        let word = SHOWN_WORD.captures(&lower).ok().flatten().and_then(|m| m.get(1).map(|g| g.as_str().to_string()));
        if let Some(word) = word && let Ok(Some(mint)) = token_research::solana_mint(&word).await {
            self.token_command(TokenCommand::Show { address: mint, chain: Some("solana".into()) }, None).await;
            return true;
        }
        false
    }

    /// A wallet removed from the signer: what was kept under its id goes too (the Mac's walletRemoved), so a wallet made
    /// later with the same id never runs the old one's tasks with its own money or shows its P/L.
    pub fn wallet_removed(&self, id: &str) {
        if id.is_empty() { return }
        // «1» too: the signer gives it to the next wallet made into an empty list (the Mac's walletRemoved).
        let list = store::scoped(task_seal::TASKS, Some(id));
        for file in [store::scoped("costs.json", Some(id)), task_seal::seal_name(&list), store::scoped("recurring-buys.json", Some(id)), list] {
            let _ = std::fs::remove_file(store::url(&file));
        }
        // Still the one loaded (the list went empty, so «1» reads as the wallet that's on): emptied, never shown for the
        // next wallet.
        let (tasks_for, costs_for) = {
            let st = self.st();
            let first = || SignerWallet::FIRST_ID.to_string();
            (st.wallet_scope_tasks.clone().unwrap_or_else(first), st.wallet_scope_costs.clone().unwrap_or_else(first))
        };
        if tasks_for == id { self.set_tasks(|t| t.clear()) }
        if costs_for == id { self.set_app_costs(|c| c.clear()) }
    }
}
