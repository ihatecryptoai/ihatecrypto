//! TradePlanTests (after a trade, «Plan updated» in Russian), Moves6Tests' second check on the route, and the plan's
//! picker, take-profit ways, orders and ruler, offline: in a test folder of their own, no signer, no network.

use std::rc::Rc;

use ihatecrypto_core::foundation::Date;
use serde_json::json;

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::snapshot::Snapshot;
use crate::store;

fn d(s: &str) -> Decimal { s.parse().expect("a decimal") }

/// An engine whose state pushes are the lock alone (no snapshot to draw in a test), in English.
fn engine() -> Rc<Engine> {
    fn draw(_: &Engine) -> Snapshot { unreachable!("the test window is locked") }
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), draw);
    e.st_mut().lock.locked = true;
    e.st_mut().settings.language = Some("en".into());
    e
}

fn plan(change: impl FnOnce(&mut TradeIntent)) -> TradeIntent {
    let mut i = TradeIntent { risk_usd: Some(d("5")), stop_pct: Some(d("3")), take_pct: Some(d("6")), ..TradeIntent::default() };
    change(&mut i);
    i
}

// MARK: After a trade

/// «stop 2%» on a long from 100: the stop goes to 98 and nothing else moves; «take 10%» moves only the take.
#[test]
fn only_what_the_words_changed_moves() {
    let before = plan(|_| {});
    let stop = plan(|i| i.stop_pct = Some(d("2")));
    assert_eq!(Engine::order_edit(&before, &stop, &stop, d("1"), d("100"), Some(d("1000")), d("2")), (vec![PositionCommand::StopTo(d("98"))], None));
    let take = plan(|i| i.take_pct = Some(d("10")));
    assert_eq!(Engine::order_edit(&before, &take, &take, d("1"), d("100"), Some(d("1000")), d("2")).0, vec![PositionCommand::TakeTo(d("110"))]);
    // A short's stop is above its entry, its take below.
    let both = plan(|i| { i.stop_pct = Some(d("2")); i.take_pct = Some(d("4")) });
    assert_eq!(Engine::order_edit(&before, &both, &both, d("-1"), d("100"), Some(d("1000")), d("2")).0,
               vec![PositionCommand::StopTo(d("102")), PositionCommand::TakeTo(d("96"))]);
    assert!(Engine::order_edit(&before, &before, &before, d("1"), d("100"), Some(d("1000")), d("2")).0.is_empty());
}

/// A wider stop that would lose more than the per-trade limit moves nothing and says what it would lose.
#[test]
fn a_stop_past_the_limit_is_refused() {
    let (before, wide) = (plan(|_| {}), plan(|i| i.stop_pct = Some(d("10"))));
    // 1 coin from 100 to 90: $10 plus fees, over 2% of $200.
    let (moves, too_wide) = Engine::order_edit(&before, &wide, &wide, d("1"), d("100"), Some(d("200")), d("2"));
    assert!(moves.is_empty());
    let loss = too_wide.expect("what it would lose");
    assert!(loss > d("10") && loss < d("10.2"), "{loss}");
    // With capital unknown there's nothing to measure against: it moves.
    assert_eq!(Engine::order_edit(&before, &wide, &wide, d("1"), d("100"), None, d("2")).0, vec![PositionCommand::StopTo(d("90"))]);
    // A stop past the arithmetic's range isn't sent on; the take still moves.
    let huge = plan(|i| { i.stop_pct = Some(Decimal::MAX); i.take_pct = Some(d("4")) });
    assert_eq!(Engine::order_edit(&before, &huge, &huge, d("1"), Decimal::MAX, None, d("2")).0, vec![]);
}

/// «Plan updated: …» in Russian: the core's English fragments said from meaning, a number kept as it is.
#[test]
fn the_plans_changes_in_russian() {
    let next = plan(|i| { i.side = Side::Short; i.pullback_pct = Some(d("2")); i.auto_rules = Some(true) });
    let ru: Vec<String> = next.changes(&TradeIntent::default()).iter().map(|c| Engine::change_ru(c)).collect();
    assert_eq!(ru, ["шорт", "риск $5.00", "стоп 3%", "тейк 6%", "остальное по правилам риска", "вход на откате 2%"]);
    assert_eq!(Engine::change_ru("limit entry at 2,650"), "лимитный вход на 2,650");
    assert_eq!(Engine::change_ru("something new"), "something new");
}

#[test]
fn what_the_plan_lacks_is_said_by_example() {
    assert_eq!(Engine::missing_example(&["risk in $".into()]), ("risk $1", "риск $1"));
    assert_eq!(Engine::missing_example(&["stop in %".into()]), ("stop 3%", "стоп 3%"));
    assert_eq!(Engine::missing_example(&["risk in $".into(), "stop in %".into()]), ("risk $1 stop 3%", "риск $1 стоп 3%"));
}

// MARK: The plan on the canvas

/// The route says when the second check couldn't run, rather than «All correct» (Moves6Tests).
#[tokio::test]
async fn the_second_check_says_when_it_could_not_run() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let market = Market::new("ETH", d("2000"), 4, 25);
        let intent = TradeIntent { asset: "ETH".into(), side: Side::Long, ..TradeIntent::default() };
        let sizing = RiskCalc::size(d("1"), d("2"), Some(d("4")), d("3"), Side::Long, &market, None, None, Decimal::ZERO).expect("a sizing");
        let at: Date = Utc::now() - chrono::TimeDelta::seconds(60);
        let p = Prepared::new(intent.clone(), market.clone(), sizing, 3, None, vec![], vec!["Long 0.01 ETH".into()], at);
        let ev = Plan::evaluate(&intent, Some(&market));
        let blocks = Plan::blocks(&intent, &ev, &e.plan_live(Some(&market), vec![], None));
        let second = |route: (Vec<Step>, String)| route.0.into_iter().find(|s| s.label == "SECOND CHECK");
        let why = "The second check couldn't run: connect a model in Settings › Sign-ins. Only code checks apply.";
        let unchecked = second(e.trade_route(&intent, &ev, &blocks, Some(&market), Some(&p), None, true, Some(why), None)).expect("its card");
        assert_eq!((unchecked.title.as_str(), unchecked.lines.clone(), unchecked.status.as_deref()), ("Not checking", vec![why.to_string()], Some("info")));
        let doubt = second(e.trade_route(&intent, &ev, &blocks, Some(&market), Some(&p), Some("It risks $5, not $1."), true, None, None)).expect("its card");
        assert_eq!((doubt.title.as_str(), doubt.status.as_deref()), ("Has doubts", Some("attention")));
        assert_eq!(doubt.lines.first().map(String::as_str), Some("It risks $5, not $1."));
        let fine = second(e.trade_route(&intent, &ev, &blocks, Some(&market), Some(&p), None, true, None, None)).expect("its card");
        assert_eq!((fine.title.as_str(), fine.status.as_deref()), ("All correct", Some("passed")));
        assert!(fine.lines.last().is_some_and(|l| l == "Market: ETH on Hyperliquid."));
        // Not reviewed and nothing prepared: no second check on the route at all.
        assert!(second(e.trade_route(&intent, &ev, &blocks, Some(&market), None, None, true, None, None)).is_none());
    }).await;
}

/// The picker's markets by group, sorted, read only; the three take-profit ways with «in parts» recommended.
#[tokio::test]
async fn the_picker_and_the_take_ways() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        {
            let mut st = e.st_mut();
            for (name, dex) in [("ETH", None), ("BTC", None), ("xyz:TSLA", Some("xyz")), ("xyz:GOLD", Some("xyz")), ("xyz:SP500", Some("xyz"))] {
                st.markets.insert(name.into(), Market { dex: dex.map(String::from), ..Market::new(name, d("1"), 2, 10) });
            }
        }
        let answer = e.plan_op(&Request { op: "planMarkets".into(), ..Request::default() }).await;
        let groups: serde_json::Value = serde_json::from_str(&answer["json"]).expect("JSON");
        assert_eq!(groups, json!({"Crypto": ["BTC", "ETH"], "Commodities": ["xyz:GOLD"], "Stocks": ["xyz:TSLA"], "Indices & FX": ["xyz:SP500"]}));

        let takes = e.take_choices(Some(d("1")));
        assert_eq!(takes.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["classic", "partial", "trend"]);
        assert_eq!(takes.iter().find(|t| t.recommended).map(|t| t.id.as_str()), Some("partial"));
        assert_eq!(takes.iter().map(|t| t.payoff.as_str()).collect::<Vec<_>>(), ["+$2.00", "+$1.50", "no cap"]);
        // A risk too big to double says no payoff rather than taking the engine down.
        assert_eq!(e.take_choices(Some(Decimal::MAX))[0].payoff, "");
        e.st_mut().settings.language = Some("ru".into());
        assert_eq!(e.take_choices(None)[1].title, "По частям");
        assert_eq!(e.describe_order(&PositionCommand::Close(d("0.5"))), "закрыть 50%");
        e.st_mut().settings.language = Some("en".into());
        assert_eq!([PositionCommand::Close(Decimal::ONE), PositionCommand::StopToBreakeven, PositionCommand::StopTo(d("2300")), PositionCommand::TakeTo(d("2600")),
                    PositionCommand::Trail(d("2")), PositionCommand::CancelOrders].iter().map(|c| e.describe_order(c)).collect::<Vec<_>>(),
                   ["close", "stop to breakeven", "stop to 2,300", "take-profit to 2,600", "trail the stop 2%", "cancel orders"]);
    }).await;
}

/// The plan's board goes with the plan; a move that ran stays, and «stop 2%» no longer edits the last trade. The plan
/// the next words change is the traded one only while its position is open.
#[tokio::test]
async fn the_plan_and_its_board() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let plan_board = Board { state: Some("plan".into()), ..Board::new("move", "Plan · long ETH") };
        let done = Board { state: Some("done".into()), ..Board::new("move", "Send 5 USDC to Mom") };
        let (id, done_id) = (plan_board.id.clone(), done.id.clone());
        e.set_boards(|b| { b.push(plan_board); b.push(done) });
        let traded = plan(|i| i.asset = "SOL".into());
        {
            let mut st = e.st_mut();
            st.plan_board = Some(id.clone());
            st.traded_plan = Some(traded.clone());
        }
        assert_eq!(e.plan_base(), TradeIntent::default());
        e.st_mut().positions = vec![Position { coin: "SOL".into(), size: d("2"), entry: d("150"), unrealized_pnl: Decimal::ZERO, liquidation: None }];
        assert_eq!(e.plan_base(), traded);
        e.drop_plan_board();
        let st = e.st();
        assert!(st.plan_board.is_none() && st.traded_plan.is_none());
        assert_eq!(st.boards().iter().map(|b| b.id.clone()).collect::<Vec<_>>(), [done_id]);
    }).await;
}

/// The position board's ruler: entry, stop and takes as numbers with their labels, the mark, the guardian's word.
#[tokio::test]
async fn the_position_ruler() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let order = |oid: u64, px: &str, kind: &str| Order::read(&json!({"coin": "ETH", "oid": oid, "isTrigger": true, "triggerPx": px, "orderType": kind, "reduceOnly": true})).expect("an order");
        {
            let mut st = e.st_mut();
            st.hl_orders = vec![order(1, "2300", "Stop Market"), order(2, "2600", "Take Profit Market")];
            st.markets.insert("ETH".into(), Market::new("ETH", d("2450"), 4, 25));
        }
        let p = Position { coin: "ETH".into(), size: d("0.05"), entry: d("2412.5"), unrealized_pnl: Decimal::ZERO, liquidation: None };
        let ruler = e.position_ruler(&p, None);
        assert_eq!(ruler.points.iter().map(|x| (x.kind.as_str(), x.at, x.text.as_str())).collect::<Vec<_>>(),
                   [("entry", 2412.5, "entry 2,412.5"), ("stop", 2300.0, "stop 2,300"), ("take", 2600.0, "take 2,600")]);
        assert_eq!((ruler.now, ruler.now_text.as_deref(), ruler.guard_line.as_deref()), (Some(2450.0), Some("2,450"), None));
        let mut rules = ExitRules::new(d("0.05"), d("2412.5"), true, true, None);
        assert_eq!(e.position_ruler(&p, Some(&rules)).guard_line.as_deref(), Some("Guardian: stop to breakeven after the first half"));
        rules.moved_to_breakeven = true;
        assert_eq!(e.position_ruler(&p, Some(&rules)).guard_line.as_deref(), Some("Guardian: stop already at breakeven"));
    }).await;
}

/// A TWAP is turned down before any plan is drawn: the Windows engine has no guardian to stop one whose stop fired.
#[tokio::test]
async fn a_twap_is_turned_down_before_any_plan() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        e.plan_trade(plan(|i| { i.asset = "ETH".into(); i.twap_minutes = Some(30) }), None, "long ETH 100 USDC over 30 min").await;
        let st = e.st();
        assert!(st.pending.is_none() && st.card.is_none() && st.intent == TradeIntent::default());
        assert!(st.messages().last().is_some_and(|m| m.text.starts_with("A TWAP isn't on Windows yet")));
    }).await;
}
