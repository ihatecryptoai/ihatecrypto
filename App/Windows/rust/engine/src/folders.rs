//! The canvas list's folders and pictures (Folders.swift): the icon a canvas or folder has, the folders kept in
//! folders.json, and the title the list shows for a canvas (kept into it on every save).
//! Layer 2 adds here: canvas_folders, folder_views, canvas_rows, folder_op (pin, move, create, rename, delete, newCanvasIn,
//! setIcon, siteIcon) and the rest of the file's `extension Engine`.

use std::collections::HashMap;

use ihatecrypto_core::intent::IntentParser;
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

use crate::canvas::{Board, CanvasDoc};
use crate::engine::State;

/// A picture from the icon library (the Mac's AddressIcon): a token or network logo, one of the library's glyphs, an
/// emoji, or the person's own picture as a small PNG («data:image/png;base64,…», 128 px, made by the window).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanvasIcon {
    pub kind: String,
    pub value: String,
}

impl CanvasIcon {
    /// What the window may send: a known kind, a short name, an emoji, or a small picture of its own making.
    pub fn checked(self) -> Option<CanvasIcon> {
        let count = self.value.graphemes(true).count();
        let ok = match self.kind.as_str() {
            "logo" | "symbol" => count <= 60 && !self.value.is_empty()
                && self.value.graphemes(true).all(|g| g.chars().next().is_some_and(|c| c.is_alphabetic() || c.is_numeric() || ".-_".contains(c))),
            "emoji" => (1..=16).contains(&count),
            // ponytail: kept inline in canvases.json and sent with every state; a file beside it if many pictures slow it.
            "image" => self.value.starts_with("data:image/png;base64,") && count <= 80_000,
            _ => false,
        };
        ok.then_some(self)
    }
}

/// The folders' names and pictures: a folder lasts until it's deleted, even emptied (the Mac's `folders`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FolderStore {
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub icons: HashMap<String, CanvasIcon>,
}

/// A folder as the canvas list draws it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FolderView {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<CanvasIcon>,
}

/// At most this many canvases stay pinned to the top of the list.
pub const MAX_PINNED: usize = 5;

/// What the list calls a canvas, as the Mac's: the person's own name, else its board's name, else the name kept from
/// its last board (keep_board_title), else its first message's words.
pub fn shown_title(c: &CanvasDoc, boards: &[Board]) -> String {
    if let Some(name) = c.name.as_ref().filter(|n| !n.is_empty()) { return name.clone() }
    board_title(boards).unwrap_or_else(|| c.title.clone())
}

/// A canvas's board named in the Mac's canvasBubbles order: a move (waiting, done, a spread, an address save), else a
/// chart while no plan or rates are there, else the rates, else the plan — «Take-profit choices» while it offers them.
pub fn board_title(boards: &[Board]) -> Option<String> {
    let named: Vec<&Board> = boards.iter().filter(|b| !b.title.is_empty()).collect();
    if let Some(m) = named.iter().find(|b| b.kind == "move" && b.plan.is_none()) { return Some(m.title.clone()) }
    let plan = named.iter().find(|b| b.kind == "move" && b.plan.is_some());
    let rates = named.iter().find(|b| b.kind == "result" && b.data.as_ref().is_some_and(|d| d.rates.is_some()));
    if plan.is_none() && rates.is_none() && let Some(chart) = named.iter().find(|b| b.kind == "chart") {
        return Some(format!("Chart · {}", chart.chart.as_ref().and_then(|c| c.coin.as_deref()).map_or_else(|| chart.title.clone(), IntentParser::pretty)));
    }
    if let Some(r) = rates { return Some(r.title.clone()) }
    let plan = plan?;
    Some(if plan.plan.as_ref().and_then(|p| p.takes.as_ref()).is_some_and(|t| !t.is_empty()) { "Take-profit choices".into() } else { plan.title.clone() })
}

impl State {
    /// The open canvas keeps its board's name, as the Mac writes it into the canvas on every save: closing the chart or
    /// the rates later doesn't give the canvas its first words back.
    pub fn keep_board_title(&mut self) {
        let i = self.canvas_index();
        if let Some(title) = board_title(&self.canvases[i].boards) && self.canvases[i].title != title { self.canvases[i].title = title }
    }
}
