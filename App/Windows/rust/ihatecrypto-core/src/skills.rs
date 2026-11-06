//! Instructions the chat follows for one kind of work, switched on and off in Settings › Skills (`Skills.swift`; the
//! owner, 2026-10-16: «было бы неплохо сделать в настройках скиллы»). The model is told each switched-on skill's name
//! and what it is for; when a message is that work it loads the whole text (the `skill` lookup), which then comes with
//! every message on that canvas. A skill only steers the chat: Confirm, the risk rules and the checks are code and stay
//! as they are.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::foundation::{chars, first, group, is_letter, is_number, replacing, split_chars, trim_set, trim_spaces};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    /// What it is for, in a line: all the model sees of it until it loads it.
    pub about: String,
    pub text: String,
    pub built_in: bool,
}

impl Skill {
    pub fn new(id: &str, name: &str, about: &str, text: &str) -> Self {
        Self { id: id.into(), name: name.into(), about: about.into(), text: text.into(), built_in: false }
    }

    /// A skill from a file: a SKILL.md's front matter (`name:`, `description:`) when it has one, else the file's name
    /// and its first line of text.
    pub fn parse(file: &str, file_name: &str) -> Self {
        let (mut body, mut fields) = (file, HashMap::new());
        if let Some(m) = first(file, r"(?s)\A\s*---\n(.*?)\n---[ \t]*\n?") {
            // Lines as Swift splits them: a CRLF is one Character, not a «\n».
            for line in split_chars(group(&m, 1), |g| g == "\n") {
                let Some((key, value)) = line.split_once(':') else { continue };
                fields.insert(trim_spaces(key).to_lowercase(), trim_set(value, " \t\"'").to_string());
            }
            body = &file[m.get(0).expect("a match").end()..];
        }
        let stem = replacing(delete_extension(file_name), r"[_\-]+", |_| " ".into());
        let stem = trim_spaces(&stem);
        let name = fields.get("name").filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| {
            let c = chars(stem);
            if c.is_empty() { "Skill".into() } else { c[0].to_uppercase() + &c[1..].concat() }
        });
        let first_line = split_chars(body, |g| g == "\n").into_iter().map(trim_spaces).find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"));
        let about = fields.get("description").filter(|d| !d.is_empty()).cloned()
            .or_else(|| first_line.map(|l| chars(l).into_iter().take(200).collect()))
            .unwrap_or_default();
        Self::new(&Self::slug(&name), &name, &about, body.trim())
    }

    /// «Trading study» → «trading-study»: how the model names it in the lookup.
    pub fn slug(s: &str) -> String {
        let lower = s.to_lowercase();
        let parts = split_chars(&lower, |g| !is_letter(g) && !is_number(g));
        if parts.is_empty() { "skill".into() } else { parts.join("-") }
    }

    /// The skills that come with ihatecrypto.
    pub fn built_ins() -> Vec<Skill> { vec![Self::trading_study()] }
}

/// `NSString.deletingPathExtension`: «SKILL.md» → «SKILL»; «.hidden» and «x.» keep theirs.
fn delete_extension(name: &str) -> &str {
    let last = name.rfind('/').map_or(0, |i| i + 1);
    match name[last..].rfind('.') {
        Some(dot) if dot > 0 && last + dot + 1 < name.len() => &name[..last + dot],
        _ => name,
    }
}
