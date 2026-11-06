//! A site's request and a doubted trade waiting for the person's word (Consent.swift): what they need ticked, the
//! request as its own block, and declining it (here because the lock declines one).
//! Layer 2 adds here: asking a site's request (ask_site), answer_site, expire_site, decline_site(onCanvas:), the second
//! check's ticks on a trade, lookalike_tick and the rest of the file's `extension Engine`.

use serde::Serialize;

use crate::engine::Engine;

/// What a site's request still needs the person's word for before it can be approved (the Mac's ApprovalSheet `ask`):
/// a lookalike address, programs off the list, a payee or contract that isn't theirs, the second check's concern.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteTick {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub toggle: String,
}

/// A site's request as the window draws it: apart from the person's own card, with its whole origin, and answered by
/// its id (`siteAnswer`), never by the move card's Confirm.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteAskView {
    pub id: String,
    pub origin: String,
    pub host: String,
    /// «known», «lookalike» (of `real`), «unknown», «local».
    pub check: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real: Option<String>,
    pub title: String,
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    /// Every one must be ticked for Approve to count.
    pub ticks: Vec<SiteTick>,
    /// «Connect» or «Sign».
    pub approve: String,
    /// The canvas its block is on: «Review on the canvas» opens that one.
    pub canvas: String,
    /// A Solana transaction as the Mac's sheet reads it: «You send 5 USDC to Kamino Vault», each balance change with its
    /// ≈ $, the checks folded to one line, and what happens after signing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    pub balances: Vec<SiteBalance>,
    pub checks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One balance a site's transaction changes, simulated: «−5» USDC ≈ $5.00; SOL that only pays the fee and rent says so.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteBalance {
    pub symbol: String,
    pub amount: String,
    pub up: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Engine {
    /// The site's request waiting is answered no, its block cancelled with `why`.
    pub fn decline_site(&self, why: &str) {
        let Some(mut ask) = self.st_mut().site_ask.take() else { return };
        self.update_board(Some(&ask.view.id), |b| {
            b.state = Some("cancelled".into());
            b.lines.push(why.into());
        });
        ask.answer(false);
        self.changed();
    }
}
