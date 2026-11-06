//! Commands, their parsers, plans, risk, holdings and the chat router's forms: the Rust port of
//! `App/Sources/IhatecryptoCore`, one module per Swift file, with the same answers and the same words for the same
//! input. Modules marked «part» hold only what an earlier round needed of a later round's file; that round completes
//! them.

pub mod foundation;

pub mod address_book_command;
pub mod agent_commands;
pub mod alerts;
pub mod charts;
pub mod chat_language;
pub mod folders;
pub mod funding_plan;
pub mod guardian;
pub mod hedge;
pub mod intent;
pub mod model_edit;
pub mod modules;
pub mod moved_line;
pub mod plan;
pub mod position_command;
pub mod risk;
pub mod scheduled_task;
pub mod site_check;
pub mod skills;
pub mod stablecoins;
pub mod stock_names;
pub mod take_style;
pub mod token_research;
pub mod tokens;
pub mod top_up;
pub mod trading_study_skill;

// Round 5 (the other session).
pub mod holdings;
pub mod pnl;
pub mod trade_history;
pub mod trade_journal;
pub mod trade_study;

/// Part: Solana's balance and holdings come with round 5's `Holding`.
pub mod network;

#[cfg(test)]
mod tests;
