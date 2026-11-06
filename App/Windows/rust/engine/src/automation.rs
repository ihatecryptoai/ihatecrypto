//! The automation wallet and scheduled moves (Automation.swift): a budget beside the main wallet that scheduled tasks
//! spend from without Windows Hello. The signer lets it sign only a task's moves, and only to the user's own wallets or
//! addresses already confirmed in person; what its tasks buy goes on to the wallet that's on, and selling it takes
//! Windows Hello there. Here: the task-local wallet a run signs with, what a task does, the automation wallet (made, its
//! runs, what it bought sent on to the wallet that's on: for-main.json) and the recurring buys (recurring-buys.json,
//! their runs at their time with Windows Hello only, token-names.json, recurring-assets.json).

use std::collections::{HashMap, HashSet};

use chrono::{TimeDelta, Utc};
use ihatecrypto_core::foundation::{Date, Num, Uuid, number, text};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::network::Solana;
use ihatecrypto_core::position_command::{ContractCall, PositionCommand, SpotAsset, WalletCommand};
use ihatecrypto_core::scheduled_task::{Every, ScheduledTask};
use ihatecrypto_core::stablecoins;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use signer_api::{SignerAddress, SignerRequest, SignerWallet};

use crate::canvas::Board;
use crate::chains::ChainKind;
use crate::engine::{ChainStep, Engine, Pending};
use crate::from_core::RouterOption;
use crate::modules::CommandRefusal;
use crate::notices::Notice;
use crate::schedule::ScheduleRun;
use crate::snapshot::Running;
use crate::{json, store};

tokio::task_local! {
    /// The automation wallet a scheduled task runs on, for the moves inside that run only (Swift's `TaskRun.automation`;
    /// the executor picks it up). Outside a run: none.
    pub static TASK_RUN: Option<SignerWallet>;
}

/// The automation wallet of the run under way, if any.
pub fn task_run() -> Option<SignerWallet> { TASK_RUN.try_with(Clone::clone).ok().flatten() }

/// Below this the automation wallet can't pay Solana's fee plus a new account's rent (0.005 SOL).
pub const SOL_FLOOR: Decimal = Decimal::from_parts(5, 0, 0, false, 3);

/// What a scheduled task does, read from its steps (Automation.swift's and Schedule.swift's `extension ScheduledTask`).
pub trait TaskFacts {
    /// A step that moves money: without the automation wallet it waits for Confirm and Windows Hello.
    fn moves_money(&self) -> bool;
    /// It changes an open position (close, stop, take, trail): on the automation wallet it runs by itself, on that
    /// wallet's own account, as the Mac's confirmed runSteps carries a position out.
    fn manages_position(&self) -> bool;
    /// It sells a coin: from the wallet that's on, with Windows Hello, never by itself from the automation wallet.
    fn sells(&self) -> bool;
    /// It buys coins: what it gets goes on to the wallet that's on after a run from the automation wallet.
    fn buys_coins(&self) -> bool;
    /// It spreads USDC across the best rates: it spends what's free where it pays from.
    fn spreads_usdc(&self) -> bool;
}

impl TaskFacts for ScheduledTask {
    fn moves_money(&self) -> bool {
        self.steps.iter().any(|s| !matches!(WalletCommand::parse(s),
            None | Some(WalletCommand::Yields(_) | WalletCommand::Receive | WalletCommand::Schedule { .. } | WalletCommand::StopSchedules)))
    }

    fn manages_position(&self) -> bool { self.steps.iter().any(|s| WalletCommand::parse(s).is_none() && PositionCommand::parse(s).is_some()) }

    fn sells(&self) -> bool {
        self.steps.iter().any(|s| matches!(WalletCommand::parse(s), Some(WalletCommand::Sell { .. } | WalletCommand::SellOn { .. }
            | WalletCommand::SellNear { .. } | WalletCommand::SellStock { .. } | WalletCommand::HlSpotSell { .. })))
    }

    fn buys_coins(&self) -> bool {
        self.steps.iter().any(|s| matches!(WalletCommand::parse(s), Some(WalletCommand::Buy { .. } | WalletCommand::BuyStock { .. } | WalletCommand::BuyOn { .. })))
    }

    fn spreads_usdc(&self) -> bool {
        self.steps.iter().any(|s| matches!(WalletCommand::parse(s), Some(WalletCommand::Allocate { symbol, .. }) if symbol == "USDC"))
    }
}

/// A recurring buy as its board on the canvas draws it (the Mac's ScheduleBoard): the buy, how often, the next run and
/// the USDC free on Solana that pays for it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecurringView {
    pub id: String,
    pub symbol: String,
    #[serde(with = "number")]
    pub usdc: Decimal,
    pub days: i64,
    #[serde(with = "json::date")]
    pub next: Date,
    pub paused: bool,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub solana: Option<Decimal>,
}

/// The token a recurring buy's first Confirm bought, by its mint (recurring-assets.json): each run buys that token, as
/// the Mac's schedule keeps its asset, and never checks it against Jupiter's list again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptAsset {
    pub symbol: String,
    pub decimals: i64,
    pub token2022: bool,
}

/// What one main wallet is owed (for-main.json keeps one per wallet), its amounts as Swift writes a Decimal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Owed(#[serde(with = "json::numbers")] HashMap<String, Decimal>);

/// `key.split(separator: ":", maxSplits: 1)`: empty pieces left out, as Swift's split does.
pub(crate) fn split_key(key: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut start = 0;
    for (i, c) in key.char_indices() {
        if c != ':' { continue }
        if i > start {
            out.push(&key[start..i]);
            start = i + 1;
            break;
        }
        start = i + 1;
    }
    if start < key.len() { out.push(&key[start..]) }
    out
}

/// The first `n` characters, as Swift's `String(text.prefix(n))`.
pub(crate) fn prefix(text: &str, n: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).take(n).collect()
}

/// A Date as Swift's `"\(date)"` prints it: «2026-11-06 09:00:00 +0000».
pub(crate) fn date_text(d: Date) -> String { d.format("%Y-%m-%d %H:%M:%S +0000").to_string() }

impl Engine {
    pub fn automation_wallet(&self) -> Option<SignerWallet> {
        self.st().status.as_ref()?.wallets.as_ref()?.iter().find(|w| w.automation == Some(true)).cloned()
    }

    /// A task runs by itself from the automation wallet when there is one, it was typed in this window, it isn't set to pay
    /// from the wallet that's on, and it sells nothing.
    pub fn uses_automation(&self, t: &ScheduledTask) -> bool {
        t.untrusted != Some(true) && t.from_active != Some(true) && !t.sells() && self.automation_wallet().is_some()
    }

    /// Its moves would go with no one at the computer: a new one starts paused, and only its switch in the window — a
    /// click, never a chat message — turns it on.
    pub fn waits_for_switch(&self, t: &ScheduledTask) -> bool { self.uses_automation(t) && (t.moves_money() || t.manages_position()) }

    pub async fn create_automation_wallet(&self, note: Option<&str>) {
        let note = note.map(str::to_string);
        if let Some(w) = self.automation_wallet() {
            self.reply(&self.say(&format!("There's an automation wallet already: «{}».", w.label), &format!("Кошелёк автоматизации уже есть: «{}».", w.label)), note);
            return;
        }
        if let Err(e) = self.signer_call(SignerRequest::CreateAutomationWallet {}).await {
            self.reply(&(self.say("No automation wallet: ", "Кошелёк автоматизации не создан: ") + &e.to_string()), note);
            return;
        }
        // A task made before this wallet would now spend from it by itself: each waits for its switch, as a new one does.
        let held: Vec<usize> = {
            let tasks = self.st().tasks.clone();
            (0..tasks.len()).filter(|&i| tasks[i].paused != Some(true) && self.waits_for_switch(&tasks[i])).collect()
        };
        if !held.is_empty() { self.set_tasks(|t| for &i in &held { t[i].paused = Some(true) }) }
        // Into Addresses on Solana and Base, so «send 20 USDC to Automation» tops it up: the signer counts its own
        // wallets as known, so no Windows Hello for these.
        if let Some(w) = self.automation_wallet() {
            for (address, network) in [(w.solana.clone(), "solana"), (w.evm.clone(), "base")] {
                let Some(address) = address else { continue };
                let saved = self.st().book.iter().any(|e| e.address.to_lowercase() == address.to_lowercase() && e.network == network);
                if saved { continue }
                let _ = self.save_address(SignerAddress { name: w.label.clone(), network: network.into(), address, memo: String::new() }).await;
            }
        }
        self.pin(Board::new("schedule", "schedule"));
        // Its key is under Windows' data protection for this account only (docs/windows.md § Keys): the Mac's «if this
        // Mac's keychain is lost, so is what's on it» (Tasks.swift:493-495), said for this PC.
        let label = self.automation_wallet().map_or_else(|| "Automation".into(), |w| w.label);
        let n = held.len();
        let text = self.say(&format!("Made «{label}»: scheduled tasks spend from it without Windows Hello. Keep only their budget there: if this PC's Windows account is lost, so is what's on it. Send it USDC and a little SOL (its address is on the Schedule block)."),
                            &format!("Создан «{label}»: задачи по расписанию тратят с него без Windows Hello. Держи там только их бюджет: если учётная запись Windows на этом ПК пропадёт, пропадёт и то, что на нём. Отправь на него USDC и немного SOL (адрес на блоке «Расписание»)."))
            + &if held.is_empty() { String::new() } else {
                self.say(&format!(" Tasks made before it would now spend from it by themselves, so they’re paused ({n}): each starts only when you switch it on in the Schedule."),
                         &format!(" Задачи, созданные раньше, теперь тратили бы с него сами, поэтому они на паузе ({n}): каждая начнёт работать, только когда ты включишь её в «Расписании»."))
            };
        self.reply(&text, note);
        if let Some(w) = self.automation_wallet() {
            self.set_options(vec![RouterOption::new(&self.say("Top up 20 USDC", "Пополнить на 20 USDC"),
                                                    &self.say("From your wallet on Solana: Windows Hello once for the new address.", "С твоего кошелька на Solana: Windows Hello один раз на новый адрес."),
                                                    &format!("send 20 USDC to {}", w.label), true)]);
        }
    }

    /// The switch on the Schedule block.
    pub fn set_task(&self, id: &str, on: bool) {
        let Some(i) = self.st().tasks.iter().position(|t| t.id.to_string() == id) else { return };
        self.set_tasks(|t| t[i].paused = if on { None } else { Some(true) });
        self.changed();
    }

    // MARK: What the automation wallet bought, owed to the wallet that's on

    /// «solana:<mint>» (SOL as «solana:SOL») or «<network>:<contract>» → amount; kept per main wallet, so a send that
    /// failed goes with the next run.
    pub fn for_main(&self) -> HashMap<String, Decimal> {
        let id = self.st().active_wallet().map(|w| w.id).unwrap_or_default();
        store::load::<HashMap<String, Owed>>("for-main.json").unwrap_or_default().remove(&id).unwrap_or_default().0
    }

    pub fn set_for_main(&self, owed: HashMap<String, Decimal>) {
        let id = self.st().active_wallet().map(|w| w.id).unwrap_or_default();
        let mut all: HashMap<String, Owed> = store::load("for-main.json").unwrap_or_default();
        if owed.is_empty() { all.remove(&id); } else { all.insert(id, Owed(owed)); }
        store::save(&all, "for-main.json");
    }

    /// A swap on the automation wallet during a task: what it got is owed to the wallet that's on; spending a coin owed
    /// before owes that much less. Dollars stay: they're the budget.
    pub fn owe(&self, key: &str, amount: Decimal) {
        let parts = split_key(key);
        if parts.len() != 2 || stablecoins::on(parts[0]).unwrap_or_default().iter().any(|c| c.address.to_lowercase() == parts[1].to_lowercase()) { return }
        let mut owed = self.for_main();
        match (Num::of(owed.get(key).copied().unwrap_or_default()) + amount).0 {
            Some(left) if left > Decimal::ZERO => { owed.insert(key.into(), left); }
            _ => { owed.remove(key); }
        }
        self.set_for_main(owed);
    }

    /// What its tasks bought goes on to the wallet that's on, as it is, signed by the automation wallet: the signer counts
    /// the user's own wallets as known, so nothing waits on a prompt with no one at the computer.
    pub async fn send_bought_to_main(&self, automation: &SignerWallet) {
        tokio::task::yield_now().await; // the swaps' `owe` comes in a task of its own after the move returns
        let owed = self.for_main();
        let main = self.st().active_wallet();
        let Some(main) = main.filter(|m| !owed.is_empty() && m.id != automation.id) else { return };
        let held = match &automation.solana { Some(a) => Solana::balance(a, false).await.ok(), None => None };
        let mut owed: Vec<(String, Decimal)> = owed.into_iter().collect();
        owed.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, amount) in owed {
            let parts = split_key(&key);
            let mut left = self.for_main();
            if parts.len() != 2 {
                left.remove(&key);
                self.set_for_main(left);
                continue;
            }
            let (network, token) = (parts[0], parts[1]);
            let mut send = amount;
            // What's there now: a later step may have spent some; SOL keeps the floor its fees need.
            if network == "solana" && let Some(held) = &held {
                let there = if token == "SOL" { (Num::of(held.sol) - SOL_FLOOR).0.unwrap_or_default() }
                            else { held.tokens.iter().find(|t| t.contract == token).map_or(Decimal::ZERO, |t| t.amount) };
                send = amount.min(there);
            }
            if send <= Decimal::ZERO {
                left.remove(&key);
                self.set_for_main(left);
                continue;
            }
            let Some(to) = (if network == "solana" { main.solana.clone() } else { main.evm.clone() }) else { continue };
            let sent = TASK_RUN.scope(Some(automation.clone()), async {
                let executor = self.executor().map_err(|e| e.to_string())?;
                executor.send_token(token, Some(send), network, &to, &main.label).await.map_err(|e| e.to_string())
            }).await;
            match sent {
                Ok(line) => {
                    left.remove(&key);
                    self.set_for_main(left);
                    self.reply(&line, Some(self.say(&format!("to «{}»", main.label), &format!("на «{}»", main.label))));
                }
                Err(e) => self.reply(&self.say(&format!("It stays on «{}» for now: {e} The next run sends it on to «{}».", automation.label, main.label),
                                               &format!("Пока остаётся на «{}»: {e} Следующий запуск отправит это на «{}».", automation.label, main.label)), None),
            }
        }
    }

    /// A task's steps on the automation wallet, as the Mac's runSteps(confirmed: true): one confirmed chain, each move
    /// carried out at once and drawn on the canvas as it runs — a send to a saved name, a payment, a step that only shows
    /// something, the next step spending what the one before brought; the first that fails stops the rest.
    /// False when the run stopped before anything moved (a step it can't take, said in the chat).
    pub async fn run_on_automation(&self, task: &ScheduledTask, automation: &SignerWallet) -> bool {
        let started = Utc::now();
        self.reply(&self.say(&format!("From the automation wallet «{}», no Windows Hello.", automation.label),
                             &format!("С кошелька автоматизации «{}», без Windows Hello.", automation.label)), Some("schedule".into()));
        if !TASK_RUN.scope(Some(automation.clone()), self.run_automation_steps(task)).await { return false }
        // Short of SOL, it fails the same way every run: said with what its tasks need for 4 weeks, and that as a card.
        let failed = self.st().boards().iter().any(|b| b.kind == "move" && b.state.as_deref() == Some("failed") && b.at >= started);
        if failed && let Some(a) = &automation.solana && let Ok(read) = Solana::balance(a, false).await && read.sol < SOL_FLOOR {
            let sol = read.sol;
            let need = Fmt::num(self.sol_short(sol).max((Num::of(SOL_FLOOR) - sol).0.unwrap_or_default()), 9);
            let (label, has) = (&automation.label, Fmt::num(sol, 6));
            self.reply(&self.say(&format!("The automation wallet «{label}» has {has} SOL — too little for Solana’s fees and the accounts a move opens. Send it {need} SOL: enough for its tasks for the next 4 weeks."),
                                 &format!("На кошельке автоматизации «{label}» {has} SOL — мало для комиссий Solana и счетов, которые открывает движение. Отправь на него {need} SOL — хватит на его задачи на 4 недели вперёд.")),
                       Some("schedule".into()));
            self.set_options(vec![RouterOption::new(&self.say(&format!("Send {need} SOL to {label}"), &format!("Отправить {need} SOL на {label}")),
                                                    &self.say("From your main wallet: Confirm and Windows Hello.", "С основного кошелька: «Подтвердить» и Windows Hello."),
                                                    &format!("send {need} SOL to {label}"), true)]);
        }
        // A run that stopped at a card (a contract call waits for its Confirm) leaves what it bought for the next run, as
        // the Mac's `if let automation, !waits`.
        let waits = self.st().pending.is_some();
        if !waits { self.send_bought_to_main(automation).await }
        true
    }

    /// The task's steps as the chain a Confirm would run: moves resolved and checked as their cards would be, then
    /// carried out in order on one block. False when a step stopped it before anything ran.
    async fn run_automation_steps(&self, task: &ScheduledTask) -> bool {
        let Some(parsed) = self.automation_steps(task).await else { return false };
        let title = if parsed.len() == 1 { parsed[0].title() } else { task.steps.join(&self.say(" then ", " потом ")) };
        self.run_automation_chain(parsed, &title).await;
        true
    }

    /// A task's steps as chain steps: a Solana token Jupiter-verified first, Modules asked, a send to a saved name as the
    /// transfer to that entry, a contract call or a step that moves nothing run by its words. None when one can't go (said).
    pub async fn automation_steps(&self, task: &ScheduledTask) -> Option<Vec<ChainStep>> {
        let note = Some("schedule".to_string());
        let mut parsed = vec![];
        for step in &task.steps {
            // A contract call is never carried out unseen: its card waits for Confirm (the Mac's runSteps).
            let c = WalletCommand::parse(step).filter(|c| {
                task.moves_money() && matches!(self.chain_kind(step, Some(c)), ChainKind::Move(_)) && ContractCall::parse(step).is_none()
            });
            let Some(c) = c else {
                parsed.push(ChainStep::Said(step.clone()));
                continue;
            };
            // A move that names no amount never runs as «all of it»: the Mac's run asks «How much?» and stops there.
            // Nobody may be at the computer, so nothing moves until the task says how much.
            if let Some(question) = c.amount_question() {
                self.reply(&self.say(&format!("“{step}” doesn't say how much ({question}), so this run stops before anything moves. Change the task to name the amount."),
                                     &format!("В «{step}» нет суммы, поэтому запуск остановлен, ничего не ушло. Измени задачу и укажи сумму.")), note);
                return None;
            }
            let c = match c {
                WalletCommand::Buy { asset, usdc } if !asset.resolved() => WalletCommand::Buy { asset: self.spot_token(&asset, Some("schedule")).await?, usdc },
                WalletCommand::Sell { asset, amount } if !asset.resolved() => WalletCommand::Sell { asset: self.spot_token(&asset, Some("schedule")).await?, amount },
                WalletCommand::JupiterDca { asset, usdc, orders, every } if !asset.resolved() => {
                    WalletCommand::JupiterDca { asset: self.spot_token(&asset, Some("schedule")).await?, usdc, orders, every }
                }
                c => c,
            };
            let ru = self.st().russian();
            if let Some(off) = trade::modules().refusal_command(&c, ru) {
                self.reply(&off, note);
                return None;
            }
            if let WalletCommand::SendTo { name, amount, network } = &c {
                let (entry, why) = self.chain_transfer(name, network.as_deref());
                let Some(e) = entry else {
                    self.reply(&why.unwrap_or_default(), note);
                    return None;
                };
                parsed.push(ChainStep::Transfer(e, *amount));
            } else {
                parsed.push(ChainStep::Wallet(c));
            }
        }
        Some(parsed)
    }

    async fn run_automation_chain(&self, parsed: Vec<ChainStep>, title: &str) {
        let steps = self.steps(&Pending::Chain(parsed.clone()), title, &[]);
        let id = self.pin(Board { steps: Some(steps), state: Some("running".into()), ..Board::new("move", title) });
        {
            let mut st = self.st_mut();
            st.running_board = Some(id.clone());
            st.running = Some(Running { title: title.into(), lines: vec![] });
        }
        self.changed();
        let (said, started) = (self.st().messages().len(), Utc::now());
        // No one may be at the window: Windows Hello waiting on a move is told as a notification.
        let hello = self.watch_for_hello(title);
        match self.run_chain(parsed).await {
            Ok(()) => self.update_board(Some(&id), |b| if b.state.as_deref() == Some("running") { b.state = Some("done".into()) }),
            Err(e) => {
                let since = started - TimeDelta::seconds(1);
                let sent: Vec<_> = self.journal().unresolved().unwrap_or_default().into_iter().filter(|x| x.at >= since).collect();
                self.update_board(Some(&id), |b| b.state = Some(if sent.is_empty() { "failed" } else { "unknown" }.into()));
                let text = if sent.is_empty() {
                    self.say("Failed: ", "Не вышло: ") + &e.to_string()
                } else {
                    self.say("Sent, status unknown: ", "Отправлено, статус неизвестен: ") + &e.to_string() + " "
                        + &self.say("Check before doing it again: ", "Проверь, прежде чем повторять: ")
                        + &sent.iter().map(|x| format!("{} {}", x.chain, x.id)).collect::<Vec<_>>().join(" · ")
                };
                self.reply(&text, Some("schedule".into()));
            }
        }
        hello.abort();
        let outcome = self.st().messages().iter().skip(said).rev().find(|m| !m.from_user).map(|m| m.text.clone());
        self.update_board(Some(&id), |b| if let Some(o) = outcome { b.lines.push(prefix(&o, 400)) });
        {
            let mut st = self.st_mut();
            st.running_board = None;
            st.running = None;
            st.last_own_move = Utc::now();
        }
        self.refresh().await;
    }

    // MARK: Recurring buys (the Mac's runSchedules)

    /// The tasks that are recurring buys: made by a recurring buy's first Confirm, they run at their time from the wallet
    /// that's on with Windows Hello only, as the Mac's schedules do — no card, no waiting for the person to step away.
    /// Kept per wallet, as its tasks are (the Mac keeps schedules per wallet): a switch never turns them into plain tasks.
    pub fn recurring_buys(&self) -> HashSet<Uuid> {
        let file = store::scoped("recurring-buys.json", self.st().wallet_scope_tasks.as_deref());
        store::load(&file).unwrap_or_default()
    }

    pub fn set_recurring_buys(&self, marks: HashSet<Uuid>) {
        let file = store::scoped("recurring-buys.json", self.st().wallet_scope_tasks.as_deref());
        store::save(&marks, &file);
    }

    /// Due recurring buys, one run each; the next date moves on even after a failure, so it doesn't retry in a loop.
    pub async fn run_recurring_buys(&self) {
        let marked = self.recurring_buys();
        let off = { let st = self.st(); st.settings.schedule_paused == Some(true) || st.watching() };
        if marked.is_empty() || off { return }
        // A marker whose task is gone goes with it.
        let ids: HashSet<Uuid> = self.st().tasks.iter().map(|t| t.id).collect();
        if !marked.is_subset(&ids) { self.set_recurring_buys(marked.intersection(&ids).copied().collect()) }
        let now = Utc::now();
        let due: Vec<Uuid> = self.st().tasks.iter().filter(|t| marked.contains(&t.id) && t.paused != Some(true) && t.next <= now).map(|t| t.id).collect();
        for id in due {
            // Found again after every await: «stop recurring buys» or a deletion meanwhile changes the list.
            let found = {
                let st = self.st();
                let task = st.tasks.iter().find(|t| t.id == id).filter(|t| st.running.is_none() && st.signing == 0 && t.next <= Utc::now());
                match task.and_then(|t| WalletCommand::parse(t.steps.first().map_or("", String::as_str))) {
                    Some(WalletCommand::Buy { asset, usdc: Some(usdc) }) => Some((asset, usdc)),
                    _ => None,
                }
            };
            let Some((asset, usdc)) = found else { continue };
            // On a Ledger a purchase needs the device and the person: while it's away the buy stays due and says so once;
            // when it's back, it runs and asks for its approval there (the Mac's runSchedules).
            let ledger = self.st().ledger_active();
            if ledger && !self.ledger_reachable().await {
                let first = self.st_mut().ledger_wait_notified.insert(id);
                if first {
                    let symbol = &asset.symbol;
                    self.notify(Notice::Signature, &self.say(&format!("Scheduled {symbol} buy waits for your Ledger"), &format!("Регулярная покупка {symbol} ждёт Ledger")),
                                &self.say("Connect and unlock the Ledger: the buy runs then and asks you to approve it on the device.",
                                          "Подключи и разблокируй Ledger: покупка запустится и попросит подтверждение на устройстве."),
                                // A run of that buy, as the Mac's `schedule: "buy:…"`: its LAST RUNS and the Schedule's History.
                                false, false, None, Some(id.to_string()));
                }
                continue;
            }
            self.st_mut().ledger_wait_notified.remove(&id);
            let Some(i) = self.st().tasks.iter().position(|t| t.id == id) else { continue };
            let mut task = self.st().tasks[i].clone();
            task.advance(Utc::now());
            let kept = self.st_mut().kept_next.remove(&id.to_string());
            if let Some(kept) = kept && kept > Utc::now() { task.next = kept }
            self.st_mut().run_now_keys.remove(&format!("task:{id}"));
            let saved = task.clone();
            self.set_tasks(|t| t[i] = saved);
            self.recurring_buy(&asset, usdc, &task).await;
        }
    }

    pub fn recurring_views(&self) -> Vec<RecurringView> {
        let marked = self.recurring_buys();
        if marked.is_empty() { return vec![] }
        let free = self.usdc_free("solana");
        let names = self.token_names();
        let tasks = self.st().tasks.clone();
        tasks.iter().filter(|t| marked.contains(&t.id)).filter_map(|t| {
            let Some(WalletCommand::Buy { asset, usdc: Some(usdc) }) = WalletCommand::parse(t.steps.first().map_or("", String::as_str)) else { return None };
            let days = match t.every { Some(Every::Days(n)) => n, Some(Every::Months(n)) => n.saturating_mul(30), Some(Every::Hours(n)) => (n / 24).max(1), None => 1 };
            Some(RecurringView { id: t.id.to_string(), symbol: names.get(&asset.mint).cloned().unwrap_or(asset.symbol), usdc, days: days.max(1), next: t.next,
                                 paused: t.paused == Some(true), solana: free })
        }).collect()
    }

    /// The board's facts for the chat: the buy, how often, the next run, what Solana has for it.
    pub fn recurring_facts(&self, symbol: &str) -> Vec<String> {
        let Some(r) = self.recurring_views().into_iter().find(|r| r.symbol.to_uppercase() == symbol.to_uppercase()) else {
            return vec![format!("stopped: no recurring {symbol} buy now")];
        };
        vec![format!("{} USDC → {} every {} day(s){}", text(r.usdc), r.symbol, r.days, if r.paused { ", paused" } else { "" }),
             format!("next run {}", date_text(r.next)),
             r.solana.map_or_else(|| "USDC on Solana: not read yet".into(), |s| format!("USDC free on Solana: {}", text(s)))]
    }

    /// The symbol each recurring buy kept by its mint goes by (its step names only the mint), from its first buy and each
    /// run since: the Schedule's row and the notices say «SAMO», not the mint.
    pub fn token_names(&self) -> HashMap<String, String> { store::load("token-names.json").unwrap_or_default() }

    pub fn set_token_names(&self, names: HashMap<String, String>) { store::save(&names, "token-names.json") }

    /// «buy token 7xKX… for 5 USDC» as the person reads it: «buy SAMO for 5 USDC».
    pub fn shown_step(&self, step: &str) -> String {
        let Some(WalletCommand::Buy { asset, usdc }) = WalletCommand::parse(step) else { return step.into() };
        if asset.mint.is_empty() { return step.into() }
        let Some(name) = self.token_names().remove(&asset.mint) else { return step.into() };
        format!("buy {name} for {} USDC", usdc.map_or_else(|| "all".into(), |u| Fmt::num(u, 6)))
    }

    /// A recurring buy's coin by its symbol, kept as it's made: its step keeps only the mint.
    pub fn remember_name(&self, buy: &WalletCommand) {
        let WalletCommand::Buy { asset: a, .. } = buy else { return };
        if a.mint.is_empty() || a.symbol.contains('…') || SpotAsset::all().iter().any(|s| s.mint == a.mint) { return }
        let mut names = self.token_names();
        names.insert(a.mint.clone(), a.symbol.clone());
        self.set_token_names(names);
        if a.resolved() {
            let mut kept = self.kept_assets();
            kept.insert(a.mint.clone(), KeptAsset { symbol: a.symbol.clone(), decimals: a.decimals, token2022: a.token2022 });
            self.set_kept_assets(kept);
        }
    }

    /// The token a recurring buy's first Confirm bought, by its mint: each run buys that token, as the Mac's schedule
    /// keeps its asset, and never checks it against Jupiter's list again.
    pub fn kept_assets(&self) -> HashMap<String, KeptAsset> { store::load("recurring-assets.json").unwrap_or_default() }

    pub fn set_kept_assets(&self, kept: HashMap<String, KeptAsset>) { store::save(&kept, "recurring-assets.json") }

    /// The step a recurring buy keeps: a coin off the fixed list by its mint, so each run buys that token and never
    /// another one trading under its ticker (the Mac keeps the schedule's asset).
    pub fn recurring_step(buy: &WalletCommand) -> String {
        match buy {
            WalletCommand::Buy { asset: a, usdc } if !a.mint.is_empty() && *a != SpotAsset::usdc() && !SpotAsset::all().iter().any(|s| s.mint == a.mint) => {
                format!("buy token {} for {} USDC", a.mint, usdc.map_or_else(|| "all".into(), |u| Fmt::num(u, 6)))
            }
            _ => buy.summary(),
        }
    }

    /// One run: no chat line when it goes through, a notification and its record either way (the Mac's notify with
    /// `schedule:`) — the Schedule's History, the task's own canvas and History › Agent list it.
    async fn recurring_buy(&self, asset: &SpotAsset, usdc: Decimal, task: &ScheduledTask) {
        // A coin kept by its mint shows the symbol it had (its step names only the mint).
        let mut name = self.token_names().remove(&asset.mint).unwrap_or_else(|| asset.symbol.clone());
        let mut title = self.say(&format!("{name} purchase"), &format!("Покупка {name}"));
        let started = Utc::now();
        let record = |result: &str, line: &str| {
            let run = ScheduleRun {
                at: started, title: task.steps.iter().map(|s| self.shown_step(s)).collect::<Vec<_>>().join(" then "), result: result.into(),
                line: Some(prefix(line, 300)), canvas: None, task: Some(task.id.to_string()), activity: None,
            };
            self.set_schedule_runs(|r| {
                r.push(run);
                let over = r.len().saturating_sub(100);
                r.drain(..over);
            });
        };
        // Jupiter or Solana switched off: this run is skipped, the schedule stays.
        let ru = self.st().russian();
        if let Some(off) = trade::modules().refusal_command(&WalletCommand::Buy { asset: asset.clone(), usdc: Some(usdc) }, ru) {
            self.notify(Notice::Moves, &self.say(&format!("Scheduled {name} buy skipped"), &format!("Регулярная покупка {name} пропущена")), &off, true, false, None, None);
            record("failed", &off);
            return;
        }
        let mut token = self.kept_assets().remove(&asset.mint).map(|k| SpotAsset { token2022: k.token2022, ..SpotAsset::new(&k.symbol, &asset.mint, k.decimals) });
        if token.is_none() { token = self.spot_token(asset, Some("schedule")).await }
        let Some(token) = token else {
            let said = self.st().messages().iter().rev().find(|m| !m.from_user).map(|m| m.text.clone());
            record("failed", &said.unwrap_or(title));
            return;
        };
        if !asset.mint.is_empty() && token.symbol != name && !SpotAsset::all().iter().any(|s| s.mint == asset.mint) {
            let mut names = self.token_names();
            names.insert(asset.mint.clone(), token.symbol.clone());
            self.set_token_names(names);
            name = token.symbol.clone();
            title = self.say(&format!("{name} purchase"), &format!("Покупка {name}"));
        }
        self.st_mut().running = Some(Running { title: title.clone(), lines: vec![] });
        self.changed();
        // Nobody may be at the computer: a session opened for a few minutes doesn't cover this buy, Windows Hello asks.
        let _ = self.signer.send(&SignerRequest::EndSession {}).await;
        let hello = self.watch_for_hello(&title);
        let bought = match self.executor() {
            Ok(executor) => {
                let equity = self.rule_equity();
                executor.swap(&token, true, Some(usdc), equity, None).await.map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        };
        match bought {
            Ok(line) => {
                self.notify(Notice::Moves, &title, &line, false, false, None, None);
                record("done", &line);
            }
            Err(e) => {
                let next = Fmt::date(task.next);
                let text = self.say(&format!("Scheduled {name} purchase failed: {e} Next one: {next}."), &format!("Регулярная покупка {name} не прошла: {e} Следующая: {next}."));
                self.reply(&text, Some("schedule".into()));
                self.notify(Notice::Moves, &self.say(&format!("{name} purchase failed"), &format!("Покупка {name} не прошла")), &text, true, false, None, None);
                record("failed", &text);
            }
        }
        hello.abort();
        {
            let mut st = self.st_mut();
            st.running = None;
            st.last_own_move = Utc::now();
        }
        self.changed();
        self.refresh().await;
    }
}

#[cfg(test)]
#[path = "automation/tests.rs"]
mod tests;
