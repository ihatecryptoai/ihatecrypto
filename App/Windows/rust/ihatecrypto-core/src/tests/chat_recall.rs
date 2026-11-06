//! ChatRecallTests (its third test, which needs ChatRouter's instructions, is normalize's).

use crate::chat_recall::ChatRecall;

fn lines(n: usize) -> Vec<String> { (1..=n).map(|i| format!("user: buy SOL for {i} USDC")).collect() }

fn last(lines: &[String], n: usize) -> Vec<String> { lines[lines.len() - n..].to_vec() }

#[test]
fn a_short_chat_goes_whole_and_a_long_one_keeps_its_newest_lines() {
    let lines = lines(40);
    assert_eq!(ChatRecall::fitting(&lines, 4_000), lines);
    // 7 tokens a line: 70 hold the last ten, oldest first.
    assert_eq!(ChatRecall::fitting(&lines, 70), last(&lines, 10));
    // The question a follow-up answers always goes, whatever its size.
    let long = vec![format!("user: {}", "я".repeat(3_000)), "ihatecrypto: How much?".to_string()];
    assert_eq!(ChatRecall::fitting(&[lines.clone(), long.clone()].concat(), 10), long);
    assert!(ChatRecall::fitting(&[], 10).is_empty());
}

#[test]
fn a_wide_window_gets_the_whole_chat_and_a_small_one_never_more_than_before() {
    let lines = lines(40);
    let most = ChatRecall::MOST;
    // A cloud model's window, or one its service didn't tell: the whole chat, up to the cap (7 tokens a line).
    assert_eq!(ChatRecall::sent(&lines, Some(1_000_000), false, 10_000, most), lines);
    assert_eq!(ChatRecall::sent(&lines, None, false, 10_000, most), lines);
    assert_eq!(ChatRecall::sent(&lines, Some(1_000_000), false, 10_000, 70), last(&lines, 10));
    // Ollama's 16,384, or a local server that doesn't say (LM Studio): the last 12 lines, as before the whole chat was sent.
    assert_eq!(ChatRecall::sent(&lines, Some(16_384), true, 10_000, most), last(&lines, 12));
    assert_eq!(ChatRecall::sent(&lines, None, true, 10_000, most), last(&lines, 12));
    assert_eq!(ChatRecall::sent(&lines, Some(32_000), false, 10_000, most), last(&lines, 12));
    // …and fewer when they're long: 3,384 tokens are left beside 10k of instructions, the brief and the answer.
    let pasted: Vec<String> = (1..=12).map(|i| format!("user: {i} {}", "x".repeat(2_990))).collect();
    assert_eq!(ChatRecall::sent(&pasted, Some(16_384), true, 10_000, most), last(&pasted, 4));
    // Instructions that nearly fill the window still leave the question a follow-up answers.
    assert_eq!(ChatRecall::sent(&pasted, Some(8_192), true, 10_000, most), last(&pasted, 2));
    // Cyrillic costs about twice a Latin letter.
    assert!(ChatRecall::tokens("купи") == 2 && ChatRecall::tokens("buy!") == 1);
    // A window past what Swift's Int holds, or under the instructions: no overflow, the floor of 400.
    assert_eq!(ChatRecall::sent(&lines, Some(i64::MIN), false, i64::MAX, most), last(&lines, 12));
}
