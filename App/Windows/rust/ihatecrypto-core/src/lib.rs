//! Commands, their parsers, plans, risk, holdings and the chat router's forms: the Rust port of
//! `App/Sources/IhatecryptoCore`, one module per Swift file, with the same answers and the same words for the same
//! input. Modules marked «part» hold only what an earlier round needed of a later round's file; that round completes
//! them.

pub mod foundation;

pub mod address_book_command;
pub mod agent_commands;
pub mod alerts;
pub mod chat_language;
pub mod folders;
pub mod funding_plan;
pub mod guardian;
pub mod hedge;
pub mod moved_line;
pub mod scheduled_task;
pub mod site_check;
pub mod skills;
pub mod stablecoins;
pub mod stock_names;
pub mod take_style;
pub mod tokens;
pub mod top_up;
pub mod trading_study_skill;

/// Part.
pub mod charts;
/// Part.
pub mod intent;
/// Part.
pub mod network;
/// Part.
pub mod plan;
/// Part.
pub mod position_command;

#[cfg(test)]
mod tests;
