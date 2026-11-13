//! Canvas.swift and Folders.swift (Chat6Tests, Rail13Tests, Balance15Tests, Model7Tests, FoldersTests, EngineTests'
//! canvases, Cards13Tests' steps): no canvas switch while the chat answers or a move runs, a canvas's plan, skills and
//! cards back when it opens again, a live block shown once, renames and removals, the list's folders, pins and pictures
//! across a restart, a block in words, a move's route for what has one step. No network.

use std::rc::Rc;

use ihatecrypto_core::intent::TradeIntent;
use ihatecrypto_core::plan::Market;
use ihatecrypto_core::position_command::PositionCommand;
use rust_decimal::Decimal;
use signer_api::SignerAddress;

use super::{Lines, engine};
use crate::canvas::{Board, Money, Step};
use crate::engine::{ChartLine, Engine, Message, Pending, Request};
use crate::folders::{board_title, shown_title};
use crate::from_core::RouterOption;
use crate::portfolio::{Network, Row};
use crate::store;

fn en(lines: &Lines) -> Rc<Engine> {
    let e = engine(lines);
    e.set_settings(|s| s.language = Some("en".into()));
    e
}

fn said(e: &Engine, text: &str) { e.st_mut().messages_mut().push(Message::new(true, text, None)) }

fn id(e: &Engine) -> String { e.st().canvas_id.clone() }

fn row_title(e: &Engine, id: &str) -> Option<String> { e.canvas_rows().into_iter().find(|r| r.id == id).map(|r| r.title) }

#[tokio::test]
async fn no_canvas_opens_while_the_chat_answers_or_a_move_runs() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        said(&e, "first");
        let a = id(&e);
        e.new_canvas(false).unwrap();
        said(&e, "second");
        let b = id(&e);
        e.st_mut().thinking = true;
        assert!(!e.can_switch_canvas());
        assert!(e.open_canvas(&a).is_err() && e.new_canvas(false).is_err());
        assert_eq!(id(&e), b);
        // The chat's own «new canvas» is the answer itself, and goes.
        e.new_canvas(true).unwrap();
        assert_ne!(id(&e), b);
        e.st_mut().thinking = false;
        e.st_mut().running_board = Some("a move".into());
        assert_eq!(e.open_canvas(&a).unwrap_err().0, "A move is running: wait for it to finish.");
        e.st_mut().running_board = None;
        e.open_canvas(&a).unwrap();
        assert_eq!(e.st().messages()[0].text, "first");
    }).await;
}

#[tokio::test]
async fn a_canvas_keeps_its_plan_skills_and_cards() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        said(&e, "long SOL risk $5");
        let a = id(&e);
        let plan = TradeIntent { asset: "SOL".into(), risk_usd: Some(Decimal::from(5)), ..TradeIntent::default() };
        e.st_mut().intent = plan.clone();
        e.st_mut().loaded_skills = vec!["trading-study".into()];
        e.st_mut().messages_mut().push(Message::new(false, "Two ways:", None));
        e.set_options(vec![RouterOption::new("Kamino", "", "deposit 1 USDC into Kamino", false)]);
        e.new_canvas(false).unwrap();
        {
            let st = e.st();
            assert!(st.intent == TradeIntent::default() && st.loaded_skills.is_empty() && st.options.is_empty());
            // What's kept with the canvas is what canvases.json carries across a restart.
            let kept = st.canvases.iter().find(|c| c.id == a).unwrap();
            assert!(kept.intent.as_ref() == Some(&plan) && kept.skills.as_deref() == Some(&["trading-study".to_string()][..]));
            assert_eq!(kept.offers.as_ref().unwrap()[0].title, "Kamino");
        }
        let saved: Vec<crate::canvas::CanvasDoc> = store::load("canvases.json").unwrap();
        assert!(saved.iter().any(|c| c.id == a && c.intent.as_ref() == Some(&plan)));
        e.open_canvas(&a).unwrap();
        {
            let st = e.st();
            assert!(st.intent == plan && st.loaded_skills == ["trading-study"] && st.options[0].title == "Kamino");
        }
        // A card picked, or something said after, and the cards don't come back.
        e.st_mut().messages_mut().last_mut().unwrap().picked = Some(0);
        e.new_canvas(false).unwrap();
        e.open_canvas(&a).unwrap();
        assert!(e.st().options.is_empty());
    }).await;
}

#[tokio::test]
async fn each_canvas_keeps_its_own_chat_and_blocks_and_a_live_block_shows_once() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        said(&e, "remember: canvases keep their chat");
        let first = id(&e);
        let receive = e.pin(Board::new("receive", "receive"));
        e.pin(Board::new("result", "P/L"));
        // A live block asked again comes to the top instead of showing twice, with its own id.
        assert_eq!(e.pin(Board::new("receive", "receive")), receive);
        assert_eq!(e.st().boards().iter().map(|b| b.kind.as_str()).collect::<Vec<_>>(), ["receive", "result"]);
        e.new_canvas(false).unwrap();
        assert!(id(&e) != first && e.st().messages().is_empty() && e.st().boards().is_empty() && e.st().canvases.len() == 2);
        e.open_canvas(&first).unwrap();
        assert_eq!(e.st().messages()[0].text, "remember: canvases keep their chat");
        assert_eq!(e.st().boards().len(), 2);
        // The empty canvas left behind is gone, as the Mac keeps a new canvas only once it's written on.
        assert_eq!(e.st().canvases.len(), 1);
        // A move keeps the request that drew it, not a «yes» said after.
        said(&e, "yes");
        e.pin(Board::new("move", "Send 5 USDC to Mom"));
        assert_eq!(e.st().boards()[0].request.as_deref(), Some("remember: canvases keep their chat"));
        e.pin_kind("result", "Rates", vec!["Kamino · 5%".into()], None, None, None);
        assert_eq!(e.st().boards()[0].request.as_deref(), Some("remember: canvases keep their chat"));
    }).await;
}

#[tokio::test]
async fn a_waiting_card_left_without_confirm_is_cancelled_or_parked() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        said(&e, "send 5 USDC to Mom");
        let card = e.pin(Board { state: Some("waiting".into()), ..Board::new("move", "Send 5 USDC to Mom") });
        // A card with nothing pending behind it can't wait elsewhere: cancelled, and said so on its block.
        e.st_mut().card = Some(crate::engine::Card { id: card.clone(), title: "Send 5 USDC to Mom".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(),
                                                     ticks: vec![], over_limit: false });
        e.new_canvas(false).unwrap();
        let b = e.st().canvases.iter().flat_map(|c| &c.boards).find(|b| b.id == card).cloned().unwrap();
        assert_eq!(b.state.as_deref(), Some("cancelled"));
        assert_eq!(b.lines.last().map(String::as_str), Some("Left without Confirm: nothing was signed."));
        assert!(e.st().card.is_none() && e.st().pending.is_none());
    }).await;
}

#[tokio::test]
async fn renames_and_removals_as_on_the_mac() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        e.new_canvas(false).unwrap();
        let canvas = id(&e);
        e.rename_canvas(&canvas, "  Rent  ");
        assert_eq!(e.st().canvases.iter().find(|c| c.id == canvas).unwrap().name.as_deref(), Some("Rent"));
        assert_eq!(row_title(&e, &canvas).as_deref(), Some("Rent"));
        e.rename_canvas(&canvas, "   ");
        assert!(e.st().canvases.iter().find(|c| c.id == canvas).unwrap().name.is_none());
        assert_eq!(row_title(&e, &canvas), e.st().canvases.iter().find(|c| c.id == canvas).map(|c| c.title.clone()));

        // Removing the open canvas opens a new blank one, not another.
        e.pin(Board::new("receive", "receive"));
        let other = id(&e);
        e.new_canvas(false).unwrap();
        let open = id(&e);
        e.remove_canvas(&open).unwrap();
        assert!(!e.st().canvases.iter().any(|c| c.id == open));
        assert!(id(&e) != other && e.st().canvases[0].id == id(&e) && e.st().boards().is_empty());
        // Another canvas removed leaves the open one open; its plan's alert goes with it.
        let now = id(&e);
        let alert: ihatecrypto_core::alerts::PriceAlert = serde_json::from_value(serde_json::json!({
            "id": "5D1F0B5E-0000-4000-8000-0000000000AA", "coin": "ETH", "above": true, "price": 3000, "created": 784_123_200.5, "plan": other,
        })).unwrap_or_else(|e| panic!("{e}"));
        e.set_alerts(|a| a.push(alert));
        e.remove_canvas(&other).unwrap();
        assert!(id(&e) == now && !e.st().canvases.iter().any(|c| c.id == other) && e.st().alerts.is_empty());
    }).await;
}

#[tokio::test]
async fn closing_the_chart_keeps_the_canvas_name_and_the_first_words_name_it() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        e.new_canvas(false).unwrap();
        let canvas = id(&e);
        e.set_boards(|b| *b = vec![Board::new("chart", "BTC · 1 day")]);
        e.set_boards(|b| b.clear());
        assert_eq!(row_title(&e, &canvas).as_deref(), Some("Chart · BTC · 1 day"));
        // A new canvas is named by its first message, and only by that one.
        e.new_canvas(false).unwrap();
        said(&e, "what's bitcoin doing today and should I buy some now or wait for a dip");
        e.name_canvas("what's bitcoin doing today and should I buy some now or wait for a dip");
        let title = e.st().canvases[0].title.clone();
        assert_eq!(title, "what's bitcoin doing today and should I ");
        said(&e, "and ETH?");
        e.name_canvas("and ETH?");
        assert_eq!(e.st().canvases[0].title, title);
    }).await;
}

#[tokio::test]
async fn a_canvas_row_shows_its_plans_coin_or_its_note() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let row = |e: &Engine| e.canvas_rows().into_iter().find(|r| r.id == id(e)).unwrap();
        assert!(row(&e).coin.is_none() && !row(&e).note && row(&e).empty);
        e.st_mut().intent.asset = "SOL".into();
        assert_eq!(row(&e).coin.as_deref(), Some("SOL"));
        e.st_mut().intent = TradeIntent::default();
        e.pin(Board { lines: vec!["buy the dip".into()], ..Board::new("note", "Ideas") });
        assert!(row(&e).note && row(&e).blocks && !row(&e).empty);
    }).await;
}

#[test]
fn a_canvas_is_named_in_the_macs_order() {
    let mut c = crate::canvas::CanvasDoc::new("buy some SOL");
    assert_eq!(shown_title(&c, &[]), "buy some SOL");
    let mv = Board::new("move", "Move 5 USDC to Base");
    assert_eq!(shown_title(&c, &[Board::new("result", "x"), mv.clone()]), "Move 5 USDC to Base");
    c.name = Some("Rent".into());
    assert_eq!(shown_title(&c, &[mv]), "Rent");
    let chart = Board { chart: Some(ChartLine { title: "BTC · 1D".into(), closes: vec![1.0, 2.0], times: None, low: "1".into(), high: "2".into(), change: "+1%".into(),
                                                coin: Some("BTC".into()), network: None, extras: None }), ..Board::new("chart", "BTC · 1D") };
    let rates = Board { data: Some(crate::blocks::BlockData { rates: Some(vec![]), ..Default::default() }), ..Board::new("result", "Rates · USDC") };
    assert_eq!(board_title(std::slice::from_ref(&chart)).as_deref(), Some("Chart · BTC"));
    assert_eq!(board_title(&[chart, rates.clone()]).as_deref(), Some("Rates · USDC"));
    assert_eq!(board_title(&[Board::new("result", "P/L · 7 days")]), None);
}

#[tokio::test]
async fn folders_pins_and_pictures_as_on_the_mac() {
    let dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let op = async |e: &Engine, op: &str, f: fn(&mut Request)| {
            let mut r = Request { op: op.into(), ..Default::default() };
            f(&mut r);
            e.folder_op(&r).await
        };
        let folders = |e: &Engine| e.folder_views().into_iter().map(|f| f.name).collect::<Vec<_>>();
        let row = |e: &Engine, id: &str| e.canvas_rows().into_iter().find(|r| r.id == id);
        let first = id(&e);
        e.pin(Board::new("receive", "receive"));

        // An empty canvas left for another one goes; it's shown as the new canvas while it's open.
        e.new_canvas(false).unwrap();
        let empty = id(&e);
        assert!(row(&e, &empty).unwrap().empty);
        op(&e, "newCanvasIn", |r| r.name = Some("  Ideas  ".into())).await.unwrap();
        let in_ideas = id(&e);
        assert!(row(&e, &empty).is_none(), "an empty canvas stayed");
        assert!(row(&e, &in_ideas).unwrap().folder.as_deref() == Some("Ideas") && folders(&e) == ["Ideas"]);
        e.pin(Board::new("receive", "receive"));

        // A folder made by a move; a rename onto another folder merges them, pictures kept.
        e.move_canvas(&first, Some("Trading"));
        e.folder_op(&Request { op: "setIcon".into(), name: Some("Trading".into()), text: Some(r#"{"kind":"emoji","value":"📈"}"#.into()), ..Default::default() }).await.unwrap();
        e.rename_folder("Trading", "Ideas");
        assert!(folders(&e) == ["Ideas"] && row(&e, &first).unwrap().folder.as_deref() == Some("Ideas"));
        assert_eq!(e.folder_views()[0].icon.as_ref().map(|i| i.value.as_str()), Some("📈"));

        // A picture is one of the library's or a small PNG of the window's own; anything else is refused.
        for bad in [r#"{"kind":"file","value":"x"}"#, r#"{"kind":"image","value":"https://example.com/a.png"}"#, r#"{"kind":"logo","value":"../../x"}"#, "not json"] {
            let asked = e.folder_op(&Request { op: "setIcon".into(), wallet: Some(first.clone()), text: Some(bad.into()), ..Default::default() }).await;
            assert!(asked.is_err(), "{bad} was kept");
        }
        e.folder_op(&Request { op: "setIcon".into(), wallet: Some(first.clone()), text: Some(r#"{"kind":"logo","value":"sol"}"#.into()), ..Default::default() }).await.unwrap();
        assert_eq!(row(&e, &first).unwrap().icon.map(|i| i.value).as_deref(), Some("sol"));

        // Five pin; the sixth is refused with what to do.
        let mut ids = vec![first.clone(), in_ideas.clone()];
        for _ in 0..4 {
            e.new_canvas(false).unwrap();
            e.pin(Board::new("receive", "receive"));
            ids.push(id(&e));
        }
        for id in &ids[..5] { e.pin_canvas(id, true).unwrap() }
        let sixth = e.pin_canvas(&ids[5], true).unwrap_err();
        assert!(sixth.0.contains("unpin one first"), "{}", sixth.0);
        // Pinned again keeps its place; unpinned goes.
        e.pin_canvas(&ids[0], true).unwrap();
        e.pin_canvas(&ids[4], false).unwrap();
        e.pin_canvas(&ids[4], true).unwrap();

        // Deleting a folder keeps its canvases, out of any folder.
        e.delete_folder("Ideas");
        assert!(folders(&e).is_empty() && row(&e, &first).unwrap().folder.is_none() && row(&e, &in_ideas).is_some());
        e.create_folder("Taxes");
        e.create_folder("   ");
        assert_eq!(folders(&e), ["Taxes"]);
    }).await;

    // A restart reads it all back: the pins, the picture, the emptied folder.
    tokio::task::LocalSet::new().run_until(async {
        let again = en(&Lines::default());
        let rows = again.canvas_rows();
        assert_eq!(rows.iter().filter(|r| r.pinned_at.is_some()).count(), 5);
        assert!(rows.iter().any(|r| r.icon.as_ref().is_some_and(|i| i.kind == "logo" && i.value == "sol")));
        assert_eq!(again.folder_views().into_iter().map(|f| f.name).collect::<Vec<_>>(), ["Taxes"]);
    }).await;
    drop(dir);
}

#[tokio::test]
async fn folders_sort_as_finder_sorts_them_and_a_site_icon_needs_a_site() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        for f in ["folder 10", "Folder 9", "alpha", "Beta"] { e.create_folder(f) }
        assert_eq!(e.canvas_folders(), ["alpha", "Beta", "Folder 9", "folder 10"]);
        assert_eq!(Engine::folder_name(&"x".repeat(50)).map(|n| n.len()), Some(40));
        assert!(Engine::folder_name(" \n ").is_none());
        // Not a site: refused before anything is asked of the network.
        for bad in ["bybit", "https://", "a b.com", ""] {
            assert_eq!(e.site_icon(bad).await.unwrap_err().0, "Type a site's address, like bybit.com.");
        }
    }).await;
}

#[tokio::test]
async fn a_block_in_words_as_the_model_gets_it() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let note = Board { lines: vec!["buy the dip".into()], ..Board::new("note", "Ideas") };
        assert_eq!(e.block_facts(&note), "«note»: buy the dip; this canvas is a note in folder «Ideas»");
        let opens = Board { lines: vec!["a".into(), "b".into()], opens: Some("x".into()), ..Board::new("note", "Ideas") };
        assert_eq!(e.block_facts(&opens), "«note»: a; b; a card opens 2 notes in folder «Ideas»");
        e.st_mut().markets.insert("BTC".into(), Market::new("BTC", Decimal::from(65_000), 5, 40));
        let chart = Board { chart: Some(ChartLine { title: "BTC · Day".into(), closes: vec![1.0, 2.0], times: None, low: "1".into(), high: "2".into(),
                                                    change: "+100%".into(), coin: Some("BTC".into()), network: None, extras: None }), ..Board::new("chart", "BTC · Day") };
        assert_eq!(e.block_facts(&chart), "«BTC · Day»: low 1, high 2, +100% over the range, live price now 65,000");
        let money = Money { place: "Base".into(), symbol: "USDC".into(), was: "10".into(), after: "5".into() };
        let mv = Board { state: Some("waiting".into()), steps: Some(vec![Step { label: "SEND · Base".into(), title: "5 USDC to Mom".into(), lines: vec![],
                                                                                money: Some(money), status: None }]), ..Board::new("move", "Send 5 USDC to Mom") };
        assert_eq!(e.block_facts(&mv), "«Send 5 USDC to Mom» [waiting]: SEND · Base: 5 USDC to Mom (Base 10 → 5 USDC)");
        assert_eq!(e.block_facts(&Board::new("receive", "receive")), "«receive»: ");
    }).await;
}

#[tokio::test]
async fn a_route_with_one_step_and_the_balance_a_move_spends() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = en(&Lines::default());
        let lines = vec!["stop to the entry".to_string()];
        let stop = e.steps(&Pending::Position { command: PositionCommand::StopToBreakeven, coin: "ETH".into() }, "stop", &lines);
        assert!(stop.len() == 1 && stop[0].label == "HYPERLIQUID" && stop[0].lines == lines);
        let mom = SignerAddress { name: "Mom".into(), network: "hyperevm".into(), address: "0x90F79bf6EB2c4f870365E785982E1f101E93b906".into(), memo: String::new() };
        assert_eq!(e.steps(&Pending::Address(mom.clone()), "Save Mom", &[])[0].label, "SAVE · HyperEVM");
        let network = |key: &str, name: &str, rows: Vec<Row>| Network { key: key.into(), name: name.into(), usd: Decimal::ZERO, rows, note: None, hidden: vec![], hidden_more: 0 };
        let networks = vec![network("solana", "Solana", vec![Row::new("USDC", "45.97", Decimal::new(4597, 2))]), network("hyperevm", "HyperEVM (mine)", vec![])];
        e.st_mut().portfolio.networks = networks;
        assert_eq!(e.steps(&Pending::Address(mom), "Save Mom", &[])[0].label, "SAVE · HyperEVM (mine)");
        // Read against the balance on screen, never from the step's words; past the balance it's all of it.
        assert_eq!(e.money_spending(Decimal::from(10), "usdc", "solana"),
                   Some(Money { place: "Solana".into(), symbol: "USDC".into(), was: "45.97".into(), after: "35.97".into() }));
        assert_eq!(e.money_spending(Decimal::MAX, "USDC", "solana").map(|m| m.after).as_deref(), Some("0"));
        assert_eq!(e.money_spending(Decimal::ONE, "SOL", "solana"), None);
    }).await;
}
