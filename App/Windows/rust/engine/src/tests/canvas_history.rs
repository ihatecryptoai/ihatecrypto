//! HistoryTabs.swift, Notices.swift and SiteOpen.swift (MoneyTests' tabs, Chat6Tests' History, Cards15–17Tests' cards and
//! frozen canvases, History18Tests' wave 17, Blocks14Tests' runs, NoticesTests, BrowserTests): the History kept apart
//! from the canvases, what the app did by itself recorded, a request's canvas frozen as it was handled (and again as it
//! goes on), the three tabs from plain inputs, notifications by kind, and a site named in the chat opened in the browser.
//! No network, no model; public test data only.

use std::rc::Rc;

use chrono::TimeDelta;
use execution::{Entry, Kind};
use ihatecrypto_core::foundation::{self, distant_past};
use ihatecrypto_core::trade_history::Fill;
use rust_decimal::Decimal;
use serde_json::Value;

use super::{Lines, engine};
use crate::blocks::{BlockData, RateRow};
use crate::canvas::{Board, Step};
use crate::engine::{Card, Engine, Message};
use crate::history_tabs::{Activity, Bubble, Snapshot};
use crate::notices::Notice;
use crate::store;

fn en(lines: &Lines) -> Rc<Engine> {
    let e = engine(lines);
    e.set_settings(|s| s.language = Some("en".into()));
    e
}

fn said(e: &Engine, text: &str, note: Option<&str>) { e.st_mut().messages_mut().push(Message::new(true, text, note.map(str::to_string))) }

fn rates(rows: &[(&str, i64)]) -> Option<BlockData> {
    Some(BlockData { rates: Some(rows.iter().map(|(place, apy)| RateRow { place: (*place).into(), asset: "USDC".into(), apy: Decimal::from(*apy), how: None,
                                                                            note: String::new() }).collect()), ..Default::default() })
}

fn events(lines: &Lines, name: &str) -> Vec<Value> { lines.all().into_iter().filter(|l| l["event"] == name).map(|l| l["data"].clone()).collect() }

#[tokio::test]
async fn the_history_keeps_what_a_canvas_had_after_it_goes() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        said(&e, "kept in the history 1a2b", None);
        e.save_chat();
        let gone = e.st().canvas_id.clone();
        e.new_canvas(false).unwrap();
        e.remove_canvas(&gone).unwrap();
        assert!(!e.st().canvases.iter().any(|c| c.id == gone));
        assert!(e.history_activities().iter().any(|a| a.request.as_deref() == Some("kept in the history 1a2b")));
        // A scheduled run is a request of its own, «Scheduled: …», with no coin: never a «Guardian» step of a trade.
        said(&e, "Scheduled: buy ETH 3c4d", Some("schedule"));
        e.save_chat();
        let all = e.history_activities();
        let run = all.iter().find(|a| a.request.as_deref() == Some("Scheduled: buy ETH 3c4d")).unwrap();
        assert!(run.kind == "request" && run.coin.is_none());
        // «understood as» goes with the request it was read from.
        e.note_understood("buy ETH for 5 USDC");
        e.save_chat();
        assert_eq!(e.history_activities().iter().rev().find(|a| a.kind == "request").unwrap().understood.as_deref(), Some("buy ETH for 5 USDC"));
    }).await;
}

#[tokio::test]
async fn what_the_app_did_by_itself_is_a_record_and_a_notice_by_kind() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = en(&lines);
        e.notify(Notice::Alerts, "Alert 1a", "ETH went above $3,000", false, false, None, None);
        assert!(e.history_activities().iter().any(|a| a.kind == "automatic" && a.title.as_deref() == Some("Alert 1a") && a.lines == ["ETH went above $3,000"]));
        // A confirmed move has its own record already, a card waiting isn't one.
        e.notify(Notice::Moves, "Moved 2b", "Done", false, false, None, None);
        e.notify(Notice::Confirm, "Waiting 3c", "A card", false, true, None, None);
        assert!(!e.history_activities().iter().any(|a| a.title.as_deref() == Some("Moved 2b") || a.title.as_deref() == Some("Waiting 3c")));
        // A trailing stop moving again is one more line on its record.
        e.record_automatic("Trailing stop 4d", "stop moved to $100", None, false, true, None);
        e.record_automatic("Trailing stop 4d", "stop moved to $101", None, false, true, None);
        let kept: Vec<Vec<String>> = e.st().history.saved.automatic.iter().filter(|a| a.title.as_deref() == Some("Trailing stop 4d")).map(|a| a.lines.clone()).collect();
        assert_eq!(kept, [["stop moved to $100", "stop moved to $101"]]);
        // A position closed outside is a run of its item, never its story: no coin.
        e.notify(Notice::Positions, "Blocks14", "Blocks14 position closed outside ihatecrypto at 100: profit $1.00.", false, false, None, Some("perp:BLOCKS14".into()));
        let last = e.st().history.saved.automatic.last().cloned().unwrap();
        assert!(last.schedule.as_deref() == Some("perp:BLOCKS14") && last.coin.is_none());
        // What went out: its kind, sound and flags.
        let sent = events(&lines, "notify");
        assert_eq!(sent.len(), 4);
        assert!(sent[0]["kind"] == "alerts" && sent[0]["sound"] == "chime" && sent[0].get("whenAway").is_none());
        assert!(sent[1]["kind"] == "moves" && sent[1]["sound"] == "coin");
        assert!(sent[2]["kind"] == "confirm" && sent[2]["whenAway"] == "1");
    }).await;
}

#[tokio::test]
async fn each_kind_has_its_switch_and_only_those_on_go_out() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = en(&lines);
        let views = e.notice_views();
        assert_eq!(views.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
                   ["incoming", "positions", "orders", "alerts", "moves", "hlp", "signature", "confirm", "update"]);
        assert!(views.iter().all(|v| v.on) && views[0].title == "Money arrived");
        e.update_ready("0.2.0");
        let ready = events(&lines, "notify");
        assert!(ready.len() == 1 && ready[0]["kind"] == "update" && ready[0]["title"].as_str().unwrap().contains("0.2.0"));
        assert_eq!(e.st().available_update.as_deref(), Some("0.2.0"));
        // Its switch off: nothing goes out, and the state says so; kept sorted in settings.json.
        e.set_notice("update", false);
        e.set_notice("alerts", false);
        assert!(!e.notice_views().iter().find(|v| v.id == "update").unwrap().on);
        assert_eq!(store::load::<crate::store::Settings>("settings.json").unwrap().notices_off.unwrap(), ["alerts", "update"]);
        e.update_ready("0.2.1");
        assert_eq!(events(&lines, "notify").len(), 1);
        // Settings' test goes out whatever is off.
        e.notice_test();
        assert_eq!(events(&lines, "notify").last().unwrap()["kind"], "test");
        // On again; a kind the engine doesn't know changes nothing.
        e.set_notice("update", true);
        e.set_notice("alerts", true);
        e.set_notice("everything", false);
        assert!(e.notice_views().iter().all(|v| v.on) && e.st().settings.notices_off.is_none());
        e.update_ready("0.2.2");
        assert_eq!(events(&lines, "notify").len(), 3);
        // A move's end, and a card right after another notice is left alone.
        e.notice_move("Send 5 USDC", Some("failed"));
        let failed = events(&lines, "notify").last().cloned().unwrap();
        assert!(failed["failed"] == "1" && failed["sound"] == "knock" && failed["body"] == "It didn’t go through — the chat says why.");
        e.notice_card("Buy SOL");
        assert_eq!(events(&lines, "notify").len(), 4);
        e.st_mut().notice_last = distant_past();
        e.notice_card("Buy SOL");
        let card = events(&lines, "notify").last().cloned().unwrap();
        assert!(card["title"] == "Waiting for your Confirm" && card["body"] == "Buy SOL" && card["whenAway"] == "1");
    }).await;
}

#[tokio::test]
async fn open_at_login_is_kept_and_told_and_a_kept_crash_is_one_line() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = en(&lines);
        e.set_open_at_login(true);
        assert_eq!(events(&lines, "openAtLogin"), [serde_json::json!({"on": "1"})]);
        assert_eq!(e.st().settings.open_at_login, Some(true));
        e.crashed("1792800000", "the engine stopped (exit code 0x80000003)", r"C:\Users\dev\AppData\Local\app\Crashes");
        let line = e.st().messages().last().unwrap().text.clone();
        assert!(line.contains("closed unexpectedly") && line.contains("exit code 0x80000003") && line.contains("Crashes") && !line.contains("1792800000"), "{line}");
        e.crashed("yesterday", "the window stopped", "Crashes");
        assert!(e.st().messages().last().unwrap().text.contains("on yesterday — the window stopped"));
    }).await;
}

#[tokio::test]
async fn a_site_named_in_the_chat_opens_in_the_browser() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let lines = Lines::default();
        let e = en(&lines);
        assert!(e.open_site_command("open jup.ag"));
        assert_eq!(events(&lines, "openSite").last().unwrap()["url"], "jup.ag");
        assert!(e.st().messages().last().unwrap().text.starts_with("jup.ag is open in ihatecrypto's browser"));
        assert!(e.open_site_command("открой сайт app.hyperliquid.xyz"));
        assert_eq!(events(&lines, "openSite").last().unwrap()["url"], "app.hyperliquid.xyz");
        assert!(e.open_site_command("  Go to HTTPS://App.Kamino.Finance/lend  "));
        assert_eq!(events(&lines, "openSite").last().unwrap()["url"], "HTTPS://App.Kamino.Finance/lend");
        for not in ["open a long on ETH", "open http://jup.ag", "please open jup.ag", "open jup"] { assert!(!e.open_site_command(not), "{not}") }
        assert_eq!(events(&lines, "openSite").len(), 3);
    }).await;
}

#[test]
fn history_keeps_a_route_as_its_steps_and_rates_as_small_rate_cards() {
    let ten: Vec<(String, i64)> = (1..=10).map(|n| (format!("Place {n}"), n)).collect();
    let rows: Vec<(&str, i64)> = ten.iter().map(|(p, n)| (p.as_str(), *n)).collect();
    let b = Board { data: rates(&rows), ..Board::new("result", "Rates") };
    let bubbles = Engine::bubbles(&b);
    assert!(bubbles.len() == 8 && bubbles.iter().all(|b| b.kind == "yield"));
    assert_eq!((bubbles[0].label.as_str(), bubbles[0].title.as_str(), bubbles[0].lines.clone()), ("USDC", "Place 1", vec!["1%".to_string()]));
    assert_eq!(Engine::bubbles(&Board::new("chart", "BTC"))[0].label, "Chart");
    let step = |l: &str| Step { label: l.into(), title: "t".into(), lines: vec!["1".into(), "2".into(), "3".into()], money: None, status: None };
    let done = Engine::bubbles(&Board { steps: Some(vec![step("FROM"), step("BUY")]), state: Some("failed".into()), ..Board::new("move", "Buy") });
    assert!(done.len() == 2 && done.iter().all(|b| b.kind == "step" && b.status == "blocked" && b.lines.len() == 2));
}

#[test]
fn a_request_that_drew_nothing_keeps_what_its_canvas_had_standing() {
    let chart = Board::new("chart", "BTC");
    assert_eq!(Engine::standing(std::slice::from_ref(&chart)).map(|b| b.id.clone()), Some(chart.id.clone()));
    let r = Board { data: rates(&[("Kamino", 5)]), ..Board::new("result", "Rates") };
    assert_eq!(Engine::standing(&[chart, r.clone()]).map(|b| b.id.clone()), Some(r.id.clone()));
    assert!(Engine::standing(&[]).is_none());
}

#[tokio::test]
async fn a_requests_history_card_is_the_canvas_frozen_as_it_was_handled() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let shown = |e: &Engine, m: &Message| e.st().history.saved.shown.as_ref().and_then(|s| s.get(&m.id)).cloned();
        let earlier = chrono::Utc::now() - TimeDelta::seconds(60);
        // A rates board shown by an earlier message, then a follow-up: the Mac clears the rates at the next message.
        e.pin(Board { at: earlier, data: rates(&[("Kamino", 5)]), ..Board::new("result", "Rates · USDC") });
        let follow = Message::new(true, "which is safest?", None);
        e.snapshot_canvas(&follow);
        assert_eq!(shown(&e, &follow), Some(Snapshot::default()));
        // A chart standing on the canvas with no plan under way: the follow-up is that chart's.
        e.pin(Board { at: earlier, ..Board::new("chart", "SOL") });
        let next = Message::new(true, "and now?", None);
        e.snapshot_canvas(&next);
        assert_eq!(shown(&e, &next).unwrap().title.as_deref(), Some("Chart · SOL"));
        // Rates this very message showed win over the chart.
        let ask = Message { at: chrono::Utc::now() - TimeDelta::seconds(1), ..Message::new(true, "best rates for USDC", None) };
        e.pin(Board { data: rates(&[("Jupiter Lend", 6)]), ..Board::new("result", "Rates · USDC") });
        e.snapshot_canvas(&ask);
        let snap = shown(&e, &ask).unwrap();
        assert!(snap.title.as_deref() == Some("Rates") && snap.bubbles[0].kind == "yield");
        // A card waiting for Confirm: its title and route, frozen.
        let step = |l: &str| Step { label: l.into(), title: "5 USDC".into(), lines: vec![], money: None, status: None };
        let id = e.pin(Board { steps: Some(vec![step("FROM · Solana"), step("BUY · Base")]), state: Some("waiting".into()), ..Board::new("move", "Buy AERO for 5 USDC") });
        e.st_mut().card = Some(Card { id, title: "Buy AERO for 5 USDC".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![] });
        let waiting = Message::new(true, "is that safe?", None);
        e.snapshot_canvas(&waiting);
        let snap = shown(&e, &waiting).unwrap();
        assert!(snap.title.as_deref() == Some("Buy AERO for 5 USDC") && snap.bubbles.len() == 2);
        // Kept across a restart, in history.json.
        let saved: crate::history_tabs::Saved = store::load("history.json").unwrap();
        assert!(saved.shown.unwrap().contains_key(&waiting.id));
    }).await;
}

#[test]
fn history_tabs_follow_the_mac() {
    let t0 = foundation::unix(1_790_000_000.0);
    let d = |s: &str| foundation::decimal(s).unwrap();
    let fills = vec![
        Fill { coin: "ETH".into(), px: d("3000"), sz: d("0.01"), side: "B".into(), time: t0, start_position: d("0"), dir: "Open Long".into(), closed_pnl: d("0"),
               fee: d("0.02"), hash: None },
        Fill { coin: "ETH".into(), px: d("3100"), sz: d("0.01"), side: "A".into(), time: t0 + TimeDelta::seconds(3600), start_position: d("0.01"), dir: "Close Long".into(),
               closed_pnl: d("1"), fee: d("0.02"), hash: Some("0xabc".into()) },
    ];
    let asked = Activity { request: Some("long eth by the book".into()), understood: Some("long ETH by the book".into()), coin: Some("ETH".into()),
                           lines: vec!["Opened.".into()], ..Activity::new("request", t0 - TimeDelta::seconds(60)) };
    let entry = |ago: i64, chain: &str, id: &str, kind: Kind, note: &str| Entry { at: t0 - TimeDelta::seconds(ago), chain: chain.into(), id: id.into(), kind, note: note.into() };
    let journal = vec![entry(90_000, "solana", "sig1", Kind::Signed, "вход"), entry(89_900, "solana", "sig2", Kind::Signed, "swap"),
                       entry(80_000, "hyperliquid", "n1", Kind::Signed, "close")];
    let (agent, earlier, stories, rows) = Engine::history_tabs_from(&[asked], &journal, &fills, &[], false);
    assert!(agent.len() == 1 && agent[0].understood.as_deref() == Some("long ETH by the book"));
    // Two sessions: a pause of over 10 minutes starts a new one; the old Russian note reads in English.
    assert!(earlier.len() == 2 && earlier[1].bubbles[0].title == "entry" && earlier[1].lines == ["2 signatures · Solana"]);
    assert_eq!(stories.len(), 1);
    let story = &stories[0];
    assert!(!story.open && story.long && story.realized == d("0.96") && story.fees == d("0.04"));
    // The request about ETH a minute before it opened is among the position's events, in time order.
    assert_eq!(story.events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["activity", "open", "close"]);
    assert_eq!(story.events[0].label, "You asked");
    assert!(rows[0].text.starts_with("Close Long ETH") && rows[0].link.as_deref() == Some("https://app.hyperliquid.xyz/explorer/tx/0xabc"));
    assert_eq!(rows.len(), 1 + 2 + 3);
    assert_eq!(rows.last().unwrap().text, "Signed · Solana: entry");
    assert_eq!(rows.last().unwrap().link.as_deref(), Some("https://solscan.io/tx/sig1"));
    // In Russian, as the page asks.
    let (_, earlier, _, rows) = Engine::history_tabs_from(&[], &journal, &fills, &[], true);
    assert!(earlier[1].lines == ["Подписей: 2 · Solana"] && rows[0].detail.as_deref() == Some("комиссия $0.02"));
}

#[test]
fn signatures_group_into_sessions_with_what_came_of_them() {
    let t0 = foundation::unix(1_790_000_000.0);
    let entry = |ago: i64, id: &str, kind: Kind, note: &str| Entry { at: t0 - TimeDelta::seconds(ago), chain: "base".into(), id: id.into(), kind, note: note.into() };
    let journal = vec![entry(100, "a", Kind::Signed, "supply 5 USDC · valid until block 9"), entry(90, "a", Kind::Failed, "reverted"),
                       entry(80, "b", Kind::Signed, "swap"), entry(70, "b", Kind::Confirmed, "ok"), entry(60, "c", Kind::Signed, "wrap"),
                       entry(50, "d", Kind::Signed, "approve"), entry(40, "e", Kind::Signed, "after the start")];
    let s = Engine::sessions(&journal, Some(t0 - TimeDelta::seconds(45)), false);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].title.as_deref(), Some("supply 5 USDC, swap, wrap…"));
    assert!(s[0].bubbles[0].status == "blocked" && s[0].bubbles[0].lines == ["failed: reverted"] && s[0].bubbles[1].lines.is_empty());
    assert_eq!(s[0].lines, ["4 signatures · Base"]);
    assert_eq!(Engine::english("исполнено 0.5 по 3000"), "filled 0.5 at 3000");
    assert_eq!(Engine::english("закрыть пустой счёт; действует до блока 5"), "close the empty account · valid until block 5");
    assert_eq!(Engine::english("плечо"), "leverage");
}

// History18Tests (wave 17, audit16 D15-1, D15-2, D16-2, D16-3, D16-5, A16-C2-1): a request's History card is the canvas
// frozen in the Mac's order — a plan waiting for Confirm isn't a waiting card —, frozen for a picked card too, frozen
// again as the request goes on after a Confirm with its first title kept, «Question» for a question, a chart as
// «Chart · 1D», the coin and its last price; the notices remember nothing before the wallet's own positions are read.

/// Hardhat's account #3.
const MOM: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";

fn shown(e: &Engine, m: &Message) -> Option<Snapshot> { e.st().history.saved.shown.as_ref().and_then(|s| s.get(&m.id)).cloned() }

/// A request said `ago` seconds back.
fn asked(e: &Engine, text: &str, ago: f64) -> Message {
    let m = Message { at: chrono::Utc::now() - TimeDelta::milliseconds((ago * 1000.0) as i64), ..Message::new(true, text, None) };
    e.st_mut().messages_mut().push(m.clone());
    m
}

fn move_step(label: &str, title: &str) -> Step { Step { label: label.into(), title: title.into(), lines: vec![], money: None, status: None } }

#[tokio::test]
async fn a_plan_waiting_for_confirm_falls_to_the_rates_and_the_ways_asked_over_it() {
    use ihatecrypto_core::intent::{Side, TradeIntent};
    use ihatecrypto_core::plan::{Market, RiskCalc};
    use trade::executor::Prepared;

    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let market = Market::new("ETH", Decimal::from(2000), 4, 25);
        // TradeIntent::default() is a long ETH with nothing else set.
        let intent = TradeIntent { asset: "ETH".into(), side: Side::Long, risk_usd: Some(Decimal::ONE), ..TradeIntent::default() };
        let sizing = RiskCalc::size(Decimal::ONE, Decimal::TWO, Some(Decimal::from(4)), Decimal::from(3), Side::Long, &market, None, None, Decimal::ZERO).unwrap();
        let p = Prepared::new(intent.clone(), market, sizing, 3, None, vec![], vec!["Long 0.01 ETH".into()], chrono::Utc::now());
        let plan = asked(&e, "long ETH", 30.0);
        let view = crate::trade_plan::PlanView { market: "ETH".into(), group: "crypto".into(), stopped: None, short: None, blocked: false, fix: None,
                                                 fix_detail: None, over_limit: false, takes: None, footer: String::new(), preview: false };
        let board = Board { steps: Some(vec![move_step("LONG · HYPERLIQUID", "0.01 ETH")]), state: Some("waiting".into()), plan: Some(view),
                            ..Board::new("move", "Plan · long ETH") };
        let id = e.pin(board.clone());
        {
            let mut st = e.st_mut();
            st.intent = intent;
            st.positions = vec![];
            st.pending = Some(crate::engine::Pending::Trade(Box::new(p)));
            st.card = Some(Card { id: id.clone(), title: "Plan · long ETH".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![] });
            st.plan_board = Some(id.clone());
        }
        // The plan's own request: its title and route, as before.
        e.snapshot_canvas(&plan);
        let pinned = e.st().boards().iter().find(|b| b.id == id).cloned().unwrap();
        assert_eq!(shown(&e, &plan), Some(Snapshot { title: Some("Plan · long ETH".into()), bubbles: Engine::bubbles(&pinned) }));
        assert_eq!(Engine::bubbles(&pinned), Engine::bubbles(&board));
        // The ways to take profit asked while it waits: «Take-profit choices», not the plan again.
        let takes = asked(&e, "take profit like the pros", 1.0);
        let ways = e.take_choices(Some(Decimal::ONE));
        e.update_board(Some(&id), |b| if let Some(p) = b.plan.as_mut() { p.takes = Some(ways) });
        e.st_mut().takes_at = Some(chrono::Utc::now());
        e.snapshot_canvas(&takes);
        let snap = shown(&e, &takes).unwrap();
        assert!(snap.title.as_deref() == Some("Take-profit choices") && snap.bubbles.iter().map(|b| b.kind.as_str()).collect::<Vec<_>>() == ["choice"; 3], "{snap:?}");
        // The rates asked while it waits: «Rates».
        let rates_asked = asked(&e, "best rates for USDC", 0.5);
        e.pin(Board { data: rates(&[("Kamino", 5)]), ..Board::new("result", "Rates · USDC") });
        e.snapshot_canvas(&rates_asked);
        let snap = shown(&e, &rates_asked).unwrap();
        assert!(snap.title.as_deref() == Some("Rates") && snap.bubbles[0].kind == "yield", "{snap:?}");
    }).await;
}

#[tokio::test]
async fn a_picked_card_is_frozen_as_a_typed_message_is() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let save = format!("save {MOM} as Mom on base");
        e.offer(vec![crate::from_core::RouterOption::new("Save Mom", "", &save, false)], &format!("keep {MOM} as Mom"), vec![]);
        e.pick(Some(0)).await;
        let picked = e.st().messages().iter().rfind(|m| m.from_user).cloned().unwrap();
        assert_eq!(picked.text, "Save Mom");
        let snap = shown(&e, &picked);
        let card = e.st().card.as_ref().map(|c| c.title.clone());
        assert!(card.is_some() && snap.as_ref().is_some_and(|s| s.title == card && !s.bubbles.is_empty()), "{snap:?} · {card:?}");
    }).await;
}

#[tokio::test]
async fn a_request_frozen_again_keeps_its_first_title_and_takes_the_new_cards() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let id = foundation::Uuid::new().to_string();
        let bubble = |label: &str, title: &str| Bubble { kind: "step".into(), label: label.into(), title: title.into(), lines: vec![], status: "info".into() };
        let (a, b) = (bubble("BRIDGE · RELAY", "5 USDC"), bubble("BUY · BASE", "AERO"));
        let freeze = |title: Option<&str>, bubbles: Vec<Bubble>| e.st_mut().history.freeze(Snapshot { title: title.map(str::to_string), bubbles }, &id);
        let kept = |e: &Engine| e.st().history.saved.shown.as_ref().and_then(|s| s.get(&id)).cloned();
        freeze(Some("Bridge 5 USDC"), vec![a]);
        freeze(Some("Buy AERO"), vec![b.clone()]);
        let both = Snapshot { title: Some("Bridge 5 USDC".into()), bubbles: vec![b] };
        assert_eq!(kept(&e), Some(both.clone()));
        // Nothing to freeze, or a title only: the cards stay.
        freeze(None, vec![]);
        freeze(Some("Question"), vec![]);
        assert_eq!(kept(&e), Some(both));

        // The canvas after a Confirm: the move carried out since the request comes before a chart standing there.
        let since = chrono::Utc::now() - TimeDelta::seconds(10);
        let chart = Board { at: since - TimeDelta::seconds(60), ..Board::new("chart", "SOL") };
        let old = Board { steps: Some(vec![move_step("SEND", "1 USDC")]), state: Some("done".into()), at: since - TimeDelta::seconds(5), ..Board::new("move", "Earlier send") };
        let mut moved = Board { steps: Some(vec![move_step("BRIDGE · RELAY", "5 USDC")]), state: Some("running".into()), at: since, ..Board::new("move", "Bridge 5 USDC") };
        let snap = |boards: &[Board], running: Option<&str>| Engine::canvas_snapshot(boards, since, None, running, false, None, None, false);
        assert_eq!(snap(&[old.clone(), chart.clone()], None).title.as_deref(), Some("Chart · SOL"), "a move from before the request isn't its own");
        assert_eq!(snap(&[moved.clone(), old.clone(), chart.clone()], Some(&moved.id)).title.as_deref(), Some("Bridge 5 USDC"));
        moved.state = Some("done".into());
        assert_eq!(snap(&[moved.clone(), old.clone(), chart.clone()], None).bubbles.iter().map(|b| b.status.as_str()).collect::<Vec<_>>(), ["done"]);
        assert_eq!(snap(&[moved, old, chart], None).title.as_deref(), Some("Bridge 5 USDC"));
        // Through the engine: the request is frozen again from the canvas as it is now.
        let request = asked(&e, "bridge 5 USDC to base, then buy AERO", 20.0);
        e.snapshot_canvas(&request);
        let first = shown(&e, &request).and_then(|s| s.title);
        e.pin(Board { steps: Some(vec![move_step("BRIDGE · RELAY", "5 USDC")]), state: Some("done".into()), ..Board::new("move", "Bridge 5 USDC") });
        e.refreeze_request();
        let now = shown(&e, &request).unwrap();
        assert!(now.title == Some(first.unwrap_or_else(|| "Bridge 5 USDC".into())) && now.bubbles.iter().map(|b| b.label.as_str()).collect::<Vec<_>>() == ["BRIDGE · RELAY"],
                "{now:?}");
    }).await;
}

#[tokio::test]
async fn a_questions_card_is_titled_question() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        for language in ["en", "ru"] {
            e.set_settings(|s| s.language = Some(language.into()));
            let q = asked(&e, "what's open?", 1.0);
            e.answer_without_model(&q.text).await;
            e.snapshot_canvas(&q);
            let note = e.st().messages().last().and_then(|m| m.note.clone());
            assert_eq!(shown(&e, &q).and_then(|s| s.title).as_deref(), Some("Question"), "{language}: {note:?}");
        }
        // With the canvas's cards, as the Mac's answer() keeps them.
        let snap = Engine::canvas_snapshot(&[Board::new("chart", "BTC · 1D")], distant_past(), None, None, false, None, None, true);
        assert!(snap.title.as_deref() == Some("Question") && snap.bubbles.iter().map(|b| b.kind.as_str()).collect::<Vec<_>>() == ["chart"]);
        assert!(!Engine::answers_question(Some("Hyperliquid")) && !Engine::answers_question(None));
        assert!(Engine::answers_question(Some("claude-opus-5-5 · from your portfolio and history")));
    }).await;
}

#[test]
fn a_charts_bubble_reads_its_range_the_coin_and_its_last_price() {
    let line = crate::engine::ChartLine { title: "BTC · 1D".into(), closes: vec![64000.0, 65000.5], times: None, low: String::new(), high: String::new(),
                                          change: String::new(), coin: Some("BTC".into()), network: None, extras: None };
    let bubble = Engine::bubbles(&Board { chart: Some(line), ..Board::new("chart", "BTC · 1D") });
    let price = ihatecrypto_core::intent::Fmt::price(foundation::decimal("65000.5").unwrap());
    assert_eq!(bubble.iter().map(|b| [vec![b.label.clone(), b.title.clone()], b.lines.clone()].concat()).collect::<Vec<_>>(), [vec!["Chart · 1D".to_string(), "BTC".into(), price]]);
}

#[tokio::test]
async fn the_notices_remember_nothing_before_this_wallets_positions_are_read() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        e.st_mut().status = Some(signer_api::SignerReply { ok: true, has_wallet: Some(true), evm_address: Some(MOM.into()), ..Default::default() });
        // Read for no one yet (just switched), or still for the last wallet: nothing is seeded (and nothing is asked).
        for read_for in [None, Some("0x70997970c51812dc3a010c7d01b50e0d17dc79c8")] {
            e.st_mut().positions_for = read_for.map(str::to_string);
            e.notice_hyperliquid().await;
            assert!(e.st().notice_positions.is_none() && e.st().notice_entries.is_empty());
        }
    }).await;
}
