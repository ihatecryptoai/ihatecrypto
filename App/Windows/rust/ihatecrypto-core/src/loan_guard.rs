//! `LoanGuard.swift`: «Don't let my Kamino loan get liquidated». The loan's health factor (liquidation at 1) is read with
//! every refresh; under `below` it prepares a repay that brings the health back to `target`, the USDC brought first from
//! wherever it is (through NEAR Intents), and sends it to the user to confirm like a price alert's steps. One warning per
//! dip: once it fires it waits until the health is back over `rearm`, halfway to `target`, so a repay that lands just
//! short of the target still re-arms it.

use chrono::Utc;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::foundation::{Date, Rounding, Uuid, dec, has, rounded};
use crate::position_command::WalletCommand;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoanGuard {
    pub id: Uuid,
    /// "kamino" (its Backpack market) or "aave".
    pub loan: String,
    /// The Aave network ("base", "arbitrum", "polygon", "ethereum"); None on Kamino.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(with = "crate::foundation::number")]
    pub below: Decimal,
    #[serde(with = "crate::foundation::number")]
    pub target: Decimal,
    /// The wallet it watches, by its Solana address: another wallet's loan is never read as this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(with = "crate::foundation::date")]
    pub created: Date,
    /// false from the moment it fires until the health is back over `rearm`.
    pub armed: bool,
    /// The repay task it made last: «stop guarding» takes it away too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fired: Option<Uuid>,
    /// Set through the local debug API: its repay never runs on the automation wallet by itself (ScheduledTask).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub untrusted: Option<bool>,
}

impl LoanGuard {
    /// Under this the repay is prepared; ihatecrypto's own borrows stop at 1.5.
    pub fn default_below() -> Decimal { dec("1.25") }
    pub fn default_target() -> Decimal { dec("1.5") }

    pub fn new(loan: &str, network: Option<&str>, below: Decimal, target: Decimal) -> Self {
        Self {
            id: Uuid::new(), loan: loan.into(), network: (loan == "aave").then(|| network.unwrap_or("base").to_string()), below,
            // A gap between the line and the target, or every small dip would make another tiny repay.
            target: target.max(below + dec("0.2")), owner: None, created: Utc::now(), armed: true, fired: None, untrusted: None,
        }
    }

    pub fn rearm(&self) -> Decimal { self.below + (self.target - self.below) / Decimal::TWO }

    /// The repay as the router's command.
    pub fn repay_command(&self, usdc: Decimal) -> String {
        let usdc = crate::foundation::text(usdc);
        if self.loan == "aave" { format!("repay {usdc} USDC on Aave on {}", self.network.as_deref().unwrap_or("base")) } else { format!("repay {usdc} USDC on kamino") }
    }

    /// The USDC to repay so the health rises from `health` to `target`. With one debt asset the health is a fixed amount
    /// over the debt, so the debt that leaves `target` is debt × health / target. Rounded up to the cent, at most the debt.
    pub fn repay(debt: Decimal, health: Decimal, target: Decimal) -> Decimal {
        if debt <= Decimal::ZERO || target <= Decimal::ZERO || health >= target { return Decimal::ZERO }
        rounded(debt * (Decimal::ONE - health / target), 2, Rounding::Up).min(debt)
    }
}

/// «guard my loan on kamino», «guard my loan on aave on arbitrum below 1.3», «stop guarding my loans»; with no model
/// connected also «следи за займом на камино», «не дай ликвидировать займ на aave», «убери защиту займа».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoanGuardCommand {
    /// `loan` None: the one loan the wallet has; `network` None on Aave: the network its debt is on. `below` None:
    /// LoanGuard::default_below.
    Set { loan: Option<String>, network: Option<String>, below: Option<Decimal> },
    Clear,
}

impl LoanGuardCommand {
    pub fn parse(text: &str) -> Option<Self> {
        let q = text.to_lowercase();
        if has(&q, r"(?:stop|cancel|remove|delete|clear)\s+(?:all\s+|my\s+|the\s+)*(?:guarding\b|loan\s+guards?)|(?:don'?t|do\s+not|no\s+longer)\s+guard\b|(?:убери|отмени|сними|выключи|останови)\w*\s+(?:все\s+)?(?:защит|слежк|охран)\w*\s+(?:за\s+|для\s+)?займ|(?:перестань|хватит|не\s+надо|не\s+нужно|больше\s+не)\s+(?:следить|защищать|охранять|смотреть)\w*\s+(?:за\s+)?(?:моим\s+|мой\s+)?займ") {
            return Some(Self::Clear);
        }
        // The router's form starts the line; in Russian the request itself, the loan named in it.
        if !has(&q, r"^\s*(?:please\s+)?(?:guard|protect)\s+(?:my\s+|the\s+)?(?:\w+\s+)?loans?\b|(?:следи|присматривай|смотри)\w*\s+за\s+(?:моим\s+|мой\s+)?займ|(?:защити|охраняй|спаси)\w*\s+(?:мой\s+)?займ|не\s+да\w*\s+(?:[^.]*\s)?ликвидир[^.]*займ|не\s+да\w*\s+(?:[^.]*\s)?займ[^.]*ликвидир") {
            return None;
        }
        let loan = if has(&q, "kamino|камино") { Some("kamino") } else if has(&q, "aave|ааве|аав") { Some("aave") } else { None };
        let network = if has(&q, "arbitrum|арбитрум") { Some("arbitrum") } else if has(&q, "polygon|полигон") { Some("polygon") }
            else if has(&q, "ethereum|mainnet|эфириум|мейннет") { Some("ethereum") } else if has(&q, r"\bbase\b|\bбейс") { Some("base") } else { None };
        let below = crate::foundation::first(&q, r"(?:below|under|\bat|ниже|меньше)\s*(\d+(?:[.,]\d+)?)")
            .and_then(|m| crate::foundation::decimal(&crate::foundation::group(&m, 1).replace(',', ".")))
            // Any line above liquidation the user names; 10+ is a typo, not a health.
            .filter(|b| *b > Decimal::ONE && *b < Decimal::TEN);
        Some(Self::Set { loan: loan.map(Into::into), network: if loan == Some("aave") { network.map(Into::into) } else { None }, below })
    }
}

/// The repay a guard prepares is the move the router already knows.
pub fn reads_as_repay(command: &str) -> bool { matches!(WalletCommand::parse(command), Some(WalletCommand::AaveRepay { .. } | WalletCommand::KaminoRepay { .. })) }
