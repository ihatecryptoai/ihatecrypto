//! The Swift tests' cases (App/Tests/IhatecryptoCoreTests and the others that touch these files), with their vectors,
//! and what Swift and Foundation printed for the behaviour mirrored in `foundation` (probed with swiftc, 2026-11-06).

mod address_book;
mod agent_commands;
mod alerts;
mod chat_language;
mod folders;
mod foundation;
mod funding_plan;
mod guardian;
mod hedge;
mod moved_line;
mod scheduled_task;
mod site_check;
mod skills;
mod stablecoins;
mod stock_names;
mod take_style;
mod tokens;
mod top_up;

use std::collections::HashSet;

use rust_decimal::Decimal;

fn d(s: &str) -> Decimal { crate::foundation::dec(s) }

fn set(items: &[&str]) -> HashSet<String> { items.iter().map(|s| s.to_string()).collect() }

fn strings(items: &[&str]) -> Vec<String> { items.iter().map(|s| s.to_string()).collect() }
