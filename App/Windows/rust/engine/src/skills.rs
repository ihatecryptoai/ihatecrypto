//! Skills and the Trading study (Skills.swift): Settings › Skills, the built-in Trading study, its journal and notes in
//! the engine's folder. Swift's WalletErrorText is `wallet::WalletError` here.
//! Layer 2 adds here: add_skill, the skills the chat loads, the study's journal (journal.json) and files, open_study_files.

use std::path::PathBuf;

use crate::store;

/// The study's folder in the engine's.
pub fn study_folder() -> PathBuf {
    let folder = store::url("study");
    let _ = std::fs::create_dir_all(&folder);
    folder
}

/// The study's journal is kept: Settings › Skills' «Journal» opens it (greyed out before the first entry, as on the Mac).
pub fn journal_kept() -> bool { store::url("journal.json").exists() }
