//! Price alerts (Alerts.swift): «Tell me when ETH is above 4000», and «when ETH is below 2,680: long ETH leverage 3x»:
//! the level is checked against Hyperliquid's mark every 10 s while ihatecrypto is open. A plain alert says so once and
//! is done; one with steps runs them as a task due now, so a move among them becomes a card that waits for Confirm,
//! never a signature by itself. An alert tied to a canvas's trade plan is drawn as that plan's first step.

use std::collections::HashSet;
use std::time::Duration;

use chrono::Utc;
use ihatecrypto_core::alerts::{AlertCommand, PriceAlert};
use ihatecrypto_core::foundation::{Num, Rounding, Uuid};
use ihatecrypto_core::intent::{Fmt, IntentParser, TradeIntent};
use ihatecrypto_core::network::Hyperliquid;
use ihatecrypto_core::scheduled_task::ScheduledTask;
use rust_decimal::Decimal;

use crate::canvas::{Board, Step};
use crate::engine::Engine;
use crate::notices::Notice;

/// «away» as Fmt.num prints it: Swift's Decimal is NaN where the arithmetic gives out (a mark of 0).
pub(crate) fn away(price: Decimal, mark: Decimal) -> String {
    ((Num::of(price) - mark) / mark * 100).rounded(1, Rounding::Plain).abs().0.map_or_else(|| "NaN".into(), |d| Fmt::num(d, 2))
}

/// «WTI oil (xyz:CL)» → «WTI oil».
pub(crate) fn short_name(coin: &str) -> String { IntentParser::pretty(coin).split(" (").next().unwrap_or(coin).to_string() }

impl Engine {
    pub fn alert_text(&self, a: &PriceAlert) -> String {
        format!("{} {} {}{}", IntentParser::pretty(&a.coin), if a.above { self.say("above", "выше") } else { self.say("below", "ниже") }, Fmt::price(a.price),
                a.steps.as_ref().map(|s| format!(": {}", s.join(" then "))).unwrap_or_default())
    }

    pub async fn alert(&self, command: AlertCommand, note: Option<&str>) {
        let note = note.map(str::to_string);
        match command {
            AlertCommand::Clear => {
                let n = self.st().alerts.len();
                self.set_alerts(Vec::clear);
                self.reply(&if n == 0 { self.say("There were no price alerts.", "Ценовых оповещений не было.") }
                           else { self.say(&format!("Price alerts cleared ({n})."), &format!("Ценовые оповещения сняты ({n}).")) }, note);
            }
            AlertCommand::Set { coin, price, above, steps } => {
                let empty = self.st().markets.is_empty();
                if empty {
                    let read = Hyperliquid::markets().await.unwrap_or_default();
                    self.st_mut().markets = read;
                }
                let (mark, none) = { let st = self.st(); (st.markets.get(&coin).map(|m| m.mark_px), st.markets.is_empty()) };
                let Some(mark) = mark else {
                    let pretty = IntentParser::pretty(&coin);
                    // No markets at all: Hyperliquid didn't answer, which isn't «no market» (the Mac: «No price … right now»).
                    self.reply(&if none {
                        self.say(&format!("No price for {pretty} right now — Hyperliquid didn't answer. Nothing was set; try again in a moment."),
                                 &format!("Цены {pretty} сейчас нет — Hyperliquid не ответил. Оповещение не поставлено, попробуй через минуту."))
                    } else {
                        self.say(&format!("Hyperliquid has no market for {coin}, so there's no price to watch."), &format!("На Hyperliquid нет рынка {coin} — нечего отслеживать."))
                    }, note);
                    return;
                };
                // A step must be something a chain runs (Chains.knows, the Mac's Alerts.swift): it fires as a task due now.
                if let Some(bad) = steps.iter().find(|s| !self.knows(s)) {
                    self.reply(&self.say(&format!("When it fires, “{bad}” isn't something the Windows app can do yet. Nothing was set."),
                                         &format!("«{bad}» Windows-версия пока не умеет. Оповещение не поставлено.")), note);
                    return;
                }
                let mut a = PriceAlert::new(&coin, above.unwrap_or(price > mark), price, if steps.is_empty() { None } else { Some(steps.clone()) });
                if a.fires(mark) {
                    let pretty = IntentParser::pretty(&coin);
                    self.reply(&self.say(&format!("{pretty} is already at {}: that level is passed.", Fmt::price(mark)), &format!("{pretty} уже {}: этот уровень пройден.", Fmt::price(mark))), note);
                    return;
                }
                // «Add it to the plan»: a step that is this canvas's plan (its market and side) ties the alert to the plan.
                // It becomes the plan's first step, one per plan: a new level replaces the old.
                let (intent, managed, canvas) = {
                    let st = self.st();
                    let managed = st.traded_plan.as_ref() == Some(&st.intent) || st.positions.iter().any(|p| p.coin == st.intent.asset);
                    (st.intent.clone(), managed, st.canvas_id.clone())
                };
                if intent != TradeIntent::default() && !managed && self.same_plan(Some(&steps), &intent) && let Some(id) = Uuid::parse(&canvas) {
                    a.plan = Some(id);
                    let above = a.above;
                    self.set_alerts(|list| {
                        list.retain(|x| x.plan != Some(id));
                        list.push(a);
                    });
                    if let Some(step) = self.plan_alert_step() {
                        let board = self.st().plan_board.clone();
                        self.update_board(board.as_deref(), |b| {
                            let rest = b.steps.take().unwrap_or_default().into_iter().filter(|s| s.status.as_deref() != Some("watching"));
                            b.steps = Some(std::iter::once(step).chain(rest).collect());
                        });
                    }
                    let (name, away, (p, m)) = (short_name(&coin), away(price, mark), (Fmt::price(price), Fmt::price(mark)));
                    self.reply(&self.say(
                        &format!("🔔 Added to the plan as its first step: when {name} is {} {p} (now {m}, {away}% away), this plan comes back to you to confirm, with any changes you make to it until then.",
                                 if above { "above" } else { "below" }),
                        &format!("🔔 Добавил в план первым шагом: когда {name} будет {} {p} (сейчас {m}, {away}% до уровня), этот план придёт тебе на подтверждение — со всеми правками, что внесёшь до тех пор.",
                                 if above { "выше" } else { "ниже" })), note);
                    return;
                }
                let text = self.alert_text(&a);
                let steps_none = a.steps.is_none();
                self.set_alerts(|list| list.push(a));
                self.pin(Board::new("alerts", "alerts"));
                let m = Fmt::price(mark);
                self.reply(&(self.say(&format!("Set: {text} (now {m}). It's checked every 10 s while ihatecrypto is open"),
                                      &format!("Поставил: {text} (сейчас {m}). Проверяю каждые 10 с, пока ihatecrypto открыт"))
                             + &if steps_none { ".".into() } else { self.say("; when it fires, its moves come as cards for Confirm.", "; когда сработает, действия придут карточками на подтверждение.") }), note);
            }
        }
    }

    /// The alerts the new marks crossed: said in the chat, and their steps read.
    pub async fn check_alerts(&self) {
        let due: Vec<PriceAlert> = {
            let st = self.st();
            st.alerts.iter().filter(|a| st.markets.get(&a.coin).is_some_and(|m| a.fires(m.mark_px))).cloned().collect()
        };
        if due.is_empty() { return }
        let ids: HashSet<Uuid> = due.iter().map(|a| a.id).collect();
        self.set_alerts(|list| list.retain(|a| !ids.contains(&a.id)));
        for a in &due {
            // Its steps run as a task due now, as on the Mac: once nothing else is going on (never over a card waiting
            // for Confirm), from the wallet that's on, each move waiting for Confirm — never off the automation wallet.
            if let Some(mut steps) = a.steps.clone() {
                // Tied to a plan: that plan as it is now on its own canvas, edits included (the open one's live, another's as
                // saved, the Mac's CanvasSession) — unless it's no longer that trade.
                let plan = a.plan.and_then(|id| {
                    let st = self.st();
                    let id = id.to_string();
                    if id == st.canvas_id { Some(st.intent.clone()) } else { st.canvases.iter().find(|c| c.id == id).and_then(|c| c.intent.clone()) }
                });
                if let Some(plan) = plan.filter(|p| *p != TradeIntent::default()) {
                    steps = steps.into_iter()
                        .map(|step| if self.plan_step(&step).is_some_and(|t| t.asset == plan.asset && t.side == plan.side) { plan.command() } else { step })
                        .collect();
                }
                let because = format!("{} {} {}", IntentParser::pretty(&a.coin), if a.above { self.say("went above", "выше") } else { self.say("went below", "ниже") },
                                      Fmt::price(a.price));
                let mut task = ScheduledTask::new(steps, Utc::now(), None, Some(because));
                task.from_active = Some(true);
                self.set_tasks(|t| t.push(task));
                continue;
            }
            let mark = self.st().markets.get(&a.coin).map_or(a.price, |m| m.mark_px);
            let text = format!("🔔 {}{}{}", self.alert_text(a), self.say(" — now ", " — сейчас "), Fmt::price(mark));
            self.notify(Notice::Alerts, &self.say("Price alert", "Ценовое оповещение"), &text, false, false, None, None);
            self.reply(&(text + &self.say(" The alert is done; set another any time.", " Оповещение сработало; поставь новое, когда захочешь.")), Some("price alert".into()));
        }
        // Its steps run as a task due now, in the scheduled run beside the loops (never twice at once).
        self.start_scheduled_run();
    }

    /// A step that is a trade plan («long SOL risk $0.3 …»), read as that plan; None for any other command.
    pub fn plan_step(&self, step: &str) -> Option<TradeIntent> {
        let known: HashSet<String> = self.st().markets.keys().cloned().collect();
        if IntentParser::starts_plan(step, &known) { IntentParser::apply(step, &TradeIntent::default(), &known) } else { None }
    }

    /// Whether one of the steps is this trade (its market and side).
    pub fn same_plan(&self, steps: Option<&[String]>, plan: &TradeIntent) -> bool {
        steps.unwrap_or_default().iter().any(|s| self.plan_step(s).is_some_and(|t| t.asset == plan.asset && t.side == plan.side))
    }

    /// The alert tied to this canvas's plan while the plan is still that trade: drawn as its first step, not in the
    /// alerts block, and gone with «clear the plan».
    pub fn plan_alert(&self) -> Option<PriceAlert> {
        let (intent, canvas, alerts) = { let st = self.st(); (st.intent.clone(), st.canvas_id.clone(), st.alerts.clone()) };
        if intent == TradeIntent::default() { return None }
        let id = Uuid::parse(&canvas)?;
        alerts.into_iter().find(|a| a.plan == Some(id) && self.same_plan(a.steps.as_deref(), &intent))
    }

    /// The plan's alert as its first route step: the level, how far the price is, and what happens when it gets there.
    pub fn plan_alert_step(&self) -> Option<Step> {
        let a = self.plan_alert()?;
        let mut lines = vec![];
        let mark = self.st().markets.get(&a.coin).map(|m| m.mark_px).filter(|m| *m > Decimal::ZERO);
        if let Some(mark) = mark {
            let away = ((Num::of(a.price) - mark) / mark * 100).rounded(1, Rounding::Plain).0.map_or_else(|| "NaN".into(), |d| Fmt::num(d, 2));
            lines = vec![self.say("now", "сейчас") + "\t" + &Fmt::price(mark), self.say("away", "до уровня") + "\t" + &away + "%"];
        }
        lines.push(self.say("then this plan comes to you to confirm", "потом этот план придёт тебе на подтверждение"));
        // The label in English as the route's other steps (it picks the card's picture); the lines in the reply language.
        Some(Step {
            label: format!("WHEN · PRICE {} TO", if a.above { "RISES" } else { "FALLS" }), title: format!("{} {}", short_name(&a.coin), Fmt::price(a.price)),
            lines, money: None, status: Some("watching".into()),
        })
    }

    /// For the second check: a plan waiting for its alert enters at the price then, not today's — without this line the
    /// check reads «long below 117.44» against today's entry and cries mismatch.
    pub fn plan_alert_facts(&self) -> Vec<String> {
        let Some(a) = self.plan_alert() else { return vec![] };
        let asset = self.st().intent.asset.clone();
        let p = Fmt::price(a.price);
        vec![format!("It waits for a price alert: when {asset} {} to {p}, the plan comes to the user to confirm and enters at the market price then (≈ {p}); \
                      the entry above is today's price, for sizing.", if a.above { "rises" } else { "falls" })]
    }

    /// The alerts' prices every 10 s while there are any, as the Mac checks them; the block's «now · away» follows.
    pub fn watch_alerts(&self) {
        if self.st().watching_alerts { return }
        self.st_mut().watching_alerts = true;
        let me = self.rc();
        tokio::task::spawn_local(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                // Exit rules too, as the Mac's watch(): a trailing stop or the move to breakeven lags 10 s, not a minute.
                let none = me.st().alerts.is_empty();
                let idle = none && me.exit_rules().is_empty();
                if idle { continue }
                let Ok(m) = Hyperliquid::markets().await else { continue };
                me.st_mut().markets = m;
                me.check_alerts().await;
                me.guard_now().await;
                me.changed();
            }
        });
    }
}

#[cfg(test)]
#[path = "alerts/tests.rs"]
mod tests;
