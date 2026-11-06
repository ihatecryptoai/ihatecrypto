//! `ChatRecall.swift`: how much of a canvas's chat goes to the model with each message — the whole conversation while
//! it fits, its newest part once it doesn't, so a chat never ends by itself.

use llm::Request;

use crate::normalize::ChatRouter;

pub struct ChatRecall;

impl ChatRecall {
    /// What `sent` keeps at most, in tokens, when the window allows more.
    pub const MOST: i64 = 4_000;

    /// A text's size in tokens, near enough for a budget: four bytes a token (English ≈ 4 letters, Cyrillic ≈ 2). Claude
    /// Opus 5.5 counted about twice that for a chat of both languages (2026-10-13): the budgets below leave room.
    pub fn tokens(text: &str) -> i64 { (text.len() as i64 + 3) / 4 }

    /// The chat's lines that go with a message, oldest first.
    /// A wide window (a cloud model's): the whole chat up to `most` tokens (`MOST`), then its newest lines.
    /// A small one (`small`: a model on this computer, whose window is 16k or unknown; or a window under 100k): the last
    /// 12 lines, as every model got before 2026-10-13, and fewer when they're long — it holds the instructions and little
    /// else, and Ollama cuts a prompt that doesn't fit from its start, the instructions.
    pub fn sent(lines: &[String], window: Option<i64>, small: bool, instructions: i64, most: i64) -> Vec<String> {
        let small = small || window.unwrap_or(i64::MAX) < 100_000;
        // 3,000: what the brief adds around the chat (canvas, balances, positions, the message) and the answer.
        let room = window.map_or(most, |w| w.saturating_sub(instructions).saturating_sub(3_000).max(400));
        Self::fitting(if small { &lines[lines.len().saturating_sub(12)..] } else { lines }, most.min(room))
    }

    /// The newest lines that fit `budget` tokens, oldest first. The last two go whatever their size: a follow-up («1»,
    /// «да») is read by the question before it.
    pub fn fitting(lines: &[String], budget: i64) -> Vec<String> {
        let (mut left, mut kept) = (budget, 0);
        for line in lines.iter().rev() {
            let size = Self::tokens(line);
            if kept >= 2 && size > left { break }
            left = left.saturating_sub(size);
            kept += 1;
        }
        lines[lines.len() - kept..].to_vec()
    }
}

impl ChatRouter {
    /// Whether a request is the chat reading a message (the instructions lead it): the one that carries everything the
    /// model is given, so the one that fills its window.
    pub fn reads(request: &Request) -> bool { request.system() == Self::system() }

    /// The instructions' size in tokens, near enough.
    pub fn instruction_tokens() -> i64 { ChatRecall::tokens(Self::system()) }
}
