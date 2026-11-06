//! Skills and the Trading study (Skills.swift): Settings › Skills (the built-in Trading study, and .md or .txt files the
//! user adds), the study's journal and notes in the engine's folder, and what the user asked the chat to remember — the
//! Mac's, over the same core. A skill changes how the assistant works with the user, never the risk rules, the checks or
//! the Confirm. Swift's WalletErrorText is `wallet::WalletError` here; its `static var`s `studyFolder` and `journalKept`
//! are `study_folder()` and `journal_kept()`.

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;

use ihatecrypto_core::agent_commands::{JournalCommand, MemoryCommand, StudyNote};
use ihatecrypto_core::intent::{Fmt, IntentParser};
use ihatecrypto_core::normalize::Lookup;
use ihatecrypto_core::skills::Skill;
use ihatecrypto_core::trade_journal::JournalEntry;
use ihatecrypto_core::{trade_history, trade_study};
use rust_decimal::Decimal;
use unicode_segmentation::UnicodeSegmentation;
use wallet::WalletError;
use wallet::address_check::{self, Network};

use crate::chat_commands::{prefix, trim_spaces};
use crate::engine::Engine;
use crate::store;

/// The study's folder in the engine's.
pub fn study_folder() -> PathBuf {
    let folder = store::url("study");
    let _ = std::fs::create_dir_all(&folder);
    folder
}

/// The study's journal is kept: Settings › Skills' «Journal» opens it (greyed out before the first entry, as on the Mac).
pub fn journal_kept() -> bool { store::url("journal.json").exists() }

impl Engine {
    pub fn skills_on(&self) -> Vec<Skill> {
        let st = self.st();
        Skill::built_ins().into_iter().chain(st.added_skills.iter().cloned()).filter(|s| !st.skills_off.contains(&s.id)).collect()
    }

    /// A skill from a file's text the window read (its own file dialog).
    pub fn add_skill(&self, text: &str, file_name: &str) -> Result<(), WalletError> {
        if text.trim().is_empty() { return Err(WalletError(self.say("The file is empty.", "Файл пустой."))) }
        let mut s = Skill::parse(&prefix(text, 60_000), file_name);
        let same = self.st().added_skills.iter().position(|a| a.id == s.id);
        if let Some(i) = same {
            self.set_added_skills(|a| a[i] = s);
            return Ok(());
        }
        let (base, built) = (s.id.clone(), Skill::built_ins());
        let mut n = 2;
        while built.iter().any(|b| b.id == s.id) {
            s.id = format!("{base}-{n}");
            n += 1;
        }
        self.set_added_skills(|a| a.push(s));
        Ok(())
    }

    /// The person's last own message: a scheduled run's request on its canvas (note «schedule») is the task's steps, which
    /// a model may have written, so a «remember» or a journal entry in it is never kept as theirs.
    pub fn own_words(&self) -> String {
        let st = self.st();
        st.messages().iter().rev().find(|m| m.from_user && m.note.as_deref() != Some("schedule")).map(|m| m.text.clone()).unwrap_or_default()
    }

    pub fn memory_command(&self, c: MemoryCommand, note: Option<String>) {
        let line = match c {
            MemoryCommand::Remember(words) => {
                // Only words of their own last message: a page, a note or a token's name can't plant a preference.
                if !MemoryCommand::in_their_words(&words, &self.own_words()) {
                    let line = self.say("I keep only your own words. Write «remember: …» and I'll keep exactly that.",
                                        "Запоминаю только твои собственные слова. Напиши «запомни: …» — сохраню ровно это.");
                    return self.reply(&line, note);
                }
                let kept = prefix(&words, 300);
                let lower = kept.to_lowercase();
                self.set_memory(|m| {
                    m.retain(|x| x.to_lowercase() != lower);
                    m.push(kept.clone());
                });
                self.say(&format!("I'll keep in mind: «{kept}». «What do you remember» lists it; «forget …» drops it."),
                         &format!("Запомнил: «{kept}». «Что ты помнишь» — весь список; «забудь …» — убрать."))
            }
            MemoryCommand::Forget(words) => {
                let w = words.to_lowercase();
                let before = self.st().memory.len();
                self.set_memory(|m| if w == "all" { m.clear() } else { m.retain(|x| !x.to_lowercase().contains(&w)) });
                let after = self.st().memory.len();
                if before == after { self.say("Nothing I keep has those words.", "Среди запомненного таких слов нет.") } else { self.say("Forgotten.", "Забыл.") }
            }
            MemoryCommand::List => {
                let memory = self.st().memory.clone();
                if memory.is_empty() {
                    self.say("I don't keep anything yet: say «remember: …».", "Пока ничего не запомнено: скажи «запомни: …».")
                } else {
                    self.say("I keep in mind:\n", "Помню:\n") + &memory.iter().enumerate().map(|(i, m)| format!("{}. {m}", i + 1)).collect::<Vec<_>>().join("\n")
                }
            }
        };
        self.reply(&line, note);
    }

    pub async fn journal_command(&self, j: JournalCommand, note: Option<String>) {
        if !MemoryCommand::in_their_words(&j.words, &self.own_words()) {
            let line = self.say("The journal keeps only your own words: write the entry yourself and I'll save exactly that.",
                                "В журнал записываю только твои слова: напиши запись сам — сохраню ровно её.");
            return self.reply(&line, note);
        }
        let names: HashSet<String> = self.st().markets.keys().cloned().collect();
        let coin = IntentParser::named_asset(&j.asset, &names).unwrap_or_else(|| j.asset.clone());
        let mut e = JournalEntry::new(chrono::Utc::now(), &coin, &prefix(&j.words, 2000));
        let market = self.st().markets.get(&coin).cloned();
        if let Some(m) = market {
            (e.price, e.funding, e.change24h) = (Some(m.mark_px), m.funding, m.change24h());
            e.volume24h = (m.volume24h > Decimal::ZERO).then_some(m.volume24h);
        }
        e.open_interest_usd = JournalEntry::open_interest_usd(&coin).await;
        let others: Vec<_> = ["BTC", "ETH"].into_iter().filter(|c| *c != coin).filter_map(|c| self.st().markets.get(c).cloned().map(|m| (c, m))).collect();
        e.market = others.into_iter().map(|(c, m)| {
            format!("{c} {}", Fmt::price(m.mark_px)) + &m.change24h().map_or(String::new(), |d| format!(" (24h {})", self.signed(d)))
        }).collect();
        let position = self.st().positions.iter().find(|p| p.coin == coin).cloned();
        if let Some(p) = position {
            e.position = Some(format!("{} {} at {}, unrealized {}", if p.size > Decimal::ZERO { "long" } else { "short" }, Fmt::num(p.size.abs(), 6),
                                      Fmt::price(p.entry), self.signed_usd(p.unrealized_pnl)));
        }
        let mut entries: Vec<JournalEntry> = store::load("journal.json").unwrap_or_default();
        entries.push(e.clone());
        store::save(&entries, "journal.json");
        let (name, snapshot) = (IntentParser::pretty(&coin), e.snapshot());
        let line = self.say(&format!("Saved to your journal — {name}: {snapshot}."), &format!("Записал в журнал — {name}: {snapshot}."));
        self.reply(&line, note);
    }

    pub fn study_note(&self, n: StudyNote, note: Option<String>) {
        let url = study_folder().join(format!("{}.md", n.file));
        let entry = format!("\n## {}\n{}\n", Fmt::date(chrono::Utc::now()), n.text);
        match std::fs::OpenOptions::new().append(true).open(&url) {
            Ok(mut h) => { let _ = h.write_all(entry.as_bytes()); }
            Err(_) => { let _ = std::fs::write(&url, format!("# {}\n{entry}", n.file)); }
        }
        let line = self.say(&format!("Noted in the study's {}.", n.file), &format!("Записал в {} исследования.", n.file));
        self.reply(&line, note);
    }

    /// The skills' lookups: a skill's whole text, an account's trades rebuilt, the journal, the study's notes.
    pub async fn skill_lookup(&self, l: Lookup) -> String {
        match l.tool.as_str() {
            "skill" => {
                let on = self.skills_on();
                let Some(s) = on.iter().find(|s| s.id == Skill::slug(&l.arg) || s.name.to_lowercase() == l.arg.to_lowercase()) else {
                    let ids = if on.is_empty() { "none".into() } else { on.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ") };
                    return format!("No switched-on skill «{}». Switched on: {ids}.", l.arg);
                };
                {
                    let mut st = self.st_mut();
                    if !st.loaded_skills.contains(&s.id) { st.loaded_skills.push(s.id.clone()) }
                }
                format!("Skill «{}», loaded on this chat from now on — follow it:\n{}", s.name, s.text)
            }
            "trades" => {
                let asked = trim_spaces(&l.arg);
                let me = asked.is_empty() || asked.to_lowercase() == "me";
                let address = if me { self.st().evm_address() } else { Some(asked.to_string()) };
                let Some(address) = address.filter(|a| address_check::problem(a, Network::Base).is_none()) else {
                    return format!("trades takes a Hyperliquid account's 0x address, or me: «{asked}» isn't one.");
                };
                let (fills, funding) = tokio::join!(trade_history::all_fills(&address), trade_history::all_funding(&address));
                let Ok(all) = fills else {
                    return format!("Hyperliquid didn't answer for {}; try again in a minute.", if me { "this wallet" } else { address.as_str() });
                };
                let stories = trade_history::stories(&all);
                let trades = trade_study::trades(&stories, &funding.unwrap_or_default());
                let open = stories.iter().filter(|s| s.is_open()).count();
                format!("{}: {} fills{}.\n{}", if me { "The wallet in ihatecrypto".to_string() } else { format!("Account {address}") }, all.len(),
                        if all.len() >= 10_000 { " (Hyperliquid serves only the last 10,000: older trades aren't in it)" } else { "" },
                        trade_study::report(&trades, open, trade_study::cut(&stories), &chrono::Local))
            }
            "journal" => {
                let entries: Vec<JournalEntry> = store::load("journal.json").unwrap_or_default();
                if entries.is_empty() { return "The journal is empty: nothing was written with the journal command yet.".into() }
                let (st, now) = (self.st(), chrono::Utc::now());
                let last = &entries[entries.len().saturating_sub(40)..];
                format!("The journal, oldest first{}:\n{}", if entries.len() > 40 { format!(" (the last 40 of {})", entries.len()) } else { String::new() },
                        last.iter().map(|e| e.review(st.markets.get(&e.asset).map(|m| m.mark_px), now)).collect::<Vec<_>>().join("\n"))
            }
            // study
            _ => {
                let one = StudyNote::parse(&format!("study {}: x", l.arg)).map(|n| n.file);
                let notes: Vec<String> = StudyNote::FILES.iter().map(|(_, file)| *file).filter(|f| one.as_deref().is_none_or(|o| o == *f)).filter_map(|file| {
                    let text = std::fs::read_to_string(study_folder().join(format!("{file}.md"))).ok()?;
                    let keep = if one.is_none() { 1200 } else { 6000 };
                    let all: Vec<&str> = text.graphemes(true).collect();
                    Some(format!("{file}:\n{}", if all.len() > keep { "…".to_string() + &all[all.len() - keep..].concat() } else { text.clone() }))
                }).collect();
                if notes.is_empty() { format!("No study notes{} yet.", one.map_or(String::new(), |o| format!(" in {o}"))) } else { notes.join("\n\n") }
            }
        }
    }

    /// Settings › Skills: the study's notes folder, or its journal, in Explorer (the Mac opens them in Finder). Only these
    /// two fixed paths, never one the page names.
    pub fn open_study_files(&self, journal: bool) -> Result<(), WalletError> {
        if journal && !journal_kept() { return Err(WalletError(self.say("No journal entries yet.", "В журнале пока нет записей."))) }
        let target = if journal { store::url("journal.json") } else { study_folder() };
        let opener = if cfg!(windows) { format!("{}\\explorer.exe", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())) } else { "/usr/bin/open".into() };
        std::process::Command::new(opener).arg(target).spawn().map(drop).map_err(|e| WalletError(e.to_string()))
    }
}
