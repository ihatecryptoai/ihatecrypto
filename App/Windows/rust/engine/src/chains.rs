//! Several moves under one Confirm (Chains.swift): «withdraw 5 USDC to Solana then deposit 5 USDC to kamino», «chart SOL
//! then buy SOL for 10 USDC» — the Mac's runSteps (Tasks.swift). Steps that show or set something run at once; the moves
//! go on one card and one Confirm covers them, the steps between and after them run in turn once it's pressed. A move
//! without its amount asks «How much?» and holds the rest. After Confirm a bridge leg signs and goes on while its money
//! lands, a step waits only for the legs landing where it spends, and spends what arrived when a fee left it a little
//! short.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use ihatecrypto_core::address_book_command::AddressBookCommand;
use ihatecrypto_core::alerts::AlertCommand;
use ihatecrypto_core::charts::ChartCommand;
use ihatecrypto_core::folders::FolderCommand;
use ihatecrypto_core::foundation::{Date, Num, Rounding, rounded};
use ihatecrypto_core::intent::{Fmt, IntentParser, TradeIntent};
use ihatecrypto_core::pnl::PnLCommand;
use ihatecrypto_core::position_command::{PositionCommand, SpotAsset, WalletCommand};
use ihatecrypto_core::stablecoins;
use ihatecrypto_core::tokens::TokenCommand;
use net::solana_rpc::SolanaRpc;
use rust_decimal::Decimal;
use signer_api::SignerAddress;
use tokio::task::JoinHandle;
use trade::{EvmNetwork, Landings};

use crate::automation::task_run;
use crate::canvas::Step;
use crate::engine::{ChainStep, Engine, Pending};
use crate::modules::CommandRefusal;
use crate::moves::EVM_PLACES;
use crate::notices::Notice;
use crate::payments::PriceAsked;

use WalletCommand as W;

/// A chain waiting on the person: the move whose «How much?» is open, with the steps held behind it; and whether the last
/// chain stopped before its card (a scheduled run's History marks it failed).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChainState {
    pub held: Option<Held>,
    pub stopped: bool,
    /// A contract call's card in a chain, and the steps after it: that card's Confirm names them, and once the call is
    /// through they run on it (the Mac's runSteps → continueTask), on the same canvas within half an hour.
    pub after_call: Option<AfterCall>,
    /// The steps held behind a card are running now: the model's request waits for them to end (continueAgent).
    pub running_held: bool,
    /// Cards drawn so far (show()): a held step's Confirm takes only the one card it drew itself.
    pub cards_drawn: i64,
}

/// The move whose «How much?» is open and the steps' words after it.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub command: WalletCommand,
    pub rest: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AfterCall {
    pub board: String,
    pub canvas: String,
    pub rest: Vec<String>,
    pub since: Date,
}

/// What a step of a chain is.
#[derive(Debug, Clone, PartialEq)]
pub enum ChainKind {
    /// It shows or sets something: run when it's reached, before the card if it comes first.
    Now,
    /// A move the chain's card signs.
    Move(WalletCommand),
    /// It has a card of its own (a position, a trade plan, a recurring buy): on the chain's card, run once it's confirmed.
    Later,
}

/// The future of a method on a call cycle (chain → run_canonical → chain; confirm → run_chain → run_said → … → confirm):
/// boxed, so the cycle has a size. On the engine's one thread: not `Send`.
pub type Local<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

impl Engine {
    // MARK: What a chain holds


    /// Whether a step reads as a command ihatecrypto carries out: the parsers `run_canonical` tries (the Mac's knows).
    pub fn knows(&self, step: &str) -> bool {
        let m: HashSet<String> = self.st().markets.keys().cloned().collect();
        let s = step.to_lowercase();
        let s = s.trim_matches(['.', '!', ' ']);
        s == "lock" || s == "clear the plan" || TokenCommand::parse(step).is_some() || ChartCommand::parse(step, &m).is_some()
            || PnLCommand::parse(step, &m).is_some() || AlertCommand::parse(step, &m).is_some()
            || AddressBookCommand::parse(step).is_some() || FolderCommand::parse(step).is_some() || WalletCommand::parse(step).is_some()
            || PositionCommand::parse(step).is_some()
            || (IntentParser::starts_plan(step, &m) && IntentParser::apply(step, &TradeIntent::default(), &m).is_some())
    }

    /// In the order `run_canonical` reads a step: what shows or sets something comes before the moves («chart SOL» names a
    /// coin a plan could start on).
    pub fn chain_kind(&self, step: &str, command: Option<&WalletCommand>) -> ChainKind {
        let m: HashSet<String> = self.st().markets.keys().cloned().collect();
        let s = step.to_lowercase();
        let s = s.trim_matches(['.', '!', ' ']);
        if s == "lock" || s == "clear the plan" || FolderCommand::parse(step).is_some() || AlertCommand::parse(step, &m).is_some()
            || TokenCommand::parse(step).is_some() || ChartCommand::parse(step, &m).is_some() || PnLCommand::parse(step, &m).is_some()
            || AddressBookCommand::parse(step).is_some() {
            return ChainKind::Now;
        }
        if let Some(c) = command {
            return match c {
                W::Receive | W::Yields(_) | W::ShowApprovals | W::JupiterDcaOrders | W::LimitOrders | W::MultisigShow | W::StopSchedules | W::ReceiveAcross { .. } => ChainKind::Now,
                W::Schedule { .. } => ChainKind::Later,
                _ => ChainKind::Move(c.clone()),
            };
        }
        if PositionCommand::parse(step).is_some() || (IntentParser::starts_plan(step, &m) && IntentParser::apply(step, &TradeIntent::default(), &m).is_some()) {
            return ChainKind::Later;
        }
        ChainKind::Now
    }

    // MARK: Before Confirm

    /// Several steps from one message. False when a step isn't something ihatecrypto does (the caller says so).
    /// `first`: the first step as a command already (a «How much?» answered), its words only for the record.
    /// `confirmed`: the steps a confirmed card named (a contract call's, hold_after_call): their card is carried out at once.
    pub fn chain<'a>(&'a self, steps: &'a [String], first: Option<&'a WalletCommand>, note: Option<&'a str>, confirmed: bool) -> Local<'a, bool> {
        Box::pin(async move {
            let command = |i: usize| if i == 0 && first.is_some() { first.cloned() } else { WalletCommand::parse(&steps[i]) };
            if !(0..steps.len()).all(|i| (i == 0 && first.is_some()) || self.knows(&steps[i])) { return false }
            self.st_mut().chain_state.stopped = false;
            let stop = |why: &str| {
                self.reply(why, note.map(Into::into));
                self.st_mut().chain_state.stopped = true;
            };
            let unconfirmed = |step: &str| self.say(&format!("“{step}” didn’t get to its Confirm, so the steps after it don’t run."),
                                                    &format!("«{step}» не дошёл до подтверждения, поэтому следующие шаги не запускаю."));
            // Up to the first step with a card: each runs now, in order.
            let mut k = 0;
            while k < steps.len() && self.chain_kind(&steps[k], command(k).as_ref()) == ChainKind::Now {
                let before = self.card_id();
                if !self.run_canonical(&steps[k], &steps[k]).await {
                    stop(&self.say(&format!("ihatecrypto can’t do “{}” — the steps stop here.", steps[k]), &format!("ihatecrypto не умеет «{}» — на этом шаги останавливаются.", steps[k])));
                    return true;
                }
                k += 1;
                // It drew a card of its own (an address to save): the steps after it wait behind that card's Confirm, as the
                // Mac's runSteps holds its taskQueue behind a pendingAction — a step needing the new name isn't read before
                // it's saved, and no later card takes its place.
                let now = self.card_id();
                if now.is_some() && now != before {
                    self.hold_after_call(&steps[k..]);
                    return true;
                }
            }
            if k >= steps.len() { return true }
            let rest = &steps[k + 1..];
            // One step with a card left: its own card, as when it's asked alone.
            if rest.is_empty() {
                let (before, drawn) = (self.card_id(), self.st().chain_state.cards_drawn);
                match command(k) {
                    Some(W::SendTo { name, amount, network }) => self.prepare_transfer(amount, &name, network.as_deref(), note),
                    Some(c) => self.handle_wallet(&c, note).await,
                    None => { self.run_canonical(&steps[k], &steps[k]).await; }
                }
                // Carried out on the Confirm that named it only when the Mac's runSteps would: a contract call or a trade
                // plan waits on its own card, so one Confirm never signs what no card showed. And only the card this step
                // drew itself: exactly one drawn meanwhile (nothing else may draw while a held run is busy, but a card that
                // isn't provably the step's waits for its own Confirm).
                let (id, carried) = {
                    let st = self.st();
                    (st.card.as_ref().map(|c| c.id.clone()), st.chain_state.cards_drawn == drawn + 1 && Self::carried_on_chain(st.pending.as_ref()))
                };
                if confirmed && let Some(id) = id && Some(&id) != before.as_ref() && carried {
                    // Its card is known by its id now: the chat opens again for the move itself, as after a card's own Confirm
                    // (running_held still keeps scheduled runs and the model's next step out).
                    self.st_mut().thinking = false;
                    self.changed();
                    self.confirm(Some(&id), &[]).await;
                }
                return true;
            }
            // The first move without its amount: asked now, the steps after it held for the answer.
            if let ChainKind::Move(c) = self.chain_kind(&steps[k], command(k).as_ref()) && let Some(question) = c.amount_question() {
                {
                    let mut st = self.st_mut();
                    st.chain_state.held = Some(Held { command: c.clone(), rest: rest.to_vec() });
                    st.awaiting_amount = Some(c.clone());
                }
                self.reply(&(self.say(&question, "Сколько? Напиши число.") + &self.amount_hint(&c)), note.map(Into::into));
                return true;
            }
            let mut parsed: Vec<ChainStep> = vec![];
            // The first move's cautions on the chain's card, as when it's asked alone (the Mac's runSteps draws its card).
            let (mut cautions, mut cautioned): (Vec<String>, bool) = (vec![], false);
            // A later move without its amount: asked when it's reached; it and the steps after it run by their words.
            let mut asks: Option<String> = None;
            for i in k..steps.len() {
                let step = &steps[i];
                let kind = if asks.is_none() { Some(self.chain_kind(step, command(i).as_ref())) } else { None };
                let Some(ChainKind::Move(mut c)) = kind else {
                    parsed.push(ChainStep::Said(step.clone()));
                    continue;
                };
                if c.amount_question().is_some() {
                    asks = Some(step.clone());
                    parsed.push(ChainStep::Said(step.clone()));
                    continue;
                }
                if let Some(missing) = self.missing_key(&c) { stop(&missing); return true }
                let ru = self.st().russian();
                if let Some(off) = trade::modules().refusal_command(&c, ru) { stop(&off); return true }
                // A SOL or NEAR send's recipient before the card, as asked alone: a memo/tag address is refused here.
                if let Err(e) = self.native_recipient(&c) { stop(&e.to_string()); return true }
                // A contract call is checked and simulated on its own card, never carried out on a chain's Confirm: after
                // the moves before it, its card comes for its own Confirm (the Mac's runSteps). One that comes first is
                // asked alone. So is a paid request this card wouldn't price (after the first step) or that no one would see
                // (steps a confirmed card holds): its price on its own card, for its own Confirm (the Mac's paysUnseen).
                let pays = matches!(c, W::Pay { .. }) && (i > k || confirmed);
                if pays || matches!(c, W::ContractCall(_)) {
                    if i > k {
                        parsed.push(ChainStep::Said(step.clone()));
                        continue;
                    }
                    // First: its own card now (a call checked and simulated, a request priced); the steps after it ride on
                    // that card's Confirm.
                    let before = self.card_id();
                    if pays { self.prepare_payment(&c, note.map(Into::into)).await } else { self.prepare_contract_call(&c, note.map(Into::into)).await }
                    let drawn = {
                        let st = self.st();
                        matches!(st.pending, Some(Pending::Wallet(W::ContractCall(_) | W::Pay { .. }))) && st.card.as_ref().map(|c| c.id.clone()) != before
                    };
                    if !drawn {
                        // Refused before its card: the rest doesn't run, and a scheduled run reads it as stopped.
                        if i + 1 < steps.len() { stop(&unconfirmed(step)) } else { self.st_mut().chain_state.stopped = true }
                        return true;
                    }
                    self.hold_after_call(&steps[i + 1..]);
                    return true;
                }
                // A multisig made or paid from: its own card, checked as when asked alone (prepare_multisig/-send); the steps
                // after it ride on that card's Confirm (the Mac's runSteps → continueTask). Later in a chain: by its words.
                if matches!(c, W::MultisigCreate { .. } | W::MultisigSend { .. }) {
                    if self.multisig_step(&c, i, k, steps, note, &stop).await { return true }
                    parsed.push(ChainStep::Said(step.clone()));
                    continue;
                }
                // A Solana token is Jupiter-verified before the card, as a buy asked alone is.
                let unresolved = match &c {
                    W::Buy { asset, .. } | W::Sell { asset, .. } | W::JupiterDca { asset, .. } if !asset.resolved() => Some(asset.clone()),
                    _ => None,
                };
                if let Some(asset) = unresolved {
                    let Some(token) = self.spot_token(&asset, note).await else {
                        self.st_mut().chain_state.stopped = true;
                        return true;
                    };
                    c = match c {
                        W::Buy { usdc, .. } => W::Buy { asset: token, usdc },
                        W::Sell { amount, .. } => W::Sell { asset: token, amount },
                        W::JupiterDca { usdc, orders, every, .. } => W::JupiterDca { asset: token, usdc, orders, every },
                        other => other,
                    };
                }
                // A paid request first, on a card the person sees: its price asked now, so the chain's card shows what it
                // pays (the Mac's runSteps → preparePayment); pay() takes this quote at Confirm.
                if i == k && matches!(c, W::Pay { .. }) {
                    match self.ask_price(&c).await {
                        Ok(PriceAsked::Answered(text)) => {
                            self.reply(&text, note.map(Into::into));
                            stop(&unconfirmed(step));
                            return true;
                        }
                        Ok(PriceAsked::Quoted(q)) => self.st_mut().payment = Some(*q),
                        Err(e) => { stop(&e.to_string()); return true }
                    }
                }
                if let W::SendTo { name, amount, network } = &c {
                    let (entry, why) = self.chain_transfer(name, network.as_deref());
                    let Some(e) = entry else { stop(&why.unwrap_or_default()); return true };
                    // The first step reads the balances as they are now; later ones spend what the steps before bring.
                    if i == k && let Some(short) = self.transfer_shortfall(&e.network, *amount) {
                        stop(&short);
                        self.note_refusal(step, &short, note);
                        return true;
                    }
                    // The chain's first move carries its cautions, a plan or another card before it or not (T13).
                    if !cautioned {
                        let saved_for = self.entry(name, network.as_deref()).map_or_else(|| e.network.clone(), |s| s.network);
                        cautions = [self.other_network_warning(&saved_for, &e.network), self.first_send_warning(&e.address)].into_iter().flatten().collect();
                        cautioned = true;
                    }
                    parsed.push(ChainStep::Transfer(e, *amount));
                } else {
                    if i == k {
                        // Its source read fresh first, as a move asked alone is (freshen_source).
                        self.freshen_source(&c).await;
                        if let Some(why) = self.refusal_before_card(&c) {
                            stop(&why);
                            self.note_refusal(&c.summary(), &why, note);
                            return true;
                        }
                    }
                    if !cautioned {
                        cautions = self.send_cautions(&c);
                        cautioned = true;
                    }
                    parsed.push(ChainStep::Wallet(c));
                }
            }
            let later = asks.map(|a| self.say(&format!("“{a}” asks how much when it's reached, then comes on a card of its own, and the steps after it wait for that."),
                                              &format!("«{a}» спросит сумму, когда до него дойдёт, и придёт своей карточкой. Шаги после него ждут этого.")));
            let count = parsed.len();
            let lines: Vec<String> = parsed.iter().enumerate().map(|(i, s)| format!("{}. {}", i + 1, self.chain_line(s)))
                .chain(later)
                .chain([self.say("They run in order. The first that fails stops the rest. Windows Hello asks for each move that signs.",
                                 "Идут по порядку. Первый неудачный останавливает остальные. Windows Hello спросит на каждом подписываемом шаге.")])
                .collect();
            let title = self.say3(&format!("{count} steps, one Confirm"), &format!("Шагов: {count}, одно подтверждение"), &format!("{count} pasos, un Confirm"));
            self.show(Pending::Chain(parsed.clone()), &title, lines, vec![], cautions);
            if confirmed {
                let id = self.card_id();
                self.st_mut().thinking = false;
                self.changed();
                self.confirm(id.as_deref(), &[]).await;
                return true;
            }
            let said = parsed.iter().map(|s| self.chain_line(s)).collect::<Vec<_>>().join(" → ");
            self.reply(&(self.say3("In steps: ", "По шагам: ", "Por pasos: ") + &said + &self.say3(". Press Confirm, or Cancel.", ". Нажми «Подтвердить» или «Отмена».", ". Pulsa Confirm o Cancel.")),
                       note.map(Into::into));
            true
        })
    }

    /// A multisig step: first in the chain, its own card now with the rest held behind it (true: the chain is handled);
    /// later, false — it runs by its words once reached and draws its card then.
    async fn multisig_step(&self, c: &WalletCommand, i: usize, k: usize, steps: &[String], note: Option<&str>, stop: &dyn Fn(&str)) -> bool {
        if i != k { return false }
        let before = self.card_id();
        self.handle_wallet(c, note).await;
        let drawn = {
            let st = self.st();
            matches!(st.pending, Some(Pending::Wallet(W::MultisigCreate { .. } | W::MultisigSend { .. }))) && st.card.as_ref().map(|c| c.id.clone()) != before
        };
        if !drawn {
            if i + 1 < steps.len() {
                let step = &steps[i];
                stop(&self.say(&format!("“{step}” didn’t get to its Confirm, so the steps after it don’t run."),
                               &format!("«{step}» не дошёл до подтверждения, поэтому следующие шаги не запускаю.")));
            } else {
                self.st_mut().chain_state.stopped = true;
            }
            return true;
        }
        self.hold_after_call(&steps[i + 1..]);
        true
    }

    /// A card a chain stopped at (a contract call, an address to save…) with steps after it: its card names them, its
    /// Confirm covers them, and they run once it's through (the Mac's taskQueue → continueTask). Nothing is held when no
    /// card was drawn; a trade plan's card holds nothing (the steps after it go on, as on the Mac).
    pub fn hold_after_call(&self, rest: &[String]) {
        let (call, id) = {
            let st = self.st();
            let (Some(p), Some(card)) = (&st.pending, &st.card) else { return };
            if rest.is_empty() || matches!(p, Pending::Trade(_)) { return }
            (matches!(p, Pending::Wallet(W::ContractCall(_))), card.id.clone())
        };
        let then = self.say("Then, on this Confirm, once it’s through: ", "Потом, на этом же «Подтвердить», когда он пройдёт: ") + &rest.join(" → ");
        if let Some(card) = self.st_mut().card.as_mut() { card.lines.push(then.clone()) }
        self.update_board(Some(&id), |b| b.lines.push(then));
        {
            let mut st = self.st_mut();
            let canvas = st.canvas_id.clone();
            st.chain_state.after_call = Some(AfterCall { board: id, canvas, rest: rest.to_vec(), since: chrono::Utc::now() });
        }
        let line = if call {
            self.say("The call waits on its card. Its Confirm covers the steps after it too — they run once it’s through.",
                     "Вызов ждёт на своей карточке. Его «Подтвердить» распространяется и на следующие шаги — они пойдут, когда он пройдёт.")
        } else {
            self.say("It waits on its card. Its Confirm covers the steps after it too — they run once it’s through.",
                     "Ждёт на своей карточке. Её «Подтвердить» распространяется и на следующие шаги — они пойдут, когда он пройдёт.")
        };
        self.reply(&line, None);
    }

    /// What a confirmed run carries out on the Confirm that named it, as the Mac's runSteps carries out a pendingAction: a
    /// move, a transfer, a position, its orders, an address, a recurring buy's first buy (Execution.swift:684-703) — never
    /// a contract call (its own simulated card waits), a paid request (its price, never seen yet: the Mac's paysUnseen) or
    /// a trade plan (its own route and checklist).
    pub fn carried_on_chain(p: Option<&Pending>) -> bool {
        match p {
            Some(Pending::Wallet(W::Pay { .. })) => false,
            // A multisig's own card (its keys, networks and cost; its payment's check) waits for its own Confirm. Stricter
            // than the Mac's runSteps, which carries out the multisig it just drew without showing it for adjustment.
            Some(Pending::Wallet(W::ContractCall(_) | W::MultisigCreate { .. } | W::MultisigSend { .. })) => false,
            Some(Pending::Wallet(_) | Pending::Transfer(..) | Pending::Position { .. } | Pending::Address(_) | Pending::Orders { .. } | Pending::Recurring { .. }) => true,
            Some(Pending::Trade(_) | Pending::Chain(_)) | None => false,
        }
    }

    /// The call's Confirm went through: the steps it named run now, as on the chain's own Confirm.
    pub async fn continue_after_call(&self, board: Option<&str>) {
        // Busy for the whole run, as the Mac's runSteps holds `thinking`: no scheduled run (run_due_tasks reads
        // running_held) and no message lands in the middle, so the card a held step draws is the only one there is.
        self.st_mut().chain_state.running_held = true;
        let held = self.st().chain_state.after_call.clone();
        let Some(held) = held.filter(|h| Some(h.board.as_str()) == board) else {
            self.st_mut().chain_state.running_held = false;
            return;
        };
        {
            let mut st = self.st_mut();
            st.chain_state.after_call = None;
            st.thinking = true;
        }
        self.changed();
        self.run_after_call(&held).await;
        // The request the model carries on waits for the held steps, then hears how they went (the Mac's afterMove).
        let agent = {
            let mut st = self.st_mut();
            st.thinking = false;
            st.agent_goal.is_some()
        };
        self.changed();
        if agent {
            let me = self.rc();
            tokio::task::spawn_local(async move { me.continue_agent().await });
        }
        self.st_mut().chain_state.running_held = false;
    }

    async fn run_after_call(&self, held: &AfterCall) {
        // The call's own Confirm finishes first (its block marked, the signing session ended): then the rest.
        let busy = |e: &Engine| {
            let st = e.st();
            st.running.is_some() || st.running_board.is_some()
        };
        for _ in 0..100 {
            if !busy(self) { break }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if busy(self) { return }
        let canvas = self.st().canvas_id.clone();
        if held.canvas != canvas || chrono::Utc::now() - held.since >= chrono::TimeDelta::seconds(1800) { return }
        self.chain(&held.rest, None, None, true).await;
        self.refreeze_request();
    }

    /// A step as the chain's card names it: a paid request with its price («pay 0.01 USDC to StableEnrich»).
    pub fn chain_line(&self, s: &ChainStep) -> String {
        if let ChainStep::Wallet(c @ W::Pay { .. }) = s {
            let quote = self.st().payment.clone();
            if let Some(q) = quote && q.command == *c {
                let usdc = Fmt::num(q.challenge.usdc(), 6);
                return self.say(&format!("pay {usdc} USDC to {}", q.service), &format!("оплатить {usdc} USDC · {}", q.service));
            }
        }
        s.title()
    }

    /// A USDC transfer to a saved name as a step of a chain: the entry on the network the step names (a 0x address saved
    /// on one EVM network is the same on another), or why it can't be one.
    pub fn chain_transfer(&self, name: &str, network: Option<&str>) -> (Option<SignerAddress>, Option<String>) {
        let entry = self.entry(name, network)
            .filter(|e| e.network != "near" && protocols::one_click::asset::across(&e.network).is_empty() && (e.memo.is_empty() || e.network != "solana"));
        let Some(mut e) = entry else {
            return (None, Some(self.say(&format!("“{name}” can't take a USDC transfer in a chain: send it on its own."), &format!("«{name}» не может получить USDC в цепочке: отправь отдельно."))));
        };
        let evm = |n: &str| n == "hyperliquid" || EVM_PLACES.contains(&n);
        if let Some(net) = network && net != e.network {
            if !(evm(net) && evm(&e.network)) {
                return (None, Some(self.say(&format!("“{}” is a {} address: it can't receive on {net}.", e.name, e.network),
                                            &format!("«{}» — адрес в {}: на {net} он получить не может.", e.name, e.network))));
            }
            e.network = net.into();
        }
        (Some(e), None)
    }

    /// The answer to a chain's «How much?»: that step with its amount, and the steps held behind it, go on as a chain again.
    /// Anything else is left to the plain «How much?» (move_checks.rs answered_amount), and the held steps go with its
    /// question.
    pub async fn answered_chain_amount(&self, text: &str, declines: bool) -> bool {
        let (held, awaiting) = {
            let st = self.st();
            (st.chain_state.held.clone(), st.awaiting_amount.clone())
        };
        let Some(held) = held else { return false };
        if awaiting.as_ref() != Some(&held.command) {
            self.st_mut().chain_state.held = None;
            return false;
        }
        if declines { return false }
        let Some(value) = WalletCommand::amount_answer_for(text, &held.command) else { return false };
        {
            let mut st = self.st_mut();
            st.chain_state.held = None;
            st.awaiting_amount = None;
        }
        let filled = held.command.with_amount(value);
        let steps: Vec<String> = std::iter::once(filled.summary()).chain(held.rest).collect();
        self.chain(&steps, Some(&filled), None, false).await;
        true
    }

    /// A send no balance can cover, said at once instead of a card whose Confirm could only fail (the Mac's
    /// transferShortfall). It pays from its network, what that lacks from Solana or another EVM network (Executor's
    /// send_usdc); one on Solana only from Solana. A balance not read yet, or a read that failed (its network then reads as
    /// empty), leaves it to the move.
    pub fn transfer_shortfall(&self, net: &str, asked: Decimal) -> Option<String> {
        if !self.st().portfolio.errors.is_empty() { return None }
        let have = self.usdc_free(net)?;
        let others: Vec<Option<Decimal>> = if net == "solana" { vec![] } else {
            std::iter::once(self.usdc_free("solana")).chain(EvmNetwork::searched().into_iter().filter(|n| n.key != net).map(|n| self.usdc_free(n.key))).collect()
        };
        if others.iter().any(Option::is_none) { return None }
        // ponytail: balances past Decimal's range leave it to the move, as a read that failed does.
        let all = others.into_iter().flatten().try_fold(have, |a, x| a.checked_add(x))?;
        if asked <= all { return None }
        Some(self.say(&format!("All the USDC this send can draw on is {}, less than {} — nothing was prepared. Say up to that, or top up first.", Fmt::num(all, 2), Fmt::num(asked, 2)),
                      &format!("Всего USDC, из которых может пойти этот перевод, — {}, меньше {}. Ничего не готовлю: назови сумму не больше или сначала пополни.", Fmt::num(all, 2), Fmt::num(asked, 2))))
    }

    /// A step the chain's card runs by its words, drawn as them: a chain never loses a step on the canvas.
    pub fn said_step(&self, text: &str) -> Step {
        let parsed = WalletCommand::parse(text);
        let step = |label: String, lines: Vec<String>| Step { label, title: text.into(), lines, money: None, status: None };
        if parsed.is_none() && PositionCommand::parse(text).is_some() { return step("HYPERLIQUID".into(), vec![]) }
        if parsed.as_ref().and_then(WalletCommand::amount_question).is_some() {
            return step(self.say("THEN", "ПОТОМ"), vec![self.say("how much\tasked when it's reached", "сколько\tспрошу, когда дойдёт")]);
        }
        step(self.say("THEN", "ПОТОМ"), vec![])
    }

    // MARK: After Confirm

    /// The chain's Confirm: its steps in order with the bridge legs landing side by side (Executor's landings).
    pub fn run_chain(&self, steps: Vec<ChainStep>) -> Local<'_, Result<(), trade::Error>> {
        Box::pin(async move {
            let landings = Arc::new(Landings::new());
            let ran = trade::LANDINGS.scope(Some(landings.clone()), self.run_chain_steps(&steps, &landings)).await;
            // The chain's end, through or stopped: its request's History card takes its board (the Mac's runSteps).
            self.refreeze_request();
            ran
        })
    }

    async fn run_chain_steps(&self, steps: &[ChainStep], landings: &Landings) -> Result<(), trade::Error> {
        let mut arrived: HashSet<String> = HashSet::new();
        for (i, step) in steps.iter().enumerate() {
            let line = self.say(&format!("Step {}: ", i + 1), &format!("Шаг {}: ", i + 1)) + &step.title();
            if let Some(r) = self.st_mut().running.as_mut() { r.lines.push(line) }
            self.changed();
            let started = chrono::Utc::now();
            match self.run_chain_step(i, step, steps, landings, &mut arrived).await {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) if i > 0 => {
                    // Steps before went through: said, and the rest stop. Never «Try again» on the whole chain, which would
                    // lay out the steps already done once more. Sent with no outcome is «unknown», never «failed».
                    let since = started - chrono::TimeDelta::seconds(1);
                    let sent: Vec<_> = self.journal().unresolved().map(|v| v.into_iter().filter(|e| e.at >= since).collect()).unwrap_or_default();
                    let board = self.st().running_board.clone();
                    self.update_board(board.as_deref(), |b| b.state = Some(if sent.is_empty() { "failed" } else { "unknown" }.into()));
                    let text = if sent.is_empty() {
                        self.say("Failed: ", "Не вышло: ") + &error.to_string()
                    } else {
                        self.say("Sent, status unknown: ", "Отправлено, статус неизвестен: ") + &error.to_string() + " "
                            + &self.say("Check before doing it again: ", "Проверь, прежде чем повторять: ")
                            + &sent.iter().map(|e| format!("{} {}", e.chain, e.id)).collect::<Vec<_>>().join(" · ")
                    };
                    self.reply(&text, None);
                    if i + 1 < steps.len() {
                        let title = step.title();
                        self.reply(&self.say(&format!("“{title}” didn’t go through, so the steps after it don’t run."), &format!("«{title}» не прошёл, поэтому следующие шаги не запускаю.")), None);
                    }
                    // The model works out why and the way that works, as after a failed Confirm (not in a scheduled run).
                    if sent.is_empty() { self.note_refusal_error(&step.title(), &error, None) }
                    break;
                }
                Err(error) => return Err(error),
            }
            // The next step reads balances after this one.
            self.refresh().await;
        }
        // What's still landing when the chain ends (it ended on a bridge, or a step stopped it) is waited for and said.
        if let Err(error) = self.landed(landings, None).await {
            let board = self.st().running_board.clone();
            self.update_board(board.as_deref(), |b| b.state = Some("failed".into()));
            self.reply(&error.to_string(), None);
        }
        Ok(())
    }

    /// One step of a confirmed chain: Ok(true) goes on to the next, Ok(false) stops the chain here (it waits on a card or
    /// for an amount).
    async fn run_chain_step(&self, i: usize, step: &ChainStep, steps: &[ChainStep], landings: &Landings, arrived: &mut HashSet<String>)
                            -> Result<bool, trade::Error> {
        match step {
            ChainStep::Wallet(c) => {
                // The legs still landing where this step spends (all of them for one that names no place).
                self.landed(landings, Self::source(c).as_deref()).await?;
                // Money a move before brought to Solana: the step waits until Solana's balance shows it.
                if arrived.contains("solana") && Self::spends_on_solana(c) {
                    self.wait_for_deposit(Self::solana_usdc(c), &c.summary()).await;
                }
                let c = if i > 0 { self.fitted_to_arrival(c).await } else { c.clone() };
                // A paid request pays a price a card showed: the chain's card shows its first step's (chain()). Any other,
                // a step of a task's run on the automation wallet, is priced now and goes by itself only up to the price
                // Confirmed for it in that task. Else its own card waits, the steps after it held behind it.
                let shown = i == 0 && task_run().is_none() && self.st().payment.as_ref().is_some_and(|q| q.command == c);
                if matches!(c, W::Pay { .. }) && !shown {
                    match self.ask_price(&c).await? {
                        PriceAsked::Answered(text) => {
                            self.reply(&text, None);
                            return Ok(true);
                        }
                        PriceAsked::Quoted(q) => {
                            let rest: Vec<String> = steps[i + 1..].iter().map(ChainStep::title).collect();
                            if !self.pays_by_itself(*q, &rest) { return Ok(false) }
                        }
                    }
                }
                self.r#move(&c).await?;
                if let Some(to) = Self::arrives(&c) { arrived.insert(to); }
                Ok(true)
            }
            ChainStep::Transfer(entry, amount) => {
                // Its network switched off in Modules after the card was drawn: refused now (consent.rs confirm_refusal).
                if let Some(off) = self.confirm_refusal(&Pending::Transfer(entry.clone(), *amount)) { return Err(trade::Error::Execution(off)) }
                self.landed(landings, None).await?;
                let said = self.transfer(*amount, entry).await?;
                self.reply(&said, Some("transfer".into()));
                self.record_flow(-*amount, &format!("sent to {}", entry.name));
                Ok(true)
            }
            ChainStep::Said(text) => {
                self.landed(landings, None).await?;
                let rest: Vec<String> = steps[i + 1..].iter().map(ChainStep::title).collect();
                if !self.run_said(text, &rest).await? { return Ok(false) }
                // It drew a card of its own: a trade plan waits on it and the steps after it go on (the Mac's runSteps); any
                // other card (a contract call, an address to save) waits for its own Confirm, and the steps after it run on
                // it once it's through (the Mac's continueTask).
                let pending = self.st().pending.clone();
                match pending {
                    None | Some(Pending::Trade(_)) => Ok(true),
                    Some(Pending::Recurring { command: buy, days }) => {
                        // Named on the card this Confirm is for: started on it, as the Mac's carryOut starts the first buy
                        // (Execution.swift:684-703). Its own block says so; the chain goes on.
                        let drawn = {
                            let mut st = self.st_mut();
                            st.pending = None;
                            st.card.take().map(|c| c.id)
                        };
                        self.update_board(drawn.as_deref(), |b| b.state = Some("running".into()));
                        match self.start_recurring(&buy, days).await {
                            Ok(()) => self.update_board(drawn.as_deref(), |b| b.state = Some("done".into())),
                            Err(error) => {
                                self.update_board(drawn.as_deref(), |b| b.state = Some("failed".into()));
                                return Err(error);
                            }
                        }
                        Ok(true)
                    }
                    Some(_) => {
                        self.hold_after_call(&rest);
                        let held = self.st().chain_state.after_call.is_some();
                        if !held && !rest.is_empty() {
                            let rest = rest.join(" → ");
                            self.reply(&self.say(&format!("“{text}” waits on its card, so the steps after it don’t run: {rest}."),
                                                 &format!("«{text}» ждёт на своей карточке, поэтому следующие шаги не запускаю: {rest}.")), None);
                        }
                        Ok(false)
                    }
                }
            }
        }
    }

    /// A step of a confirmed chain run by its words: a position managed at once (the card listed it), a move without its
    /// amount asked for with the rest held for the answer, anything else as typed. False when the chain stops here.
    async fn run_said(&self, text: &str, rest: &[String]) -> Result<bool, trade::Error> {
        let command = WalletCommand::parse(text);
        if let Some(c) = &command && let Some(question) = c.amount_question() {
            {
                let mut st = self.st_mut();
                st.chain_state.held = Some(Held { command: c.clone(), rest: rest.to_vec() });
                st.awaiting_amount = Some(c.clone());
            }
            self.reply(&(self.say(&question, "Сколько? Напиши число.") + &self.amount_hint(c)), None);
            return Ok(false);
        }
        if command.is_none() && let Some(p) = PositionCommand::parse(text) {
            let Some(coin) = self.target_coin(text) else {
                self.reply(&self.say(&format!("There’s no open position for “{text}”, so the steps stop here."), &format!("Для «{text}» нет открытой позиции — на этом шаги останавливаются.")), None);
                return Ok(false);
            };
            if let PositionCommand::Trail(pct) = p {
                self.reply(&self.trail(&coin, pct), Some("Hyperliquid".into()));
            } else {
                let (policy, equity) = (self.policy(), self.rule_equity());
                let said = self.executor()?.manage(&p, &coin, &policy, equity).await?;
                self.reply(&said, Some("Hyperliquid".into()));
            }
            return Ok(true);
        }
        if !self.run_canonical(text, text).await {
            self.reply(&self.say(&format!("ihatecrypto can’t do “{text}” — the steps stop here."), &format!("ihatecrypto не умеет «{text}» — на этом шаги останавливаются.")), None);
            return Ok(false);
        }
        Ok(true)
    }

    /// Waits for the legs still landing at `place` (None: all of them), the running card saying so; each landing says how
    /// it went. One that failed stops the chain.
    async fn landed(&self, landings: &Landings, place: Option<&str>) -> Result<(), trade::Error> {
        if landings.count(place) == 0 { return Ok(()) }
        let line = self.say3("Waiting for the moves before to land", "Жду, пока придут деньги с прошлых шагов", "Esperando a que lleguen los pasos anteriores");
        if let Some(r) = self.st_mut().running.as_mut() { r.lines.push(line) }
        self.changed();
        match landings.wait(place).await {
            Ok(said) => {
                for line in said { self.reply(&line, None) }
                Ok(())
            }
            Err(error) => Err(trade::Error::Execution(
                self.say("A move before didn't land, so the steps after it don't run: ", "Деньги с прошлого шага не пришли, дальше не иду: ") + &error.to_string())),
        }
    }

    /// A step that spends USDC held on Solana: one that names it (`spends`), or a spread of the free USDC wherever it is.
    /// A deposit of another coin («lend 2 SOL») spends none.
    pub fn spends_on_solana(c: &WalletCommand) -> bool {
        match c {
            W::Allocate { symbol, from: None, .. } => symbol.eq_ignore_ascii_case("USDC"),
            W::LendDeposit(symbol, _) | W::Kamino { deposit: true, symbol, .. } => symbol.eq_ignore_ascii_case("USDC"),
            _ => Self::spends(c).as_deref() == Some("solana"),
        }
    }

    /// The wait for money a move brought to Solana is over. Balances are read at «finalized» and a move ends at
    /// «confirmed», some 13 s earlier, so the money is counted before it shows. With the step's amount there it is over.
    /// Without one («then move all»), or when the move brings less than the step asks (the rest comes from another
    /// network), it is over once the finalized balance equals the confirmed one — equal, not «at least»: what an earlier
    /// step spent is confirmed before it is final too — and that one has read the same several times in a row: three
    /// when it stands above `entry`, the balance the wait began with (the money is seen coming), eight when it doesn't
    /// (it had landed already, or a node a little behind still answers the old balance). No confirmed read at all: only
    /// the step's own amount ends it.
    pub fn deposit_shown(need: Option<Decimal>, finalized: Decimal, confirmed: Option<Decimal>, steady: u32, entry: Decimal) -> bool {
        need.is_some_and(|n| finalized >= n) || confirmed.is_some_and(|c| finalized == c && steady >= if c > entry { 3 } else { 8 })
    }

    /// A step that spends on Solana what a move before it brought: an «ARRIVES · SOLANA» card on the route until Solana's
    /// balance shows the money (`deposit_shown`), two minutes at most; then the step goes on and its own check says
    /// what's short. `need`: the step's amount, None when it names none («move all», a spread) — it read the old balance
    /// then, and a step asking for more than the move brought waited the whole two minutes (the Mac, 2026-11-10).
    async fn wait_for_deposit(&self, need: Option<Decimal>, step: &str) {
        let owner = self.st().solana_address();
        let Some(owner) = owner else { return };
        let (rpc, mint) = (SolanaRpc::new(), SpotAsset::usdc().mint);
        // The one account, read lightly: USDC is the classic token program's.
        let Ok(account) = solana::spl::associated_account(&owner, &mint, solana::spl::TOKEN_PROGRAM) else { return };
        let have = rpc.account_amount(&account, "finalized").await.unwrap_or_default();
        if need.is_some_and(|n| have >= n) { return }
        let mut confirmed = rpc.account_amount(&account, "confirmed").await;
        let card = |have: Decimal, confirmed: Option<Decimal>| {
            // What the step asks for, else what is seen coming.
            let coming = need.or(confirmed.filter(|c| *c > have).map(|c| c - have));
            Step {
                label: "ARRIVES · SOLANA".into(), title: coming.map_or("USDC".to_string(), |c| format!("{} USDC", Fmt::num(c, 2))),
                lines: vec![self.say3("on Solana now", "на Solana сейчас", "en Solana ahora") + &format!("\t{} USDC", Fmt::num(have, 2)),
                            self.say3("Then: ", "Потом: ", "Luego: ") + step],
                money: None, status: None,
            }
        };
        let id = self.st().running_board.clone();
        let mut at = 0;
        let first = card(have, confirmed);
        self.update_board(id.as_deref(), |b| {
            let mut list = b.steps.take().unwrap_or_default();
            at = list.iter().position(|s| s.title == step).unwrap_or(list.len());
            list.insert(at, first);
            b.steps = Some(list);
        });
        let line = self.say3("Waiting for the deposit on Solana", "Жду депозит на Solana", "Esperando el depósito en Solana");
        if let Some(r) = self.st_mut().running.as_mut() { r.lines.push(line) }
        self.changed();
        let (mut now, mut steady, mut unread) = (have, 0u32, 0u32);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        while tokio::time::Instant::now() < deadline && !Self::deposit_shown(need, now, confirmed, steady, have) {
            // No amount to wait for and the node says nothing of what is counted: nothing to wait on.
            if need.is_none() && confirmed.is_none() && unread >= 5 { break }
            tokio::time::sleep(Duration::from_secs(2)).await;
            if let Some(finalized) = rpc.account_amount(&account, "finalized").await { now = finalized }
            // A read that failed says nothing: it neither counts toward «the same» nor starts the count again.
            match rpc.account_amount(&account, "confirmed").await {
                Some(again) => {
                    steady = if Some(again) == confirmed { steady + 1 } else { 0 };
                    confirmed = Some(again);
                }
                None => unread += 1,
            }
            let shown = card(now, confirmed);
            self.update_board(id.as_deref(), |b| if let Some(steps) = b.steps.as_mut() && at < steps.len() { steps[at] = shown });
        }
        // It was a wait, not a step: the route goes back to the moves.
        self.update_board(id.as_deref(), |b| if let Some(steps) = b.steps.as_mut() && at < steps.len() && steps[at].label == "ARRIVES · SOLANA" { steps.remove(at); });
    }

    /// A step that spends money on a network, run after a move landed there: when the move's fee left it a little short
    /// of the step's amount (cents from an L2, under a dollar from Ethereum), the step spends what is there. A bigger gap
    /// is left for the step to refuse.
    pub async fn fitted_to_arrival(&self, c: &WalletCommand) -> WalletCommand {
        // «buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit»: the swap gave 749.8, and the step takes
        // what it gave rather than failing on the cents.
        let token: Option<(String, Decimal)> = match c {
            W::SendToken { token, amount: Some(x), network: Some(n), .. } if n == "solana" => Some((token.clone(), *x)),
            W::LendDeposit(t, Some(x)) if !["USDC", "SOL"].contains(&t.to_uppercase().as_str()) => Some((t.clone(), *x)),
            _ => None,
        };
        let rpc = SolanaRpc::new();
        let (solana, evm) = {
            let st = self.st();
            (st.solana_address(), st.evm_address())
        };
        if let Some((symbol, asked)) = token && let Some(owner) = &solana {
            // The mint from the dollar coins' table or the tokens already read.
            let up = symbol.to_uppercase();
            let mint = stablecoins::on("solana").and_then(|coins| coins.iter().find(|x| x.symbol.to_uppercase() == up)).map(|x| x.address.to_string())
                .or_else(|| {
                    let st = self.st();
                    st.portfolio.networks.iter().find(|n| n.key == "solana")
                        .and_then(|n| n.rows.iter().find(|r| r.symbol.to_uppercase() == up || r.contract.as_deref() == Some(symbol.as_str())))
                        .and_then(|r| r.contract.clone())
                });
            if let Some(mint) = mint && let Ok(have) = rpc.owned_amount(owner, &mint).await {
                return Self::fitted(c, asked, have, false).unwrap_or_else(|| c.clone());
            }
            return c.clone();
        }
        let (Some(net), Some(asked)) = (Self::spends(c), c.editable_amount()) else { return c.clone() };
        if asked <= Decimal::ZERO { return c.clone() }
        let mut there: Option<Decimal> = None;
        if net == "solana" && let Some(owner) = &solana {
            there = rpc.owned_amount(owner, &SpotAsset::usdc().mint).await.ok();
        } else if let Some(n) = EvmNetwork::named(&net) && let Some(owner) = &evm && let Ok(units) = n.usdc_balance(owner).await {
            // Tempo's fees come out of the USDC itself: a cent stays.
            let fee = if n.key == "tempo" { Decimal::new(1, 2) } else { Decimal::ZERO };
            there = Some((Decimal::from(units) / Decimal::from(1_000_000) - fee).max(Decimal::ZERO));
        }
        let Some(there) = there else { return c.clone() };
        Self::fitted(c, asked, there, true).unwrap_or_else(|| c.clone())
    }

    /// The step for what arrived, when it's within 2 % of what it asked (dollars: or within $1 — Ethereum's fee — but
    /// never more than a quarter of the amount: with a balance read before the money showed, «deposit 1» became
    /// «deposit 0.1» on the Mac, 2026-11-10); None when the gap is bigger, nothing arrived, or the fitted step wouldn't
    /// read back the same from its words.
    pub fn fitted(c: &WalletCommand, asked: Decimal, there: Decimal, dollars: bool) -> Option<WalletCommand> {
        let slack = (Num::of(asked) * Decimal::new(2, 2)).0?;
        let quarter = (Num::of(asked) * Decimal::new(25, 2)).0?;
        let gap = (Num::of(asked) - there).0?;
        if !(there > Decimal::ZERO && there < asked && gap <= if dollars { slack.max(Decimal::ONE.min(quarter)) } else { slack }) { return None }
        let fitted = c.with_amount(if dollars { rounded(there, 2, Rounding::Down) } else { there });
        (fitted != *c && WalletCommand::parse(&fitted.summary()).as_ref() == Some(&fitted)).then_some(fitted)
    }

    // MARK: Where a step's money is

    /// The network a move delivers USDC to («solana» for Solana); None for anything else.
    pub fn arrives(c: &WalletCommand) -> Option<String> {
        match c {
            W::Bridge { to, .. } => (to != "hyperliquid").then(|| to.clone()),
            W::EvmToSolana(..) | W::BaseToSolana(_) | W::ArbitrumToSolana(_) | W::RobinhoodToSolana(_) | W::WithdrawToSolana(_) | W::SellNear { .. } => Some("solana".into()),
            // Out of a vault into the wallet: the balance shows it a moment after the move is through, and the step
            // after it waits for that like for a bridge («withdraw 1 then deposit 1» read the old balance).
            W::LendWithdraw(symbol, _) | W::Kamino { deposit: false, symbol, .. } if symbol == "USDC" => Some("solana".into()),
            W::SolanaTo(to, _) => Some(to.clone()),
            _ => None,
        }
    }

    /// The network a step spends USDC on; None when it spends none.
    pub fn spends(c: &WalletCommand) -> Option<String> {
        match c {
            W::Aave { deposit: true, network: n, .. } | W::Morpho { deposit: true, network: n, .. } | W::BuyOn { place: n, .. } => Some(n.clone()),
            W::LendDeposit(..) | W::Kamino { deposit: true, .. } | W::Buy { .. } | W::Hlp { deposit: true, .. } | W::SolanaTo(..) | W::JupiterDca { .. } => Some("solana".into()),
            W::Allocate { from: Some(f), .. } if f == "solana" => Some("solana".into()),
            W::BuyStock { .. } => Some("robinhood".into()),
            _ => None,
        }
    }

    /// Where a step takes its money from: what it spends on, or the place a bridge leg leaves; None when it names none.
    pub fn source(c: &WalletCommand) -> Option<String> {
        match c {
            W::Bridge { from, .. } | W::Cctp { from, .. } | W::EvmToSolana(from, _) | W::Usdt { from, .. } => Some(from.clone()),
            W::BaseToSolana(_) => Some("base".into()),
            W::ArbitrumToSolana(_) => Some("arbitrum".into()),
            W::RobinhoodToSolana(_) => Some("robinhood".into()),
            W::WithdrawToSolana(_) => Some("hyperliquid".into()),
            W::SellNear { .. } => Some("near".into()),
            _ => Self::spends(c),
        }
    }

    /// The USDC a step spends on Solana: what it waits for when a move before it brings the money.
    pub fn solana_usdc(c: &WalletCommand) -> Option<Decimal> {
        match c {
            W::LendDeposit(s, a) if s == "USDC" => *a,
            W::Kamino { deposit: true, symbol, amount } if symbol == "USDC" => *amount,
            W::Hlp { deposit: true, amount } => *amount,
            W::Buy { usdc, .. } | W::JupiterDca { usdc, .. } => *usdc,
            W::Allocate { symbol, amount, from: Some(f), .. } if symbol == "USDC" && f == "solana" => *amount,
            W::SolanaTo(_, a) => *a,
            _ => None,
        }
    }

    // MARK: A scheduled move at Windows Hello

    /// A scheduled task's move signing with no one at the window: while the signer still waits after a few seconds —
    /// Windows Hello for a new address or a fresh confirmation — a notification says so (the Mac's `.signature`). Abort it
    /// when the move is through.
    pub fn watch_for_hello(&self, title: &str) -> JoinHandle<()> {
        let (me, title) = (Rc::downgrade(&self.rc()), title.to_string());
        tokio::task::spawn_local(async move {
            if let Some(e) = me.upgrade() { e.notice_while_hello_waits(&title).await }
        })
    }

    async fn notice_while_hello_waits(&self, title: &str) {
        let mut seen = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let signing = self.st().signing;
            seen = if signing > 0 { seen + 1 } else { 0 };
            if seen < 2 { continue }
            // A notice only, as the Mac's (record: false): the run itself is what History keeps.
            let ledger = self.st().ledger_active();
            let body = if ledger {
                self.say("Waiting for your Ledger — approve it on the device.", "Ждёт Ledger — подтверди на устройстве.")
            } else {
                self.say("Waiting for Windows Hello — confirm it when Windows asks.", "Ждёт Windows Hello — подтверди, когда Windows спросит.")
            };
            self.notify(Notice::Signature, &self.say(&format!("Scheduled: {title}"), &format!("По расписанию: {title}")), &body, false, false, Some(false), None);
            return;
        }
    }
}

#[cfg(test)]
#[path = "chains/tests.rs"]
mod tests;
