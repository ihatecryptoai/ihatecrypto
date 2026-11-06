//! A line of the history that says money moved (`MovedLine.swift`): the sentence ihatecrypto writes when a move is
//! done — «Sent 5 USDT to Automation on Solana. https://…», «Swapped 3 USDC → USDT +2.997906.», «💸 Received +55.09
//! USDC on Solana (now 55.09).». The History board's rows and the chat's account of where money went both read them,
//! so they say the same.
// ponytail: read from the executor's own sentences by their first word; a new kind of move shows once its verb is listed here.

use rust_decimal::Decimal;

use crate::foundation::{decimal, first, group, replacing};
use crate::intent::Fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Sent,
    Received,
    Swapped,
    Moved,
    Bought,
    Sold,
    Deposited,
    Withdrew,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MovedLine {
    pub kind: Kind,
    /// Its first sentence, without the link, the emoji or what follows a colon: what the row says.
    pub text: String,
    /// The first amount it names, and its coin.
    pub amount: Option<Decimal>,
    pub symbol: Option<String>,
    /// Who a send went to: «Automation».
    pub to: Option<String>,
    pub link: Option<String>,
}

const VERBS: [(&str, Kind); 11] = [
    ("Sent ", Kind::Sent), ("💸 Received ", Kind::Received), ("💸 Пришло ", Kind::Received), ("Swapped ", Kind::Swapped), ("Moved ", Kind::Moved),
    ("Bought ", Kind::Bought), ("Sold ", Kind::Sold), ("Deposited ", Kind::Deposited), ("Supplied ", Kind::Deposited), ("Withdrew ", Kind::Withdrew),
    ("Closed ", Kind::Closed),
];

impl MovedLine {
    pub fn new(line: &str) -> Option<Self> {
        let mut kind = VERBS.iter().find(|(verb, _)| line.starts_with(verb))?.1;
        let link = first(line, r"https?://[^\s]+[^\s.,)]").map(|m| group(&m, 0).to_string());
        let mut s = replacing(line, r"\s*https?://\S+", |_| String::new());
        if let Some(rest) = s.strip_prefix("💸 ") { s = rest.into() }
        // The first clause: a period ending a sentence (not one inside a number), a colon, a bracket or a semicolon.
        if let Some(end) = first(&s, r"\.(\s|$)|: |; | \(").and_then(|m| m.get(0)).map(|m| m.start()) { s.truncate(end) }
        // «Sent 5 USDT from Arbitrum to Optimism over USDT0»: the wallet's own money between its networks.
        if kind == Kind::Sent && s.contains(" from ") && !s.contains("from the multisig") { kind = Kind::Moved }
        let found = first(&s, r"(\d[\d,]*(?:\.\d+)?)\s+([A-Z][A-Z0-9]{1,9})\b");
        let amount = found.as_ref().and_then(|m| decimal(&group(m, 1).replace(',', "")));
        let symbol = found.as_ref().map(|m| group(m, 2).to_string());
        let to = if kind == Kind::Sent { first(&s, r"\sto (.+?)(?: on [A-Z][\w ]*)?$").map(|m| group(&m, 1).to_string()) } else { None };
        Some(Self { kind, text: s, amount, symbol, to, link })
    }

    /// What left the wallet and what came in, by coin and by whom, then the swaps and moves: «25 USDT to Automation · 5
    /// sends». Oldest to newest or the other way, the same.
    pub fn summary(lines: &[MovedLine]) -> Vec<String> {
        let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        let mut out = Vec::new();
        // ponytail: sends to the same count keep the order they came in; Swift's grouping leaves that order to its hash.
        let mut sent: Vec<(String, Vec<&MovedLine>)> = Vec::new();
        for l in lines.iter().filter(|l| l.kind == Kind::Sent && l.amount.is_some()) {
            let key = format!("{}\u{1}{}", l.symbol.as_deref().unwrap_or(""), l.to.as_deref().unwrap_or(""));
            match sent.iter_mut().find(|(k, _)| *k == key) { Some((_, g)) => g.push(l), None => sent.push((key, vec![l])) }
        }
        sent.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (_, group) in &sent {
            let total: Decimal = group.iter().filter_map(|l| l.amount).sum();
            out.push(format!("−{} {} sent{} · {}", Fmt::num(total, 6), group[0].symbol.as_deref().unwrap_or(""),
                             group[0].to.as_ref().map_or(String::new(), |t| format!(" to {t}")), plural(group.len(), "send")));
        }
        let mut came: Vec<(String, Decimal)> = Vec::new();
        for l in lines.iter().filter(|l| l.kind == Kind::Received && l.amount.is_some()) {
            let symbol = l.symbol.clone().unwrap_or_default();
            match came.iter_mut().find(|(s, _)| *s == symbol) { Some((_, t)) => *t += l.amount.unwrap_or_default(), None => came.push((symbol, l.amount.unwrap_or_default())) }
        }
        came.sort_by(|a, b| a.0.cmp(&b.0));
        for (symbol, total) in came { out.push(format!("+{} {symbol} came in", Fmt::num(total, 6))) }
        let swaps = lines.iter().filter(|l| matches!(l.kind, Kind::Swapped | Kind::Bought | Kind::Sold)).count();
        if swaps > 0 { out.push(format!("{}, the money stays in the wallet", plural(swaps, "swap"))) }
        let moves = lines.iter().filter(|l| matches!(l.kind, Kind::Moved | Kind::Deposited | Kind::Withdrew)).count();
        if moves > 0 { out.push(format!("{} between its own places", plural(moves, "move"))) }
        out
    }
}
