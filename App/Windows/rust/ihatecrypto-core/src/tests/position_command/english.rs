//! EnglishCommandTests, PhraseTableTests, SwapEverywhereTests, BNBTests, HyperEVMTests, StablecoinsTests and TempoTests:
//! their WalletCommand, PositionCommand, IntentParser, SpotAsset and Fmt vectors. The helpers below are Swift's case
//! constructors with every argument spelled out, in Swift's order.

use chrono::Utc;
use rust_decimal::Decimal;

use super::super::{d, set, strings};
use crate::address_book_command::AddressBookCommand;
use crate::alerts::AlertCommand;
use crate::intent::{Fmt, IntentParser, Side, TradeIntent};
use crate::position_command::{PositionCommand, SpotAsset, WalletCommand as W};
use crate::scheduled_task::ScheduledTask;
use crate::stablecoins;
use crate::tokens::TokenCommand;

fn some(s: &str) -> Option<Decimal> { Some(d(s)) }

/// `#expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")`.
fn reads_back(c: &W) { assert_eq!(W::parse(&c.summary()), Some(c.clone()), "{}", c.summary()) }

fn kamino(deposit: bool, symbol: &str, amount: Option<Decimal>) -> W { W::Kamino { deposit, symbol: symbol.into(), amount } }

fn allocate(symbol: &str, max_share: Option<Decimal>, amount: Option<Decimal>, without: &[&str], from: Option<&str>) -> W {
    W::Allocate { symbol: symbol.into(), max_share, amount, without: strings(without), from: from.map(Into::into) }
}

fn aave(deposit: bool, amount: Option<Decimal>, network: &str) -> W { W::Aave { deposit, amount, network: network.into() } }

fn morpho(deposit: bool, amount: Option<Decimal>, network: &str) -> W { W::Morpho { deposit, amount, network: network.into() } }

fn send_to(name: &str, amount: Decimal, network: Option<&str>) -> W { W::SendTo { name: name.into(), amount, network: network.map(Into::into) } }

fn buy_on(place: &str, token: &str, usdc: Option<Decimal>, from: Option<&str>) -> W {
    W::BuyOn { place: place.into(), token: token.into(), usdc, from: from.map(Into::into) }
}

fn sell_on(place: &str, token: &str, amount: Option<Decimal>, keep: bool) -> W { W::SellOn { place: place.into(), token: token.into(), amount, keep } }

fn swap_on(place: &str, from: &str, to: &str, amount: Option<Decimal>) -> W { W::SwapOn { place: place.into(), from: from.into(), to: to.into(), amount } }

fn cross_swap(from: &str, on: &str, to: &str, at: &str, amount: Option<Decimal>) -> W {
    W::CrossSwap { from: from.into(), on: on.into(), to: to.into(), at: at.into(), amount }
}

fn bridge(from: &str, to: &str, amount: Option<Decimal>) -> W { W::Bridge { from: from.into(), to: to.into(), amount } }

fn gas(place: &str, usd: Option<Decimal>) -> W { W::Gas { place: place.into(), usd } }

fn save(address: &str, name: &str, network: Option<&str>) -> Option<AddressBookCommand> {
    Some(AddressBookCommand::Save { address: address.into(), name: name.into(), network: network.map(Into::into) })
}

// EnglishCommandTests.swift.
// The app is in English; the parsers read English as well as Russian. Other languages go through the model,
// which rewrites them into these same English forms (ChatRouter.forms).

#[test]
fn english_trade_phrases() {
    let a = IntentParser::apply("long ETH risk $10 stop 3% take 6% leverage 2x", &TradeIntent::default(), &set(&[])).unwrap();
    assert!(
        a.side == Side::Long && a.asset == "ETH" && a.risk_usd == some("10") && a.stop_pct == some("3") && a.take_pct == some("6") && a.leverage == some("2"),
        "{a:?}"
    );

    let b = IntentParser::apply("short oil on a 2% pullback by the book", &TradeIntent::default(), &set(&["BTC", "xyz:CL"])).unwrap();
    assert!(b.side == Side::Short && b.asset == "xyz:CL" && b.pullback_pct == some("2") && b.auto_rules == Some(true), "{b:?}");

    let c = IntentParser::apply("long BTC take 2R take in parts trailing 2%", &TradeIntent::default(), &set(&["BTC"])).unwrap();
    assert!(c.take_r == some("2") && c.partial_take == Some(true) && c.trailing_pct == some("2"), "{c:?}");
}

#[test]
fn english_position_commands() {
    assert_eq!(PositionCommand::parse("move stop to breakeven"), Some(PositionCommand::StopToBreakeven));
    assert_eq!(PositionCommand::parse("stop at 93.4"), Some(PositionCommand::StopTo(d("93.4"))));
    assert_eq!(PositionCommand::parse("take at 88"), Some(PositionCommand::TakeTo(d("88"))));
    assert_eq!(PositionCommand::parse("close half"), Some(PositionCommand::Close(d("0.5"))));
    assert_eq!(PositionCommand::parse("close"), Some(PositionCommand::Close(d("1"))));
    assert_eq!(PositionCommand::parse("cancel the limit order"), Some(PositionCommand::CancelOrders));
    assert_eq!(PositionCommand::parse("take closer"), None);
}

#[test]
fn english_wallet_commands() {
    let (sol, btc) = (SpotAsset::all()[0].clone(), SpotAsset::all()[1].clone());
    assert_eq!(W::parse("withdraw 1 USDC to Solana"), Some(W::WithdrawToSolana(some("1"))));
    assert_eq!(W::parse("lend 1 USDC"), Some(W::LendDeposit("USDC".into(), some("1"))));
    assert_eq!(W::parse("withdraw USDC from the vault"), Some(W::LendWithdraw("USDC".into(), None)));
    assert_eq!(W::parse("deposit 50 USDC to kamino"), Some(kamino(true, "USDC", some("50"))));
    assert_eq!(W::parse("stake 0.01 SOL"), Some(W::Stake(some("0.01"))));
    assert_eq!(W::parse("unstake 0.005 JitoSOL"), Some(W::Unstake(some("0.005"))));
    assert_eq!(W::parse("where to keep USDC"), Some(W::Yields(Some("USDC".into()))));
    assert_eq!(W::parse("deposit 5 USDC to HLP"), Some(W::Hlp { deposit: true, amount: some("5") }));
    assert_eq!(W::parse("spread USDC across the best rates, max 70% per protocol"), Some(allocate("USDC", some("70"), None, &[], None)));
    // A limit: at most that much of the free USDC.
    assert_eq!(W::parse("spread 2 USDC across the best rates"), Some(allocate("USDC", None, some("2"), &[], None)));
    assert_eq!(W::parse("разложи 2 USDC по лучшим ставкам, не больше 70% в один"), Some(allocate("USDC", some("70"), some("2"), &[], None)));
    assert_eq!(W::parse("spread 0.5 SOL across the best rates"), Some(allocate("SOL", None, some("0.5"), &[], None)));
    // «а без kamino можно?»: a spread leaves protocols out, and its summary reads back the same.
    let without = allocate("USDC", some("34"), some("900"), &["kamino", "aave"], None);
    assert_eq!(W::parse("spread 900 USDC across the best rates max 34% per protocol without kamino, aave"), Some(without.clone()));
    assert_eq!(W::parse(&without.summary()), Some(without));
    assert_eq!(W::parse("разложи 900 USDC по лучшим ставкам без камино"), Some(allocate("USDC", None, some("900"), &["kamino"], None)));
    assert_eq!(W::parse("spread 2 USDC across the best rates").map(|c| c.summary()).as_deref(), Some("spread 2 USDC across the best rates"));
    assert_eq!(W::parse("buy SOL for 2 USDC"), Some(W::Buy { asset: sol.clone(), usdc: some("2") }));
    assert_eq!(W::parse("buy SOL for 2 USDC every week"), Some(W::Schedule { asset: sol.clone(), usdc: d("2"), days: 7 }));
    assert_eq!(W::parse("buy BTC for 5 USDC every day"), Some(W::Schedule { asset: btc, usdc: d("5"), days: 1 }));
    assert_eq!(W::parse("stop recurring buys"), Some(W::StopSchedules));
    assert_eq!(W::parse("sell 0.1 SOL"), Some(W::Sell { asset: sol, amount: some("0.1") }));
    assert_eq!(W::parse("купи неар на 3 бакса"), Some(W::BuyNear { usdc: some("3") }));
    assert_eq!(W::parse("buy NEAR for 5 USDC"), Some(W::BuyNear { usdc: some("5") }));
    assert_eq!(W::parse("купи NEAR"), Some(W::BuyNear { usdc: None }));
    assert_eq!(W::parse("buy 2 NEAR"), Some(W::BuyNear { usdc: None }));   // coins, not dollars: asks
    assert_ne!(W::parse("buy SOL near 150"), Some(W::BuyNear { usdc: None }));
    let near = W::parse("buy ETH near 2500 for 3 USDC");
    assert!(near.is_none() || near != Some(W::BuyNear { usdc: some("3") }), "{near:?}");
    // Russian keeps working.
    assert_eq!(W::parse("выведи 1 USDC в Solana"), Some(W::WithdrawToSolana(some("1"))));
}

#[test]
fn every_withdrawal_takes_an_amount() {
    // «выведи 3 USDC из камино» drew «only everything can come out»: the forms said no N.
    assert_eq!(W::parse("withdraw 3 USDC from kamino"), Some(kamino(false, "USDC", some("3"))));
    assert_eq!(W::parse("выведи 3 USDC из камино"), Some(kamino(false, "USDC", some("3"))));
    assert_eq!(W::parse("withdraw 3 USDC from HLP"), Some(W::Hlp { deposit: false, amount: some("3") }));
    assert_eq!(W::parse("withdraw 3 USDC from Aave on Arbitrum"), Some(aave(false, some("3"), "arbitrum")));
    assert_eq!(W::parse("withdraw 3 USDC from Morpho"), Some(morpho(false, some("3"), "base")));
    assert_eq!(W::parse("withdraw 3 USDC from the vault"), Some(W::LendWithdraw("USDC".into(), some("3"))));
    // pending: everyWithdrawalTakesAnAmount: ChatRouter.forms contains each «withdraw [N] … from …» form (Normalize round)
}

// pending: aCommandsOwnSectionsOfTheForms: ChatRouter.forms(for:), the whole test (Normalize round)

#[test]
fn router_forms_cover_the_parsers() {
    // Every form the model is told to write must parse locally, in the order the app tries the parsers.
    let ai = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18";
    let markets = set(&["BTC", "ETH", "SOL", "xyz:CL", "xyz:TSLA"]);
    for command in [
        "withdraw 1 USDC to Solana", "lend 1 USDC", "stake 0.1 SOL", "where to keep SOL", "stop recurring buys",
        "buy token {ai} for 2 USDC", "sell 3 tokens {ai}", "buy Tesla stock for 3 USDC", "sell TSLA stock",
        "move USDG from Robinhood to Solana", "buy NEAR for 5 USDC", "top up", "deposit 5 USDC to HLP",
        "lend 2 USDC on Aave", "deposit 2 USDC into Morpho", "withdraw USDC from Morpho",
        "spread USDC across the best rates max 50% per protocol", "rebalance USDC max 50% per protocol",
        "move 3 USDC from Solana to Robinhood", "move 2 USDC from Solana to Base", "move 1 USDC from Solana to Arbitrum",
    ] {
        let command = command.replace("{ai}", ai);
        assert!(W::parse(&command).is_some(), "{command}");
    }
    assert_eq!(W::parse("sell NEAR"), Some(W::SellNear { amount: None, to: None }));
    assert_eq!(W::parse("sell 0.5 NEAR"), Some(W::SellNear { amount: some("0.5"), to: None }));
    assert_eq!(W::parse("продай неар"), Some(W::SellNear { amount: None, to: None }));
    assert_eq!(W::parse("продай 0,3 неар"), Some(W::SellNear { amount: some("0.3"), to: None }));
    assert_ne!(W::parse("sell ETH near 2500"), Some(W::SellNear { amount: None, to: None }));
    // pending: routerFormsCoverTheParsers: ChatRouter.forms contains «sell [N] NEAR» (Normalize round)
    assert_eq!(TokenCommand::parse(&format!("watch token {ai} on robinhood")), Some(TokenCommand::Favorite { address: ai.into(), chain: Some("robinhood".into()) }));
    assert_eq!(TokenCommand::parse("remove token AI from favorites"), Some(TokenCommand::Unfavorite { symbol: "AI".into() }));
    // pending: routerFormsCoverTheParsers: ChartCommand.parse «chart ETH week» and «what's moving» (round 3)
    assert!(AlertCommand::parse("alert me when ETH is above 4000", &markets).is_some());
    assert_eq!(PositionCommand::parse("close half"), Some(PositionCommand::Close(d("0.5"))));
    assert_eq!(IntentParser::apply("short oil risk $1 by the book", &TradeIntent::default(), &markets).map(|i| i.asset).as_deref(), Some("xyz:CL"));
    // pending: routerFormsCoverTheParsers: ChatRouter.forms contains «buy token <0xCONTRACT> for N USDC» (Normalize round)
    // pending: routerFormsCoverTheParsers: PnLCommand.parse «profit and loss week for oil» (later round)
    // Native NEAR and SOL to saved addresses, and closing the NEAR account.
    assert_eq!(W::parse("send 0.05 NEAR to Bybit"), Some(W::SendNear { name: "Bybit".into(), amount: some("0.05") }));
    assert_eq!(W::parse("send all NEAR to Bybit"), Some(W::SendNear { name: "Bybit".into(), amount: None }));
    assert_eq!(W::parse("close the NEAR account into Bybit"), Some(W::CloseNear { name: "Bybit".into() }));
    assert_eq!(W::parse("send 0.001 SOL to Old wallet"), Some(W::SendSol { name: "Old wallet".into(), amount: d("0.001") }));
    // pending: routerFormsCoverTheParsers: ChatRouter.forms contains «send N SOL to <NAME from the address book>» (Normalize round)
}

#[test]
fn near_and_sol_transfers_parse() {
    assert_eq!(W::parse("отправь 0.05 NEAR на Bybit"), Some(W::SendNear { name: "Bybit".into(), amount: some("0.05") }));
    assert_eq!(W::parse("отправь весь NEAR на Мой кошелёк"), Some(W::SendNear { name: "Мой кошелёк".into(), amount: None }));
    assert_eq!(W::parse("закрой NEAR-аккаунт и переведи всё на Bybit"), Some(W::CloseNear { name: "Bybit".into() }));
    assert_eq!(W::parse("отправь 0,00002 SOL на старый кошелёк"), Some(W::SendSol { name: "старый кошелёк".into(), amount: d("0.00002") }));
    // Never guessed: no number and no «all» is not a NEAR transfer; SOL into a vault is a deposit.
    assert_ne!(W::parse("send NEAR to Bybit"), Some(W::SendNear { name: "Bybit".into(), amount: None }));
    assert_ne!(W::parse("send 1 SOL to the vault"), Some(W::SendSol { name: "the vault".into(), amount: d("1") }));
    // The other NEAR phrases keep working.
    assert_eq!(W::parse("продай весь NEAR"), Some(W::SellNear { amount: None, to: None }));
    assert_eq!(W::parse("send 5 USDC to Bybit"), Some(send_to("Bybit", d("5"), None)));
    // A NEAR account in a save command, and the router never inventing one.
    let implicit = "ab".repeat(32);
    assert_eq!(AddressBookCommand::parse(&format!("save {implicit} as Bybit on near")), save(&implicit, "Bybit", Some("near")));
    assert_eq!(AddressBookCommand::parse("save alice.near as Alice"), save("alice.near", "Alice", Some("near")));
    // pending: nearAndSolTransfersParse: !ChatRouter.addressesAllowed of an implicit NEAR account the user never wrote (Normalize round)
}

#[test]
fn moves_keep_their_direction() {
    // «из соланы» is out of Solana: it must never become a withdrawal to Solana (the bug in the user's screenshot).
    assert_eq!(W::parse("переведи 3 USDC из соланы в робингуд"), Some(W::SolanaTo("robinhood".into(), some("3"))));
    assert_eq!(W::parse("move 3 USDC from Solana to Robinhood"), Some(W::SolanaTo("robinhood".into(), some("3"))));
    assert_eq!(W::parse("закинь 2 USDC с соланы на base"), Some(W::SolanaTo("base".into(), some("2"))));
    assert_eq!(W::parse("move USDC from Solana to Arbitrum"), Some(W::SolanaTo("arbitrum".into(), None)));
    assert_eq!(W::parse("выведи 1 USDC в Solana"), Some(W::WithdrawToSolana(some("1"))));
    assert_eq!(W::parse("move 2 USDC from Base to Solana"), Some(W::BaseToSolana(some("2"))));
    assert_eq!(W::parse("верни USDG с робингуда на солану"), Some(W::RobinhoodToSolana(None)));
    assert!(W::SolanaTo("robinhood".into(), None).amount_question().is_some());
}

// pending: routerNeverTakesAnAddressFromTheModel: ChatRouter.addressesAllowed, addresses and decode, the whole test (Normalize round)
// takeProfitChoices: in tests/take_style.rs.

#[test]
fn weak_parses_go_to_the_model() {
    assert!(IntentParser::has_trade_words("long ETH risk $10"));
    assert!(IntentParser::has_trade_words("шорт нефти как учат"));
    assert!(IntentParser::has_trade_words("SOL 3x"));
    assert!(!IntentParser::has_trade_words("compra SOL por 1 USDC"));
    assert!(!IntentParser::has_trade_words("kaufe ETH für 5 USDC"));
}

#[test]
fn yields_for_both_assets() {
    assert_eq!(W::parse("where to keep USDC and SOL"), Some(W::Yields(None)));
    assert_eq!(W::parse("где лучше держать SOL"), Some(W::Yields(Some("SOL".into()))));
}

#[test]
fn named_asset_for_position_commands() {
    let markets = set(&["BTC", "ETH", "xyz:CL"]);
    assert_eq!(IntentParser::named_asset("close ETH", &markets).as_deref(), Some("ETH"));
    assert_eq!(IntentParser::named_asset("закрой нефть", &markets).as_deref(), Some("xyz:CL"));
    assert_eq!(IntentParser::named_asset("close", &markets), None);
    assert_eq!(IntentParser::named_asset("close half", &markets), None);
}

#[test]
fn questions_are_not_orders() {
    assert!(IntentParser::is_question("а где наш шорт по нефти?"));
    assert!(IntentParser::is_question("what happened to my ETH long"));
    assert!(IntentParser::is_question("сколько я заработал на нефти"));
    assert!(!IntentParser::is_question("шорт нефти как учат"));
    assert!(!IntentParser::is_question("long ETH risk $10"));
    assert!(!IntentParser::is_question("поставь тейк как советуют лучшие инвесторы"));
}

#[test]
fn everyday_words_are_not_markets() {
    let markets = set(&["ETH", "SOL", "MOVE", "NOT", "xyz:NOW", "xyz:CL"]);
    assert_eq!(IntentParser::named_asset("what is open right now?", &markets), None);
    assert_eq!(IntentParser::named_asset("move stop to breakeven", &markets), None);
    assert_eq!(IntentParser::named_asset("close NOW", &markets).as_deref(), Some("xyz:NOW"));
    assert_eq!(IntentParser::named_asset("long sol risk $5", &markets).as_deref(), Some("SOL"));
    assert_eq!(IntentParser::named_asset("short MOVE", &markets).as_deref(), Some("MOVE"));
    assert_eq!(IntentParser::named_asset("а где наш шорт по нефти?", &markets).as_deref(), Some("xyz:CL"));
}

#[test]
fn trailing_an_open_position() {
    assert_eq!(PositionCommand::parse("trail 2%"), Some(PositionCommand::Trail(d("2"))));
    assert_eq!(PositionCommand::parse("трейлинг стоп 1,5%"), Some(PositionCommand::Trail(d("1.5"))));
    assert_eq!(PositionCommand::parse("long ETH by the book trailing 2%"), None);
}

#[test]
fn aave_commands() {
    assert_eq!(W::parse("lend 2 USDC on Aave"), Some(aave(true, some("2"), "base")));
    assert_eq!(W::parse("положи 1,5 usdc в aave на base"), Some(aave(true, some("1.5"), "base")));
    assert_eq!(W::parse("buy USDC on Base and put it into Aave"), Some(aave(true, None, "base")));
    assert_eq!(W::parse("withdraw USDC from Aave"), Some(aave(false, None, "base")));
    // Rewards to collect, not a withdrawal or the rates: «claim … from Aave», «claim yield».
    for text in ["claim rewards", "claim my rewards from Morpho", "claim rewards from Aave", "claim yield", "забери награды", "собери мои награды", "клейм наград", "получи бонусы"] {
        assert_eq!(W::parse(text), Some(W::ClaimRewards), "{text}");
    }
    assert_ne!(W::parse("сними 1 usdc из aave"), Some(W::ClaimRewards));
    assert_eq!(W::parse("сними 1 usdc из aave"), Some(aave(false, some("1"), "base")));
    assert_eq!(W::parse("what do I have on Aave?"), None);
    assert_eq!(W::parse("move USDC from Base to Solana"), Some(W::BaseToSolana(None)));
    assert_eq!(W::parse("верни 1 usdc с base в солану"), Some(W::BaseToSolana(some("1"))));
    assert_eq!(W::parse("withdraw 3 USDC to Solana"), Some(W::WithdrawToSolana(some("3"))));
}

// addressBookFromChat: in tests/address_book.rs. priceAlerts: in tests/alerts.rs.

#[test]
fn send_to_saved_names() {
    assert_eq!(W::parse("send 5 USDC to Bybit"), Some(send_to("Bybit", d("5"), None)));
    assert_eq!(W::parse("отправь 2,5 usdc на Binance депозит"), Some(send_to("Binance депозит", d("2.5"), None)));
    assert_eq!(W::parse("send 3 USDC to Solana"), Some(W::WithdrawToSolana(some("3"))));
    assert_eq!(W::parse("send 0.5 USDC to My Base wallet"), Some(send_to("My Base wallet", d("0.5"), None)));
    assert_ne!(
        W::parse("send 1 USDC to 0x742d35Cc6634C0532925a3b844Bc454e4438f44e"),
        Some(send_to("0x742d35Cc6634C0532925a3b844Bc454e4438f44e", d("1"), None))
    );
}

#[test]
fn morpho_commands_read_in_both_languages() {
    assert_eq!(W::parse("deposit 2 USDC into Morpho"), Some(morpho(true, some("2"), "base")));
    assert_eq!(W::parse("положи 2 USDC в морфо"), Some(morpho(true, some("2"), "base")));
    assert_eq!(W::parse("withdraw USDC from Morpho"), Some(morpho(false, None, "base")));
    assert_eq!(W::parse("выведи из морфо"), Some(morpho(false, None, "base")));
    assert_eq!(W::parse("lend 2 USDC on Aave"), Some(aave(true, some("2"), "base")));
}

#[test]
fn lending_commands_name_their_network() {
    assert_eq!(W::parse("lend 2 USDC on Aave on Arbitrum"), Some(aave(true, some("2"), "arbitrum")));
    assert_eq!(W::parse("положи 3 usdc в aave на полигоне"), Some(aave(true, some("3"), "polygon")));
    assert_eq!(W::parse("deposit 50 USDC into Morpho on Ethereum"), Some(morpho(true, some("50"), "ethereum")));
    assert_eq!(W::parse("выведи всё из morpho на арбитруме"), Some(morpho(false, None, "arbitrum")));
    // No network named: Base, as before.
    assert_eq!(W::parse("lend 2 USDC on Aave"), Some(aave(true, some("2"), "base")));
    // From Solana into Aave on a network is a deposit there, not an idle transfer.
    assert_eq!(W::parse("переведи 2 usdc из соланы в aave на арбитрум"), Some(aave(true, some("2"), "arbitrum")));
    // Idle USDC to and from the new networks.
    assert_eq!(W::parse("move 3 USDC from Solana to Polygon"), Some(W::SolanaTo("polygon".into(), some("3"))));
    assert_eq!(W::parse("move 5 USDC from Polygon to Solana"), Some(W::EvmToSolana("polygon".into(), some("5"))));
    assert_eq!(W::parse("верни usdc с эфириума в солану"), Some(W::EvmToSolana("ethereum".into(), None)));
    assert_eq!(aave(true, None, "polygon").amount_question().as_deref(), Some("How much USDC into Aave on Polygon?"));
    assert_eq!(morpho(false, some("1"), "ethereum").summary(), "withdraw 1 USDC from Morpho on Ethereum to Solana");
}

#[test]
fn send_to_on_the_network_named() {
    assert_eq!(W::parse("send 2 USDC to main wallet on ethereum"), Some(send_to("main wallet", d("2"), Some("ethereum"))));
    assert_eq!(W::parse("переведи 2 USDC на main wallet по эфир сети"), Some(send_to("main wallet", d("2"), Some("ethereum"))));
    assert_eq!(W::parse("отправь 5 usdc на Маму в сети полигон"), Some(send_to("Маму", d("5"), Some("polygon"))));
    // A network inside the name stays in it.
    assert_eq!(W::parse("send 0.5 USDC to My Base wallet"), Some(send_to("My Base wallet", d("0.5"), None)));
}

/// A vault deposit funded from another EVM network runs as these two steps (Workspace.fundedFromElsewhere): both must read back.
#[test]
fn a_vault_deposit_funded_from_another_network_reads_back() {
    for c in [aave(true, some("300"), "arbitrum"), morpho(true, some("200"), "base"), morpho(true, some("50"), "ethereum"),
              W::LendDeposit("USDC".into(), some("500")), kamino(true, "USDC", some("500"))] {
        reads_back(&c);
    }
    assert_eq!(W::parse("move 301 USDC from Ethereum to Arbitrum"), Some(bridge("ethereum", "arbitrum", some("301"))));
    // pending: aVaultDepositFundedFromAnotherNetworkReadsBack: ChatRouter.steps of «move 301 … then deposit 300 … into Aave on Arbitrum» has 2 (Normalize round)
    assert_eq!(W::parse("move 501 USDC from Ethereum to Solana"), Some(W::EvmToSolana("ethereum".into(), some("501"))));
}

// PhraseTableTests.swift.

/// Many ways people ask for the same thing, in Russian and English, with the typos they make.
/// Each must land on exactly one command — or on none, when it's a trade or a question.
#[test]
fn wallet_phrases() {
    let (sol, btc) = (SpotAsset::all()[0].clone(), SpotAsset::all()[1].clone());
    let lend = |amount: Option<Decimal>| W::LendDeposit("USDC".into(), amount);
    let out = |amount: Option<Decimal>| W::LendWithdraw("USDC".into(), amount);
    for (phrase, expected) in [
        // Jupiter Lend deposits: the one that went wrong («jupyter vault», «задепозити», no amount).
        ("добавь в jupyter vault 1 USDC", lend(some("1"))),
        ("задепозити в джупитер USDC", lend(None)),
        ("задепозить 1 usdc в джупитер", lend(some("1"))),
        ("депозитни 2 usdc в jupiter", lend(some("2"))),
        ("положи 1 usdc в хранилище", lend(some("1"))),
        ("положи 1 в джупитер", lend(some("1"))),
        ("закинь 1,5 юсдс в юпитер", lend(some("1.5"))),
        ("1 USDC в джупитер", lend(some("1"))),
        ("пополни хранилище на 1 usdc", lend(some("1"))),
        ("положи USDC с соланы в джупитер", lend(None)),
        ("put 1 USDC into the Jupiter vault", lend(some("1"))),
        ("deposit 1 USDC to Jupiter Lend", lend(some("1"))),
        ("add 1 USDC to jupyter vault", lend(some("1"))),
        ("lend 1 USDC", lend(some("1"))),
        ("jupiter vault 1 USDC", lend(some("1"))),
        ("idk just deposit to vault on jupyter", lend(None)),
        ("vault deposit", lend(None)),
        ("положи 0.01 SOL в джупитер", W::LendDeposit("SOL".into(), some("0.01"))),
        // Jupiter Lend withdrawals.
        ("выведи 1 usdc из джупитера", out(some("1"))),
        ("withdraw USDC from the vault", out(None)),
        ("забери всё из хранилища", out(None)),
        ("сними 0,5 usdc с джупитера", out(some("0.5"))),
        ("take 1 USDC out of jupiter", out(some("1"))),
        ("переведи 1 usdc из джупитера в солану", out(some("1"))),
        // Kamino, staking, buys.
        ("положи 1 usdc в камино", kamino(true, "USDC", some("1"))),
        ("выведи usdc из kamino", kamino(false, "USDC", None)),
        ("move 1 USDC from kamino to solana", kamino(false, "USDC", some("1"))),
        ("застейкай 0.01 sol", W::Stake(some("0.01"))),
        ("купи сол", W::Buy { asset: sol.clone(), usdc: None }),
        ("buy SOL for 2 USDC", W::Buy { asset: sol.clone(), usdc: some("2") }),
        ("купи SOL на 1 USDC", W::Buy { asset: sol, usdc: some("1") }),
        ("купи биткоин на $5", W::Buy { asset: btc, usdc: some("5") }),
        // Own places elsewhere.
        ("выведи 1 USDC в Solana", W::WithdrawToSolana(some("1"))),
        ("withdraw 3 USDC to Solana", W::WithdrawToSolana(some("3"))),
        ("lend 2 USDC on Aave", aave(true, some("2"), "base")),
        ("move USDC from Base to Solana", W::BaseToSolana(None)),
        ("send 5 USDC to Bybit", send_to("Bybit", d("5"), None)),
        ("move USDC from Arbitrum to Solana", W::ArbitrumToSolana(None)),
        ("верни 2 usdc с арбитрума в солану", W::ArbitrumToSolana(some("2"))),
        // Topping up from outside: the addresses, not a trade.
        ("задепозить", W::Receive),
        ("хочу задепозить", W::Receive),
        ("задепозить 5 usdc", W::Receive),
        ("пополнить", W::Receive),
        ("пополни кошелек", W::Receive),
        ("top up", W::Receive),
        ("I want to deposit", W::Receive),
        ("где наши адреса?", W::Receive),
        ("мой адрес", W::Receive),
        ("what is my Solana address", W::Receive),
        ("куда отправить деньги?", W::Receive),
        ("how do I top up?", W::Receive),
        ("как пополнить баланс", W::Receive),
    ] {
        assert_eq!(W::parse(phrase), Some(expected), "«{phrase}»");
    }
}

#[test]
fn not_wallet_commands() {
    for phrase in [
        "лонг ETH риск $5 стоп 2%", "short oil by the book", "long ETH 5 USDC risk $1", "шорт нефть как учат",
        "what do I have on Aave?", "а где наш шорт по нефти?", "сколько сейчас стоит нефть", "move stop to breakeven",
        "что за параметр?", "1 USDC", "1",
    ] {
        assert_eq!(W::parse(phrase), None, "«{phrase}»");
    }
}

#[test]
fn amount_replies() {
    assert_eq!(W::amount_answer("1"), some("1"));
    assert_eq!(W::amount_answer("1 USDC"), some("1"));
    assert_eq!(W::amount_answer("да, 2"), some("2"));
    assert_eq!(W::amount_answer("$1.5"), some("1.5"));
    assert_eq!(W::amount_answer("на 3 доллара"), some("3"));
    assert_eq!(W::amount_answer("0,5"), some("0.5"));
    assert_eq!(W::amount_answer("long ETH"), None);
    assert_eq!(W::amount_answer("1 USDC в джупитер"), None);
    assert!(W::LendDeposit("USDC".into(), None).amount_question().is_some());
    assert_eq!(W::LendDeposit("USDC".into(), None).with_amount(d("1")), W::LendDeposit("USDC".into(), some("1")));
    assert_eq!(W::LendDeposit("USDC".into(), some("1")).summary(), "deposit 1 USDC into Jupiter Lend");
}

#[test]
fn bare_amounts_dont_start_plans() {
    let none = set(&[]);
    assert!(!IntentParser::starts_plan("1 USDC", &none));
    assert!(!IntentParser::starts_plan("vault deposit", &none));
    assert!(!IntentParser::starts_plan("yes", &none));
    assert!(IntentParser::starts_plan("лонг", &none));
    assert!(IntentParser::starts_plan("ETH risk $5", &none));
    assert!(IntentParser::starts_plan("short oil", &set(&["xyz:CL"])));
}

#[test]
fn stock_phrases() {
    for (phrase, expected) in [
        ("buy SpaceX stock for 2 USDC", W::BuyStock { symbol: "SPCX".into(), usdc: some("2") }),
        ("купи акции спейсикс на 2 доллара", W::BuyStock { symbol: "SPCX".into(), usdc: some("2") }),
        ("купи SpaceX через robinhood на 1 usdc", W::BuyStock { symbol: "SPCX".into(), usdc: some("1") }),
        ("buy SPCX stock tokens", W::BuyStock { symbol: "SPCX".into(), usdc: None }),
        ("купи акции теслы на $3", W::BuyStock { symbol: "TSLA".into(), usdc: some("3") }),
        ("sell my SpaceX shares", W::SellStock { symbol: "SPCX".into(), amount: None }),
        ("продай 0.005 SPCX акций", W::SellStock { symbol: "SPCX".into(), amount: some("0.005") }),
        ("move USDG from Robinhood to Solana", W::RobinhoodToSolana(None)),
    ] {
        assert_eq!(W::parse(phrase), Some(expected), "«{phrase}»");
    }
}

#[test]
fn stocks_dont_steal_other_commands() {
    assert_eq!(W::parse("buy SOL for 2 USDC"), Some(W::Buy { asset: SpotAsset::all()[0].clone(), usdc: some("2") }));
    assert_eq!(W::parse("long SPCX risk $1"), None);
    assert_eq!(W::parse("chart TSLA"), None);
}

#[test]
fn decimal_amounts_in_withdrawals() {
    assert_eq!(W::parse("withdraw 2.7 USDC to Solana"), Some(W::WithdrawToSolana(some("2.7"))));
    assert_eq!(W::parse("выведи 1.5 usdc в солану"), Some(W::WithdrawToSolana(some("1.5"))));
    assert_eq!(W::parse("move 0.5 USDC from Base to Solana"), Some(W::BaseToSolana(some("0.5"))));
    assert_eq!(W::parse("move 1.25 USDC from Arbitrum to Solana"), Some(W::ArbitrumToSolana(some("1.25"))));
    assert_eq!(W::parse("move 2.5 USDG from Robinhood to Solana"), Some(W::RobinhoodToSolana(some("2.5"))));
}

// SwapEverywhereTests.swift.

/// Any token for any other on every network ihatecrypto has (2026-10-08): Solana through Jupiter, Hyperliquid's spot book,
/// Robinhood Chain, the EVM networks, and across networks through Relay — each read back from its own summary.
#[test]
fn any_token_for_any_other_on_every_network() {
    let bonk = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";
    // Solana: two tokens straight; USDC on a side stays a buy or a sale.
    assert_eq!(W::parse("swap 0.1 SOL for BONK on solana"), Some(swap_on("solana", "SOL", "BONK", some("0.1"))));
    assert_eq!(W::parse("поменяй 0.1 SOL на BONK на солане"), Some(swap_on("solana", "SOL", "BONK", some("0.1"))));
    assert_eq!(W::parse(&format!("swap 1 JUP for {bonk} on solana")), Some(swap_on("solana", "JUP", bonk, some("1"))));
    assert_eq!(W::parse("swap 5 USDC for BONK on solana"), Some(W::Buy { asset: SpotAsset::by_symbol("BONK"), usdc: some("5") }));
    assert_eq!(W::parse("swap all JUP for USDC on solana"), Some(W::Sell { asset: SpotAsset::by_symbol("JUP"), amount: None }));
    // Hyperliquid's spot book: a sale and a buy; USDC on a side is one order.
    assert_eq!(W::parse("swap 1 HYPE for PURR on hyperliquid spot"), Some(swap_on("hyperliquid", "HYPE", "PURR", some("1"))));
    assert_eq!(W::parse("swap 20 USDC for HYPE on hyperliquid spot"), Some(W::HlSpotBuy { symbol: "HYPE".into(), usdc: some("20") }));
    // What the book can't swap still parses as a swap there; hlSpotSwap turns it down before the sale.
    assert_eq!(W::parse("swap 1 HYPE for HYPE on hyperliquid spot"), Some(swap_on("hyperliquid", "HYPE", "HYPE", some("1"))));
    assert_eq!(W::parse("поменяй 1 HYPE на HYPE на хайперликвиде"), Some(swap_on("hyperliquid", "HYPE", "HYPE", some("1"))));
    assert_eq!(W::parse("swap 12 USDT for USDT0 on hyperliquid"), Some(swap_on("hyperliquid", "USDT", "USDT0", some("12"))));
    assert_eq!(W::parse("swap 1 HYPE for USDC.e on hyperliquid spot"), Some(swap_on("hyperliquid", "HYPE", "USDC.E", some("1"))));
    assert_eq!(W::parse("обменяй 1 HYPE на USDC.e на хайперликвиде"), Some(swap_on("hyperliquid", "HYPE", "USDC.E", some("1"))));
    assert_eq!(W::parse("поменяй 1 HYPE на USDC на хайперликвиде"), Some(W::HlSpotSell { symbol: "HYPE".into(), amount: some("1") }));
    // Robinhood Chain: Stock Tokens for one another.
    assert_eq!(W::parse("swap 1 TSLA for NVDA on robinhood"), Some(swap_on("robinhood", "TSLA", "NVDA", some("1"))));
    assert_eq!(W::parse("поменяй 1 TSLA на NVDA на робингуде"), Some(swap_on("robinhood", "TSLA", "NVDA", some("1"))));
    // NEAR through NEAR Intents: any of its tokens for another there, and to or from any network; NEAR for USDC and
    // back stays 1Click's sale and buy.
    assert_eq!(W::parse("swap 2 NEAR for USDC on near"), Some(W::SellNear { amount: some("2"), to: None }));
    assert_eq!(W::parse("swap 5 USDC for NEAR on near"), Some(W::BuyNear { usdc: some("5") }));
    assert_eq!(W::parse("swap 1 NEAR for AURORA on near"), Some(swap_on("near", "NEAR", "AURORA", some("1"))));
    assert_eq!(W::parse("поменяй 1 NEAR на AURORA на неар"), Some(swap_on("near", "NEAR", "AURORA", some("1"))));
    assert_eq!(W::parse("swap 1 NEAR for SOL on solana"), Some(cross_swap("NEAR", "near", "SOL", "solana", some("1"))));
    assert_eq!(W::parse("swap 0.1 SOL on solana for NEAR on near"), Some(cross_swap("SOL", "solana", "NEAR", "near", some("0.1"))));
    // The EVM networks as before, and across networks to and from the new ones.
    assert_eq!(W::parse("swap 0.01 ETH for AERO on base"), Some(swap_on("base", "ETH", "AERO", some("0.01"))));
    assert_eq!(W::parse("swap 0.1 SOL for CAKE on bnb"), Some(cross_swap("SOL", "solana", "CAKE", "bnb", some("0.1"))));
    assert_eq!(W::parse("swap 1 OP on optimism for ETH on base"), Some(cross_swap("OP", "optimism", "ETH", "base", some("1"))));
    assert_eq!(W::parse("swap all UBTC on hyperevm for SOL on solana"), Some(cross_swap("UBTC", "hyperevm", "SOL", "solana", None)));
    for c in [
        swap_on("solana", "SOL", "BONK", some("1")), swap_on("solana", "JUP", bonk, some("2")),
        swap_on("hyperliquid", "HYPE", "PURR", some("1")), swap_on("robinhood", "TSLA", "NVDA", None),
        cross_swap("SOL", "solana", "CAKE", "bnb", some("1")), cross_swap("OP", "optimism", "UBTC", "hyperevm", some("1")),
        swap_on("near", "NEAR", "AURORA", some("1")), cross_swap("NEAR", "near", "SOL", "solana", some("1")),
        cross_swap("SOL", "solana", "NEAR", "near", some("1")),
    ] {
        reads_back(&c);
    }
}

// BNBTests.swift.

/// BNB Chain (BSC, chain 56) as a network (2026-10-08): its money in and out, its gas, sends and swaps there, in English
/// and in Russian — «bnb», «bsc», «бнб» — and never taken for Base, which starts with a «b» too.
#[test]
fn bnb_chain_is_a_place_of_its_own() {
    assert_eq!(W::parse("move 5 USDC from Solana to BNB"), Some(W::SolanaTo("bnb".into(), some("5"))));
    assert_eq!(W::parse("переведи 10 USDC из соланы на бнб"), Some(W::SolanaTo("bnb".into(), some("10"))));
    assert_eq!(W::parse("move 2 USDC from Solana to BNB Chain"), Some(W::SolanaTo("bnb".into(), some("2"))));
    assert_eq!(W::parse("move all USDC from BNB to Solana"), Some(W::EvmToSolana("bnb".into(), None)));
    assert_eq!(W::parse("верни всё с бнб на солану"), Some(W::EvmToSolana("bnb".into(), None)));
    assert_eq!(W::parse("move 3 USDC from base to bsc"), Some(bridge("base", "bnb", some("3"))));
    assert_eq!(W::parse("add gas on bnb"), Some(gas("bnb", None)));
    assert_eq!(W::parse("send 5 USDC to Mom on bnb"), Some(send_to("Mom", d("5"), Some("bnb"))));
    assert_eq!(W::parse("move 2 USDC from Solana to Base"), Some(W::SolanaTo("base".into(), some("2"))));
    // pending: bnbChainIsAPlaceOfItsOwn: ModuleNetwork(place:) of «bnb» and «base» (round 3)
    assert_eq!(
        AddressBookCommand::parse("save 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC as Shop on BNB"),
        save("0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC", "Shop", Some("bnb"))
    );
    for c in [W::SolanaTo("bnb".into(), some("2")), W::EvmToSolana("bnb".into(), some("1")), bridge("arbitrum", "bnb", some("4")), gas("bnb", some("1"))] {
        reads_back(&c);
    }
}

/// Buys, sales and swaps on BNB Chain: a token for USDC, for another, BNB as the network's own coin; «BNB» as a token
/// on another network stays a token there.
#[test]
fn swaps_on_bnb_chain() {
    assert_eq!(W::parse("buy CAKE on bnb for 5 USDC"), Some(buy_on("bnb", "CAKE", some("5"), None)));
    assert_eq!(W::parse("купи CAKE на бнб на 5 USDC"), Some(buy_on("bnb", "CAKE", some("5"), None)));
    assert_eq!(W::parse("sell all CAKE on bsc"), Some(sell_on("bnb", "CAKE", None, false)));
    assert_eq!(W::parse("swap 0.01 BNB for CAKE on bnb"), Some(swap_on("bnb", "BNB", "CAKE", some("0.01"))));
    // BNB is the network's coin: for USDC or back it stays a swap, not a buy that looks BNB up as a token.
    assert_eq!(W::parse("swap 5 USDC for BNB on bnb"), Some(swap_on("bnb", "USDC", "BNB", some("5"))));
    assert_eq!(W::parse("swap 5 USDC for CAKE on bnb"), Some(buy_on("bnb", "CAKE", some("5"), Some("bnb"))));
    for c in [buy_on("bnb", "CAKE", some("5"), None), swap_on("bnb", "BNB", "CAKE", some("1"))] {
        reads_back(&c);
    }
}

/// Fees past a tenth of a small swap are said on its card (2026-10-08: $0.18 of a $1 UBTC buy); a fair swap says nothing.
#[test]
fn fees_eating_a_swap_are_said() {
    assert_eq!(Fmt::fee_warning(d("1"), d("0.82")).as_deref(), Some("⚠️ fees take ≈ $0.18 (18%): a bigger amount loses less"));
    assert_eq!(Fmt::fee_warning(d("100"), d("99.5")), None);
    assert_eq!(Fmt::fee_warning(d("1"), d("0")), None);
}

// pending: aNewModuleWaitsForTheUser: Modules, ModuleNetwork and ModuleProtocol, the whole test (round 3)

// HyperEVMTests.swift.

/// HyperEVM (Hyperliquid's EVM, chain 999) as a network: its money in and out, its gas, sends there — never mistaken for
/// the Hyperliquid trading account, in English and in Russian.
#[test]
fn hyper_evm_is_a_place_of_its_own() {
    assert_eq!(W::parse("move 2 USDC from Solana to HyperEVM"), Some(W::SolanaTo("hyperevm".into(), some("2"))));
    assert_eq!(W::parse("переведи 2 USDC из соланы на хайперевм"), Some(W::SolanaTo("hyperevm".into(), some("2"))));
    assert_eq!(W::parse("move all USDC from HyperEVM to Solana"), Some(W::EvmToSolana("hyperevm".into(), None)));
    assert_eq!(W::parse("move 3 USDC from base to hyperevm"), Some(bridge("base", "hyperevm", some("3"))));
    assert_eq!(W::parse("move 3 USDC from hyperevm to hyperliquid"), Some(bridge("hyperevm", "hyperliquid", some("3"))));
    assert_eq!(W::parse("add gas on hyperevm"), Some(gas("hyperevm", None)));
    assert_eq!(W::parse("send 5 USDC to Mom on hyperevm"), Some(send_to("Mom", d("5"), Some("hyperevm"))));
    assert_ne!(W::parse("move 2 USDC from Solana to Hyperliquid"), Some(W::SolanaTo("hyperevm".into(), some("2"))));
    // pending: hyperEVMIsAPlaceOfItsOwn: ModuleNetwork(place:) of «hyperevm» and «hyperliquid» (round 3)
    assert_eq!(
        AddressBookCommand::parse("save 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC as Shop on HyperEVM"),
        save("0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC", "Shop", Some("hyperevm"))
    );
    for c in [W::SolanaTo("hyperevm".into(), some("2")), W::EvmToSolana("hyperevm".into(), some("1")), bridge("arbitrum", "hyperevm", some("4")), gas("hyperevm", some("1"))] {
        reads_back(&c);
    }
}

/// Swaps on HyperEVM and Optimism (2026-10-07): a token there for USDC, for another, HYPE as HyperEVM's own coin.
#[test]
fn swaps_on_optimism_and_hyper_evm() {
    assert_eq!(W::parse("buy UBTC on hyperevm for 5 USDC"), Some(buy_on("hyperevm", "UBTC", some("5"), None)));
    assert_eq!(W::parse("купи OP на оптимизме на 5 USDC"), Some(buy_on("optimism", "OP", some("5"), None)));
    assert_eq!(W::parse("sell all OP on optimism"), Some(sell_on("optimism", "OP", None, false)));
    assert_eq!(W::parse("swap 5 USDC for OP on optimism"), Some(buy_on("optimism", "OP", some("5"), Some("optimism"))));
    assert_eq!(W::parse("swap 0.001 ETH for OP on optimism"), Some(swap_on("optimism", "ETH", "OP", some("0.001"))));
    // HYPE is HyperEVM's coin: for USDC or back it stays a swap, not a buy that looks HYPE up as a token.
    assert_eq!(W::parse("swap 5 USDC for HYPE on hyperevm"), Some(swap_on("hyperevm", "USDC", "HYPE", some("5"))));
    assert_eq!(W::parse("swap 5 USDC for UBTC on hyperevm"), Some(swap_on("hyperevm", "USDC", "UBTC", some("5"))));
    assert_eq!(W::parse("поменяй 0.1 HYPE на UBTC на хайперевм"), Some(swap_on("hyperevm", "HYPE", "UBTC", some("0.1"))));
    for c in [buy_on("hyperevm", "UBTC", some("5"), None), swap_on("optimism", "ETH", "OP", some("1")), swap_on("hyperevm", "USDC", "HYPE", some("5"))] {
        reads_back(&c);
    }
}

/// Optimism, the first network added on demand (2026-10-06): moves to and from it, gas, sends, CCTP by name.
#[test]
fn optimism_is_a_place_of_its_own() {
    assert_eq!(W::parse("move 2 USDC from Solana to Optimism"), Some(W::SolanaTo("optimism".into(), some("2"))));
    assert_eq!(W::parse("переведи 2 USDC из соланы на оптимизм"), Some(W::SolanaTo("optimism".into(), some("2"))));
    assert_eq!(W::parse("move all USDC from Optimism to Solana"), Some(W::EvmToSolana("optimism".into(), None)));
    assert_eq!(W::parse("move 100 USDC from Optimism to Base via CCTP"), Some(W::Cctp { from: "optimism".into(), to: "base".into(), amount: some("100") }));
    assert_eq!(W::parse("add gas on optimism"), Some(gas("optimism", None)));
    assert_eq!(W::parse("send 5 USDC to Mom on optimism"), Some(send_to("Mom", d("5"), Some("optimism"))));
    // pending: optimismIsAPlaceOfItsOwn: ModuleNetwork(place: "optimism") (round 3)
    assert_eq!(
        AddressBookCommand::parse("save 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC as Shop on Optimism"),
        save("0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC", "Shop", Some("optimism"))
    );
    for c in [W::SolanaTo("optimism".into(), some("2")), W::EvmToSolana("optimism".into(), some("1")), bridge("optimism", "arbitrum", some("4")), gas("optimism", some("1"))] {
        reads_back(&c);
    }
}

/// A money move the wallet parser can't take — a network ihatecrypto doesn't have, a swap it can't place — is never read
/// as a trade plan of the ticker it names (route-check, 2026-10-07: «move 5 USDC from Solana to BNB» was «long BNB»).
#[test]
fn an_unknown_money_move_is_not_a_trade_plan() {
    let markets = set(&["ETH", "BNB", "CAKE", "OP", "HYPE", "SOL"]);
    for line in ["move 5 USDC from Solana to BNB", "move 5 USDC from Solana to BSC", "buy CAKE on bnb for 5 USDC",
                 "swap 0.01 BNB for USDT on bsc", "переведи 10 USDC на бнб", "купи CAKE на бнб чейне на 5$", "send 5 USDT to Binance on bsc"] {
        assert_eq!(IntentParser::apply(line, &TradeIntent::default(), &markets), None, "{line}");
    }
    // Plans still read: a long, a stop moved.
    assert_eq!(IntentParser::apply("long BNB 5 USDC", &TradeIntent::default(), &markets).map(|i| i.asset).as_deref(), Some("BNB"));
    assert!(IntentParser::apply("move stop to 3%", &TradeIntent::default(), &markets).is_some());
}

/// A tiny coin amount keeps four significant digits (2026-10-08: 1 USDC of UBTC showed «0.000011», and the card's rate
/// worked out from it read $90,909 instead of ≈ $83,600); dollars and ordinary amounts look as before.
#[test]
fn a_tiny_coin_amount_keeps_its_digits() {
    assert_eq!(Fmt::num(d("0.00001196"), 6), "0.00001196");
    assert_eq!(Fmt::num(d("0.0000119612"), 6), "0.00001196");
    assert_eq!(Fmt::num(d("8.1535351"), 6), "8.153535");
    assert_eq!(Fmt::num(d("0.054737"), 6), "0.054737");
    assert_eq!(Fmt::num(d("0.00012345678"), 8), "0.00012346");
    assert_eq!(Fmt::num(d("0.004"), 2), "0");
    assert_eq!(Fmt::num(d("0"), 6), "0");
}

/// The chat's «Move 11 USDC to Hyperliquid» card (enableAgent on an account Hyperliquid hasn't opened yet): a move it can run.
#[test]
fn opening_hyperliquid_card_is_a_move() {
    assert_eq!(W::parse("move 11 USDC from Solana to Hyperliquid"), Some(W::SolanaTo("hyperliquid".into(), some("11"))));
    assert_eq!(W::parse("переведи 11 USDC с соланы на хайперликвид"), Some(W::SolanaTo("hyperliquid".into(), some("11"))));
    reads_back(&W::SolanaTo("hyperliquid".into(), some("11")));
    // HyperEVM is still its own network, never the trading account.
    assert_eq!(W::parse("move 2 USDC from Solana to HyperEVM"), Some(W::SolanaTo("hyperevm".into(), some("2"))));
}

// StablecoinsTests.swift.
// stablecoinsAreFoundByTicker: in tests/stablecoins.rs.

#[test]
fn stablecoin_swaps_parse() {
    // A stablecoin for USDC on an EVM network is a sale kept there; USDC for one, a buy with the USDC already there.
    assert_eq!(W::parse("swap 5 USDT for USDC on arbitrum"), Some(sell_on("arbitrum", "USDT", some("5"), true)));
    assert_eq!(W::parse("swap 2 USDC for USDe on base"), Some(buy_on("base", "USDe", some("2"), Some("base"))));
    assert_eq!(W::parse("swap all DAI for USDC on polygon"), Some(sell_on("polygon", "DAI", None, true)));
    assert_eq!(W::parse("swap 4 USDC for DAI on bnb"), Some(buy_on("bnb", "DAI", some("4"), Some("bnb"))));
    assert_eq!(W::parse("swap 2 USDT for DAI on ethereum"), Some(swap_on("ethereum", "USDT", "DAI", some("2"))));
    assert_eq!(W::parse("swap 2 USDC for USDT0 on tempo"), Some(swap_on("tempo", "USDC", "USDT0", some("2"))));
    assert_eq!(W::parse("swap 11 USDC for USDT on hyperliquid"), Some(W::HlSpotBuy { symbol: "USDT".into(), usdc: some("11") }));
    // The brief's own examples.
    assert_eq!(W::parse("buy USDe on base for 5 USDC from base"), Some(buy_on("base", "USDe", some("5"), Some("base"))));
    assert_eq!(W::parse("sell all DAI on polygon and keep the USDC there"), Some(sell_on("polygon", "DAI", None, true)));
    assert_eq!(W::parse("buy USDT0 on hyperliquid spot for 11 USDC"), Some(W::HlSpotBuy { symbol: "USDT0".into(), usdc: some("11") }));
    // pending: stablecoinSwapsParse: ChatRouter.forms contains «Arbitrum: USDC, USDT, USDS, USDe, DAI» (Normalize round)
    match W::parse("buy USDT on solana for 3 USDC") {
        Some(W::Buy { asset, usdc }) if usdc == some("3") => assert_eq!(asset.symbol, "USDT"),
        other => panic!("buy USDT on solana: {other:?}"),
    }
    assert_eq!(W::parse("swap 2 USDC for USDT on near"), Some(swap_on("near", "USDC", "USDT", some("2"))));
    match W::parse("swap 3 USDC for USDT on solana") {
        Some(W::Buy { asset, usdc }) => assert!(asset.symbol == "USDT" && usdc == some("3"), "{asset:?} {usdc:?}"),
        other => panic!("USDT on Solana: {other:?}"),
    }
}

#[test]
fn stablecoin_sends_and_moves_parse() {
    let send = |token: &str, amount: &str, network: &str| W::SendToken { name: "Mom".into(), token: token.into(), amount: some(amount), network: Some(network.into()) };
    assert_eq!(W::parse("send 5 USDT on arbitrum to Mom"), Some(send("USDT", "5", "arbitrum")));
    assert_eq!(W::parse("send 3 DAI on polygon to Mom"), Some(send("DAI", "3", "polygon")));
    assert_eq!(W::parse("swap 5 USDT on arbitrum for USDC on solana"), Some(cross_swap("USDT", "arbitrum", "USDC", "solana", some("5"))));
    assert_eq!(W::parse("swap 2 USDe on base for USDe on solana"), Some(cross_swap("USDe", "base", "USDe", "solana", some("2"))));
    assert!(stablecoins::is_genuine("0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9", "arbitrum") && !stablecoins::is_genuine("0xdead", "arbitrum"));
}

// TempoTests.swift.

#[test]
fn pay_and_tempo_commands_parse() {
    assert_eq!(
        W::parse(r#"pay POST https://a.example/v1/search {"q": "flights MOW"}"#),
        Some(W::Pay { method: "POST".into(), url: "https://a.example/v1/search".into(), body: Some(r#"{"q": "flights MOW"}"#.into()) })
    );
    assert_eq!(W::parse("pay https://a.example/price"), Some(W::Pay { method: "GET".into(), url: "https://a.example/price".into(), body: None }));
    assert_eq!(W::parse(r#"pay https://a.example/x {"a":1}"#), Some(W::Pay { method: "POST".into(), url: "https://a.example/x".into(), body: Some(r#"{"a":1}"#.into()) }));
    assert_eq!(W::parse("pay http://a.example/x"), None);   // https only
    assert_eq!(W::parse("move 2 USDC from Solana to Tempo"), Some(W::SolanaTo("tempo".into(), some("2"))));
    assert_eq!(W::parse("переведи 3 USDC из соланы в темпо"), Some(W::SolanaTo("tempo".into(), some("3"))));
    assert_eq!(W::parse("move 2 USDC from Tempo to Solana"), Some(W::EvmToSolana("tempo".into(), some("2"))));
    assert_eq!(W::parse("send 5 USDC to Mom"), Some(send_to("Mom", d("5"), None)));
    let pay = r#"pay POST https://coingecko.mpp.paywithlocus.com/coingecko/simple-price {"ids":"bitcoin","vs_currencies":"usd"}"#;
    // pending: payAndTempoCommandsParse: ChatRouter.planChange and ChatRouter.steps of the pay request are nil (Normalize round)
    assert!(ScheduledTask::parse(pay, Utc::now()).is_none());
    // pending: payAndTempoCommandsParse: ChatRouter.parses(pay, markets: ["BTC"]) (Normalize round)
    assert_eq!(W::place_name("tempo"), "Tempo");
}

// pending: mppChallengeReadsWhatThePayeeAsks, mppRefusesWhatItCantPay: MPP.challenge and MPP.credential, the whole tests (later)
// aTaskPaysByItselfOnlyUpToTheConfirmedPrice, everyMonthOnTheFirst: in tests/scheduled_task.rs.
// pending: tempoIsAModule: Modules.refusal, ModuleNetwork and ModuleProtocol, the whole test (round 3)
// pending: mppDirectoryRanksByName, serviceAnswerReadsResultsAndValues, chatFoldsServiceJSON, serviceDocsStartAtTheEndpoint: MPP, the whole tests (later)
// pending: aiServiceAnswerIsItsWriting, serviceAnswerSurvivesSaving: MPP.Answer and MPP.digest, the whole tests (later)

// Between two EVM networks straight, not through Solana: the user's «из бейза на робингуд».
#[test]
fn evm_to_evm_moves_parse() {
    assert_eq!(W::parse("move 20 USDC from Base to Robinhood"), Some(bridge("base", "robinhood", some("20"))));
    assert_eq!(W::parse("переведи 20 USDC из бейза в робингуд"), Some(bridge("base", "robinhood", some("20"))));
    assert_eq!(W::parse("move all USDC from Arbitrum to Tempo"), Some(bridge("arbitrum", "tempo", None)));
    assert_eq!(W::parse("move 20 USDC from Base to Hyperliquid"), Some(bridge("base", "hyperliquid", some("20"))));
    assert_eq!(W::parse("переведи 20 USDC с бейза на гипер"), Some(bridge("base", "hyperliquid", some("20"))));
    assert_eq!(W::parse("move 20 USDC from Base to Solana"), Some(W::BaseToSolana(some("20"))));
    assert_eq!(W::parse("move 5 USDC from Solana to Base"), Some(W::SolanaTo("base".into(), some("5"))));
    assert_ne!(W::parse("withdraw USDC from Aave on Arbitrum"), Some(bridge("arbitrum", "arbitrum", None)));
    // pending: evmToEvmMovesParse: Modules.refusal of a bridge to Robinhood with Robinhood off and on (round 3)
}

// Gas is never a dead end: «добавь газа на робингуд на 3 бакса» is a command, not «ihatecrypto can't».
#[test]
fn gas_top_ups_parse() {
    assert_eq!(W::parse("добавь газа на робингуд на 3 бакса"), Some(gas("robinhood", some("3"))));
    assert_eq!(W::parse("add gas on base for 2 USDC"), Some(gas("base", some("2"))));
    assert_eq!(W::parse("top up gas on arbitrum"), Some(gas("arbitrum", None)));
    assert_eq!(W::parse("докинь газ в полигон"), Some(gas("polygon", None)));
    assert_eq!(W::parse("move 20 USDC from Base to Robinhood"), Some(bridge("base", "robinhood", some("20"))));
    // pending: gasTopUpsParse: ChatRouter.parses «add gas on robinhood for 1 USDC» (Normalize round)
}

// pending: servicesThatRefuseTheProofAreNeverOffered, aHostIsNamedOnlyWhole, aFailureAfterPayingNamesThePayment: MPP.unpayable, MPP.named and MPP.paid, the whole tests (later)
