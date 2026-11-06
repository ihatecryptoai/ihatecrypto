//! ModelExamTests, and the exam run against a scripted model.

use std::collections::VecDeque;
use std::sync::Mutex;

use llm::Request;

use crate::model_exam::{ModelExam, Score};
use crate::normalize::{Boxed, Model, Route};

/// The judge takes a right reading of every question and refuses a wrong one.
#[test]
fn the_model_exam_judges_right_and_wrong_readings() {
    let m = ModelExam::markets();
    for q in ModelExam::questions() {
        assert!(q.passes(&q.ideal, m), "{} → {:?}", q.message, q.ideal);
        assert!(!q.passes(&Route::Command("sell all SOL".into()), m) || !q.message.contains("сколько"), "{}", q.message);
    }
    // A move read for a question, a chat for a move: both wrong.
    let money = ModelExam::questions().iter().find(|q| q.message.starts_with("сколько")).unwrap();
    assert!(!money.passes(&Route::Command("send 5 USDC to Mom".into()), m));
    let buy = &ModelExam::questions()[0];
    assert!(!buy.passes(&Route::Chat("Sure!".into()), m) && !buy.passes(&Route::Command("buy SOL for 50 USDC".into()), m));
    let score = |passed| Score { passed, total: 10, seconds: 2.0, misses: vec![] };
    assert!(score(8).good() && !score(7).good());
    assert_eq!(Score { passed: 9, total: 10, seconds: 2.25, misses: vec![] }.short(), "9/10 · 2.2 s");
    assert_eq!(serde_json::to_value(score(8)).unwrap(), serde_json::json!({"passed": 8, "total": 10, "seconds": 2.0, "misses": []}));
}

/// Answers each question with its ideal reading, but the third with a chat and the last with nothing readable.
struct Student(Mutex<VecDeque<String>>);

impl Model for Student {
    fn complete<'a>(&'a self, _: &'a Request) -> Boxed<'a, llm::Result<String>> {
        let reply = self.0.lock().unwrap().pop_front().unwrap_or_default();
        Box::pin(async move { Ok(reply) })
    }
}

#[tokio::test]
async fn the_exam_counts_what_it_misread() {
    let mut replies = VecDeque::new();
    for (i, q) in ModelExam::questions().iter().enumerate() {
        let reply = match (&q.ideal, i) {
            (_, 2) => r#"{"route":"chat","reply":"Готово!"}"#.to_string(),
            (_, 9) => "no json".into(),
            (Route::Command(c), _) => serde_json::json!({"route": "command", "command": c}).to_string(),
            (Route::Chat(r), _) => serde_json::json!({"route": "chat", "reply": r}).to_string(),
            _ => r#"{"route":"question"}"#.into(),
        };
        // The last one twice unreadable, and more: every round of it.
        let times = if i == 9 { 4 } else { 1 };
        for _ in 0..times { replies.push_back(reply.clone()) }
    }
    let done = Mutex::new(vec![]);
    let score = ModelExam::run(&Student(Mutex::new(replies)), "m", |n| done.lock().unwrap().push(n)).await;
    assert_eq!((score.passed, score.total), (8, 10));
    assert_eq!(score.misses, ["положи 10 usdc в jupiter", "какая завтра погода в Москве?"]);
    assert!(score.good() && score.seconds >= 0.0);
    assert_eq!(*done.lock().unwrap(), (1..=10).collect::<Vec<_>>());
}
