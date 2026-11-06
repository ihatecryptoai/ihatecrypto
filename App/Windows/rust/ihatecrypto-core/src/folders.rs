//! Canvas folders from the chat (`Folders.swift`): «save this canvas to folder Research» moves the open canvas there,
//! «write to folder Research: …» keeps the text there as a note of its own, without leaving the open canvas. A folder
//! that isn't there yet is made.

use crate::foundation::{chars, first, group};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderCommand {
    Save { folder: String },
    Note { folder: String, text: String },
}

impl FolderCommand {
    /// The folder a name means among those there: itself, the same name in other letter case, or one a typo away —
    /// one letter wrong, missing or extra, two in a name of eight or more («Reesarch», «Ресеррч» → «Research»,
    /// «Ресерч»). Without one it's a new folder. A closest name, never a guess between two.
    pub fn existing(name: &str, folders: &[String]) -> Option<String> {
        let lower = name.to_lowercase();
        if let Some(same) = folders.iter().find(|f| f.to_lowercase() == lower) { return Some(same.clone()) }
        let want = chars(&lower);
        let near: Vec<&String> = folders.iter().filter(|f| {
            let f = f.to_lowercase();
            let have = chars(&f);
            want.len().min(have.len()) >= 4 && distance(&want, &have) <= if want.len().max(have.len()) >= 8 { 2 } else { 1 }
        }).collect();
        (near.len() == 1).then(|| near[0].clone())
    }

    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim();
        if let Some(m) = first(t, r#"(?is)^(?:write|note|add\s+a\s+note|запиши\w*|напиши\w*|заметк\w*|добавь\s+заметку)\s+(?:(?:to|in|into)\s+(?:the\s+)?folder|в\s+папк[уе])\s+[«"“]?([^:«»"“”]+?)[»"”]?\s*:\s*(.+)$"#) {
            if let Some(folder) = name(group(&m, 1)) {
                let note = group(&m, 2).trim();
                return (!note.is_empty()).then(|| Self::Note { folder, text: note.into() });
            }
        }
        if let Some(m) = first(t, r#"(?i)^(?:save|put|move|keep|сохрани\w*|положи\w*|перенеси\w*|перемести\w*|убери\w*|добавь)\s+(?:(?:this|the|my|этот|это|мой)\s+)?(?:canvas|chat|it|холст|канвас|чат|его)?\s*(?:to|in|into|в)\s+(?:the\s+)?(?:folder|папк[уе])\s+[«"“]?(.+?)[»"”]?\s*[.!]?$"#) {
            if let Some(folder) = name(group(&m, 1)) { return Some(Self::Save { folder }) }
        }
        None
    }
}

/// The edit distance between two names: Levenshtein's, with two neighbours swapped as one slip («реесрч» for
/// «ресерч»), the typo a hand makes most.
pub(crate) fn distance(a: &[&str], b: &[&str]) -> usize {
    if a.is_empty() || b.is_empty() { return a.len().max(b.len()) }
    let mut d = vec![vec![0; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() { row[0] = i }
    for j in 0..=b.len() { d[0][j] = j }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] { d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1) }
        }
    }
    d[a.len()][b.len()]
}

/// Up to 40 characters, as the sidebar keeps it; nil for an empty one.
fn name(raw: &str) -> Option<String> {
    let n = raw.trim();
    (!n.is_empty()).then(|| chars(n).into_iter().take(40).collect())
}
