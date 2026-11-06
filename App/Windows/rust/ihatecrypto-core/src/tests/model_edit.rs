use llm::Role;

use super::d;
use crate::intent::{Side, TradeIntent};
use crate::model_edit::ChatModel;

/// IhatecryptoCoreTests.modelEditAcceptsFencedJSONFromAnyProvider: what the stub model was asked (JSON wanted, no
/// address in the user's message), and its fenced answer read.
#[test]
fn model_edit_accepts_fenced_json_from_any_provider() {
    let current = TradeIntent { take_pct: Some(d("6")), ..TradeIntent::default() };
    let request = ChatModel::request(&current, "тейк ближе", "m", &[], "English");
    assert!(request.wants_json);
    assert!(!request.messages[1].text.contains("0x"));
    let reply = "```json\n{\"intent\": {\"asset\": \"ETH\", \"side\": \"long\", \"takePct\": 4}, \"reply\": \"Тейк: 6% → 4%.\"}\n```";
    let (next, answer) = ChatModel::answer(reply).unwrap();
    assert_eq!(next.and_then(|i| i.take_pct), Some(d("4")));
    assert_eq!(answer, "Тейк: 6% → 4%.");
}

/// What the model is sent: the system's rules, then the recent chat, the reply language, the plan in TradeIntent's own
/// JSON and the message.
#[test]
fn the_request_carries_only_the_plan_and_the_words() {
    let plan = TradeIntent { asset: "SOL".into(), side: Side::Short, risk_usd: Some(d("0.295")), ..TradeIntent::default() };
    let request = ChatModel::request(&plan, "1", "claude-x", &["user: стоп?".into(), "ihatecrypto: 3% or 5%?".into()], "Russian");
    assert_eq!(request.model, "claude-x");
    assert!(request.messages[0].role == Role::System && request.messages[0].text == ChatModel::SYSTEM);
    assert!(ChatModel::SYSTEM.starts_with("You change a trade plan's parameters") && ChatModel::SYSTEM.ends_with("executing the trade."));
    assert_eq!(request.messages[1].role, Role::User);
    assert_eq!(
        request.messages[1].text,
        "Recent chat (oldest first):\nuser: стоп?\nihatecrypto: 3% or 5%?\nReply language: Russian\nCurrent parameters (json): {\"asset\":\"SOL\",\"side\":\"short\",\"riskUSD\":0.295}\nMessage: 1"
    );
    assert!(!ChatModel::request(&plan, "1", "m", &[], "English").messages[1].text.contains("Recent chat"));
}

/// No plan change («intent»: null, or left out), no reply, no JSON at all, or a plan the decoder refuses.
#[test]
fn the_answer_says_what_the_model_left_out() {
    assert_eq!(ChatModel::answer(r#"{"intent": null, "reply": "Это не изменение плана."}"#).unwrap(), (None, "Это не изменение плана.".into()));
    assert_eq!(ChatModel::answer(r#"Sure: {"reply": "Which take?"}"#).unwrap(), (None, "Which take?".into()));
    assert_eq!(ChatModel::answer(r#"{"intent": {"asset": "ETH", "side": "short"}}"#).unwrap().1, "");
    let none = ChatModel::answer(&"x".repeat(200)).unwrap_err();
    assert_eq!(none.message(), format!("The model did not reply with JSON: {}", "x".repeat(120)));
    // Swift's synthesized Decodable needs `asset` and `side`; a number as text isn't a Decimal.
    assert!(ChatModel::answer(r#"{"intent": {"side": "long"}, "reply": "ok"}"#).is_err());
    assert!(ChatModel::answer(r#"{"intent": {"asset": "ETH", "side": "long", "riskUSD": "10"}}"#).is_err());
}
