//! Rest9Tests' recurring buy in the History block, and the moves a canvas's replies say were made, offline.

use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use signer_api::{SignerReply, SignerWallet};

use super::*;
use crate::engine::Message;
use crate::outbox::Outbox;
use crate::schedule::ScheduleRun;
use crate::signer_link::SignerLink;
use crate::store;

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::writer(std::io::sink()), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("locked: no state is drawn"));
    e.st_mut().lock.locked = true;
    e
}

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(tokio::task::LocalSet::new().run_until(test));
}

fn run_of(at: Date, result: &str, line: &str) -> ScheduleRun {
    ScheduleRun { at, title: "buy SOL for 5 USDC".into(), result: result.into(), line: Some(line.into()), canvas: None, task: None, activity: None }
}

/// A recurring buy runs with no canvas and writes no chat line: its run's swap is still a move in «History · latest»
/// and in the day's line (the Mac records it as an activity), with no canvas to open.
#[test]
fn a_recurring_buys_swap_is_a_move_in_the_history_block() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        let at = chrono::Utc::now() - chrono::Duration::hours(1);
        e.st_mut().schedule_runs = vec![run_of(at, "done", "Swapped 5 USDC → SOL +0.0312."), run_of(at, "failed", "Swapped nothing: refused.")];
        let moves = e.history_moves();
        let row = &moves[0];
        assert!(row.kind == "swapped" && row.canvas.is_none() && row.request.as_deref() == Some("buy SOL for 5 USDC"), "{row:?}");
        assert_eq!(moves.iter().filter(|m| m.at == at).count(), 1, "a failed run isn't a move");
        assert_eq!(e.moves_day().unwrap().day, start_of_day(at));
    });
}

#[test]
fn a_reply_that_says_money_moved_is_a_move_with_the_request_it_answered() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        assert!(e.history_moves().is_empty() && e.moves_day().is_none());
        let automation = SignerWallet { id: "2".into(), label: "Automation".into(), kind: "phrase".into(), solana: None, evm: None, near: None, test: None, automation: Some(true) };
        e.st_mut().status = Some(SignerReply { ok: true, wallets: Some(vec![automation]), ..Default::default() });
        for (user, text) in [(true, "send 25 USDT to Automation"), (false, "Sent 25 USDT to Automation on Solana. https://solscan.io/tx/abc\nNothing else to do."),
                             (true, "swap 3 USDC to USDT"), (false, "Swapped 3 USDC → USDT +2.997906.")] {
            e.st_mut().messages_mut().push(Message::new(user, text, None));
        }
        // Saved as a reply saves: the History keeps its own copy.
        e.save_chat();
        let canvas = e.st().canvas_id.clone();
        let moves = e.history_moves();
        assert_eq!(moves.iter().map(|m| (m.kind.as_str(), m.request.as_deref(), m.own)).collect::<Vec<_>>(),
                   [("swapped", Some("swap 3 USDC to USDT"), false), ("sent", Some("send 25 USDT to Automation"), true)]);
        assert_eq!((moves[1].to.as_deref(), moves[1].link.as_deref(), moves[1].canvas.as_deref()), (Some("Automation"), Some("https://solscan.io/tx/abc"), Some(canvas.as_str())));
        let day = e.moves_day().unwrap();
        assert_eq!(day.own, ["Automation"]);
        assert_eq!(day.lines, MovedLine::summary(&[MovedLine::new("Swapped 3 USDC → USDT +2.997906.").unwrap(),
                                                    MovedLine::new("Sent 25 USDT to Automation on Solana.").unwrap()]));
    });
}
