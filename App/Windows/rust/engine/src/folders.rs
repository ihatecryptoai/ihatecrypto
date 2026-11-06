//! The canvas list's folders and pictures (Folders.swift): the icon a canvas or folder has, the folders kept in
//! folders.json, and the title the list shows for a canvas (kept into it on every save).
//! Layer 2: the list's folders and rows, the window's folder and pin requests (`folder_op`), a site's own icon, a new
//! canvas kept only once something is on it, and the model a canvas was talked through.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ihatecrypto_core::intent::{IntentParser, TradeIntent};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use wallet::{Result, WalletError};

use crate::canvas::{Board, CanvasDoc};
use crate::engine::{Engine, Request, State};
use crate::snapshot::CanvasRow;

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

/// Finder's order (Swift's localizedStandardCompare): letters without case, a run of digits by its number.
// ponytail: no locale collation (accents sort by code point); ICU's if folder names need it.
fn standard_order(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = String::new();
                    while let Some(c) = it.next_if(char::is_ascii_digit) { n.push(c) }
                    n.trim_start_matches('0').to_string()
                };
                let (m, n) = (run(&mut x), run(&mut y));
                match m.len().cmp(&n.len()).then_with(|| m.cmp(&n)) { Ordering::Equal => {} other => return other }
            }
            (Some(c), Some(d)) => {
                match c.to_lowercase().cmp(d.to_lowercase()) { Ordering::Equal => {} other => return other }
                x.next();
                y.next();
            }
        }
    }
}

impl Engine {
    pub fn canvas_folders(&self) -> Vec<String> {
        let st = self.st();
        let all: BTreeSet<String> = st.folder_store.names.iter().cloned().chain(st.canvases.iter().filter_map(|c| c.folder.clone())).collect();
        let mut list: Vec<String> = all.into_iter().collect();
        list.sort_by(|a, b| standard_order(a, b));
        list
    }

    pub fn folder_views(&self) -> Vec<FolderView> {
        let folders = self.canvas_folders();
        let st = self.st();
        folders.into_iter().map(|name| FolderView { icon: st.folder_store.icons.get(&name).cloned(), name }).collect()
    }

    pub fn canvas_rows(&self) -> Vec<CanvasRow> {
        let st = self.st();
        st.canvases.iter().map(|c| {
            let open = c.id == st.canvas_id;
            let plan = if open { st.intent.clone() } else { c.intent.clone().unwrap_or_default() };
            CanvasRow {
                id: c.id.clone(), title: shown_title(c, &c.boards), at: c.messages.last().map_or(c.at, |m| m.at), pinned_at: c.pinned_at,
                folder: c.folder.clone(), icon: c.icon.clone(), provider: c.provider.clone(), model: c.model.clone(),
                empty: c.schedule.is_none() && c.messages.is_empty() && c.boards.is_empty(), blocks: !c.boards.is_empty(),
                coin: (plan != TradeIntent::default()).then_some(plan.asset),
                note: c.boards.iter().any(|b| b.kind == "note" && b.opens.is_none()),
            }
        }).collect()
    }

    /// The window's folder and pin requests, one op each.
    pub async fn folder_op(&self, r: &Request) -> Result<HashMap<String, String>> {
        let text = |v: &Option<String>| v.clone().unwrap_or_default();
        match r.op.as_str() {
            "siteIcon" => return self.site_icon(&text(&r.address)).await,
            "pinCanvas" => self.pin_canvas(&text(&r.wallet), r.test == Some(true))?,
            "moveCanvas" => self.move_canvas(&text(&r.wallet), r.name.as_deref()),
            "createFolder" => self.create_folder(&text(&r.name)),
            "renameFolder" => self.rename_folder(&text(&r.label), &text(&r.name)),
            "deleteFolder" => self.delete_folder(&text(&r.name)),
            "newCanvasIn" => {
                let Some(folder) = Engine::folder_name(&text(&r.name)) else { return Ok(HashMap::new()) };
                self.new_canvas(false)?;
                {
                    let mut st = self.st_mut();
                    let i = st.canvas_index();
                    st.canvases[i].folder = Some(folder.clone());
                }
                self.create_folder(&folder);
                self.save_canvases();
            }
            "setIcon" => {
                let icon = match r.text.as_deref().filter(|t| !t.is_empty()) {
                    // JSONDecoder's words for a shape it can't read.
                    Some(t) => Some(serde_json::from_str::<CanvasIcon>(t)
                        .map_err(|_| WalletError("The data couldn’t be read because it isn’t in the correct format.".into()))?),
                    None => None,
                };
                if let Some(icon) = &icon && icon.clone().checked().is_none() {
                    return Err(WalletError(self.say("That picture can't be used.", "Эту картинку не получится поставить.")));
                }
                let canvas = r.wallet.as_ref().and_then(|id| self.st().canvases.iter().position(|c| &c.id == id));
                if let Some(i) = canvas {
                    self.st_mut().canvases[i].icon = icon;
                    self.save_canvases();
                } else if let Some(name) = &r.name && self.canvas_folders().contains(name) {
                    self.set_folder_store(|s| match icon {
                        Some(icon) => { s.icons.insert(name.clone(), icon); }
                        None => { s.icons.remove(name); }
                    });
                }
            }
            _ => {}
        }
        self.changed();
        Ok(HashMap::new())
    }

    /// A site's own icon for the picker's «Logo from a website», asked of the site itself as the Mac does (a favicon
    /// service would learn the names): its touch icon, then its favicon. The window scales it to 128 px.
    pub async fn site_icon(&self, domain: &str) -> Result<HashMap<String, String>> {
        let mut host = domain.to_lowercase().trim().to_string();
        if let Some(at) = host.find("://") { host = host[at + 3..].to_string() }
        let host = host.split('/').find(|p| !p.is_empty()).unwrap_or("").to_string();
        if !host.contains('.') || host.chars().count() > 253 || !host.chars().all(|c| c.is_alphabetic() || c.is_numeric() || ".-".contains(c)) {
            return Err(WalletError(self.say("Type a site's address, like bybit.com.", "Напиши адрес сайта, например bybit.com.")));
        }
        for path in ["apple-touch-icon.png", "favicon.ico"] {
            let asked = net::HTTP.get(format!("https://{host}/{path}")).timeout(Duration::from_secs(8)).send().await;
            let Ok(answer) = asked else { continue };
            if answer.status().as_u16() != 200 { continue }
            let Ok(data) = answer.bytes().await else { continue };
            if !(100..512_000).contains(&data.len()) { continue }
            // A picture by its first bytes, not a page answering for one.
            let kind = if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) { "image/png" } else if data.starts_with(&[0, 0, 1, 0]) { "image/x-icon" } else { continue };
            return Ok(HashMap::from([("image".to_string(), format!("data:{kind};base64,{}", STANDARD.encode(&data)))]));
        }
        Err(WalletError(self.say(&format!("No icon found for {host}."), &format!("У {host} не нашлось иконки."))))
    }

    /// Pinning past the limit is refused with what to do instead.
    pub fn pin_canvas(&self, id: &str, on: bool) -> Result<()> {
        let full = {
            let st = self.st();
            let Some(c) = st.canvases.iter().find(|c| c.id == id) else { return Ok(()) };
            on && c.pinned_at.is_none() && st.canvases.iter().filter(|c| c.pinned_at.is_some()).count() >= MAX_PINNED
        };
        if full {
            return Err(WalletError(self.say(&format!("{MAX_PINNED} canvases are pinned already: unpin one first."),
                                            &format!("Уже закреплено {MAX_PINNED} холстов: сначала открепи один."))));
        }
        if let Some(c) = self.st_mut().canvases.iter_mut().find(|c| c.id == id) {
            c.pinned_at = if on { Some(c.pinned_at.unwrap_or_else(chrono::Utc::now)) } else { None };
        }
        self.save_canvases();
        Ok(())
    }

    /// None or an empty name takes it out of its folder; a new name makes the folder. Both folders stay in the list.
    pub fn move_canvas(&self, id: &str, folder: Option<&str>) {
        let Some(old) = self.st().canvases.iter().find(|c| c.id == id).map(|c| c.folder.clone()) else { return };
        let to = folder.and_then(Engine::folder_name);
        for f in [old, to.clone()].into_iter().flatten() { self.create_folder(&f) }
        if let Some(c) = self.st_mut().canvases.iter_mut().find(|c| c.id == id) { c.folder = to }
        self.save_canvases();
    }

    pub fn create_folder(&self, name: &str) {
        let Some(f) = Engine::folder_name(name) else { return };
        if !self.st().folder_store.names.contains(&f) { self.set_folder_store(|s| s.names.push(f)) }
    }

    /// Its canvases go with it, merging into a folder of that name if one is there; its picture too, unless that one has
    /// its own.
    pub fn rename_folder(&self, old: &str, name: &str) {
        let Some(new) = Engine::folder_name(name).filter(|n| n != old) else { return };
        for c in self.st_mut().canvases.iter_mut().filter(|c| c.folder.as_deref() == Some(old)) { c.folder = Some(new.clone()) }
        self.set_folder_store(|s| {
            s.names.retain(|n| n != old && *n != new);
            s.names.push(new.clone());
            if let Some(icon) = s.icons.remove(old) && !s.icons.contains_key(&new) { s.icons.insert(new.clone(), icon); }
        });
        self.save_canvases();
    }

    /// The folder goes; its canvases stay, out of any folder.
    pub fn delete_folder(&self, name: &str) {
        for c in self.st_mut().canvases.iter_mut().filter(|c| c.folder.as_deref() == Some(name)) { c.folder = None }
        self.set_folder_store(|s| {
            s.names.retain(|n| n != name);
            s.icons.remove(name);
        });
        self.save_canvases();
    }

    /// The canvas being left had nothing on it: it goes, as the Mac keeps a new canvas only once it's written on. A
    /// scheduled item's own canvas stays: it leaves the list when its item view closes (the live part's).
    pub fn forget_unkept(&self) {
        let mut st = self.st_mut();
        let i = st.canvas_index();
        let c = &st.canvases[i];
        if c.schedule.is_some() || !c.messages.is_empty() || !c.boards.is_empty() || st.canvases.len() <= 1 { return }
        st.canvases.remove(i);
    }

    /// The model that answered on the open canvas: its company's logo on the row, its name in the row's tooltip.
    pub fn canvas_talked(&self, provider: &str) {
        let model = self.model();
        {
            let mut st = self.st_mut();
            let i = st.canvas_index();
            let c = &mut st.canvases[i];
            if c.provider.as_deref() == Some(provider) && c.model.as_deref() == Some(model.as_str()) { return }
            c.provider = Some(provider.into());
            c.model = if model.is_empty() { None } else { Some(model) };
        }
        self.save_canvases();
    }

    pub fn folder_name(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.graphemes(true).take(40).collect()) }
    }
}
