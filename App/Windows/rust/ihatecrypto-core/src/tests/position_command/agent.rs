//! AgentTests.swift: the lines that exercise WalletCommand, SpotAsset, IntentParser and TradeIntent (and the round-1
//! modules beside them). Its ChatWords, SentHistory, TokenMentions, AddressBookCommand and TokenCommand tests live in
//! tests/chat_language.rs, tests/address_book.rs and tests/tokens.rs.

use rust_decimal::Decimal;

use super::super::{d, set};
use crate::alerts::AlertCommand;
use crate::intent::{IntentParser, TradeIntent};
use crate::position_command::{SpotAsset, WalletCommand};
use crate::stock_names::Ondo;

fn p(text: &str) -> Option<WalletCommand> { WalletCommand::parse(text) }

fn n(s: &str) -> Option<Decimal> { Some(d(s)) }

fn s(s: Option<&str>) -> Option<String> { s.map(Into::into) }

fn buy(asset: SpotAsset, usdc: Option<Decimal>) -> WalletCommand { WalletCommand::Buy { asset, usdc } }

fn sell(asset: SpotAsset, amount: Option<Decimal>) -> WalletCommand { WalletCommand::Sell { asset, amount } }

fn buy_on(place: &str, token: &str, usdc: Option<Decimal>, from: Option<&str>) -> WalletCommand {
    WalletCommand::BuyOn { place: place.into(), token: token.into(), usdc, from: s(from) }
}

fn sell_on(place: &str, token: &str, amount: Option<Decimal>, keep: bool) -> WalletCommand {
    WalletCommand::SellOn { place: place.into(), token: token.into(), amount, keep }
}

fn swap_on(place: &str, from: &str, to: &str, amount: Option<Decimal>) -> WalletCommand {
    WalletCommand::SwapOn { place: place.into(), from: from.into(), to: to.into(), amount }
}

fn cross_swap(from: &str, on: &str, to: &str, at: &str, amount: Option<Decimal>) -> WalletCommand {
    WalletCommand::CrossSwap { from: from.into(), on: on.into(), to: to.into(), at: at.into(), amount }
}

fn sell_near(amount: Option<Decimal>, to: Option<&str>) -> WalletCommand { WalletCommand::SellNear { amount, to: s(to) } }

fn send_token(name: &str, token: &str, amount: Option<Decimal>, network: Option<&str>) -> WalletCommand {
    WalletCommand::SendToken { name: name.into(), token: token.into(), amount, network: s(network) }
}

fn receive_across(network: &str, token: &str, amount: Option<Decimal>) -> WalletCommand {
    WalletCommand::ReceiveAcross { network: network.into(), token: token.into(), amount }
}

fn bridge(from: &str, to: &str, amount: &str) -> WalletCommand { WalletCommand::Bridge { from: from.into(), to: to.into(), amount: n(amount) } }

fn cctp(from: &str, to: &str, amount: &str) -> WalletCommand { WalletCommand::Cctp { from: from.into(), to: to.into(), amount: n(amount) } }

fn hl_spot_buy(symbol: &str, usdc: Option<Decimal>) -> WalletCommand { WalletCommand::HlSpotBuy { symbol: symbol.into(), usdc } }

fn hl_spot_sell(symbol: &str, amount: Option<Decimal>) -> WalletCommand { WalletCommand::HlSpotSell { symbol: symbol.into(), amount } }

fn aave_borrow(amount: Option<Decimal>, network: &str) -> WalletCommand { WalletCommand::AaveBorrow { amount, network: network.into() } }

fn aave_repay(amount: Option<Decimal>, network: &str) -> WalletCommand { WalletCommand::AaveRepay { amount, network: network.into() } }

fn aave_collateral(amount: Option<Decimal>, network: &str) -> WalletCommand { WalletCommand::AaveCollateral { amount, network: network.into() } }

fn kamino(deposit: bool, symbol: &str, amount: Option<Decimal>) -> WalletCommand { WalletCommand::Kamino { deposit, symbol: symbol.into(), amount } }

fn kamino_borrow(amount: Option<Decimal>, against: Option<&str>, adding: Option<Decimal>) -> WalletCommand {
    WalletCommand::KaminoBorrow { amount, against: s(against), adding }
}

fn kamino_repay(amount: Option<Decimal>, against: Option<&str>) -> WalletCommand { WalletCommand::KaminoRepay { amount, against: s(against) } }

fn sweep_dust(under: Option<Decimal>) -> WalletCommand { WalletCommand::SweepDust { under } }

/// Swift's `#expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")`.
fn reads_back(c: &WalletCommand) { assert_eq!(p(&c.summary()), Some(c.clone()), "{}", c.summary()) }

fn apply(text: &str, to: &TradeIntent, markets: &[&str]) -> Option<TradeIntent> { IntentParser::apply(text, to, &set(markets)) }

// The chat as an agent: the model looks things up, then answers with a block to show and cards to go on.

// pending: agentStepsReadLeniently: ChatRouter.step (Normalize round)
// pending: aQuestionsAnswersMayGoBackToTheModel: ChatRouter.step (Normalize round)
// pending: onlyTheFirstObjectOfAReplyCounts: ChatRouter.firstObject (Normalize round)
// pending: theAgentLooksUpThenAnswers: ChatRouter.route with a scripted LLMClient (Normalize round)
// pending: theAgentStopsLookingAfterItsRounds: ChatRouter.route, ChatRouter.rounds (Normalize round)
// pending: stepsRunNowAndEachMustParse: ChatRouter.steps, parses, unparsed, forms (Normalize round)
// pending: aCommandNoParserReadsGoesBackToTheModel: ChatRouter.route (Normalize round)
// tokenCardsAndWatchingParse: its TokenCommand lines are in tests/tokens.rs.
// pending: tokenCardsAndWatchingParse: ChatRouter.forms, ChatRouter.parses (Normalize round)
// pending: aMintCopiedFromALinkIsRepaired: TokenResearch.repairs (round 3)
// pending: jupiterFactsSayWhatMatters: TokenResearch.jupiterFacts (round 3)
// pending: theTokenLookupCarriesTheMonthTheCardDraws: TokenResearch.chartFacts, TokenMarket.Point (round 3)
// pending: aSearchPutsTheDeepestPoolFirst: TokenResearch.searchFacts (round 3)

#[test]
fn any_solana_token_by_its_mint() {
    let mint = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6";
    let by_mint = || SpotAsset::by_mint(mint);
    assert_eq!(p(&format!("buy token {mint} for 2 USDC")), Some(buy(by_mint(), n("2"))));
    assert_eq!(p(&format!("купи {mint} на $3")), Some(buy(by_mint(), n("3"))));
    assert_eq!(p(&format!("sell 1500 tokens {mint}")), Some(sell(by_mint(), n("1500"))));
    assert_eq!(p(&format!("sell tokens {mint}")), Some(sell(by_mint(), None)));
    assert!(!by_mint().resolved() && !by_mint().verified());
    // The fixed coins stay what they were.
    assert_eq!(p("buy SOL for 2 USDC"), Some(buy(SpotAsset::all()[0].clone(), n("2"))));
    let jup = &SpotAsset::all()[3].mint;
    assert_ne!(p(&format!("buy token {jup} for 2 USDC")), Some(buy(SpotAsset::by_mint(jup), n("2"))));
    // pending: anySolanaTokenByItsMint: ChatRouter.forms.contains("buy token <MINT> for N USDC") (Normalize round)
}

// pending: buyingByMintKnowsItsRisks: TokenResearch.solanaToken, warnings (round 3)
// pending: aPastedAddressIsLookedUpBeforeTheModelReads: ChatRouter.pastedAddress, addresses, addressesAllowed, route; TokenResearch.report (Normalize round, round 3)

#[test]
fn tokens_by_ticker_on_solana_base_and_arbitrum() {
    // Solana: a ticker is found among Jupiter's verified tokens when it runs; the fixed coins stay themselves.
    assert_eq!(p("buy BONK on solana for 5 USDC"), Some(buy(SpotAsset::by_symbol("BONK"), n("5"))));
    assert_eq!(p("buy $WIF on solana for 2 USDC"), Some(buy(SpotAsset::by_symbol("WIF"), n("2"))));
    assert_eq!(p("sell 1000 BONK on solana"), Some(sell(SpotAsset::by_symbol("BONK"), n("1000"))));
    assert_eq!(p("buy SOL on solana for 2 USDC"), Some(buy(SpotAsset::all()[0].clone(), n("2"))));
    assert!(!SpotAsset::by_symbol("BONK").resolved() && SpotAsset::by_symbol("jup") == SpotAsset::all()[3]);
    // Base and Arbitrum: by ticker or by contract, both ways.
    let aero = "0x940181a94A35A4569E4529A3CDfB74e38FD98631";
    assert_eq!(p("buy AERO on base for 5 USDC"), Some(buy_on("base", "AERO", n("5"), None)));
    assert_eq!(p("купи ARB на арбитруме на 5$"), Some(buy_on("arbitrum", "ARB", n("5"), None)));
    assert_eq!(p("купи AAVE на полигоне на 10 usdc"), Some(buy_on("polygon", "AAVE", n("10"), None)));
    assert_eq!(p("buy UNI on ethereum for 50 USDC"), Some(buy_on("ethereum", "UNI", n("50"), None)));
    assert_eq!(p("sell all LINK on polygon"), Some(sell_on("polygon", "LINK", None, false)));
    assert_eq!(p("sell all DEGEN on base"), Some(sell_on("base", "DEGEN", None, false)));
    assert_eq!(p("sell 12.5 AERO on base"), Some(sell_on("base", "AERO", n("12.5"), false)));
    assert_eq!(p(&format!("buy token {aero} on base for 3 USDC")), Some(buy_on("base", aero, n("3"), None)));
    assert_eq!(p(&format!("sell tokens {aero} on base")), Some(sell_on("base", aero, None, false)));
    assert_eq!(p("buy AERO on base").and_then(|c| c.amount_question()).as_deref(), Some("For how many USDC should I buy AERO on Base?"));
    // Where the money comes from, and a sale that keeps the USDC there; the summaries read back the same.
    let from_solana = buy_on("base", "AERO", n("5"), Some("solana"));
    assert_eq!(p("buy AERO on base for 5 USDC from solana"), Some(from_solana.clone()));
    assert_eq!(p(&from_solana.summary()), Some(from_solana));
    assert_eq!(p("купи AERO на base на 5$ с арбитрума"), Some(buy_on("base", "AERO", n("5"), Some("arbitrum"))));
    assert_eq!(p("buy AERO on base for 5 USDC from base"), Some(buy_on("base", "AERO", n("5"), Some("base"))));
    let keep = sell_on("base", "AERO", n("3"), true);
    assert_eq!(p("sell 3 AERO on base and keep the USDC there"), Some(keep.clone()));
    assert_eq!(p(&keep.summary()), Some(keep.clone()));
    assert_eq!(p("продай 3 AERO на base, USDC оставь там"), Some(keep));
    assert_eq!(p(&sell_on("base", "AERO", n("3"), false).summary()), Some(sell_on("base", "AERO", n("3"), false)));
    // Not these: a contract without a network stays Robinhood's; moving USDC is a bridge.
    assert_eq!(p(&format!("buy token {aero} for 3 USDC")), Some(WalletCommand::BuyStock { symbol: aero.into(), usdc: n("3") }));
    assert_eq!(p("move 2 USDC from Solana to Base"), Some(WalletCommand::SolanaTo("base".into(), n("2"))));
    // pending: tokensByTickerOnSolanaBaseAndArbitrum: ChatRouter.forms.contains the buy-on-a-network forms (Normalize round)
}

/// Ondo's tokenized stocks and USDY are Solana tokens by ticker (2026-10-29): the model writes them in this form, English
/// or Russian, and a stock with no «Ondo» stays Robinhood's.
#[test]
fn ondo_stocks_and_usdy_are_solana_tokens() {
    assert_eq!(p("buy NVDAon on solana for 5 USDC"), Some(buy(SpotAsset::by_symbol("NVDAon"), n("5"))));
    assert_eq!(p("купи SPYon на солане на 5$"), Some(buy(SpotAsset::by_symbol("SPYon"), n("5"))));
    assert_eq!(p("sell all TSLAon on solana"), Some(sell(SpotAsset::by_symbol("TSLAon"), None)));
    assert_eq!(p("продай 0.01 NVDAon на солане"), Some(sell(SpotAsset::by_symbol("NVDAon"), n("0.01"))));
    assert_eq!(p("buy USDY on solana for 10 USDC"), Some(buy(SpotAsset::by_symbol("USDY"), n("10"))));
    assert_eq!(p("купи USDY на солане на 10 usdc"), Some(buy(SpotAsset::by_symbol("USDY"), n("10"))));
    assert_eq!(p("buy Nvidia stock for 5 USDC"), Some(WalletCommand::BuyStock { symbol: "NVDA".into(), usdc: n("5") }));
    // pending: ondoStocksAndUSDYAreSolanaTokens: ChatRouter.forms.contains the Ondo example (Normalize round)
    // Ondo's own mints (its stocks end in «ondo») get its terms on the card; USDC doesn't.
    assert!(Ondo::issued("gEGtLTPNQ7jcg25zTetkbmF7teoDLcrfTnQfmn2ondo") && Ondo::issued(Ondo::USDY) && !Ondo::issued(&SpotAsset::usdc().mint));
}

// pending: aTickerFindsJupitersVerifiedTokenNotItsNamesakes: TokenResearch.solanaMint (round 3)

#[test]
fn money_in_the_trade_sizes_it() {
    // «увеличь до 5 USDC» on a 3× plan by the book: a $5 margin, not a $5 risk.
    let i = apply("long SOL leverage 3x by the book", &TradeIntent::default(), &["SOL"]).unwrap();
    let i = apply("5 USDC", &i, &["SOL"]).unwrap();
    assert!(i.amount_usdc == n("5") && i.risk_usd.is_none());
    // pending: moneyInTheTradeSizesIt: Market, Plan.resolve, Plan.evaluate sizing, the amount dropped and the margin (round 3)
    // A risk named with it keeps «N USDC» as the amount to send.
    assert!(apply("risk $1", &i, &["SOL"]).is_some());
    // pending: moneyInTheTradeSizesIt: Plan.resolve(both).intent.amountUSDC == 5 (round 3)
}

// pending: aPlanNeverHoldsMoreThanItsCommandNamed: Market, Plan.resolve, Plan.evaluate, Plan.moneyProblem, RiskPolicy (round 3)

#[test]
fn a_small_account_reaches_the_minimum_order() {
    // $29 at 1% sizes HYPE under $10: the rules raise the risk to the least that reaches it, within the limit.
    assert!(apply("long HYPE leverage 3x by the book", &TradeIntent::default(), &["HYPE"]).is_some());
    // pending: aSmallAccountReachesTheMinimumOrder: Market, RiskPolicy, Plan.resolve, Plan.evaluate (round 3)
}

#[test]
fn money_in_the_trade_lowers_the_rules_leverage_to_fit_the_limit() {
    // «make it 8 bucks» on a short by the book: 8 × 5× × 2.9% = $1.16 breaks a 3.5% limit of $29; 4× fits.
    let i = apply("short SOL by the book 8 USDC", &TradeIntent::default(), &["SOL"]).unwrap();
    // pending: moneyInTheTradeLowersTheRulesLeverageToFitTheLimit: Market, RiskPolicy, Plan.resolve leverage 4 and risk (round 3)
    // Leverage the user named stays.
    assert!(apply("leverage 5x", &i, &["SOL"]).is_some());
    // pending: moneyInTheTradeLowersTheRulesLeverageToFitTheLimit: Plan.resolve(named).intent.leverage == 5 (round 3)
}

#[test]
fn a_market_hyperliquid_doesnt_have_doesnt_parse() {
    let m = ["ETH", "SOL", "kPEPE", "DOGE", "xyz:CL", "xyz:TSLA"];
    // PEPE trades by the thousand; it used to leave the plan on ETH.
    assert_eq!(apply("long PEPE by the book", &TradeIntent::default(), &m).map(|i| i.asset).as_deref(), Some("kPEPE"));
    // pending: aMarketHyperliquidDoesntHaveDoesntParse: ChatRouter.parses (Normalize round)
    assert_eq!(apply("long KPEPE 4 USDC leverage 2x", &TradeIntent::default(), &m).map(|i| i.asset).as_deref(), Some("kPEPE"));
    assert!(IntentParser::named_asset("kPEPE", &set(&m)).as_deref() == Some("kPEPE")
        && IntentParser::named_asset("лонг kpepe", &set(&m)).as_deref() == Some("kPEPE"));
    // pending: aMarketHyperliquidDoesntHaveDoesntParse: the rest is ChatRouter.parses (Normalize round)
}

#[test]
fn a_chain_of_plan_changes_is_one_and_two_plans_dont_chain() {
    let m = set(&["ETH", "BTC", "SOL"]);
    // pending: aChainOfPlanChangesIsOneAndTwoPlansDontChain: ChatRouter.planChange, ChatRouter.parses (Normalize round)
    assert_eq!(AlertCommand::parse("alert me when ETH is at 2600", &m), Some(AlertCommand::Set { coin: "ETH".into(), price: d("2600"), above: None, steps: vec![] }));
}

// pending: aSecondReadingSendsAMisreadBackOnceThenAsks: ChatRouter.route with check (Normalize round)
// pending: aCommandNoFormReadsGoesBackWithItsForms: ChatRouter.route (Normalize round)

#[test]
fn what_the_user_left_out_the_rules_fill() {
    // «open a short on btc, risk half a buck»: the stop by the book, the rules' leverage, the $0.50 risk kept.
    let i = apply("short BTC risk $0.50", &TradeIntent::default(), &["BTC"]).unwrap().started();
    assert_eq!(i.auto_rules, Some(true));
    // pending: whatTheUserLeftOutTheRulesFill: Market, Plan.resolve risk/stop/leverage (round 3)
    // Both named: nothing else is filled but the leverage.
    let both = apply("short BTC risk $0.50 stop 1.2%", &TradeIntent::default(), &["BTC"]).unwrap().started();
    assert!(both.auto_rules.is_none());
    // pending: whatTheUserLeftOutTheRulesFill: Plan.resolve(both).intent.leverage == RiskPolicy().maxLeverage (round 3)
}

// pending: leverageAboveWhatTheSignerTakesIsRefusedInThePlan: Market, Plan.evaluate (round 3)

#[test]
fn the_minimum_order_never_spends_more_than_named() {
    // «нефть лонг на 4 доллара x2» is an $8 position: under the $10 minimum, said so, not quietly $5 of margin.
    let i = apply("long oil 4 USDC leverage 2x", &TradeIntent::default(), &["xyz:CL"]).unwrap().started();
    // pending: theMinimumOrderNeverSpendsMoreThanNamed: Market, Plan.resolve, Plan.evaluate «$10 minimum» (round 3)
    // At 3× the same 4 USDC reaches it.
    assert!(apply("leverage 3x", &i, &["xyz:CL"]).is_some());
    // pending: theMinimumOrderNeverSpendsMoreThanNamed: Plan.evaluate(r3).problem == nil (round 3)
}

// spanishIsReadAndConfirmsLikeTheOthers: in tests/chat_language.rs.

#[test]
fn a_stopped_plan_gets_the_one_change_that_passes() {
    // An $9.5 order: the least risk that reaches the $10 minimum.
    assert!(apply("long SOL risk $0.3 stop 3% leverage 2x", &TradeIntent::default(), &["SOL"]).is_some());
    // Money named, «3 USDC at 2×» is $6: the least money that reaches it.
    assert!(apply("long SOL 3 USDC leverage 2x", &TradeIntent::default(), &["SOL"]).is_some());
    // Risk $5 of $29 breaks 3.5%: the largest risk that fits.
    assert!(apply("long SOL risk $5 stop 3%", &TradeIntent::default(), &["SOL"]).is_some());
    // pending: aStoppedPlanGetsTheOneChangeThatPasses: Market, RiskPolicy, Plan.fix (round 3)
}

// pending: aLongChainFitsAndAnUnreadableReplyGetsASecondTry: ChatRouter.maxCommand, ChatRouter.route, Unreadable (Normalize round)
// pending: aVerifiedTokenIsFoundByItsNameAndAFakeTickerIsNot: TokenResearch.solanaMint (round 3)
// typedApprovalNeverCarriesOutAMove, upAndDownWalkWhatWasSent, dollarMentionsNameTokens: in tests/chat_language.rs.
// addressBookTakesENSAndSNSNamesOnlyToSave: in tests/address_book.rs.

/// One token for another on an EVM network, straight: English and Russian, a contract, all of it; USDC on a side is the
/// buy or the sale there, with the money on that network. The summaries read back the same.
#[test]
fn swap_one_token_for_another_on_a_network() {
    let aero = "0x940181a94A35A4569E4529A3CDfB74e38FD98631";
    let eth = swap_on("base", "ETH", "AERO", n("0.01"));
    assert_eq!(p("swap 0.01 ETH for AERO on base"), Some(eth.clone()));
    assert_eq!(p("swap 0.01 ETH to AERO on Base."), Some(eth.clone()));
    assert_eq!(p("поменяй 0.01 ETH на AERO на бейсе"), Some(eth.clone()));
    assert_eq!(p("обменяй 0,01 ETH на AERO в base"), Some(eth.clone()));
    assert!(p(&eth.summary()) == Some(eth.clone()) && eth.summary() == "swap 0.01 ETH for AERO on Base");
    let all = swap_on("ethereum", "PEPE", "ETH", None);
    assert_eq!(p("swap all PEPE for ETH on ethereum"), Some(all.clone()));
    assert_eq!(p("обменяй весь PEPE на ETH в эфириуме"), Some(all.clone()));
    assert_eq!(p(&all.summary()), Some(all));
    let contract = swap_on("base", "DEGEN", aero, n("1500"));
    assert_eq!(p(&format!("swap 1500 DEGEN for {aero} on base")), Some(contract.clone()));
    assert_eq!(p(&contract.summary()), Some(contract));
    assert_eq!(p("swap 2 POL for AAVE on polygon"), Some(swap_on("polygon", "POL", "AAVE", n("2"))));
    // USDC on a side: the buy or the sale, paid and kept on that network.
    assert_eq!(p("swap 5 USDC for AERO on base"), Some(buy_on("base", "AERO", n("5"), Some("base"))));
    assert_eq!(p("swap all AERO for USDC on base"), Some(sell_on("base", "AERO", None, true)));
    // Not a swap on an EVM network: Solana has its own forms, and a swap needs its network named.
    assert_eq!(p("swap 0.01 ETH for AERO"), None);
}

/// A NEAR sale names the network its USDC lands on: Solana by default, Base, Arbitrum or Ethereum when said; the
/// summaries read back the same.
#[test]
fn sell_near_for_usdc_on_a_network() {
    assert_eq!(p("sell NEAR"), Some(sell_near(None, None)));
    assert_eq!(p("sell NEAR for USDC on base"), Some(sell_near(None, Some("base"))));
    assert_eq!(p("продай 0,5 NEAR в USDC на арбитрум"), Some(sell_near(n("0.5"), Some("arbitrum"))));
    assert_eq!(p("sell 2 NEAR for USDC on ethereum"), Some(sell_near(n("2"), Some("ethereum"))));
    for c in [sell_near(None, None), sell_near(n("2"), Some("base")), sell_near(n("0.5"), Some("arbitrum"))] {
        assert_eq!(p(&c.summary()), Some(c));
    }
    assert_eq!(sell_near(n("2"), Some("base")).summary(), "sell 2 NEAR from your NEAR account for USDC on Base");
}

/// Bridged USDC (USDC.e) on Polygon and Arbitrum: named so in the portfolio (both contracts call themselves «USDC»), and
/// «swap all USDC.e for USDC on polygon» is the sale that keeps Circle's USDC there.
#[test]
fn bridged_usdc_is_named_and_swapped() {
    assert_eq!(p("swap all USDC.e for USDC on polygon"), Some(sell_on("polygon", "USDC.e", None, true)));
    assert_eq!(p("swap 12.5 USDC.e for USDC on arbitrum"), Some(sell_on("arbitrum", "USDC.e", n("12.5"), true)));
    // pending: bridgedUSDCIsNamedAndSwapped: Holdings.evmTokens names USDC.e (later round)
}

/// «Sweep dust»: the limit when said, $5 when not; a talk about small things isn't one; the summary reads back the same.
#[test]
fn sweep_dust_is_a_command() {
    assert_eq!(p("sweep dust"), Some(sweep_dust(None)));
    assert_eq!(p("собери пыль до $3"), Some(sweep_dust(n("3"))));
    assert_eq!(p("convert small balances to USDC"), Some(sweep_dust(None)));
    assert_eq!(p("собери мелочь в USDC"), Some(sweep_dust(None)));
    assert_eq!(p("это мелочь, не волнуйся"), None);
    for c in [sweep_dust(None), sweep_dust(n("2.5"))] { assert_eq!(p(&c.summary()), Some(c)) }
}

/// The approvals board and a revoke: English and Russian, Permit2's said, the summary reads back the same.
#[test]
fn approvals_are_shown_and_revoked() {
    assert_eq!(p("show approvals"), Some(WalletCommand::ShowApprovals));
    assert_eq!(p("мои разрешения"), Some(WalletCommand::ShowApprovals));
    assert_eq!(p("кому я дал разрешения на мои токены?"), Some(WalletCommand::ShowApprovals));
    let (token, spender) = ("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", "0x2626664c2603336e57b271c5c0b26f421741e481");
    let revoke_on = |place: &str, permit2: bool| WalletCommand::RevokeApproval { place: place.into(), token: token.into(), spender: spender.into(), permit2 };
    let revoke = revoke_on("base", false);
    assert_eq!(p(&format!("revoke the approval of {token} to {spender} on base")), Some(revoke.clone()));
    assert_eq!(p(&format!("отзови разрешение {token} для {spender} на base")), Some(revoke.clone()));
    let via_permit2 = revoke_on("arbitrum", true);
    for c in [revoke, via_permit2] { assert_eq!(p(&c.summary()), Some(c)) }
}

/// Hyperliquid's spot book: a buy for dollars, a sale of coins or all of it; a leveraged trade isn't spot; summaries read back.
#[test]
fn hyperliquid_spot_is_parsed() {
    assert_eq!(p("buy HYPE on hyperliquid spot for 10 USDC"), Some(hl_spot_buy("HYPE", n("10"))));
    assert_eq!(p("купи HYPE на споте хайперликвида за 15$"), Some(hl_spot_buy("HYPE", n("15"))));
    assert_eq!(p("sell all HYPE spot"), Some(hl_spot_sell("HYPE", None)));
    assert_eq!(p("sell 2.5 HYPE on hyperliquid spot"), Some(hl_spot_sell("HYPE", n("2.5"))));
    assert_ne!(p("long HYPE spot price 30"), Some(hl_spot_buy("HYPE", None)));
    for c in [hl_spot_buy("HYPE", n("12")), hl_spot_sell("PURR", None), hl_spot_sell("HYPE", n("3"))] { assert_eq!(p(&c.summary()), Some(c)) }
}

#[test]
fn cross_network_swap_is_parsed() {
    assert_eq!(p("swap 0.1 SOL on Solana for ETH on Arbitrum"), Some(cross_swap("SOL", "solana", "ETH", "arbitrum", n("0.1"))));
    assert_eq!(p("обменяй 0.1 SOL на ETH в арбитруме"), Some(cross_swap("SOL", "solana", "ETH", "arbitrum", n("0.1"))));
    assert_eq!(p("swap all AERO on base for SOL on solana"), Some(cross_swap("AERO", "base", "SOL", "solana", None)));
    assert_eq!(p("swap all USDC on solana for ETH on arbitrum"), Some(cross_swap("USDC", "solana", "ETH", "arbitrum", None)));
    assert_eq!(p("обменяй весь USDC с соланы на ETH в арбитруме"), Some(cross_swap("USDC", "solana", "ETH", "arbitrum", None)));
    assert_eq!(p("поменяй 5 AERO с бейса на ARB в арбитруме"), Some(cross_swap("AERO", "base", "ARB", "arbitrum", n("5"))));
    // One network is a swap there; USDC for USDC is a bridge; ETH with no network named stays the swap on that network.
    assert_eq!(p("swap 0.01 ETH for AERO on base"), Some(swap_on("base", "ETH", "AERO", n("0.01"))));
    assert_eq!(p("поменяй 0.01 ETH на AERO на бейсе"), Some(swap_on("base", "ETH", "AERO", n("0.01"))));
    assert_eq!(p("swap 5 USDC on base for USDC on arbitrum"), Some(bridge("base", "arbitrum", "5")));
    // USDC on one side is a buy or a sale, but the network's own coin isn't on Uniswap's list: ETH ↔ USDC stays a swap.
    assert_eq!(p("swap 2 USDC for AERO on base"), Some(buy_on("base", "AERO", n("2"), Some("base"))));
    assert_eq!(p("swap 2 USDC for ETH on base"), Some(swap_on("base", "USDC", "ETH", n("2"))));
    assert_eq!(p("swap 0.001 ETH for USDC on arbitrum"), Some(swap_on("arbitrum", "ETH", "USDC", n("0.001"))));
    assert_eq!(p("swap 2 USDC for POL on polygon"), Some(swap_on("polygon", "USDC", "POL", n("2"))));
    for c in [cross_swap("SOL", "solana", "ETH", "arbitrum", n("0.01")), cross_swap("AERO", "base", "SOL", "solana", None),
              cross_swap("USDC", "robinhood", "ETH", "base", n("3"))] {
        assert_eq!(p(&c.summary()), Some(c));
    }
}

#[test]
fn sends_to_networks_only_near_intents_reaches_are_parsed() {
    // «All» of a network's own coin: everything but the fee (Send.swift), no number needed.
    assert_eq!(p("send all SOL to Bybit"), Some(send_token("Bybit", "SOL", None, None)));
    assert_eq!(p("переведи весь ETH на base на Bybit"), Some(send_token("Bybit", "ETH", None, Some("base"))));
    assert_eq!(p("send 0.0005 BTC to Binance"), Some(send_token("Binance", "BTC", n("0.0005"), None)));
    assert_eq!(p("send 20 USDT on tron to Binance"), Some(send_token("Binance", "USDT", n("20"), Some("tron"))));
    assert_eq!(p("send 20 USDC on sui to Sui wallet"), Some(send_token("Sui wallet", "USDC", n("20"), Some("sui"))));
    assert_eq!(p("отправь 30 XRP на Bitstamp"), Some(send_token("Bitstamp", "XRP", n("30"), None)));
    // Its AddressBookCommand saves are in tests/address_book.rs (addresses_on_networks_without_a_wallet).
}

#[test]
fn receiving_from_networks_without_a_wallet_is_parsed() {
    assert_eq!(p("receive 50 USDC from Sui"), Some(receive_across("sui", "USDC", n("50"))));
    assert_eq!(p("получи 0.01 BTC с биткоина"), Some(receive_across("bitcoin", "BTC", n("0.01"))));
    assert_eq!(p("top up 20 from tron"), Some(receive_across("tron", "USDT", n("20"))));
    assert_eq!(p("receive SUI from sui"), Some(receive_across("sui", "SUI", None)));
    assert!(p("receive SUI from sui").and_then(|c| c.amount_question()).is_some());
    // Our own networks keep their forms.
    assert_ne!(p("bring 5 USDC from base to solana"), Some(receive_across("base", "USDC", n("5"))));
    assert_eq!(p("how do I top up"), Some(WalletCommand::Receive));
    assert_eq!(p("top up"), Some(WalletCommand::Receive));
    assert_eq!(p("пополни кошелёк"), Some(WalletCommand::Receive));
    for c in [receive_across("sui", "USDC", n("50")), receive_across("xrp", "XRP", n("30")), receive_across("dogecoin", "DOGE", n("100"))] {
        reads_back(&c);
    }
}

#[test]
fn cctp_is_asked_for_by_name() {
    assert_eq!(p("move 500 USDC from base to arbitrum via cctp"), Some(cctp("base", "arbitrum", "500")));
    assert_eq!(p("переведи 100 USDC с арбитрума на полигон через circle"), Some(cctp("arbitrum", "polygon", "100")));
    assert_eq!(p("move 500 USDC from base to arbitrum"), Some(bridge("base", "arbitrum", "500")));
    assert_eq!(p("move 5 USDC from base to robinhood via cctp"), Some(bridge("base", "robinhood", "5")));
    let c = cctp("ethereum", "base", "250");
    assert_eq!(p(&c.summary()), Some(c));
}

#[test]
fn tempo_dollar_swaps_are_parsed() {
    assert_eq!(p("swap 5 USDC for pathUSD on tempo"), Some(swap_on("tempo", "USDC", "pathUSD", n("5"))));
    assert_eq!(p("поменяй весь OUSD на USDC в темпо"), Some(swap_on("tempo", "OUSD", "USDC", None)));
    assert_eq!(p("swap 5 USDC for AERO on base"), Some(buy_on("base", "AERO", n("5"), Some("base"))));
    let c = swap_on("tempo", "USDC", "OUSD", n("2"));
    assert_eq!(p(&c.summary()), Some(c));
}

#[test]
fn aave_borrowing_is_parsed() {
    assert_eq!(p("borrow 10 USDC on aave"), Some(aave_borrow(n("10"), "base")));
    assert_eq!(p("займи 10 USDC в aave на арбитруме"), Some(aave_borrow(n("10"), "arbitrum")));
    assert_eq!(p("repay all USDC on aave"), Some(aave_repay(None, "base")));
    assert_eq!(p("погаси долг на aave"), Some(aave_repay(None, "base")));
    assert_eq!(p("repay 3 USDC on aave on base"), Some(aave_repay(n("3"), "base")));
    assert_eq!(p("supply 0.01 ETH as collateral on aave on base"), Some(aave_collateral(n("0.01"), "base")));
    assert_eq!(p("положи 0.01 ETH в залог на aave"), Some(aave_collateral(n("0.01"), "base")));
    // Lending isn't borrowing.
    assert_eq!(p("lend 10 USDC on aave"), Some(WalletCommand::Aave { deposit: true, amount: n("10"), network: "base".into() }));
    for c in [aave_borrow(n("5"), "base"), aave_repay(None, "arbitrum"), aave_repay(n("2"), "base"), aave_collateral(n("0.02"), "base")] {
        reads_back(&c);
    }
}

/// Kamino's main market: USDC borrowed against SOL, Kamino named or not, in English and Russian; the SOL put up first when
/// a number of it is said. Aave keeps its own loans, and a SOL deposit or withdrawal stays one.
#[test]
fn kamino_borrowing_against_sol_is_parsed() {
    let half = n("0.5");
    assert_eq!(p("займи 50 USDC под SOL"), Some(kamino_borrow(n("50"), Some("SOL"), None)));
    assert_eq!(p("займи 50 USDC под 0.5 SOL"), Some(kamino_borrow(n("50"), Some("SOL"), half)));
    assert_eq!(p("займи 1 под 0,5 SOL на камино"), Some(kamino_borrow(n("1"), Some("SOL"), half)));
    assert_eq!(p("возьми в долг 20 долларов под залог моих SOL"), Some(kamino_borrow(n("20"), Some("SOL"), None)));
    assert_eq!(p("borrow 50 USDC against my SOL"), Some(kamino_borrow(n("50"), Some("SOL"), None)));
    assert_eq!(p("borrow 50 USDC on kamino against 0.5 SOL"), Some(kamino_borrow(n("50"), Some("SOL"), half)));
    assert_eq!(p("borrow 10 USDC using 0.5 SOL as collateral"), Some(kamino_borrow(n("10"), Some("SOL"), half)));
    assert_eq!(p("займи 3 USDC под мои соланы"), Some(kamino_borrow(n("3"), Some("SOL"), None)));
    assert_eq!(kamino_borrow(None, Some("SOL"), None).amount_question().as_deref(), Some("How many USDC to borrow on Kamino against SOL?"));
    // «на солане» is the network, and a count of SOL beside a stock isn't the stock's.
    assert_eq!(p("займи 5 USDC под залог в камино на солане"), Some(kamino_borrow(n("5"), None, None)));
    assert_eq!(p("borrow 5 USDC on kamino against SPCX, keep 0.1 SOL"), Some(kamino_borrow(n("5"), Some("SPCX"), None)));
    // Not a loan against SOL: Aave's, a SOL deposit and its way back, a repay.
    assert_eq!(p("borrow 10 USDC on aave"), Some(aave_borrow(n("10"), "base")));
    assert_eq!(p("deposit 0.5 SOL to kamino"), Some(kamino(true, "SOL", half)));
    assert_eq!(p("выведи SOL из камино"), Some(kamino(false, "SOL", None)));
    assert_eq!(p("погаси долг на камино"), Some(kamino_repay(None, None)));
    for c in [kamino_borrow(n("50"), Some("SOL"), half), kamino_borrow(n("5"), Some("SOL"), None)] { reads_back(&c) }
}

/// Kamino's Backpack market: USDC borrowed against Backpack's tokenized stocks without selling them, paid back, the
/// stock put up and taken back — in English and Russian, by ticker or company. Aave keeps its own words.
#[test]
fn kamino_borrowing_against_stocks_is_parsed() {
    let five = n("0.05");
    assert_eq!(p("borrow 2 USDC on kamino against 0.05 SPCX"), Some(kamino_borrow(n("2"), Some("SPCX"), five)));
    assert_eq!(p("borrow 2 USDC on kamino against SPCX"), Some(kamino_borrow(n("2"), Some("SPCX"), None)));
    assert_eq!(p("borrow 1 USDC on kamino"), Some(kamino_borrow(n("1"), None, None)));
    assert_eq!(p("займи 2 USDC под 0.05 SPCX на камино"), Some(kamino_borrow(n("2"), Some("SPCX"), five)));
    assert_eq!(p("займи 3 доллара под мои акции SpaceX, не продавая"), Some(kamino_borrow(n("3"), Some("SPCX"), None)));
    assert_eq!(p("borrow 4 USDC against my Micron shares"), Some(kamino_borrow(n("4"), Some("MU"), None)));
    assert_eq!(p("займи 2 под 0.01 MU"), Some(kamino_borrow(n("2"), Some("MU"), n("0.01"))));
    assert_eq!(p("repay all USDC on kamino"), Some(kamino_repay(None, None)));
    assert_eq!(p("погаси долг на камино"), Some(kamino_repay(None, None)));
    assert_eq!(p("repay 1.5 USDC on kamino"), Some(kamino_repay(n("1.5"), None)));
    // Two loans: the one to pay back by what it stands on — SOL is the main market, a stock the Backpack one.
    assert_eq!(p("repay all USDC on kamino against SOL"), Some(kamino_repay(None, Some("SOL"))));
    assert_eq!(p("погаси долг под SOL на камино"), Some(kamino_repay(None, Some("SOL"))));
    assert_eq!(p("погаси 0.5 USDC долга под SPCX на камино"), Some(kamino_repay(n("0.5"), Some("SPCX"))));
    assert_eq!(kamino_repay(None, Some("SOL")).summary(), "repay all USDC on kamino against SOL");
    assert_eq!(p("deposit 0.05 SPCX to kamino"), Some(kamino(true, "SPCX", five)));
    assert_eq!(p("withdraw SPCX from kamino"), Some(kamino(false, "SPCX", None)));
    assert_eq!(p("выведи 0.02 SNDK из камино"), Some(kamino(false, "SNDK", n("0.02"))));
    assert_eq!(p("займи 1 USDC под 0.05 спейса на камино"), Some(kamino_borrow(n("1"), Some("SPCX"), five)));
    // «займёт» is time, not a loan.
    assert_ne!(p("сколько займёт покупка акций SpaceX?"), Some(kamino_borrow(None, Some("SPCX"), None)));
    // Aave's loans and Kamino's USDC stay what they were.
    assert_eq!(p("borrow 10 USDC on aave"), Some(aave_borrow(n("10"), "base")));
    assert_eq!(p("deposit 5 USDC to kamino"), Some(kamino(true, "USDC", n("5"))));
    assert_eq!(kamino_borrow(None, Some("SPCX"), None).amount_question().as_deref(), Some("How many USDC to borrow on Kamino against SPCX?"));
    for c in [kamino_borrow(n("2"), Some("SPCX"), five), kamino_borrow(n("1"), None, None), kamino_borrow(n("3"), Some("MU"), None),
              kamino_repay(None, None), kamino_repay(n("2"), None)] {
        reads_back(&c);
    }
}
