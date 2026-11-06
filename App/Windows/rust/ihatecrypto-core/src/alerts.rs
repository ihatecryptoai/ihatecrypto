//! Price alerts (`Alerts.swift`): «Tell me when oil drops below 90»: one notification when the mark price crosses the
//! level, then the alert is done. With steps («when ETH is below 2,680: long ETH leverage 3x») it runs them instead,
//! like a task due now.

use std::collections::HashSet;

use chrono::Utc;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::foundation::{Date, Uuid, all, decimal, group, has, number, split, whole};
use crate::intent::IntentParser;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceAlert {
    pub id: Uuid,
    pub coin: String,
    /// true: fires at or above `price`; false: at or below.
    pub above: bool,
    #[serde(with = "number")]
    pub price: Decimal,
    #[serde(with = "crate::foundation::date")]
    pub created: Date,
    /// The router's commands to run when it fires; nil: only the notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<String>>,
    /// The canvas whose trade plan it prepares: drawn as that plan's first step, and when it fires it prepares the
    /// plan as it is on that canvas then, edits included (`steps` holds it as it was when set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Uuid>,
    /// Set through the local debug API: its steps never run on the automation wallet by themselves (ScheduledTask).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub untrusted: Option<bool>,
    /// Switched off on the Schedule: it waits without firing until switched on again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
}

impl PriceAlert {
    pub fn new(coin: &str, above: bool, price: Decimal, steps: Option<Vec<String>>) -> Self {
        Self { id: Uuid::new(), coin: coin.into(), above, price, created: Utc::now(), steps, plan: None, untrusted: None, paused: None }
    }

    pub fn fires(&self, mark: Decimal) -> bool { if self.above { mark >= self.price } else { mark <= self.price } }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertCommand {
    /// `above` nil: whichever side of the current price the level is on. `steps`: what to run when it fires.
    Set { coin: String, price: Decimal, above: Option<bool>, steps: Vec<String> },
    Clear,
}

impl AlertCommand {
    pub fn parse(text: &str, markets: &HashSet<String>) -> Option<Self> {
        // «when ETH is below 2,680: long ETH leverage 3x»: the level before the colon, steps after it joined by « then ».
        if let Some(colon) = text.find(':')
            && has(&text[..colon].to_lowercase(), r"^\s*(?:when|if|once|alert)\b")
        {
            let steps: Vec<String> = split(&text[colon + 1..], r"\s+then\s+|\s*;\s*").into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            if !steps.is_empty() {
                let Some(Self::Set { coin, price, above, .. }) = Self::parse(&format!("alert me {}", &text[..colon]), markets) else { return None };
                return Some(Self::Set { coin, price, above, steps });
            }
        }
        let q = text.to_lowercase();
        if has(&q, r"(?:cancel|clear|remove|delete|stop)\s+(?:all\s+|my\s+|the\s+)*(?:price\s+)?alerts?|(?:отмени|удали|убери|сними)\w*\s+(?:все\s+)?(?:алерт|уведомлен|оповещен)") {
            return Some(Self::Clear);
        }
        if !has(&q, r"\balert|notify|tell\s+me\s+when|let\s+me\s+know|ping\s+me|remind\s+me\s+when|watch\s+for|уведом|оповест|скажи\s+(?:мне\s+)?(?:когда|если)|сообщи|напомни\s+когда|алерт|дай\s+знать") {
            return None;
        }
        let coin = IntentParser::named_asset(text, markets)?;
        let price = Self::level(&q)?;
        let above = if has(&q, r"above|over|higher|rises?|goes\s+up|climbs?|reach|выше|поднимет|вырастет|больше|превыс|достигн") { Some(true) }
            else if has(&q, r"below|under|lower|drops?|falls?|goes\s+down|dips?|ниже|упад|опуст|меньше|снизит") { Some(false) }
            else { None };
        Some(Self::Set { coin, price, above, steps: vec![] })
    }

    /// The price level: «2,800» (thousands) or «93,5» (decimal comma); a number followed by % is not a price.
    fn level(q: &str) -> Option<Decimal> {
        let numbers = all(q, r"(\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+(?:[.,]\d+)?)(?!\s*%|\d)");
        let raw = group(numbers.last()?, 1);
        let normal = if whole(raw, r"\d{1,3}(?:,\d{3})+(?:\.\d+)?").is_some() { raw.replace(',', "") } else { raw.replace(',', ".") };
        decimal(&normal).filter(|d| *d > Decimal::ZERO)
    }
}
