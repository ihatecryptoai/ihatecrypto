//! Balance15/16/17Tests, Portfolio3Tests' note, Rail13Tests' and Plan5Tests' Jupiter rows and stock day, Ui7Tests'
//! lending readings and stock rows, Cards6Tests' rows and Plan5Tests' held P/L, offline: the reads are handed in.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use ihatecrypto_core::foundation::{Uuid, distant_past, unix};
use ihatecrypto_core::pnl::Flow;
use signer_api::SignerReply;

use super::*;
use crate::outbox::Outbox;
use crate::place_facts::HlFacts;
use crate::blocks::ClosedTrade;
use crate::signer_link::SignerLink;

/// An engine in a test folder of its own, locked so a state push never draws.
fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::writer(std::io::sink()), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("locked: no state is drawn"));
    e.st_mut().lock.locked = true;
    e
}

fn d(s: &str) -> Decimal { s.parse().unwrap() }

fn usdc(x: &str) -> Row { Engine::row("USDC", d(x), Some(d(x)), None, None, None, None, None) }

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(tokio::task::LocalSet::new().run_until(test));
}

#[test]
fn the_balance_is_whole_once_each_network_answered() {
    // Solana answered in one read, Hyperliquid in the next: whole (A14-C2-4).
    assert!(Engine::balance_ready(true, true, Some(true), true, true));
    // Not yet heard from Hyperliquid, or Solana came unpriced (a coin Jupiter didn't price, new since the last read).
    assert!(!Engine::balance_ready(true, true, Some(true), false, true));
    assert!(!Engine::balance_ready(true, true, Some(false), true, true));
    assert!(!Engine::balance_ready(true, true, None, true, true));
    // A wallet of one chain waits only for its own; the Automation wallet unread keeps it loading.
    assert!(Engine::balance_ready(false, true, None, true, true));
    assert!(!Engine::balance_ready(false, true, None, true, false));
}

#[test]
fn a_refresh_asked_during_a_read_waits_for_the_read_it_queued() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        {
            let mut st = e.st_mut();
            st.refreshing = true;
            st.refresh_again = false;
            st.reads_started = 5; // read 5 is under way when the person asks
        }
        let back = Rc::new(Cell::new(false));
        let (me, flag) = (e.clone(), back.clone());
        let asked = tokio::task::spawn_local(async move {
            me.refresh().await;
            flag.set(true);
        });
        while e.st().read_waiters.is_empty() { tokio::task::yield_now().await }
        // A refresh asked for during a read must run after it, not be dropped.
        assert!(e.st().refresh_again);
        assert_eq!(e.st().read_waiters[0].pass, 6);
        // Read 5 lands: the ask came during it, so it isn't the read that was asked for.
        e.land_reads();
        for _ in 0..20 { tokio::task::yield_now().await }
        assert!(!back.get(), "a refresh returned on the read that was already running");
        assert_eq!(e.st().read_waiters.len(), 1);
        // Read 6, begun after the ask, lands: now it returns.
        e.st_mut().reads_started = 6;
        e.land_reads();
        asked.await.unwrap();
        assert!(back.get() && e.st().read_waiters.is_empty());
    });
}

#[test]
fn a_switch_forgets_what_the_last_wallets_reads_left() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        {
            let mut st = e.st_mut();
            st.positions_for = Some("0x90f79bf6eb2c4f870365e785982e1f101e93b906".into());
            st.seen_solana_priced = Some(true);
            st.seen_hl = true;
            st.solana_holdings = vec![Holding::new("solana", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", d("5"), "USDC")];
            st.place_facts.hyperliquid = Some(HlFacts { account_value: d("12"), withdrawable: d("10"), spot_usdc: d("2"), dexes: vec![], hlp_equity: Decimal::ZERO, hlp_locked_until: None });
            st.recently_closed = vec![ClosedTrade { coin: "ETH".into(), name: "ETH".into(), long: true, closed: chrono::Utc::now(), realized: d("10") }];
            st.notice_positions = Some(HashSet::from(["ETH".to_string()]));
            st.notice_entries = HashSet::from(["BTC".to_string()]);
            st.portfolio.total = d("5");
        }
        e.forget_wallet_reads();
        let st = e.st();
        assert!(st.positions_for.is_none() && st.hl_orders.is_empty() && st.positions.is_empty());
        assert!(st.seen_solana_priced.is_none() && !st.seen_hl && st.solana_holdings.is_empty());
        assert!(st.place_facts.hyperliquid.is_none() && st.recently_closed.is_empty() && st.portfolio == PortfolioView::default());
        // The new wallet's first read finds no «new» position, so nothing «filled» on the old wallet's entry is said.
        assert!(st.notice_positions.is_none() && st.notice_entries.is_empty());
    });
}

#[test]
fn the_last_whole_balance_is_kept_per_wallet() {
    let _dir = store::test_folder();
    Engine::remember_balance(d("120"), "balance15-a");
    Engine::remember_balance(d("7"), "balance15-b");
    let kept: HashMap<String, Usd> = store::load("balances.json").unwrap();
    assert_eq!((kept["balance15-a"].0, kept["balance15-b"].0), (d("120"), d("7")));
    // Written as Swift's JSONEncoder writes a [String: Decimal]: numbers.
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(store::url("balances.json")).unwrap()).unwrap();
    assert_eq!(raw["balance15-a"], serde_json::json!(120));
    run(async {
        let e = engine();
        e.set_settings(|s| s.watch_solana = Some("balance15-a".into()));
        // Dimmed until the first whole read; none once it's whole.
        assert_eq!(e.last_full_balance(), Some(d("120")));
        e.st_mut().portfolio.ready = true;
        assert_eq!(e.last_full_balance(), None);
    });
    assert_eq!(Engine::balance_key(&TradeWallet { solana: String::new(), evm: "0xab".into() }), "0xab");
    assert_eq!(Engine::balance_key(&TradeWallet { solana: "So1".into(), evm: "0xab".into() }), "So1");
}

#[test]
fn each_networks_note_reads_as_the_macs() {
    let perps = Row { section: "tokens".into(), ..Row::new("Perps", "10", d("10")) };
    let perps = Row { usd: Some(d("10")), ..perps };
    let hlp = Row { section: "defi".into(), usd: Some(d("5")), ..Row::new("HLP", "5", d("5")) };
    let hl = |rows: Vec<Row>| Network::new("hyperliquid", "Hyperliquid", rows);
    assert_eq!(Engine::note(&hl(vec![perps.clone(), hlp]), false).as_deref(), Some("$10.00 · margin · HLP $5.00"));
    assert_eq!(Engine::note(&hl(vec![perps]), false).as_deref(), Some("$10.00 · margin"));
    assert_eq!(Engine::note(&Network::new("tempo", "Tempo", vec![]), true).as_deref(), Some("USDC для платных сервисов"));
    // Idle and earning, the bigger first; a coin with a cost key isn't idle money; under a cent is left out.
    let base = Network::new("base", "Base", vec![usdc("3"), Engine::deposit("Aave", d("7.5"), "earning"),
                                                 Engine::row("ETH", d("1"), Some(d("3000")), None, Some("base"), Some("native"), None, None)]);
    assert_eq!(Engine::note(&base, false).as_deref(), Some("earning $7.50 · idle $3.00"));
    assert_eq!(Engine::note(&base, true).as_deref(), Some("в работе $7.50 · свободно $3.00"));
    assert_eq!(Engine::note(&Network::new("near", "NEAR", vec![usdc("0.001")]), false), None);
}

#[test]
fn a_deposits_earning_is_said_in_the_windows_language() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        e.set_settings(|s| s.language = Some("ru".into()));
        let n = e.noted(Network::new("solana", "Solana", vec![Engine::deposit("Kamino Lend", d("5"), "earning"),
                                                               Engine::deposit("Kamino", d("2"), "Allez USDC · 5.42%")]));
        assert_eq!(n.rows.iter().map(|r| r.note.clone().unwrap()).collect::<Vec<_>>(), ["приносит доход", "Allez USDC · 5.42%"]);
        assert_eq!(n.note.as_deref(), Some("в работе $7.00"));
    });
}

#[test]
fn jupiters_platforms_are_in_dollars_on_their_site_and_count_in_the_total() {
    let e = |fetcher: &str, platform: Option<&str>, name: Option<&str>, label: Option<&str>, value: f64, apy: Option<f64>| JupiterElement {
        fetcher: Some(fetcher.into()), platform: platform.map(str::to_string), name: name.map(str::to_string), label: label.map(str::to_string), value: Some(value), apy,
    };
    let rows = Engine::jupiter_rows(&[e("jupiter-staked", Some("jup"), Some("Staked JUP"), None, 123.456, None), e("jupiter-exchange-earn", None, None, None, 5.0, None)]);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].amount.as_str(), rows[0].usd, rows[0].site.as_deref()), ("$123.46", Some(d("123.46")), Some("jup.ag")));

    let rows = Engine::jupiter_rows(&[
        e("jupiter-exchange-earn", Some("jupiter-exchange"), Some("Earn"), None, 50.0, Some(0.05)),
        e("jupiter-governance", Some("jupiter-governance"), Some("Staked JUP"), None, 12.346, Some(0.0731)),
        e("jupiter-perps", Some("jupiter-perps"), Some("Perps"), Some("SOL long"), 7.5, None),
        e("jupiter-prediction", Some("jupiter-prediction"), None, None, 3.0, Some(0.0)),
    ]);
    assert!(rows.iter().all(|r| r.symbol == "Jupiter" && r.section == "defi" && r.icon.as_deref() == Some("logos/jup.png")));
    assert_eq!(rows.iter().map(|r| r.note.clone().unwrap()).collect::<Vec<_>>(), ["Staked JUP · 7.31%", "Perps · SOL long", "jupiter-prediction"]);
    assert_eq!(rows.iter().map(|r| r.usd.unwrap()).collect::<Vec<_>>(), [d("12.35"), d("7.5"), d("3")]);
    let card = Engine::tucked(Network::new("solana", "Solana", [vec![Row { usd: Some(d("10")), ..Row::new("USDC", "10", d("10")) }], rows].concat()), &HashMap::new());
    assert_eq!(card.usd, d("32.85"));
    // An element that isn't a number, or isn't finite, is left out.
    assert!(Engine::jupiter_rows(&[JupiterElement { value: Some(f64::NAN), ..Default::default() }, JupiterElement::default()]).is_empty());
}

#[test]
fn a_stocks_day_comes_from_its_hyperliquid_market() {
    let markets = HashMap::from([("xyz:TSLA".to_string(), Market { prev_day_px: Some(d("100")), dex: Some("xyz".into()), ..Market::new("xyz:TSLA", d("110"), 2, 10) })]);
    assert_eq!(Engine::stock_day("TSLA", &markets), Some(d("10")));
    assert_eq!(Engine::stock_day("tsla", &markets), Some(d("10")));
    assert_eq!(Engine::stock_day("AAPL", &markets), None);
}

#[test]
fn whats_lent_keeps_its_last_reading_and_its_rate() {
    run(async {
        let me = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
        let place = format!("Aave · Arbitrum test-{}", Uuid::new());
        let reads = Cell::new(0);
        let start = chrono::Utc::now();
        let at = |s: i64| start + chrono::Duration::seconds(s);
        let rate = |x: &'static str| { let reads = &reads; move || { reads.set(reads.get() + 1); async move { Some(d(x)) } } };
        let held = |x: Option<u64>| move || async move { x };
        // Read: 25 USDC at 4.123% a year; off Base it opens Aave's site.
        let first = Engine::lent("Aave", &place, me, false, held(Some(25_000_000)), rate("4.123"), start).await.unwrap();
        assert_eq!((first.usd, first.note.as_deref(), first.site.as_deref(), first.section.as_str()), (Some(d("25")), Some("earning 4.12%"), Some("app.aave.com"), "defi"));
        // The node doesn't answer: the last reading stays, the rate isn't asked again.
        let silent = Engine::lent("Aave", &place, me, false, held(None), rate("9"), at(60)).await.unwrap();
        assert!(silent == first && reads.get() == 1);
        // A new reading within 10 minutes keeps the rate; after them the rate is read again.
        let later = Engine::lent("Aave", &place, me, false, held(Some(30_000_000)), rate("5"), at(120)).await.unwrap();
        assert!(later.usd == Some(d("30")) && later.note.as_deref() == Some("earning 4.12%") && reads.get() == 1);
        let stale = Engine::lent("Aave", &place, me, false, held(Some(30_000_000)), rate("5"), at(800)).await.unwrap();
        assert!(stale.note.as_deref() == Some("earning 5%") && reads.get() == 2, "{:?}", stale.note);
        // Withdrawn: gone, and a silent node afterwards doesn't bring it back.
        assert!(Engine::lent("Aave", &place, me, false, held(Some(0)), rate("5"), chrono::Utc::now()).await.is_none());
        assert!(Engine::lent("Aave", &place, me, false, held(None), rate("5"), chrono::Utc::now()).await.is_none());
        // On Base the row opens its details board, not a site.
        let base = Engine::lent("Morpho", &format!("Morpho · Base test-{}", Uuid::new()), me, true, held(Some(10_000_000)), || async { None }, start).await.unwrap();
        assert!(base.site.is_none() && base.note.as_deref() == Some("earning"));
    });
}

#[test]
fn a_stock_token_is_read_at_robinhoods_bid() {
    let tsla = Token { symbol: "TSLA".into(), name: "Tesla".into(), address: "0x00000000000000000000000000000000000000a1".into(), decimals: 18, logo: None };
    let explorer = Row {
        id: Some(format!("robinhood:{}", tsla.address)), real: Some(false),
        ..Engine::row("TSLA", d("2"), Some(d("3")), None, Some("robinhood"), Some(&tsla.address), None, None)
    };
    let r = Engine::stock_row(&tsla, d("2"), Some(d("250")), Some(&explorer));
    // The bid's value, kept on show (the registry vouches for it), hidden or shown by the explorer row's own id.
    assert!(r.usd == Some(d("500")) && r.real == Some(true) && r.stock == Some(true) && r.verified == Some(true) && r.id == explorer.id);
    // No quote from Robinhood: the explorer's price stays.
    assert_eq!(Engine::stock_row(&tsla, d("2"), None, Some(&explorer)).usd, Some(d("3")));
    // Bought here, not listed yet: no id, so it can't be tucked away.
    assert_eq!(Engine::stock_row(&tsla, d("1"), Some(d("250")), None).id, None);
}

#[test]
fn a_stablecoin_keeps_its_contract_and_near_has_a_cost_key() {
    let usdt = Engine::row("USDT", d("5"), Some(d("5")), None, Some("ethereum"), Some("0xdAC17F958D2ee523a2206206994597C13D831ec7"), Some(d("1")), None);
    // No P/L for a dollar, but its row still opens and trades by its contract.
    assert!(usdt.key.is_none() && usdt.contract.as_deref() == Some("0xdAC17F958D2ee523a2206206994597C13D831ec7") && usdt.change24h.is_none());
    assert_eq!(Engine::row("NEAR", d("3"), None, None, Some("near"), Some("native"), None, None).key.as_deref(), Some("near:native"));
    let aero = Engine::row("AERO", d("4"), None, None, Some("base"), Some("0x940181a94A35A4569E4529A3CDfB74e38FD98631"), Some(d("2")), Some(true));
    assert_eq!((aero.key.as_deref(), aero.change24h), (Some("base:0x940181a94a35a4569e4529a3cdfb74e38fd98631"), Some(d("2"))));
    assert_eq!(Engine::row("SOL", d("2"), None, None, Some("solana"), Some(CostBasis::WSOL), None, None).key.as_deref(), Some(CostBasis::WSOL));
    // Under one coin six places, from one four.
    assert_eq!((usdc("0.1234567").amount.as_str(), usdc("12.345678").amount.as_str()), ("0.123457", "12.3457"));
    // A listed token: hidden or shown by its id, real when a market list vouches for it.
    let h = Holding { verified: true, price: Some(d("2")), ..Holding::new("base", "0xABC", d("3"), "AI") };
    let r = Engine::row_holding(&h);
    assert_eq!((r.id.as_deref(), r.real, r.verified, r.usd), (Some("base:0xabc"), Some(true), Some(true), Some(d("6"))));
    assert_eq!(Engine::row_holding(&Holding::new("hyperliquid", "HYPE", d("1"), "HYPE")).verified, None);
    assert_eq!(Engine::row_holding(&Holding::new("base", "0xdef", d("1"), "SCAM")).verified, Some(false));
}

#[test]
fn a_stock_bought_here_is_remembered_by_its_symbol() {
    let _dir = store::test_folder();
    Engine::remember_stock("tsla");
    Engine::remember_stock("TSLA");
    Engine::remember_stock("0xabc");
    Engine::remember_stock("nvda");
    assert_eq!(Engine::bought_stocks(), ["TSLA", "NVDA"]);
}

#[test]
fn the_held_pnl_is_said_as_the_mac_says_it() {
    let noon = unix(1_791_633_600.0); // 2026-10-10, noon UTC
    let mut cost = CostBasis::default();
    cost.bought(d("10"), d("20"), noon);
    cost.came(d("2"), d("5"), noon);
    let told = Engine::held_paid("NEAR", d("12"), Some(d("3")), Some(&cost), false);
    assert_eq!(told.0, "Bought 10 NEAR for $20.00 at $2.00 · 2 more came in from elsewhere, counted at the price then: $2.50 · worth $28.00 now");
    assert_eq!(told.1.as_deref(), Some("since Oct 10"));
    // More held than the cost knows: the rest is said to be uncounted.
    let mut bought = CostBasis::default();
    bought.bought(d("4"), d("8"), noon);
    let more = Engine::held_paid("SOL", d("5"), Some(d("1")), Some(&bought), false);
    assert_eq!(more.0, "Bought 4 SOL for $8.00 at $2.00 · worth $9.00 now · 1 more not counted: no price for it");
    assert_eq!(more.1.as_deref(), Some("since you bought on Oct 10"));
    assert_eq!(Engine::held_paid("SOL", d("5"), None, None, false).0, "No price yet to count its P/L from");
    // In Russian, and all of it come from elsewhere.
    let mut came = CostBasis::default();
    came.came(d("2"), d("5"), noon);
    let ru = Engine::held_paid("NEAR", d("2"), Some(d("1")), Some(&came), true);
    assert_eq!(ru.0, "2 NEAR пришло со стороны, по цене того дня: $2.50 · сейчас стоит $6.00");
    assert_eq!(ru.1.as_deref(), Some("с 10 окт."));
    assert_eq!(Engine::token_price(d("0.000123")), "$0.000123");
    assert_eq!(Engine::token_price(d("5.04")), "$5.04");
    assert_eq!(Engine::token_price(d("0.25")), "$0.25");
}

#[test]
fn money_that_came_in_is_said_once_and_counted_as_moved_in() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        {
            let mut st = e.st_mut();
            st.status = Some(SignerReply { ok: true, has_wallet: Some(true), solana_address: Some("So1Wallet".into()), ..Default::default() });
            st.markets.insert("SOL".into(), Market::new("SOL", d("200"), 2, 20));
            st.last_own_move = distant_past();
        }
        let sol = |x: &str| Engine::row("SOL", d(x), None, None, Some("solana"), Some(CostBasis::WSOL), None, None);
        let view = |rows: Vec<Row>| PortfolioView { networks: vec![Network::new("solana", "Solana", rows)], ..Default::default() };
        let (old, new) = (view(vec![usdc("10"), sol("1")]), view(vec![usdc("15.5"), sol("1.0001"), Row { id: Some("x".into()), ..usdc("99") }]));
        e.announce_incoming(&old, &new);
        let said = e.st().messages().last().map(|m| (m.text.clone(), m.note.clone()));
        // SOL under 0.0005 isn't said; a listed token (with an id) never is.
        assert_eq!(said, Some(("💸 Received +5.5 USDC on Solana.".into(), Some("incoming".into()))));
        let flows: Vec<Flow> = store::load("flows.json").unwrap();
        assert_eq!(flows.iter().map(|f| (f.usd, f.note.as_str(), f.wallet.as_str())).collect::<Vec<_>>(),
                   [(d("0.0200"), "SOL in on Solana", "So1Wallet"), (d("5.5"), "USDC in on Solana", "So1Wallet")]);
        // Right after a move of the user's own: nothing said.
        e.st_mut().last_own_move = chrono::Utc::now();
        let before = e.st().messages().len();
        e.announce_incoming(&old, &new);
        assert_eq!(e.st().messages().len(), before);
    });
}
