//! «Take profit the way the best investors do» (`TakeStyle.swift`): three common ways to take profit on the same stop.
//! The stop never changes between them; only how the profit is taken does.

use rust_decimal::Decimal;

use crate::foundation::has;
use crate::intent::{Fmt, TradeIntent};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TakeStyle {
    /// All of it at twice the risk.
    Classic,
    /// Half at 1R, the stop to breakeven, the rest at 2R. ihatecrypto's default «by the book».
    Partial,
    /// Half at 1R, the rest follows the price with a 2% trailing stop; a far take at 4R as a cap.
    Trend,
}

impl TakeStyle {
    pub const ALL: [TakeStyle; 3] = [Self::Classic, Self::Partial, Self::Trend];

    pub fn raw(self) -> &'static str { match self { Self::Classic => "classic", Self::Partial => "partial", Self::Trend => "trend" } }

    /// The user asks for the take-profit «like the best investors / pros», in Russian or English.
    pub fn asks_for_choice(text: &str) -> bool {
        let q = text.to_lowercase();
        has(&q, "тейк|профит|take|profit|выход|exit")
            && has(&q, r"как\s+советуют|как\s+(?:делают|ставят)\s+(?:лучшие|профи|про\b)|лучши[ех]\s+(?:инвестор|трейдер)|(?:best|top|pro)\s+(?:investors|traders)|like\s+(?:the\s+)?pros|how\s+(?:the\s+)?pros|take[- ]?profit\s+options|take\s+options|варианты\s+тейк")
    }

    pub fn apply(self, intent: &TradeIntent) -> TradeIntent {
        let mut i = intent.clone();
        i.take_pct = None;
        let (r, partial, trailing) = match self { Self::Classic => (2, false, None), Self::Partial => (2, true, None), Self::Trend => (4, true, Some(2)) };
        i.take_r = Some(r.into());
        i.partial_take = Some(partial);
        i.trailing_pct = trailing.map(Decimal::from);
        i
    }

    pub fn title(self) -> &'static str { match self { Self::Classic => "Classic 2R", Self::Partial => "In parts", Self::Trend => "Ride the trend" } }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Classic => "All of it at 2× the risk.",
            Self::Partial => "Half at 1R, stop to breakeven, the rest at 2R.",
            Self::Trend => "Half at 1R, the rest follows the price with a 2% trailing stop.",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            Self::Classic => "Profitable if you win more than a third of your trades.",
            Self::Partial => "After the first half the risk is almost zero.",
            Self::Trend => "No ceiling, but you give some back on the reversal.",
        }
    }

    /// What it pays at the take for a trade that loses `risk` at the stop.
    pub fn payoff(self, risk: Decimal) -> String {
        match self {
            Self::Classic => format!("+{}", Fmt::usd(risk * Decimal::TWO)),
            Self::Partial => format!("+{}", Fmt::usd(risk / Decimal::TWO + risk)),
            Self::Trend => "no cap".into(),
        }
    }
}
