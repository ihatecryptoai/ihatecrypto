//! Windows notifications (Notices.swift): each kind can be turned off in Settings › General › Notifications. The window
//! shows them (main.rs); the engine decides which go out. What it last saw (positions, limit entries, when the last went,
//! the update found) is in `State`'s notice_* fields and available_update.
//! Layer 2: notify (recorded in the History, sent unless its kind is off), the Settings switches and test, a move's end,
//! a card waiting, open at login, an update found, a kept crash, and Hyperliquid's limit fills and HLP unlock.

use std::collections::{BTreeSet, HashMap, HashSet};

use guard::hl::HLP_VAULT;
use ihatecrypto_core::foundation;
use ihatecrypto_core::intent::{Fmt, IntentParser};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use trade::HyperliquidAccount;

use crate::engine::Engine;
use crate::store;

/// What a Windows notification is about, as the Mac's Notice (its raw value in Settings' noticesOff).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Notice {
    Incoming,
    Positions,
    Orders,
    Alerts,
    Moves,
    Hlp,
    Signature,
    Confirm,
    Update,
}

impl Notice {
    pub const ALL: [Notice; 9] = [Self::Incoming, Self::Positions, Self::Orders, Self::Alerts, Self::Moves, Self::Hlp, Self::Signature, Self::Confirm, Self::Update];

    pub fn raw(self) -> &'static str {
        match self {
            Self::Incoming => "incoming", Self::Positions => "positions", Self::Orders => "orders", Self::Alerts => "alerts", Self::Moves => "moves",
            Self::Hlp => "hlp", Self::Signature => "signature", Self::Confirm => "confirm", Self::Update => "update",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> { Self::ALL.into_iter().find(|n| n.raw() == raw) }

    pub fn title(self, say: impl Fn(&str, &str) -> String) -> String {
        match self {
            Self::Incoming => say("Money arrived", "Пришли деньги"),
            Self::Positions => say("A position closed, or its stop moved", "Позиция закрылась или её стоп сдвинулся"),
            Self::Orders => say("A limit order filled", "Исполнилась лимитная заявка"),
            Self::Alerts => say("A price alert fired", "Сработало ценовое оповещение"),
            Self::Moves => say("A confirmed move finished or failed", "Подтверждённое действие завершилось или не прошло"),
            Self::Hlp => say("The HLP deposit unlocked", "Депозит в HLP разблокирован"),
            Self::Signature => say("A scheduled move waits for Windows Hello", "Действие по расписанию ждёт Windows Hello"),
            Self::Confirm => say("A move or a trade waits for your Confirm", "Действие или сделка ждёт твоего «Подтвердить»"),
            Self::Update => say("A new version of ihatecrypto is ready", "Готова новая версия ihatecrypto"),
        }
    }

    /// The Mac's sounds: a coin when money arrived or moved, a chime when something wants a look, a low knock when a move
    /// failed. The window plays it (main.rs), its toast silent.
    pub fn sound(self, failed: bool) -> &'static str {
        if failed { "knock" } else if matches!(self, Self::Incoming | Self::Orders | Self::Moves | Self::Hlp) { "coin" } else { "chime" }
    }
}

/// `Engine.NoticeView`: a kind and whether it's on, for Settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoticeView {
    pub id: String,
    pub title: String,
    pub on: bool,
}

impl Engine {
    pub fn notice_views(&self) -> Vec<NoticeView> {
        let off: HashSet<String> = self.st().settings.notices_off.clone().unwrap_or_default().into_iter().collect();
        Notice::ALL.into_iter().map(|n| NoticeView { id: n.raw().into(), title: n.title(|en, ru| self.say(en, ru)), on: !off.contains(n.raw()) }).collect()
    }

    /// One Windows notification, unless its kind is off. `when_away`: only while the window isn't in front (a card the
    /// person may not be looking at); main.rs checks that. `record`: what ihatecrypto did by itself (money arrived, an
    /// alert, the guardian, a limit filled, HLP unlocked) is a History record too, notified or not, as the Mac's
    /// notify(record:); a confirmed move has its own, and a card waiting or an update isn't one. None: by its kind.
    /// `schedule`: the Schedule item it's about («perp:BTC»), as the Mac's notify(schedule:).
    #[allow(clippy::too_many_arguments)]
    pub fn notify(&self, kind: Notice, title: &str, body: &str, failed: bool, when_away: bool, record: Option<bool>, schedule: Option<String>) {
        if record.unwrap_or(!matches!(kind, Notice::Moves | Notice::Confirm | Notice::Update)) {
            self.record_automatic(title, body, None, failed, false, schedule.as_deref());
        }
        {
            let mut st = self.st_mut();
            if st.settings.notices_off.as_ref().is_some_and(|off| off.iter().any(|o| o == kind.raw())) { return }
            st.notice_last = chrono::Utc::now();
        }
        let mut event = HashMap::from([("kind", kind.raw()), ("title", title), ("body", body), ("sound", kind.sound(failed))]);
        if failed { event.insert("failed", "1"); }
        if when_away { event.insert("whenAway", "1"); }
        self.out.event("notify", &event);
    }

    pub fn set_notice(&self, id: &str, on: bool) {
        if Notice::from_raw(id).is_none() { return }
        self.set_settings(|s| {
            let mut off: BTreeSet<String> = s.notices_off.take().unwrap_or_default().into_iter().collect();
            if on { off.remove(id); } else { off.insert(id.into()); }
            s.notices_off = if off.is_empty() { None } else { Some(off.into_iter().collect()) };
        });
    }

    /// Settings' «Send a test»: always goes out, whatever is switched off.
    pub fn notice_test(&self) {
        let body = self.say("Notifications work: scheduled tasks, stops, alerts and moves waiting for Confirm show up here.",
                            "Уведомления работают: задачи по расписанию, стопы, оповещения и действия, ждущие подтверждения, появятся здесь.");
        self.out.event("notify", &HashMap::from([("kind", "test"), ("title", "ihatecrypto"), ("sound", Notice::Incoming.sound(false)), ("body", &body)]));
    }

    /// A confirmed move's end: done or failed, with the chat saying why.
    pub fn notice_move(&self, title: &str, state: Option<&str>) {
        let ok = state != Some("failed") && state != Some("unknown");
        let body = if ok {
            self.say("Done — checked, signed and confirmed.", "Готово — проверено, подписано и подтверждено сетью.")
        } else if state == Some("unknown") {
            self.say("Sent, status unknown — check the links in the chat before doing it again.", "Отправлено, статус неизвестен — проверь ссылки в чате, прежде чем повторять.")
        } else {
            self.say("It didn’t go through; the chat says why.", "Не прошло — причина в чате.")
        };
        self.notify(Notice::Moves, title, &body, !ok, false, None, None);
    }

    /// A card on the canvas while the person looks elsewhere. Not right after another notice (a scheduled move says so
    /// itself): one tap is enough.
    pub fn notice_card(&self, title: &str) {
        if (chrono::Utc::now() - self.st().notice_last).num_milliseconds() <= 5_000 { return }
        self.notify(Notice::Confirm, &self.say("Waiting for your Confirm", "Ждёт твоего «Подтвердить»"), title, false, true, None, None);
    }

    /// Open at login, from Settings or the onboarding: kept, and main.rs told to set Windows' own entry (it answers with
    /// `loginState`, so the switch shows what Windows has).
    pub fn set_open_at_login(&self, on: bool) {
        self.set_settings(|s| s.open_at_login = Some(on));
        self.out.event("openAtLogin", &HashMap::from([("on", if on { "1" } else { "0" })]));
    }

    /// The version the updater (main.rs) found, until this copy restarts into it.
    pub fn update_ready(&self, version: &str) {
        self.st_mut().available_update = Some(version.into());
        self.changed();
        self.notify(Notice::Update, &self.say(&format!("ihatecrypto {version} is ready"), &format!("Готова ihatecrypto {version}")),
                    &self.say("Click to open ihatecrypto and install it from Settings › About.", "Нажми, чтобы открыть ihatecrypto и поставить её в «Настройки › О приложении»."),
                    false, false, None, None);
    }

    /// The window or the engine stopped unexpectedly last time (main.rs keeps the report): one line in the chat.
    pub fn crashed(&self, date: &str, summary: &str, folder: &str) {
        // main.rs passes the report's time in seconds since 1970.
        let when = date.parse::<f64>().ok().filter(|t| t.is_finite()).map_or_else(|| date.to_string(), |t| Fmt::date(foundation::unix(t)));
        self.reply(&self.say(&format!("ihatecrypto closed unexpectedly on {when} — {summary}. The report is kept in {folder}."),
                             &format!("ihatecrypto упал {when} — {summary}. Отчёт сохранён в {folder}.")), None);
    }

    /// What happened on Hyperliquid since the last read, as the Mac's refresh says it: a resting limit entry that filled,
    /// the HLP deposit's lock that ended. (A position closed outside ihatecrypto is the guardian's: position_guard.rs.)
    pub async fn notice_hyperliquid(&self) {
        let (user, positions, before, own_quiet) = {
            let mut st = self.st_mut();
            let Some(user) = st.evm_address().filter(|_| !st.watching()) else { return };
            let now: HashSet<String> = st.positions.iter().map(|p| p.coin.clone()).collect();
            let before = st.notice_positions.replace(now);
            // Not while one of ours runs: opening it is the move itself.
            let quiet = st.running.is_none() && (chrono::Utc::now() - st.last_own_move).num_milliseconds() > 60_000;
            (user, st.positions.clone(), before, quiet)
        };
        if let Some(before) = before && own_quiet {
            let entries = self.st().notice_entries.clone();
            for p in positions.iter().filter(|p| !before.contains(&p.coin) && entries.contains(&p.coin)) {
                let coin = IntentParser::pretty(&p.coin);
                let long = p.size > Decimal::ZERO;
                let (size, entry) = (Fmt::num(p.size.abs(), 6), Fmt::price(p.entry));
                self.notify(Notice::Orders, &self.say(&format!("Limit filled: {coin}"), &format!("Лимитка исполнилась: {coin}")),
                            &self.say(&format!("{} {size} at {entry}; its stop and take are on the exchange.", if long { "Long" } else { "Short" }),
                                      &format!("{} {size} по {entry}; стоп и тейк стоят на бирже.", if long { "Лонг" } else { "Шорт" })),
                            false, false, None, Some(format!("perp:{}", p.coin)));
            }
        }
        let same = |e: &Engine| e.st().evm_address().as_deref() == Some(user.as_str());
        // ponytail: its own read of the open orders, one more request a minute; the guardian's `hl_orders` can feed it.
        let orders = HyperliquidAccount::open_orders(&user).await;
        if let Ok(orders) = orders && same(self) {
            self.st_mut().notice_entries = orders.iter().filter(|o| !o.is_trigger && !o.reduce_only).map(|o| o.coin.clone()).collect();
        }
        // The HLP deposit's 4-day lock ended: said once per lock.
        let vault = HyperliquidAccount::vault_equity(&user, HLP_VAULT).await;
        if let Ok((equity, _, Some(until))) = vault && same(self) && until <= chrono::Utc::now() && equity > Decimal::ZERO {
            let key = foundation::seconds(until);
            if store::load::<f64>("hlp-unlock-noticed.json") != Some(key) {
                store::save(&key, "hlp-unlock-noticed.json");
                self.notify(Notice::Hlp, &self.say("HLP unlocked", "HLP разблокирован"),
                            &self.say(&format!("{} in the HLP vault can be withdrawn now.", Fmt::usd(equity)),
                                      &format!("{} в хранилище HLP теперь можно вывести.", Fmt::usd(equity))), false, false, None, None);
            }
        }
    }
}
