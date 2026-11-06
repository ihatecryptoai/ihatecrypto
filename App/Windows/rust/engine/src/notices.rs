//! Windows notifications (Notices.swift): each kind can be turned off in Settings › General › Notifications. The window
//! shows them (main.rs); the engine decides which go out. What it last saw (positions, limit entries, when the last went,
//! the update found) is in `State`'s notice_* fields and available_update.
//! Layer 2 adds here: notify, notice_views, set_notice, notice_test, set_open_at_login, update_ready, crashed, the
//! money-arrived and HLP-unlock checks (hlp-unlock-noticed.json).

use serde::{Deserialize, Serialize};

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
