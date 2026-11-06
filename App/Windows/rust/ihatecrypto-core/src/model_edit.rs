//! `ModelEdit.swift`: free-form plan edits («move the take closer») through whichever model the user connected.

use llm::{Client, Error, Message, Request, Result, Role};
use serde::Deserialize;

use crate::foundation::chars;
use crate::intent::TradeIntent;

pub struct ChatModel;

impl ChatModel {
    pub const SYSTEM: &str = concat!(
        "You change a trade plan's parameters at the user's request. Return only a JSON object like\n",
        r#"{"intent": {"asset": "ETH", "side": "long", "riskUSD": 10, "stopPct": 3, "takePct": 6, "leverage": 2, "amountUSDC": null, "takeR": null, "autoRules": null, "entryPx": null, "pullbackPct": null}, "reply": "Take-profit: 6% → 4%."}"#, "\n",
        r#"asset: a Hyperliquid ticker, either a crypto ("ETH") or a market on the HIP-3 exchange xyz ("xyz:CL" is WTI oil, "xyz:BRENTOIL", "xyz:GOLD", "xyz:TSLA"); side: long or short; riskUSD: the maximum loss in dollars; stopPct and takePct: percent from the entry price;"#, "\n",
        r#"leverage: whole-number leverage; amountUSDC: the money put into the trade, its margin («на 5 баксов», «увеличь до 5 USDC»): with riskUSD null the size follows from it and the stop; or null; takeR: take-profit as a multiple of the stop ("take 2R", "1:3"), then takePct is null;"#, "\n",
        r#"autoRules: true if the user asks for it by the book, by the rules or the best way (in Russian e.g. "как учат", "по правилам"): then do not invent numbers, leave missing stopPct, takePct and riskUSD null; code computes them from volatility, 2R and 1% of capital."#, "\n",
        r#"entryPx: limit entry price, or null for a market entry; pullbackPct: a limit entry on a pullback of this many % from the current price ("wait for a 2% pullback"), then entryPx is null."#, "\n",
        "Return every intent field; change only what is asked.\n",
        "The user may write in any language, Russian included. Understand the request in whatever language it is written and still return the same JSON.\n",
        "reply: one short sentence about what changed, in the language named on the «Reply language» line.\n",
        r#"Handle relative requests ("take closer", "wider stop", "less risk") yourself: change the value by about a third and name the new value in reply."#, "\n",
        r#"If the request is not about these parameters — deposits, vaults, lending, transfers, balances — return "intent": null and say in reply that it isn't a trade-plan change; never put a deposit amount into amountUSDC. If it is unclear what to change, return "intent": null and ask one clarifying question in reply, in that language."#, "\n",
        "Do not invent addresses; do not discuss signing or executing the trade.",
    );

    /// What `edit` asks: only the plan parameters and the user's words, never addresses, balances or keys. `history`: the
    /// last lines of the chat, so a follow-up like «1» or «yes» reads in context.
    pub fn request(intent: &TradeIntent, message: &str, model: &str, history: &[String], reply_language: &str) -> Request {
        let current = serde_json::to_string(intent).unwrap_or_default();
        let recent = if history.is_empty() { String::new() } else { format!("Recent chat (oldest first):\n{}\n", history.join("\n")) };
        let mut request = Request::new(model, vec![
            Message::new(Role::System, Self::SYSTEM),
            Message::new(Role::User, format!("{recent}Reply language: {reply_language}\nCurrent parameters (json): {current}\nMessage: {message}")),
        ]);
        request.wants_json = true;
        request
    }

    /// The model's answer: the new plan (nil: not a plan change, or a question back) and its reply.
    pub fn answer(text: &str) -> Result<(Option<TradeIntent>, String)> {
        #[derive(Deserialize)]
        struct Answer {
            #[serde(default)]
            intent: Option<TradeIntent>,
            #[serde(default)]
            reply: Option<String>,
        }
        let Some(json) = llm::json_object(text) else {
            return Err(Error::Llm(format!("The model did not reply with JSON: {}", chars(text).iter().take(120).copied().collect::<String>())));
        };
        // ponytail: a shape the decoder refuses is said in serde's words, not Swift's DecodingError's.
        let answer: Answer = serde_json::from_str(json).map_err(|e| Error::Llm(e.to_string()))?;
        Ok((answer.intent, answer.reply.unwrap_or_default()))
    }

    pub async fn edit(
        intent: &TradeIntent, message: &str, client: &Client, model: &str, history: &[String], reply_language: &str,
    ) -> Result<(Option<TradeIntent>, String)> {
        Self::answer(&client.complete(&Self::request(intent, message, model, history, reply_language)).await?)
    }
}
