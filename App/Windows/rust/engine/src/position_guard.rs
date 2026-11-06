//! ihatecrypto's guardian, as the Mac's (PositionGuard.swift; Execution.swift guardPositions, ExitRules.duties): the exit
//! rules the exchange can't hold by itself — the stop to breakeven once the first half is taken, a trailing stop that
//! only tightens — kept per position, run after each refresh while the app is open or in the tray, and said under the
//! position.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use fancy_regex::Regex;
use ihatecrypto_core::guardian::{self, Action, ExitRules};
use ihatecrypto_core::intent::{Fmt, IntentParser, Side, TradeIntent};
use ihatecrypto_core::position_command::PositionCommand;
use rust_decimal::Decimal;
use trade::executor::{Opened, Order};
use trade::HyperliquidAccount;

use crate::engine::Engine;
use crate::notices::Notice;
use crate::snapshot;
use crate::store;

/// Every account's rules, by its address in lower case, then by coin (exit-rules.json).
type AllRules = HashMap<String, HashMap<String, ExitRules>>;

/// «10:42 AM», «5.00»: a reason's times and amounts.
static NUMBERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d[\d:.\s]*(AM|PM|am|pm)?").expect("a regex"));

/// `orderType ?? ""`.
fn kind(o: &Order) -> &str { o.order_type.as_deref().unwrap_or("") }

impl Engine {
    /// The rules by coin, kept per Hyperliquid account: they live exactly as long as their position.
    pub fn exit_rules(&self) -> HashMap<String, ExitRules> {
        let key = self.st().evm_address().map(|a| a.to_lowercase()).unwrap_or_default();
        store::load::<AllRules>("exit-rules.json").unwrap_or_default().remove(&key).unwrap_or_default()
    }

    /// `exitRules = …`: this account's rules saved, its entry gone when none are left; nothing without an account.
    pub fn set_exit_rules(&self, rules: HashMap<String, ExitRules>) {
        let Some(key) = self.st().evm_address().map(|a| a.to_lowercase()) else { return };
        let mut all: AllRules = store::load("exit-rules.json").unwrap_or_default();
        if rules.is_empty() { all.remove(&key); } else { all.insert(key, rules); }
        store::save(&all, "exit-rules.json");
    }

    /// `exitRules[coin] = rules`: None takes the coin's rules away.
    fn set_coin_rules(&self, coin: &str, rules: Option<ExitRules>) {
        let mut all = self.exit_rules();
        match rules { Some(r) => { all.insert(coin.into(), r); } None => { all.remove(coin); } }
        self.set_exit_rules(all);
    }

    /// After a plan opened: its partial take and trailing stop become rules; rules left from an earlier position in the
    /// same coin never steer this one. A trailing stop the exchange holds (the executor's native one) needs nothing from
    /// the app, as the Mac's.
    pub fn keep_exit_rules(&self, opened: &Opened, intent: &TradeIntent) {
        let mut intent = intent.clone();
        if opened.trailing_on_exchange { intent.trailing_pct = None }
        self.set_coin_rules(&opened.asset, Self::exit_rules_for(opened.size, opened.entry, opened.pending, opened.partial_take, &intent));
    }

    /// The rules a plan's fill leaves for the guardian, or None when the exchange's stop and take are all there is
    /// (Swift's static `exitRules(size:entry:pending:partialTake:intent:)`).
    pub fn exit_rules_for(size: Decimal, entry: Decimal, pending: bool, partial_take: Option<Decimal>, intent: &TradeIntent) -> Option<ExitRules> {
        let partial = partial_take.is_some() || (pending && intent.partial_take == Some(true));
        if !partial && intent.trailing_pct.is_none() { return None }
        let mut rules = ExitRules::new(size, entry, intent.side == Side::Long, partial, intent.trailing_pct);
        rules.partial_order_pending = pending && intent.partial_take == Some(true);
        Some(rules)
    }

    /// «trail 2% on ETH»: the app keeps the stop, nothing is placed on the exchange for it.
    pub fn trail(&self, coin: &str, pct: Decimal) -> String {
        let name = IntentParser::pretty(coin);
        let Some(p) = self.st().positions.iter().find(|p| p.coin == coin).cloned() else {
            return self.say(&format!("There is no open {name} position to trail."), &format!("Открытой позиции {name} нет — тянуть нечего."));
        };
        let mut rules = self.exit_rules().remove(coin)
            .unwrap_or_else(|| ExitRules::new(p.size.abs(), p.entry, p.size > Decimal::ZERO, false, None));
        rules.trailing_pct = Some(pct);
        self.set_coin_rules(coin, Some(rules));
        let me = self.rc();
        tokio::task::spawn_local(async move { me.guard_positions().await });
        self.say(&format!("ihatecrypto now keeps the {name} stop {} behind the best price and only ever tightens it — while ihatecrypto is open or in the tray.", Fmt::pct(pct)),
                 &format!("Теперь ihatecrypto держит стоп {name} в {} от лучшей цены и только подтягивает его — пока ihatecrypto открыт или в трее.", Fmt::pct(pct)))
    }

    /// One pass over the rules: a limit that filled gets its half at 1R, then each position's stop moves if a rule says so.
    pub async fn guard_positions(&self) {
        {
            let st = self.st();
            if st.watching() || st.active_wallet().and_then(|w| w.evm).is_none() || st.running.is_some() || st.signing != 0 || st.guarding { return }
            // Only on this account's own positions: never a list not read yet (its first request failed: every rule would
            // read as closed and be dropped) or read for the wallet before a switch.
            let Some(account) = st.evm_address().map(|a| a.to_lowercase()) else { return };
            if st.positions_for.as_deref() != Some(account.as_str()) { return }
        }
        let rules = self.exit_rules();
        if rules.is_empty() { return }
        self.st_mut().guarding = true;
        self.guard_pass(rules).await;
        self.st_mut().guarding = false;
    }

    /// guardPositions after its checks, while `guarding` holds.
    async fn guard_pass(&self, mut rules: HashMap<String, ExitRules>) {
        // On a Ledger, stops move by themselves only through the trading key it approved: without it every move would
        // wait for the device's buttons. The stop and take on the exchange protect the position meanwhile.
        if self.ledger_active() && !self.agent_active() {
            if !self.st().ledger_guard_said {
                self.st_mut().ledger_guard_said = true;
                self.notify(Notice::Positions, &self.say("Trailing waits for the trading key", "Трейлинг ждёт торговый ключ"),
                            &self.say("On a Ledger, stops move by themselves once its trading key is approved — say “approve trading key”. The stop on the exchange protects the position meanwhile.",
                                      "На Ledger стопы двигаются сами, когда одобрен торговый ключ — скажи «approve trading key». Пока позицию защищает стоп на бирже."),
                            false, false, None, None);
            }
            return;
        }
        {
            let st = self.st();
            rules.retain(|coin, _| st.positions.iter().any(|p| &p.coin == coin) || st.hl_orders.iter().any(|o| &o.coin == coin && !o.is_trigger));
        }
        self.set_exit_rules(rules.clone());
        for (coin, mut r) in rules {
            let found = {
                let st = self.st();
                let p = st.positions.iter().find(|p| p.coin == coin).cloned();
                let mark = st.markets.get(&coin).map(|m| m.mark_px);
                let stop = st.hl_orders.iter().find(|o| o.coin == coin && o.is_trigger && kind(o).contains("Stop")).and_then(|o| o.trigger_px);
                p.zip(mark).map(|(p, mark)| (p, mark, stop))
            };
            let Some((p, mark, stop)) = found else { continue };
            if r.partial_order_pending {
                // The limit filled: the half at 1R goes on now, sized from the real fill.
                r.partial_order_pending = false;
                r.opened_size = p.size.abs();
                r.entry = p.entry;
                r.peak = p.entry;
                self.set_coin_rules(&coin, Some(r));
                if let Ok(x) = self.executor() { let _ = x.add_partial_take(&coin).await; }
                continue;
            }
            let action = guardian::step(&mut r, p.size, stop, mark);
            if self.exit_rules().get(&coin) != Some(&r) { self.set_coin_rules(&coin, Some(r)) }
            let name = IntentParser::pretty(&coin);
            let Some(action) = action else { continue };
            let command = match action { Action::StopToBreakeven => PositionCommand::StopToBreakeven, Action::StopTo(price) => PositionCommand::StopTo(price) };
            let moved = match self.executor() {
                Ok(x) => x.manage(&command, &coin, &self.policy(), self.rule_equity()).await.map_err(|e| e.to_string()),
                Err(e) => Err(e.0),
            };
            match (moved, action) {
                (Ok(_), Action::StopToBreakeven) => {
                    let (title, body) = (self.say(&format!("Partial profit taken on {name}"), &format!("Частичная прибыль по {name}")),
                                         self.say("Stop moved to breakeven.", "Стоп перенесён в безубыток."));
                    self.reply(&format!("{title}. {body}"), Some("guardian".into()));
                    // Its record carries the coin, so History › Positions lists it with that trade (the Mac's activity).
                    self.record_automatic(&title, &body, Some(&coin), false, false, None);
                    self.notify(Notice::Positions, &title, &body, false, false, Some(false), None);
                    self.st_mut().guard_refused.remove(&coin);
                }
                (Ok(_), Action::StopTo(price)) => {
                    // Moves in a row are one record, a line each (the Mac's): a trend would otherwise fill the History.
                    let line = format!("{} · {}", Fmt::time(chrono::Utc::now()),
                                       self.say(&format!("stop moved to {}", Fmt::price(price)), &format!("стоп передвинут на {}", Fmt::price(price))));
                    self.record_automatic(&self.say(&format!("Trailing stop on {name}"), &format!("Трейлинг-стоп по {name}")), &line, Some(&coin), false, true, None);
                    self.st_mut().guard_refused.remove(&coin);
                }
                (Err(why), _) => {
                    // Once per reason (it runs every 10 s): said again only when the reason changes or after a move went
                    // through. The reason without its numbers: the unopened trading key's says when it asks again, new
                    // every ten minutes.
                    let reason = Self::reason_only(&why);
                    if self.st().guard_refused.get(&coin) != Some(&reason) {
                        self.st_mut().guard_refused.insert(coin.clone(), reason);
                        self.reply(&(self.say(&format!("The guardian couldn't move the {name} stop: "), &format!("Не получилось передвинуть стоп {name}: ")) + &why),
                                   Some("guardian".into()));
                    }
                }
            }
        }
    }

    /// «… ask again from 10:42 AM, …» and «… from 10:52 AM, …» are one reason: the words without the times and amounts.
    /// The time it asks again goes, «AM»/«PM» with it: 11:58 AM and 12:08 PM are the same reason.
    pub fn reason_only(why: &str) -> String { NUMBERS.replace_all(why, "").into_owned() }

    /// A position gone while ihatecrypto wasn't closing it: its stop, take or a hand elsewhere closed it. Said once, in the
    /// chat and as a notification, as the Mac's closedByExchange.
    pub async fn note_closed_positions(&self, before: &[String]) {
        let user = { let st = self.st(); if st.running.is_some() { return } st.evm_address() };
        let Some(user) = user else { return };
        let now: BTreeSet<String> = self.st().positions.iter().map(|p| p.coin.clone()).collect();
        let gone: BTreeSet<&String> = before.iter().filter(|c| !now.contains(*c)).collect();
        for coin in gone {
            // Another wallet since: its rules aren't this coin's, and the old wallet's close isn't its news.
            if self.st().evm_address().as_deref() != Some(user.as_str()) { return }
            self.set_coin_rules(coin, None);
            let Ok(Some((price, pnl, time))) = HyperliquidAccount::last_close(&user, coin).await else { continue };
            if (chrono::Utc::now() - time).num_milliseconds() >= 600_000 { continue }
            if self.st().evm_address().as_deref() != Some(user.as_str()) { return }
            let (won, lost) = (pnl >= Decimal::ZERO, Fmt::usd(pnl.abs()));
            let text = self.say(
                &format!("{coin} position closed outside ihatecrypto (stop, take-profit or by hand) at {}: {} {lost}.", Fmt::price(price), if won { "profit" } else { "loss" }),
                &format!("Позиция {coin} закрылась вне ihatecrypto (стоп, тейк или вручную) по {}: {} {lost}.", Fmt::price(price), if won { "прибыль" } else { "убыток" }),
            );
            self.reply(&text, Some("exchange".into()));
            self.notify(Notice::Positions, coin, &text, false, false, None, Some(format!("perp:{coin}")));
        }
    }

    /// The position as the Mac's card says it: «0.05 at $2,412.50 · stop $2,300 · take $2,600», and the guardian's line.
    pub fn position_views(&self) -> Vec<snapshot::Position> {
        let rules = self.exit_rules();
        let words = |en: &str, ru: &str| self.say(en, ru);
        let (positions, orders) = { let st = self.st(); (st.positions.clone(), st.hl_orders.clone()) };
        positions.iter().map(|p| snapshot::Position {
            coin: IntentParser::pretty(&p.coin), side: if p.size > Decimal::ZERO { "long" } else { "short" }.into(), size: Fmt::num(p.size.abs(), 6),
            entry: Fmt::price(p.entry), pnl: Fmt::usd(p.unrealized_pnl), liquidation: p.liquidation.map(Fmt::price),
            summary: Some(Self::summary(&p.coin, p.size, p.entry, &orders, words)),
            guardian: rules.get(&p.coin).map(|r| Self::duties(r, words).join(" ")),
            ruler: Some(self.position_ruler(p, rules.get(&p.coin))), market: Some(p.coin.clone()),
        }).collect()
    }

    /// «0.05 at $2,412.50 · stop $2,300 · take $2,500 and $2,600»: the position's stop and takes on the exchange, the
    /// nearest take first (a partial one fills before the full one), as the Mac's positionSummary.
    pub fn summary(coin: &str, size: Decimal, entry: Decimal, orders: &[Order], say: impl Fn(&str, &str) -> String) -> String {
        let triggers: Vec<&Order> = orders.iter().filter(|o| o.coin == coin && o.is_trigger).collect();
        let stop = triggers.iter().find(|o| kind(o).contains("Stop")).and_then(|o| o.trigger_px);
        let mut takes: Vec<Decimal> = triggers.iter().filter(|o| kind(o).contains("Take")).filter_map(|o| o.trigger_px).collect();
        takes.sort_by(|a, b| if size > Decimal::ZERO { a.cmp(b) } else { b.cmp(a) });
        let amount = Fmt::num(size.abs(), 6);
        let mut out = say(&format!("{amount} at {} · stop {}", Fmt::price(entry), stop.map_or("none".into(), Fmt::price)),
                          &format!("{amount} по {} · стоп {}", Fmt::price(entry), stop.map_or("нет".into(), Fmt::price)));
        if !takes.is_empty() {
            out += &say(" · take ", " · тейк ");
            out += &takes.into_iter().map(Fmt::price).collect::<Vec<_>>().join(&say(" and ", " и "));
        }
        out
    }

    /// What the guardian does for this position, one line each (ExitRules.duties).
    pub fn duties(r: &ExitRules, say: impl Fn(&str, &str) -> String) -> Vec<String> {
        let mut out = Vec::new();
        if r.partial_order_pending { out.push(say("After entry, places half at 1R.", "После входа ставит половину на 1R.")) }
        if r.breakeven_after_partial {
            let entry = Fmt::price(r.entry);
            out.push(if r.moved_to_breakeven { say(&format!("Stop is already at breakeven ({entry})."), &format!("Стоп уже в безубытке ({entry}).")) }
                     else { say(&format!("After the first half, moves the stop to breakeven ({entry})."), &format!("После первой половины переносит стоп в безубыток ({entry}).")) });
        }
        if let Some(t) = r.trailing_pct {
            out.push(say(&format!("Trailing {}: the stop follows the best price, now {}.", Fmt::pct(t), Fmt::price(r.peak)),
                         &format!("Трейлинг {}: стоп идёт за лучшей ценой, сейчас {}.", Fmt::pct(t), Fmt::price(r.peak))));
        }
        if out.is_empty() { vec![say("Stop and take-profit sit on the exchange; ihatecrypto doesn't move anything.", "Стоп и тейк стоят на бирже; ihatecrypto ничего не двигает.")] } else { out }
    }
}

#[cfg(test)]
#[path = "position_guard/tests.rs"]
mod tests;
