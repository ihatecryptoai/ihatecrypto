//! PositionCommand.swift's cases: IhatecryptoCoreTests' money and position commands, JupiterDCATests, TokenSendTests,
//! MultisigCommandTests, USDTMoveCommandTests, and the WalletCommand lines of the other suites (round 1's pending ones
//! too). AgentTests, the findings suites and the English phrase tables are in the files below.

mod agent;
mod english;
mod findings;
mod limit;
mod near;

use chrono::TimeDelta;
use rust_decimal::Decimal;

use super::d;
use crate::foundation::unix;
use crate::intent::Fmt;
use crate::position_command::{Approve, ContractCall, KAMINO_STOCK_MINTS, KAMINO_STOCKS, PositionCommand, Schedule, SpotAsset, WalletCommand};

fn p(text: &str) -> Option<WalletCommand> { WalletCommand::parse(text) }

fn sol() -> SpotAsset { SpotAsset::all()[0].clone() }

fn btc() -> SpotAsset { SpotAsset::all()[1].clone() }

fn eth() -> SpotAsset { SpotAsset::all()[2].clone() }

fn jup() -> SpotAsset { SpotAsset::all()[3].clone() }

fn some(s: &str) -> Option<Decimal> { Some(d(s)) }

fn allocate(symbol: &str, max_share: Option<Decimal>, amount: Option<Decimal>, from: Option<&str>) -> WalletCommand {
    WalletCommand::Allocate { symbol: symbol.into(), max_share, amount, without: vec![], from: from.map(Into::into) }
}

fn dca(asset: SpotAsset, usdc: Option<Decimal>, orders: Option<i64>, every: i64) -> WalletCommand { WalletCommand::JupiterDca { asset, usdc, orders, every } }

fn send_token(name: &str, token: &str, amount: Option<Decimal>, network: Option<&str>) -> WalletCommand {
    WalletCommand::SendToken { name: name.into(), token: token.into(), amount, network: network.map(Into::into) }
}

fn usdt(from: &str, to: &str, amount: Option<Decimal>, layer_zero: bool) -> WalletCommand {
    WalletCommand::Usdt { from: from.into(), to: to.into(), amount, layer_zero }
}

fn cross(from: &str, on: &str, to: &str, at: &str, amount: Option<Decimal>) -> WalletCommand {
    WalletCommand::CrossSwap { from: from.into(), on: on.into(), to: to.into(), at: at.into(), amount }
}

// MARK: IhatecryptoCoreTests

#[test]
fn position_commands_are_recognised() {
    assert_eq!(PositionCommand::parse("стоп в безубыток"), Some(PositionCommand::StopToBreakeven));
    assert_eq!(PositionCommand::parse("перенеси стоп на 2661"), Some(PositionCommand::StopTo(d("2661"))));
    assert_eq!(PositionCommand::parse("закрой половину"), Some(PositionCommand::Close(d("0.5"))));
    assert_eq!(PositionCommand::parse("закрой 25%"), Some(PositionCommand::Close(d("0.25"))));
    assert_eq!(PositionCommand::parse("закрой позицию"), Some(PositionCommand::Close(Decimal::ONE)));
    assert_eq!(PositionCommand::parse("отмени лимит"), Some(PositionCommand::CancelOrders));
    assert_eq!(PositionCommand::parse("стоп 3%"), None); // a plan edit, not a command
    assert_eq!(PositionCommand::parse("лонг ETH риск $10"), None);
}

#[test]
fn money_commands_are_recognised() {
    assert_eq!(p("выведи 2 USDC в Solana"), Some(WalletCommand::WithdrawToSolana(some("2"))));
    assert_eq!(p("верни всё в солану"), Some(WalletCommand::WithdrawToSolana(None)));
    assert_eq!(p("положи 1,5 USDC в хранилище"), Some(WalletCommand::LendDeposit("USDC".into(), some("1.5"))));
    assert_eq!(p("сними из хранилища"), Some(WalletCommand::LendWithdraw("USDC".into(), None)));
    assert_eq!(p("положи 0.005 SOL в хранилище"), Some(WalletCommand::LendDeposit("SOL".into(), some("0.005"))));
    assert_eq!(p("сними SOL из хранилища"), Some(WalletCommand::LendWithdraw("SOL".into(), None)));
    assert_eq!(p("положи 50 USDC в kamino"), Some(WalletCommand::Kamino { deposit: true, symbol: "USDC".into(), amount: some("50") }));
    assert_eq!(p("застейкай 0.01 SOL"), Some(WalletCommand::Stake(some("0.01"))));
    assert_eq!(p("анстейк"), Some(WalletCommand::Unstake(None)));
    assert_eq!(p("где лучше держать USDC?"), Some(WalletCommand::Yields(Some("USDC".into()))));
    assert_eq!(p("какая доходность"), Some(WalletCommand::Yields(None)));
    assert_eq!(p("разложи USDC по лучшим ставкам"), Some(allocate("USDC", None, None, None)));
    assert_eq!(p("распредели SOL по протоколам, не больше 70% в один"), Some(allocate("SOL", some("70"), None, None)));
    // Where the money comes from: only that network's («закинь 5 долларов из бейз» took Solana's 1.69 too).
    assert_eq!(p("spread 5 USDC from Base across the best rates"), Some(allocate("USDC", None, some("5"), Some("base"))));
    assert_eq!(p("spread 5 USDC from Solana across the best rates"), Some(allocate("USDC", None, some("5"), Some("solana"))));
    assert_eq!(p("разложи 1000 USDC с базы по лучшим ставкам"), Some(allocate("USDC", None, some("1000"), Some("base"))));
    assert_eq!(p("spread 5 USDC from Base across the best rates").unwrap().summary(), "spread 5 USDC from Base across the best rates");
    assert_eq!(p("переложи в лучшие ставки"), Some(WalletCommand::Rebalance { max_share: None }));
    assert_eq!(p("rebalance my USDC, max 40% per protocol"), Some(WalletCommand::Rebalance { max_share: some("40") }));
    assert_eq!(p("перебалансируй депозиты"), Some(WalletCommand::Rebalance { max_share: None }));
    assert_ne!(p("переложи 5 USDC в Kamino"), Some(WalletCommand::Rebalance { max_share: None }));
    assert_eq!(p("лонг ETH риск $1 стоп 3%, из Solana 1.5 USDC"), None); // a plan, not a transfer
}

#[test]
fn spot_commands_use_verified_mints() {
    assert_eq!(p("купи SOL на 2 USDC"), Some(WalletCommand::Buy { asset: sol(), usdc: some("2") }));
    assert_eq!(p("купи биткоин на $5"), Some(WalletCommand::Buy { asset: btc(), usdc: some("5") }));
    assert_eq!(p("продай 0,005 SOL"), Some(WalletCommand::Sell { asset: sol(), amount: some("0.005") }));
    assert_eq!(p("продай весь эфир"), Some(WalletCommand::Sell { asset: eth(), amount: None }));
    assert_eq!(p("купи PEPE на 5 USDC"), None); // not on the verified list
    assert_eq!(btc().mint, "cbbtcf3aa214zXHbiAZQwf4122FBYbraNdFqgw4iMij");
}

#[test]
fn regular_purchases_parse_and_advance_past_a_pause() {
    let schedule = |asset, usdc: &str, days| Some(WalletCommand::Schedule { asset, usdc: d(usdc), days });
    assert_eq!(p("покупай SOL на 10 USDC каждую неделю"), schedule(sol(), "10", 7));
    assert_eq!(p("покупай биткоин на $5 каждый день"), schedule(btc(), "5", 1));
    assert_eq!(p("покупай ETH на 20 USDC каждые 3 дня"), schedule(eth(), "20", 3));
    assert_eq!(p("останови покупки"), Some(WalletCommand::StopSchedules));
    assert_eq!(p("купи SOL на 2 USDC"), Some(WalletCommand::Buy { asset: sol(), usdc: some("2") })); // one purchase, not a schedule
    let start = unix(0.0);
    let mut s = Schedule::new("SOL", d("10"), 7, start);
    s.advance(start + TimeDelta::days(20)); // app closed for 20 days
    assert_eq!(s.next, start + TimeDelta::days(21)); // one catch-up, then weekly
}

/// Swift's synthesized Codable: its keys, and `paused` left out while nil.
#[test]
fn a_schedule_keeps_swifts_shape() {
    let s = Schedule::new("SOL", d("10.5"), 7, unix(978_307_200.0));
    let json: serde_json::Value = serde_json::to_value(&s).unwrap();
    assert_eq!(json, serde_json::json!({"symbol": "SOL", "usdc": 10.5, "days": 7, "next": 0.0}));
    let read: Schedule = serde_json::from_str(r#"{"symbol":"BTC","usdc":5,"days":1,"next":-978307200,"paused":true}"#).unwrap();
    assert!(read.paused == Some(true) && read.next == unix(0.0) && read.asset() == Some(btc()));
}

#[test]
fn a_route_step_edits_the_amount_the_command_holds() {
    let buy = WalletCommand::BuyOn { place: "base".into(), token: "AERO".into(), usdc: some("2"), from: None };
    assert_eq!(buy.editable_amount(), some("2"));
    assert_eq!(buy.with_amount(d("3")).editable_amount(), some("3"));
    assert_eq!(
        WalletCommand::Aave { deposit: true, amount: some("5"), network: "arbitrum".into() }.with_amount(d("7")),
        WalletCommand::Aave { deposit: true, amount: some("7"), network: "arbitrum".into() }
    );
    assert_eq!(WalletCommand::SendSol { name: "Test".into(), amount: d("1") }.editable_amount(), None); // a transfer's amount isn't edited from a card
}

/// «All there is» when more is asked than held (shortChoices): the same move for what's there, said as words the chat
/// reads back as that move — the card's command goes through the parser again.
#[test]
fn all_there_is_reads_back_as_the_same_move() {
    let moves = [
        WalletCommand::Buy { asset: SpotAsset::new("SOL", "So11111111111111111111111111111111111111112", 9), usdc: some("150") },
        WalletCommand::SwapOn { place: "base".into(), from: "ETH".into(), to: "AERO".into(), amount: some("1") },
        cross("SOL", "solana", "ETH", "arbitrum", some("2")),
        WalletCommand::BuyOn { place: "base".into(), token: "AERO".into(), usdc: some("50"), from: Some("base".into()) },
    ];
    for m in moves {
        let all = m.with_amount(d("0.5"));
        assert_ne!(all, m);
        assert_eq!(p(&all.summary()), Some(all.clone()), "{}", all.summary());
    }
}

// MARK: JupiterDCATests

#[test]
fn jupiter_dca_reads_the_router_form_and_plain_words() {
    assert_eq!(p("Jupiter DCA 100 USDC into SOL in 2 orders every day"), Some(dca(sol(), some("100"), Some(2), 86400)));
    assert_eq!(p("jupiter dca 300 USDC into BTC over 6 weeks"), Some(dca(btc(), some("300"), Some(6), 604_800)));
    // The swaps spread over the time named.
    assert_eq!(p("jupiter dca 200 USDC into SOL in 4 orders over 2 days"), Some(dca(sol(), some("200"), Some(4), 43200)));
    assert_eq!(p("jupiter dca 120 USDC into SOL in 2 orders every 3 days"), Some(dca(sol(), some("120"), Some(2), 259_200)));
    // «jupiter» itself is not JUP; JUP named is.
    assert_eq!(p("Jupiter DCA 500 USDC into JUP"), Some(dca(jup(), some("500"), None, 86400)));
    assert_eq!(p("dca on jupiter into SOL"), Some(dca(sol(), None, None, 86400)));
    assert_eq!(p("сделай дса через юпитер на 100 usdc в sol каждую неделю"), Some(dca(sol(), some("100"), None, 604_800)));
    assert_eq!(p("Jupiter DCA 100 USDC into BONK in 2 orders every day"), Some(dca(SpotAsset::by_symbol("BONK"), some("100"), Some(2), 86400)));
    assert_eq!(p("cancel Jupiter DCA SOL"), Some(WalletCommand::JupiterDcaCancel(Some(sol()))));
    assert_eq!(p("cancel Jupiter DCA"), Some(WalletCommand::JupiterDcaCancel(None)));
    assert_eq!(p("show Jupiter DCA"), Some(WalletCommand::JupiterDcaOrders));
    // The app's own recurring buys keep their words.
    assert_eq!(p("buy SOL for 10 USDC every week"), Some(WalletCommand::Schedule { asset: sol(), usdc: d("10"), days: 7 }));
    assert_eq!(p("stop recurring buys"), Some(WalletCommand::StopSchedules));
}

// pending: jupiterDCASummaryReadsBackAndMinimumsHold: Modules().refusal (round 3).
#[test]
fn jupiter_dca_summary_reads_back_and_minimums_hold() {
    let commands = [
        dca(sol(), some("100"), Some(2), 86400), dca(btc(), some("400"), Some(4), 604_800), dca(jup(), some("150"), None, 3600),
        WalletCommand::JupiterDcaCancel(Some(sol())), WalletCommand::JupiterDcaCancel(None), WalletCommand::JupiterDcaOrders,
    ];
    for c in commands {
        let back = p(&c.summary());
        if let WalletCommand::JupiterDca { asset, usdc, orders: None, every } = &c {
            assert_eq!(back, Some(dca(asset.clone(), *usdc, Some(WalletCommand::jupiter_dca_orders(usdc.unwrap())), *every)));
        } else {
            assert_eq!(back, Some(c.clone()), "{}", c.summary());
        }
    }
    assert!(WalletCommand::jupiter_dca_orders(d("100")) == 2 && WalletCommand::jupiter_dca_orders(d("175")) == 3 && WalletCommand::jupiter_dca_orders(d("5000")) == 10);
    assert_eq!(WalletCommand::jupiter_dca_problem(d("100"), 2), None);
    assert!(WalletCommand::jupiter_dca_problem(d("60"), 2).unwrap().contains("at least 100"));
    assert!(WalletCommand::jupiter_dca_problem(d("100"), 3).unwrap().contains("2 swaps at most"));
    assert!(WalletCommand::jupiter_dca_problem(d("200"), 1).unwrap().contains("at least 2"));
    assert!(WalletCommand::every_text(86400) == "day" && WalletCommand::every_text(172_800) == "2 days" && WalletCommand::every_text(43200) == "12 hours");
}

// pending: jupiterDCAFormsReachTheWalletReaderFirst: FormsMatrixTests' `read` (Normalize round).

// MARK: TokenSendTests

#[test]
fn any_token_transfer_parses() {
    assert_eq!(p("send all RAY on solana to Bybit"), Some(send_token("Bybit", "RAY", None, Some("solana"))));
    assert_eq!(p("send 1973.719299 AI on robinhood to New wallet"), Some(send_token("New wallet", "AI", some("1973.719299"), Some("robinhood"))));
    assert_eq!(p("send all jlUSDC to New wallet"), Some(send_token("New wallet", "jlUSDC", None, None)));
    assert_eq!(p("send 0.001 ETH on base to Mom"), Some(send_token("Mom", "ETH", some("0.001"), Some("base"))));
    assert_eq!(p("send all USDC on base to New wallet"), Some(send_token("New wallet", "USDC", None, Some("base"))));
    assert_eq!(
        p("send all 6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx to Bybit"),
        Some(send_token("Bybit", "6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx", None, None))
    );
    assert_eq!(p("отправь все RAY на солане на New wallet"), Some(send_token("New wallet", "RAY", None, Some("solana"))));
    assert_eq!(p("переведи 0,5 TSLA в робинхуде на Маму"), Some(send_token("Маму", "TSLA", some("0.5"), Some("robinhood"))));
}

#[test]
fn any_token_transfer_leaves_the_other_forms_alone() {
    // USDC with a number and a network is the USDC transfer; without a network it's parsed as before.
    assert_eq!(p("send 5 USDC on base to Mom"), Some(WalletCommand::SendTo { name: "Mom".into(), amount: d("5"), network: Some("base".into()) }));
    assert_eq!(p("send 5 USDC to Mom"), Some(WalletCommand::SendTo { name: "Mom".into(), amount: d("5"), network: None }));
    assert_eq!(p("send 0.01 SOL to Bybit"), Some(WalletCommand::SendSol { name: "Bybit".into(), amount: d("0.01") }));
    assert_eq!(p("send all NEAR to Bybit"), Some(WalletCommand::SendNear { name: "Bybit".into(), amount: None }));
    // A pasted address is never a recipient; «all the money» names no token; our own places aren't names.
    assert!(!matches!(p("send all RAY to 0x1234567890abcdef1234567890abcdef12345678"), Some(WalletCommand::SendToken { .. })), "pasted address");
    assert!(!matches!(p("send all the money to Bybit"), Some(WalletCommand::SendToken { .. })), "no token");
    assert!(!matches!(p("send all RAY to the vault"), Some(WalletCommand::SendToken { .. })), "a place");
}

#[test]
fn any_token_transfer_reads_back_from_its_summary() {
    for command in [
        send_token("New wallet", "AI", some("1973.719299"), Some("robinhood")),
        send_token("Bybit", "RAY", None, Some("solana")),
        send_token("New wallet", "gtUSDCp", some("133.242014"), Some("base")),
    ] {
        assert_eq!(p(&command.summary()), Some(command.clone()), "{}", command.summary());
    }
}

// pending: aTransferPicksTheTokenItNames: Holdings.pick (a later round); nearShortOfBalanceIsSaidWithWhatToDo: Near (round 3).

// MARK: MultisigCommandTests

/// The multisig's forms parse as the chat writes them, round-trip through `summary`, and leave the Jupiter vault alone.
// pending: multisigFormsParse: ChatRouter.parses and ChatRouter.forms (Normalize round).
#[test]
fn multisig_forms_parse() {
    let create = |threshold: Option<i64>, of: Option<i64>, keys: &[&str], networks: &[&str]| {
        Some(WalletCommand::MultisigCreate { threshold, of, keys: super::strings(keys), networks: super::strings(networks) })
    };
    assert_eq!(p("make a multisig"), create(None, None, &[], &[]));
    assert_eq!(p("make a multisig 2 of 3 with Phone on arbitrum, base"), create(Some(2), Some(3), &["Phone"], &["arbitrum", "base"]));
    assert_eq!(p("create a multisig 3 of 4 with Phone, Mom and Office Mac on bnb and ethereum"), create(Some(3), Some(4), &["Phone", "Mom", "Office Mac"], &["bnb", "ethereum"]));
    assert_eq!(p("make a multisig on hyperliquid"), None); // not an EVM network a multisig is made on
    assert_eq!(
        p("send 200 USDC from the multisig to Alex on base"),
        Some(WalletCommand::MultisigSend { token: "USDC".into(), amount: some("200"), to: "Alex".into(), network: Some("base".into()) })
    );
    assert_eq!(p("send all ETH from the multisig to Old wallet"), Some(WalletCommand::MultisigSend { token: "ETH".into(), amount: None, to: "Old wallet".into(), network: None }));
    assert_eq!(p("show the multisig"), Some(WalletCommand::MultisigShow));
    for c in ["make a multisig 2 of 3 with Phone on arbitrum, base", "send 0.5 ETH from the multisig to Alex on arbitrum", "show the multisig"] {
        assert_eq!(p(&p(c).unwrap().summary()), p(c), "{c}");
    }
    // «the vault» is still Jupiter Lend's.
    assert_ne!(p("send 1 SOL to the vault"), Some(WalletCommand::MultisigShow));
    assert!(!matches!(p("lend 5 USDC in the vault"), Some(WalletCommand::MultisigSend { .. })), "a Jupiter deposit read as a multisig move");
}

// MARK: USDTMoveCommandTests

/// USDT between USDT0's networks: Relay's swap unless LayerZero is named; each form reads back from its own summary.
// pending: usdtMoveFormsParse: Modules().refusal (round 3).
#[test]
fn usdt_move_forms_parse() {
    assert_eq!(p("move 50 USDT from arbitrum to optimism"), Some(usdt("arbitrum", "optimism", some("50"), false)));
    assert_eq!(p("move 50 USDT from Arbitrum to Optimism via LayerZero"), Some(usdt("arbitrum", "optimism", some("50"), true)));
    assert_eq!(p("move all USDT from polygon to ethereum via LayerZero"), Some(usdt("polygon", "ethereum", None, true)));
    assert_eq!(p("move 2.5 USDT from hyperevm to arbitrum via USDT0"), Some(usdt("hyperevm", "arbitrum", some("2.5"), true)));
    for c in [usdt("ethereum", "polygon", some("7"), true), usdt("polygon", "hyperevm", None, false)] { assert_eq!(p(&c.summary()), Some(c)) }
    // Optimism's own «USDT» is the old bridged token: a plain move from there is Relay's swap of that one.
    assert_eq!(p("move 5 USDT from optimism to arbitrum"), Some(cross("USDT", "optimism", "USDT", "arbitrum", some("5"))));
    assert_eq!(p("move 5 USDT from optimism to arbitrum via LayerZero"), Some(usdt("optimism", "arbitrum", some("5"), true)));
    // Off USDT0's networks it's Relay's swap of USDT; named there all the same, the bridge says where it runs.
    assert_eq!(p("move 5 USDT from base to arbitrum"), Some(cross("USDT", "base", "USDT", "arbitrum", some("5"))));
    assert_eq!(p("move 5 USDT from base to arbitrum via LayerZero"), Some(usdt("base", "arbitrum", some("5"), true)));
    // USDC keeps its own forms.
    assert_eq!(p("move 5 USDC from base to arbitrum"), Some(WalletCommand::Bridge { from: "base".into(), to: "arbitrum".into(), amount: some("5") }));
    assert_eq!(p("move 5 USDC from base to arbitrum via CCTP"), Some(WalletCommand::Cctp { from: "base".into(), to: "arbitrum".into(), amount: some("5") }));
}

// MARK: The WalletCommand lines of other suites

/// ScheduledTaskTests.rewardSweepsParseAndScheduleHourly (its task half is in tests/scheduled_task.rs).
#[test]
fn reward_sweeps_parse() {
    let sweep = |min: Option<Decimal>, into: Option<&str>| Some(WalletCommand::SweepRewards { min_usd: min, into: into.map(Into::into) });
    assert_eq!(p("sweep rewards over $10 into USDC"), sweep(some("10"), None));
    assert_eq!(p("sweep rewards into WETH"), sweep(None, Some("WETH")));
    assert_eq!(p("sell my rewards for usdc"), sweep(None, None));
    assert_eq!(p("продавай награды в USDC, когда наберётся $10"), sweep(some("10"), None));
    assert_eq!(p("claim rewards"), Some(WalletCommand::ClaimRewards)); // claiming alone keeps them
}

/// ScheduledTaskTests.parsedTextIsKept: the same answer every time, a «no command» included. (Swift keeps the answers
/// to save compiling ~150 regexes; foundation keeps the compiled regexes instead, so there is no timing to check.)
#[test]
fn parsed_text_is_kept() {
    let step = format!("swap 5 USDC on base for WETH on base {}", crate::foundation::Uuid::new());
    assert_eq!(p(&step), p(&step));
    let nothing = format!("just a note {}", crate::foundation::Uuid::new());
    assert_eq!(p(&nothing), None);
    assert_eq!(p(&nothing), None);
    assert_eq!(p("buy SOL for 5 USDC"), p("buy SOL for 5 USDC"));
}

/// AgentCommandsTests.contractCallsReadTheirSignatureArgumentsCoinsAndApprove.
// pending: its ChatRouter.parses and ChatRouter.addressesAllowed lines (Normalize round).
#[test]
fn contract_calls_read_their_signature_arguments_coins_and_approve() {
    let vault = "0x1A996cb54bb95462040408C06122D45D6Cdb6096";
    let c = ContractCall::parse(&format!("call arbitrum {vault} deposit(uint256,address) 10e6 me approving 10 USDC"));
    assert_eq!(c, Some(ContractCall {
        network: "arbitrum".into(), contract: vault.into(), function: "deposit(uint256,address)".into(), args: super::strings(&["10e6", "me"]), value: None,
        approve: Some(Approve { amount: d("10"), token: "USDC".into() }),
    }));
    let lido = ContractCall::parse("call ethereum 0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84 submit(address) 0x0000000000000000000000000000000000000000 with 0.001 ETH").unwrap();
    assert!(lido.value == some("0.001") && lido.args.len() == 1 && lido.approve.is_none());
    assert_eq!(ContractCall::parse(&c.as_ref().unwrap().text()), c);
    assert_eq!(ContractCall::parse(&format!("call solana {vault} deposit(uint256) 1")), None);
    assert_eq!(
        p(&format!("call base {vault} claim()")),
        Some(WalletCommand::ContractCall(ContractCall { network: "base".into(), contract: vault.into(), function: "claim()".into(), args: vec![], value: None, approve: None }))
    );
}

/// FundingPlanTests: each leg is a command the chat reads back as the right move.
#[test]
fn funding_legs_read_back_as_the_right_moves() {
    assert_eq!(p("move 100 USDC from Base to Robinhood"), Some(WalletCommand::Bridge { from: "base".into(), to: "robinhood".into(), amount: some("100") }));
    assert_eq!(p("withdraw 194 USDC to Solana"), Some(WalletCommand::WithdrawToSolana(some("194"))));
    assert_eq!(p("move 201 USDC from Solana to Robinhood"), Some(WalletCommand::SolanaTo("robinhood".into(), some("201"))));
    assert_eq!(p("sell 3.74 NEAR"), Some(WalletCommand::SellNear { amount: some("3.74"), to: None }));
}

/// A buy run again as a chain's last step reads back from its own words, thousands and a contract included.
#[test]
fn a_buy_by_contract_reads_back_from_its_summary() {
    let c = WalletCommand::BuyStock { symbol: "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18".into(), usdc: some("1722") };
    assert_eq!(p(&c.summary()), Some(c.clone()), "{}", c.summary());
}

/// ChartTests.favoriteTokenPhrases: «купи 0x… токен на $2» is a trade, not a favorite (its TokenCommand lines are in
/// tests/tokens.rs).
#[test]
fn a_token_by_contract_is_a_trade() {
    let ai = "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18";
    assert_eq!(p(&format!("купи {ai} токен на $2")), Some(WalletCommand::BuyStock { symbol: ai.into(), usdc: some("2") }));
    assert_eq!(p(&format!("sell 3 tokens {ai}")), Some(WalletCommand::SellStock { symbol: ai.into(), amount: some("3") }));
    assert_ne!(p(&format!("send 2 USDC to {ai}")), Some(WalletCommand::BuyStock { symbol: ai.into(), usdc: some("2") }));
}

/// ExecutorTests.aShareBelowOneStepIsRefusedNotAFullClose: the shares its closes start from.
// pending: Executor.partialCloseSize itself (Execution/Trade).
#[test]
fn close_shares_parse() {
    for text in ["close 5% BTC", "закрой 5% BTC"] { assert_eq!(PositionCommand::parse(text), Some(PositionCommand::Close(d("0.05"))), "{text}") }
    for text in ["close half", "закрой половину"] { assert_eq!(PositionCommand::parse(text), Some(PositionCommand::Close(d("0.5"))), "{text}") }
}

/// GasTopUpTests.gasSendsParseAsTheyAreTyped: «send <amount> <coin> on <network> to <wallet>», as suggestGas writes them.
#[test]
fn gas_sends_parse_as_they_are_typed() {
    let (sol, eth) = (d("0.013"), d("0.000019"));
    assert_eq!(p(&format!("send {} SOL on solana to Automation", Fmt::num(sol, 9))), Some(WalletCommand::SendSol { name: "Automation".into(), amount: sol }));
    assert_eq!(p(&format!("send {} ETH on base to Automation", Fmt::num(eth, 9))), Some(send_token("Automation", "ETH", Some(eth), Some("base"))));
    assert_eq!(p(&format!("send {} POL on polygon to Automation", Fmt::num(eth, 9))), Some(send_token("Automation", "POL", Some(eth), Some("polygon"))));
}

/// KeyWalletTests: every send it asks a key wallet about is a command.
// pending: Workspace.missingKey (the window's engine).
#[test]
fn key_wallet_sends_parse() {
    for text in [
        "send 5 USDC on base to Mom", "send 0.001 ETH on base to Mom", "send all RAY on solana to Bybit", "send 5 USDC to Mom", "отправь 5 usdc на Маму в сети полигон",
        "переведи 0,5 TSLA в робинхуде на Маму", "отправь все RAY на солане на New wallet", "call arbitrum 0x1A996cb54bb95462040408C06122D45D6Cdb6096 deposit(uint256,address) 10e6 me",
        "send 5 USDC to Nobody", "send 0.001 BTC to Bybit", "send 0.001 BTC on base to Bybit",
    ] {
        assert!(p(text).is_some(), "{text}");
    }
}

/// KaminoBorrowTests: the four Backpack stocks by ticker and mint.
// pending: their match with Kamino.backpackStocks and Kamino.market (DeFi).
#[test]
fn kamino_stocks_have_their_mints() {
    assert_eq!(KAMINO_STOCK_MINTS.map(|(k, _)| k), KAMINO_STOCKS);
    assert!(KAMINO_STOCK_MINTS.iter().all(|(k, mint)| mint.starts_with(k)));
}

/// What a waiting card's «How much?» takes: its unit, and at least the asset's smallest unit.
#[test]
fn an_amount_answer_fits_its_question() {
    let stake = WalletCommand::Stake(None);
    let deposit = WalletCommand::LendDeposit("USDC".into(), None);
    assert_eq!(WalletCommand::amount_answer_for("0.5 sol", &stake), some("0.5"));
    assert_eq!(WalletCommand::amount_answer_for("$5", &stake), None);
    assert_eq!(WalletCommand::amount_answer_for("на 5 долларов", &stake), None);
    assert_eq!(WalletCommand::amount_answer_for("1 sol", &deposit), None);
    assert_eq!(WalletCommand::amount_answer_for("да, 2", &deposit), some("2"));
    assert_eq!(WalletCommand::amount_answer_for("0.0000001", &deposit), None);
    assert_eq!(WalletCommand::amount_answer("$5 sol"), None);
    assert_eq!(WalletCommand::amount_answer("1,000"), some("1000"));
    assert_eq!(stake.amount_question().as_deref(), Some("How much SOL to stake?"));
    assert_eq!(
        WalletCommand::ReceiveAcross { network: "xrp".into(), token: "XRP".into(), amount: None }.amount_question().as_deref(),
        Some("How much XRP will you send from XRP Ledger?")
    );
}
