//! What the user tells the chat to keep, check, journal and study (`AgentCommands.swift`).

use crate::foundation::{count, group, trim_set, whole};

/// «remember: …», «forget: …», «what do you remember»: what the user wants the chat to keep in mind from now on. Kept
/// only in their own words (the app checks the words are in their own message, so a page or a token's name can't
/// plant one) and given to the model with every message as preferences — never as a reason to skip a check or a
/// Confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryCommand {
    Remember(String),
    /// The notes holding these words; «all» forgets every one.
    Forget(String),
    List,
}

impl MemoryCommand {
    pub fn parse(text: &str) -> Option<Self> {
        if let Some(m) = whole(text, r"(?is)\s*remember:\s*(.+?)\s*") { return Some(Self::Remember(group(&m, 1).into())) }
        if let Some(m) = whole(text, r"(?is)\s*forget:\s*(.+?)\s*") { return Some(Self::Forget(group(&m, 1).into())) }
        whole(text, r"(?i)\s*what\s+do\s+you\s+remember\s*\??\s*").map(|_| Self::List)
    }

    /// Whether `note` is the user's own words: in their message, ignoring case, spacing and the quotes around it.
    pub fn in_their_words(note: &str, message: &str) -> bool {
        let plain = |s: &str| trim_set(&s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" "), "«»\"'“”.!,: ").to_string();
        let n = plain(note);
        count(&n) >= 3 && plain(message).contains(&n)
    }
}

/// «check: Aave health factor on Base; warn under 1.6»: a step for the schedule. When it comes due the model reads what
/// it needs itself and speaks only when the condition is met — a reply, cards and a notification; a move it suggests
/// waits for the user's Confirm, even with an automation wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCheck {
    pub what: String,
}

impl AgentCheck {
    pub fn parse(text: &str) -> Option<Self> { whole(text, r"(?is)\s*check:\s*(.+?)\s*").map(|m| Self { what: group(&m, 1).into() }) }
}

/// «journal HYPE: long here, funding isn't bad, keeps getting bought on dips»: the trader's own words about a trade,
/// kept with what the market was doing then (JournalEntry), so the trading study can later set the reasoning against
/// what happened. Only their own words, as for memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalCommand {
    pub asset: String,
    pub words: String,
}

impl JournalCommand {
    pub fn parse(text: &str) -> Option<Self> {
        // A HIP-3 market keeps its exchange's prefix: «journal xyz:GOLD: …».
        whole(text, r"(?is)\s*journal\s+((?:[a-z0-9]+:)?[^\s:]+)\s*:\s*(.+?)\s*")
            .map(|m| Self { asset: group(&m, 1).into(), words: group(&m, 2).into() })
    }
}

/// «study hypotheses: …»: the trading study's research memory, one file each as the skill names them. Each write adds
/// a dated entry; the `study` lookup reads them back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudyNote {
    pub file: String,
    pub text: String,
}

impl StudyNote {
    /// The command's word → the file the study calls it.
    pub const FILES: [(&str, &str); 8] = [
        ("profile", "TRADER_PROFILE"), ("playbook", "TRADING_PLAYBOOK"), ("sources", "DATA_SOURCES"), ("reconstruction", "TRADE_RECONSTRUCTION"),
        ("hypotheses", "HYPOTHESES"), ("experiments", "EXPERIMENT_LOG"), ("questions", "OPEN_QUESTIONS"), ("progress", "PROGRESS"),
    ];

    pub fn parse(text: &str) -> Option<Self> {
        let m = whole(text, r"(?is)\s*study\s+([a-z_]+)\s*:\s*(.+?)\s*")?;
        let word = group(&m, 1).to_lowercase();
        let (_, file) = Self::FILES.iter().find(|(key, value)| *key == word || value.to_lowercase() == word)?;
        Some(Self { file: file.to_string(), text: group(&m, 2).into() })
    }
}
