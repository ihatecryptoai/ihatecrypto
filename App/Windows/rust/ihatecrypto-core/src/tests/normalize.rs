//! The chat router's cases: AgentTests (the agent's steps, lookups, second reader, the forms), EnglishCommandTests,
//! ParserFindingsTests, ChatFindingsTests, FolderTests, TwapTests, HedgeTests, TempoTests, MultisigCommandTests,
//! StablecoinsTests, AgentCommandsTests, TradeStudyTests and JupiterDCATests' ChatRouter lines — the ones rounds 1–3
//! left pending — and FormsMatrixTests (`forms_matrix`).

mod forms_matrix;

use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

use llm::{Message, Request};

use super::set;
use crate::normalize::{Boxed, ChatRouter, Choice, Context, Failure, Lookup, Model, Reading, Route, Step};
use crate::token_research::report;

type R = ChatRouter;

/// A scripted model: its replies in order, and what it was sent.
struct Script {
    replies: Mutex<VecDeque<String>>,
    sent: Mutex<Vec<Vec<Message>>>,
}

impl Script {
    fn new(replies: &[&str]) -> Self { Self { replies: Mutex::new(replies.iter().map(|s| s.to_string()).collect()), sent: Mutex::new(vec![]) } }
    fn repeating(reply: &str, n: usize) -> Self { Self::new(&vec![reply; n]) }
    fn sent(&self) -> Vec<Vec<Message>> { self.sent.lock().unwrap().clone() }
    /// The last message of the `i`th call.
    fn told(&self, i: usize) -> String { self.sent()[i].last().map(|m| m.text.clone()).unwrap_or_default() }
}

impl Model for Script {
    fn complete<'a>(&'a self, request: &'a Request) -> Boxed<'a, llm::Result<String>> {
        self.sent.lock().unwrap().push(request.messages.clone());
        let reply = self.replies.lock().unwrap().pop_front();
        Box::pin(async move { reply.ok_or_else(|| llm::Error::Llm("no reply scripted".into())) })
    }
}

async fn route(text: &str, model: &Script, markets: Option<&HashSet<String>>) -> Result<Reading, Failure> {
    R::route(text, &Context::default(), model, "m", markets, None, None, None).await
}

fn done(json: &str) -> Reading {
    match R::step(json).unwrap() {
        Step::Done(r) => r,
        other => panic!("not an answer: {other:?}"),
    }
}

fn strings(items: &[&str]) -> Vec<String> { super::strings(items) }

// MARK: AgentTests

#[test]
fn agent_steps_read_leniently() {
    assert_eq!(R::step(r#"{"route":"lookup","lookups":[{"tool":"Token","arg":" B5AN "},{"tool":"portfolio"}]}"#).unwrap(),
               Step::Lookups(vec![Lookup::new("token", "B5AN"), Lookup::new("portfolio", "")]));
    let r = done(r#"
    {"route":"chat","reply":"Горячий мемкоин.","show":"chart token B5AN on solana",
     "options":[{"title":"Следить","comment":"цена в избранном","command":"watch token B5AN on solana","recommended":"true"},
                {"title":"","command":"chart ETH"},{"title":"No command"},{"title":"Keep it","command":"new canvas"},{"title":"Rates","command":"where to keep USDC"}]}
    "#);
    assert_eq!(r.route, Route::Chat("Горячий мемкоин.".into()));
    assert_eq!(r.show.as_deref(), Some("chart token B5AN on solana"));
    assert_eq!(r.options, [Choice::new("Следить", "цена в избранном", "watch token B5AN on solana", true), Choice::new("Rates", "", "where to keep USDC", false)]);
    assert!(R::step(r#"{"route":"lookup","lookups":[]}"#).is_err());
    // A chart offered as a card opens with the answer.
    let promoted = R::step(r#"{"route":"chat","reply":"Hot.","options":[{"title":"Watch","command":"watch token B5AN"},{"title":"Chart","command":"chart token B5AN on solana"}]}"#);
    assert_eq!(promoted.unwrap(), Step::Done(Reading::new(Route::Chat("Hot.".into()), Some("chart token B5AN on solana".into()),
                                                         vec![Choice::new("Watch", "", "watch token B5AN", false)])));
}

/// A question's answers: one with a command does it, one without goes back to the model as the reply — never one with
/// an address, since a picked answer is read as the user's own words. A chat answer's cards all need a command.
#[test]
fn a_questions_answers_may_go_back_to_the_model() {
    let r = done(r#"
    {"route":"clarify","reply":"На какой срок деньги свободны?",
     "options":[{"title":"Месяц","comment":"можно в волт с локом"},{"title":"Нужны в любой момент","command":"lend 100 USDC","recommended":true},
                {"title":"На 0x1111111111111111111111111111111111111111"}]}
    "#);
    assert_eq!(r.options, [Choice::new("Месяц", "можно в волт с локом", "", false), Choice::new("Нужны в любой момент", "", "lend 100 USDC", true)]);
}

#[test]
fn only_the_first_object_of_a_reply_counts() {
    let reply = r#"{"route":"lookup","lookups":[{"tool":"token","arg":"a{b}"}]}{"route":"chat","reply":"a guess"}"#;
    assert_eq!(R::first_object(&format!("ok: {reply}")), Some(r#"{"route":"lookup","lookups":[{"tool":"token","arg":"a{b}"}]}"#));
    assert_eq!(R::first_object("no json"), None);
}

#[tokio::test]
async fn the_agent_looks_up_then_answers() {
    let model = Script::new(&[
        r#"{"route":"lookup","lookups":[{"tool":"token","arg":"2FB5"}]}"#,
        r#"{"route":"chat","reply":"POKEMON, hot.","show":"chart token B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6 on solana"}"#,
    ]);
    let lookup = |l: Lookup| -> Boxed<'static, String> { Box::pin(async move { format!("Result for {}: mint B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6", l.tool) }) };
    let reading = R::route("расскажи про токен 2FB5", &Context::default(), &model, "m", None, Some(&lookup), None, None).await.unwrap();
    assert_eq!(reading.route, Route::Chat("POKEMON, hot.".into()));
    assert_eq!(reading.lookups, [Lookup::new("token", "2FB5")]);
    assert_eq!(reading.found, ["B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"]);
    // The second call carries the model's lookup and its result.
    let sent = model.sent();
    assert_eq!(sent[1].len(), 4);
    assert!(sent[1][3].text.contains("mint B5AN"));
}

#[tokio::test]
async fn the_agent_stops_looking_after_its_rounds() {
    let model = Script::repeating(r#"{"route":"lookup","lookups":[{"tool":"portfolio"}]}"#, R::ROUNDS + 1);
    let e = route("?", &model, None).await.unwrap_err();
    assert_eq!(e.to_string(), "The model kept looking things up without answering.");
    assert!(model.told(R::ROUNDS).contains("No more lookups"));
}

#[test]
fn steps_run_now_and_each_must_parse() {
    let m = set(&["ETH", "SOL"]);
    assert_eq!(R::steps("withdraw 5 USDC to Solana then deposit 5 USDC to kamino"), Some(strings(&["withdraw 5 USDC to Solana", "deposit 5 USDC to kamino"])));
    assert_eq!(R::steps("lend 1 USDC"), None);
    assert_eq!(R::steps("in 1 week: sell SOL then lend USDC"), None);   // a scheduled task keeps its steps
    assert!(R::parses("withdraw 5 USDC to Solana Then deposit 5 USDC to kamino", &m));
    assert!(!R::parses("withdraw 5 USDC to Solana then write me a trading bot", &m));
    assert_eq!(R::unparsed("lend 1 USDC then write me a trading bot", &m).as_deref(), Some("write me a trading bot"));
    assert!(R::forms().contains("COMMAND then COMMAND"));
}

#[tokio::test]
async fn a_command_no_parser_reads_goes_back_to_the_model() {
    let model = Script::new(&[r#"{"route":"command","command":"write me a trading bot"}"#, r#"{"route":"clarify","reply":"Бота ihatecrypto не пишет."}"#]);
    let reading = route("напиши бота", &model, Some(&set(&["ETH"]))).await.unwrap();
    assert_eq!(reading.route, Route::Clarify("Бота ihatecrypto не пишет.".into()));
    assert!(model.told(1).contains("no form reads it"));
    // Without the markets nothing is checked: the command comes back as the model wrote it.
    let unchecked = route("напиши бота", &Script::new(&[r#"{"route":"command","command":"write me a trading bot"}"#]), None).await.unwrap();
    assert_eq!(unchecked.route, Route::Command("write me a trading bot".into()));
}

/// tokenCardsAndWatchingParse's ChatRouter lines, and anySolanaTokenByItsMint's, tokensByTickerOnSolanaBaseAndArbitrum's
/// and ondoStocksAndUSDYAreSolanaTokens' forms.
#[test]
fn token_cards_and_watching_parse() {
    let mint = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6";
    let none = HashSet::new();
    assert!(R::forms().contains("chart token <CONTRACT or MINT> on"));
    assert!(R::parses(&format!("chart token {mint} on solana"), &none));
    assert!(R::parses("lend 5 USDC", &none));
    assert!(!R::parses("write me a trading bot", &none));
    assert!(R::forms().contains("buy token <MINT> for N USDC"));
    assert!(R::forms().contains("buy <TICKER> on <base|arbitrum|polygon|ethereum|optimism|bnb|hyperevm> for N USDC") && R::forms().contains("buy <TICKER> on solana for N USDC"));
    assert!(R::forms().contains("«купи акции Nvidia от Ondo на 5$» = buy NVDAon on solana for 5 USDC"));
}

#[tokio::test]
async fn a_pasted_address_is_looked_up_before_the_model_reads() {
    assert_eq!(R::pasted_address("расскажи про 2FB5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6").as_deref(), Some("2FB5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"));
    assert_eq!(R::pasted_address("купи 0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18 на $2").as_deref(), Some("0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18"));
    assert_eq!(R::pasted_address("what is the difference between staking and lending"), None);
    // A transaction's hash is no token: its first 40 hex aren't looked up as one.
    let hash = "0x9f2c4e1ab3d57e8f0c1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f70";
    assert_eq!(R::pasted_address(&format!("что с транзакцией {hash}?")), None);
    assert_eq!(R::pasted_address(&format!("tx {hash} bought 0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18")).as_deref(), Some("0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18"));
    // Nor when the model looks it up: the report names the hash whole, so a chart or a call can't take its first 40 hex.
    let report = report(hash).await;
    assert_eq!(R::addresses(&report), [hash]);
    assert!(!R::addresses_allowed_in_steps(&format!("chart token {} on base", &hash[..42]), &format!("что за токен {hash}?"), &[],
                                           &R::addresses(&report), |s| s.starts_with("chart"), &HashSet::new()));
    let model = Script::new(&[r#"{"route":"chat","reply":"POKEMON, hot."}"#]);
    let lookup = |l: Lookup| -> Boxed<'static, String> { Box::pin(async move { format!("facts about {}", l.arg) }) };
    let reading = R::route("что за токен B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6", &Context::default(), &model, "m", None, Some(&lookup), None, None)
        .await
        .unwrap();
    assert_eq!(reading.lookups, [Lookup::new("token", "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6")]);
    let sent = model.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0][1].text.contains("Already looked up for you — token B5AN"));
}

#[test]
fn a_market_hyperliquid_doesnt_have_doesnt_parse() {
    let m = set(&["ETH", "SOL", "kPEPE", "DOGE", "xyz:CL", "xyz:TSLA"]);
    // PEPE trades by the thousand; it used to leave the plan on ETH.
    assert!(R::parses("long PEPE by the book", &m));
    assert!(!R::parses("long FOOBAR by the book", &m));
    assert!(R::parses("long oil leverage 2x", &m) && R::parses("short TSLA 5 USDC", &m));
    assert!(R::parses("long 5 USDC", &m) && R::parses("short by the book", &m));
}

#[test]
fn a_chain_of_plan_changes_is_one_and_two_plans_dont_chain() {
    let m = set(&["ETH", "BTC", "SOL"]);
    assert_eq!(R::plan_change("6 USDC then leverage 4x", &m).as_deref(), Some("6 USDC leverage 4x"));
    assert_eq!(R::plan_change("long ETH 5 USDC then alert me when ETH is below 2600", &m), None);
    assert!(!R::parses("long ETH then short BTC", &m));
    assert!(R::parses("withdraw 2 USDC to Solana then long ETH 2 USDC", &m));
}

#[tokio::test]
async fn a_second_reading_sends_a_misread_back_once_then_asks() {
    // «увеличь до 5 USDC» read as risk $5: the checker disagrees, the model fixes it.
    let model = Script::new(&[r#"{"route":"command","command":"risk $5"}"#, r#"{"route":"command","command":"5 USDC"}"#]);
    let check = |c: String| -> Boxed<'static, Option<String>> {
        Box::pin(async move { (c == "risk $5").then(|| "they asked for 5 USDC in the trade, not a $5 risk".to_string()) })
    };
    let fixed = R::route("увеличь до 5 USDC", &Context::default(), &model, "m", None, None, Some(&check), None).await.unwrap();
    assert!(fixed.route == Route::Command("5 USDC".into()) && fixed.corrected.is_some() && fixed.doubt.is_none());
    assert!(model.told(1).contains("not a $5 risk"));
    // Still disagreeing after that: the model asks the user, with each reading as a card, rather than a third guess.
    let risk = r#"{"route":"command","command":"risk $5"}"#;
    let never = |_: String| -> Boxed<'static, Option<String>> { Box::pin(async { Some("not what they asked".to_string()) }) };
    let question = Script::new(&[risk, risk, r#"{"route":"clarify","reply":"5 USDC в сделку или риск $5?","options":[{"title":"5 USDC в сделку","comment":"маржа","command":"5 USDC","recommended":true},{"title":"Риск $5","comment":"потеря на стопе","command":"risk $5"}]}"#]);
    let asked = R::route("увеличь до 5 USDC", &Context::default(), &question, "m", None, None, Some(&never), None).await.unwrap();
    assert_eq!(asked.route, Route::Clarify("5 USDC в сделку или риск $5?".into()));
    assert!(asked.options.iter().map(|o| o.command.as_str()).eq(["5 USDC", "risk $5"]) && asked.doubt.is_none());
    assert!(question.told(2).contains("Don't guess with their money"));
    // A model that writes the same command a third time: nothing runs, the user decides.
    let stubborn = R::route("увеличь до 5 USDC", &Context::default(), &Script::new(&[risk, risk, risk]), "m", None, None, Some(&never), None).await.unwrap();
    assert_eq!(stubborn.doubt.as_deref(), Some("not what they asked"));
}

#[tokio::test]
async fn a_command_no_form_reads_goes_back_with_its_forms() {
    // «buy USDT for 750 USDC»: the network left out. The model hears which forms its verb has, and writes one of them.
    let model = Script::new(&[
        r#"{"route":"command","command":"buy USDT for 750 USDC then send 750 USDT on solana to Bybit"}"#,
        r#"{"route":"command","command":"buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit"}"#,
    ]);
    let r = route("переведи 750 из USDC в USDT на солане и отправь на bybit", &model, Some(&set(&["ETH"]))).await.unwrap();
    assert_eq!(r.route, Route::Command("buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit".into()));
    let told = model.told(1);
    assert!(told.contains("«buy USDT for 750 USDC» can't run") && told.contains("buy <TICKER> on solana for N USDC"));
    // And it thinks before that answer.
    assert!(!model.sent().is_empty());
}

/// spanishIsReadAndConfirmsLikeTheOthers' ChatRouter lines (its ChatWords lines are chat_language's).
#[test]
fn a_cancel_or_confirm_written_as_a_command_is_the_route() {
    assert_eq!(R::decode(r#"{"route": "command", "command": "cancel"}"#).ok(), Some(Route::Cancel));
    assert_eq!(R::decode(r#"{"route": "command", "command": "Confirm"}"#).ok(), Some(Route::Confirm));
    // A command with the card's description glued on reads as the command alone; a short one with a dash stays whole.
    let glued = "spread 800 USDC from Base across the best rates — Jupiter Lend 501 USDC at 4.55%, Morpho · Base 299 USDC at 4.46%, taking 800.1 from Base (moves first, straight: 501.2 Base → Solana); at most 50% per protocol";
    assert_eq!(R::command_only(glued), "spread 800 USDC from Base across the best rates");
    assert_eq!(R::command_only(r#"pay POST https://x.y/search {"q":"tesla — news"}"#), r#"pay POST https://x.y/search {"q":"tesla — news"}"#);
}

#[tokio::test]
async fn a_long_chain_fits_and_an_unreadable_reply_gets_a_second_try() {
    // Three token charts by contract ran past a flat 200 characters.
    let a = "0xab093def657f15df31b33922a95e047add645b29";
    let chain = format!("chart token {a} on robinhood then chart token {a} on robinhood then chart token {a} on robinhood");
    assert!(chain.chars().count() > 200 && R::max_command(&chain) >= chain.chars().count());
    // Not JSON, then fine: the model is told what's wrong and answers again.
    let model = Script::new(&["Sure! I'll show the chart.", r#"{"route":"command","command":"chart SOL"}"#]);
    let reading = route("покажи чарт сола", &model, None).await.unwrap();
    assert_eq!(reading.route, Route::Command("chart SOL".into()));
    assert!(model.told(1).contains("doesn't fit the format (it isn't a JSON object)"));
    // Twice unreadable: an Unreadable error, nothing run.
    let e = route("?", &Script::repeating("no json", R::ROUNDS + 1), None).await.unwrap_err();
    assert_eq!(e, Failure::Unreadable("no json".into()));
    assert_eq!(e.to_string(), "The model's answer didn't fit the format: no json");
}

// MARK: EnglishCommandTests

#[test]
fn every_withdrawal_takes_an_amount() {
    for form in ["withdraw [N] <USDC|SOL> from kamino", "withdraw [N] USDC from HLP", "withdraw [N] USDC from Aave", "withdraw [N] USDC from Morpho"] {
        assert!(R::forms().contains(form), "{form}");
    }
}

/// The classifier's second reading gets only the sections of the forms its command is written in.
#[test]
fn a_commands_own_sections_of_the_forms() {
    let lend = R::forms_for("lend 15 USDC");
    assert!(lend.contains("EARNING:") && lend.contains("Jupiter Lend") && !lend.contains("TRADE PLAN"));
    assert!(R::forms_for("long oil 5 USDC").starts_with("TRADE PLAN"));
    // Every step of a chain, and what a schedule runs.
    let chain = R::forms_for("withdraw 10 USDC to Solana then deposit 10 USDC to kamino");
    assert!(chain.contains("EARNING:") && chain.contains("MOVING MONEY"));
    assert!(R::forms_for("in 1 week: rebalance USDC").contains("rebalance"));
    // A verb no form starts with: all of them, rather than nothing to judge by.
    assert_eq!(R::forms_for("frobnicate 5 USDC"), R::forms());
}

/// routerFormsCoverTheParsers' and nearAndSolTransfersParse's ChatRouter lines.
#[test]
fn router_forms_cover_the_parsers() {
    assert!(R::forms().contains("sell [N] NEAR"));
    assert!(R::forms().contains("buy token <0xCONTRACT> for N USDC"));
    assert!(R::forms().contains("send N SOL to <NAME from the address book>"));
    // A NEAR account in a save command, and the router never inventing one.
    let implicit = "ab".repeat(32);
    assert!(!R::addresses_allowed(&format!("save {implicit} as X on near"), "сохрани кошелёк", &[]));
}

#[test]
fn router_never_takes_an_address_from_the_model() {
    let (ai, other) = ("0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", "0x1111111111111111111111111111111111111111");
    let allowed = |c: &str, user: &str, known: &[&str]| R::addresses_allowed(c, user, &strings(known));
    assert!(allowed(&format!("buy token {ai} for 2 USDC"), &format!("купи {ai} на 2$"), &[]));
    assert!(allowed(&format!("buy token {ai} for 2 USDC"), "купи AI на 2$", &[&ai.to_lowercase()]));
    assert!(!allowed(&format!("buy token {other} for 2 USDC"), "купи AI на 2$", &[ai]));
    assert!(allowed("long ETH risk $1", "лонг эфир", &[]));
    // A 0x one only whole: never the first 40 hex of a longer one they wrote, nor a longer one round their 40.
    let hash = "0x9f2c4e1ab3d57e8f0c1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f70";
    let sui = "0xa1b2c3d4e5f60718293a4b5c6d7e8f9011223344556677889900aabbccddeeff";
    assert_eq!(R::addresses(&format!("tx {hash}?")), [hash]);
    assert!(!allowed(&format!("buy token {} for 2 USDC", &hash[..42]), &format!("что с транзакцией {hash}? купи этот токен на 2$"), &[]));
    assert!(!allowed(&format!("save {}000000000000000000000000 as Bob on sui", &sui[..42]), &format!("сохрани {sui} как Bob на sui"), &[]));
    assert!(allowed(&format!("save {sui} as Bob on sui"), &format!("сохрани {sui} как Bob на sui"), &[]));
    let vault = "0x1A996cb54bb95462040408C06122D45D6Cdb6096";
    let to = "0x000000000000000000000000aaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbb";
    let said = format!("call base {vault} depositForBurn(uint256,uint32,bytes32,address) 1000000 6 {to} {other}");
    assert!(allowed(&said, &said, &[]));
    assert!(!allowed(&said.replace("bbbbbbbbbbbbbbbbbbbbbbbb", "cccccccccccccccccccccccc"), &said, &[]));
    assert!(!allowed(&format!("buy token {ai}a for 2 USDC"), &format!("купи {ai} на 2$"), &[]));
    assert!(!allowed(&format!("buy token {ai} for 2 USDC"), &format!("купи {ai}a на 2$"), &[]));
    assert!(allowed(&format!("buy token {ai} for 2 USDC"), &format!("купи https://basescan.org/token/{ai}#code на 2$"), &[]));
    assert_eq!(R::decode(r#"{"route":"command","command":"lend 1 USDC"}"#).unwrap(), Route::Command("lend 1 USDC".into()));
    assert_eq!(R::decode(r#"{"route":"chat","reply":"Hi!"}"#).unwrap(), Route::Chat("Hi!".into()));
    assert_eq!(R::decode(r#"{"route":"question"}"#).unwrap(), Route::Question);   // an older reply still reads
    assert!(R::decode(r#"{"route":"command","command":""}"#).is_err());
}

/// priceAlerts' ChatRouter line, and aVaultDepositFundedFromAnotherNetworkReadsBack's.
#[test]
fn an_alert_with_steps_and_a_funded_deposit_parse() {
    assert!(R::parses("when ETH is below 2680: long ETH leverage 3x", &set(&["ETH", "BTC", "SOL", "xyz:CL"])));
    assert_eq!(R::steps("move 301 USDC from Ethereum to Arbitrum then deposit 300 USDC into Aave on Arbitrum").map(|s| s.len()), Some(2));
}

// MARK: ParserFindingsTests, ChatFindingsTests

#[test]
fn what_no_parser_reads_goes_back() {
    let m = set(&["BTC", "ETH", "SOL"]);
    // aSaleInDollarsIsNeverSellAll: unparsed all the way, the model is asked again, and no long of 10 USDC is drawn.
    assert!(!R::parses("sell SOL for 10 USDC", &m));
    // aBuyInDollarsIsNeverALong: the model left out «on solana».
    assert!(!R::parses("buy USDT for 750 USDC then send all USDT on solana to Bybit USDT", &m));
    assert!(R::parses("buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit USDT", &m));
    // Whatever else slips into the plan's parser, the second reader is told what the code makes of it.
    assert!(R::plan_reading("get USDT with 750 USDC", None, &m).contains("becomes: long ETH 750 USDC"));
    assert!(R::plan_reading("buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit", None, &m).is_empty());
    // What the app reads before the plan is never described as one: a note naming SOL, a position's order.
    assert!(R::plan_reading("write to folder UITest: вернуться к SOL в пятницу", None, &m).is_empty());
    assert!(R::plan_reading("close ETH", None, &m).is_empty());
    assert!(R::plan_reading("long SOL 5 USDC", None, &m).contains("becomes: long SOL 5 USDC"));
    // everyZeroDaysIsNoSchedule (M4), aStockSaleKeepsItsCount.
    assert!(!R::parses("buy SOL for 5 USDC every 0 days", &set(&["SOL"])));
    assert!(R::forms().contains("sell [N] <TICKER> stock"));
}

/// The plan the second reader is told about starts from the one on the canvas.
#[test]
fn the_plan_reading_starts_from_the_canvas() {
    let m = set(&["BTC", "ETH", "SOL"]);
    let plan = r#"{"asset":"SOL","side":"short","riskUSD":1}"#;
    assert_eq!(R::plan_reading("leverage 3x", Some(plan), &m),
               "ihatecrypto's code reads «leverage 3x» as the trade plan — a leveraged perp on Hyperliquid — which becomes: short SOL risk $1 leverage 3x by the book\n");
    // A plan that doesn't decode is no plan.
    assert!(R::plan_reading("leverage 3x", Some("{}"), &m).contains("becomes: long ETH leverage 3x"));
}

#[test]
fn lookup_addresses_only_in_steps_that_show() {   // C1
    let a = "0xF00dF00dF00dF00dF00dF00dF00dF00dF00dF00d";
    let shows = |s: &str| s.starts_with("chart");
    let allowed = |c: &str, user: &str| R::addresses_allowed_in_steps(c, user, &[], &strings(&[a]), shows, &set(&["ETH"]));
    let user = "покажи PEPE на базе и купи на 5 баксов";
    assert!(allowed(&format!("chart token {a} on base"), user));
    assert!(!allowed(&format!("buy token {a} on base for 5 USDC"), user));
    // A display step first doesn't open the door for the buy after it, in a chain, a task or an alert.
    assert!(!allowed(&format!("chart token {a} on base then buy token {a} on base for 5 USDC"), user));
    assert!(!allowed(&format!("buy token {a} on base for 5 USDC then chart ETH"), user));
    assert!(!allowed(&format!("every day: buy token {a} on base for 5 USDC then chart ETH"), user));
    assert!(!allowed(&format!("when ETH is below 2000: buy token {a} on base for 5 USDC"), user));
    assert!(allowed(&format!("every day: chart token {a} on base"), user));
    // The address in the user's own words, or a favorite's, goes anywhere.
    assert!(allowed(&format!("chart token {a} on base then buy token {a} on base for 5 USDC"), &format!("купи {a} на 5 баксов")));
    assert!(R::addresses_allowed_in_steps(&format!("buy token {a} on base for 5 USDC"), "", &[a.to_lowercase()], &[], shows, &HashSet::new()));
}

// MARK: FolderTests

#[test]
fn folder_commands_parse() {
    let none = HashSet::new();
    assert!(R::parses("save this canvas to folder Research", &none) && R::parses("write to folder Ideas: hello", &none));
}

/// The chat's answer shows while its JSON is still being written: only a chat or clarify reply, never a command's.
#[test]
fn partial_reply_reads_unfinished_json() {
    assert_eq!(R::partial_reply(r#"{"route": "chat", "reply": "Bitcoin is at $83,5"#).as_deref(), Some("Bitcoin is at $83,5"));
    assert_eq!(R::partial_reply(r#"{"route":"clarify","reply":"Which \"MOO\"?\nThe one on Robinhood","options":[]}"#).as_deref(),
               Some("Which \"MOO\"?\nThe one on Robinhood"));
    assert_eq!(R::partial_reply(r#"{"route": "chat", "reply": "При"#).as_deref(), Some("При"));
    assert_eq!(R::partial_reply(r#"{"route": "command", "command": "buy SOL for 2 USDC", "reply": "Buying"#), None);
    assert_eq!(R::partial_reply(r#"{"route": "lookup", "lookups": [{"tool": "portfolio"#), None);
    assert_eq!(R::partial_reply(r#"{"route": "chat", "rep"#), None);
    // \u escapes, a surrogate left out (Swift's Unicode.Scalar refuses it), a cut escape.
    assert_eq!(R::partial_reply(r#"{"route":"chat","reply":"café \ud83d! \u00"#).as_deref(), Some("café ! "));
}

// MARK: TwapTests, HedgeTests, TempoTests, MultisigCommandTests, StablecoinsTests, AgentCommandsTests, TradeStudyTests

#[test]
fn twap_and_hedge_forms_parse() {
    let m = set(&["ETH", "BTC", "SOL"]);
    for text in ["long ETH 2000 USDC over 2 h leverage 1x", "long BTC 500 USDC over 30 min randomize leverage 1x"] { assert!(R::parses(text, &m), "{text}") }
    assert!(R::forms().contains("long|short <ASSET> N USDC over N min|h [randomize]"));
    assert!(R::forms().contains("= long ETH 2000 USDC over 2 h leverage 1x"));
    // Sized by its risk too: the form says so, or the model refused «риск $3.8 … за 5 минут» (live run 2026-11-05).
    assert!(R::forms().contains("= long ETH risk $4 stop 0.5% over 5 min leverage 1x"));
    let hedges = set(&["SOL", "ETH", "BTC", "HYPE", "xyz:TSLA", "xyz:NVDA", "xyz:CL"]);
    assert!(R::parses("hedge SOL", &hedges) && R::parses("hedge 0.1 SOL", &hedges));
}

#[test]
fn pay_and_tempo_commands_parse() {
    let pay = r#"pay POST https://coingecko.mpp.paywithlocus.com/coingecko/simple-price {"ids":"bitcoin","vs_currencies":"usd"}"#;
    let btc = set(&["BTC"]);
    assert!(R::plan_change(pay, &btc).is_none() && R::steps(pay).is_none());
    assert!(R::parses(pay, &btc));
    assert!(R::parses("add gas on robinhood for 1 USDC", &HashSet::new()));
}

#[test]
fn multisig_and_stablecoin_forms_parse() {
    for c in ["make a multisig 2 of 3 with Phone on arbitrum, base", "send 0.5 ETH from the multisig to Alex on arbitrum", "show the multisig"] {
        assert!(R::parses(c, &HashSet::new()), "{c}");
    }
    assert!(R::forms().contains("make a multisig [M of N]"));
    assert!(R::forms().contains("Arbitrum: USDC, USDT, USDS, USDe, DAI"));
}

#[test]
fn agent_commands_parse_and_keep_their_addresses() {
    let vault = "0x1A996cb54bb95462040408C06122D45D6Cdb6096";
    assert!(R::parses(&format!("call base {vault} claim()"), &HashSet::new()));
    // max uint256 is a number, not a mint: the address rule passes a call that withdraws everything.
    let all = format!(
        "call base {vault} withdraw(address,uint256) 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913 115792089237316195423570985008687907853269984665640564039457584007913129639935"
    );
    assert!(R::addresses_allowed(&all, "", &strings(&[vault, "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"])));
    assert!(R::parses("every hour: check: Aave health factor on Base; warn under 1.6", &HashSet::new()));
    assert!(R::parses("journal HYPE: long here then add", &HashSet::new()));   // «then» inside their words is no chain
    assert!(R::forms().contains("journal <ASSET>: <THEIR OWN WORDS>"));
    // A note or a journal entry is only kept: an address in it moves nothing; nor does the second reader check it.
    assert!(R::addresses_allowed("journal HYPE: the account 0x1111111111111111111111111111111111111111 made 561 trades", "", &[]));
    assert_eq!(R::max_command("journal HYPE: words"), 2000);
}

// MARK: ChatRecallTests

#[test]
fn only_the_chats_own_reading_counts_as_its_context() {
    use crate::chat_recall::ChatRecall;
    use llm::Role;
    assert!(R::reads(&Request::new("m", vec![Message::new(Role::System, R::system()), Message::new(Role::User, "hi")])));
    assert!(!R::reads(&Request::new("m", vec![Message::new(Role::User, "Reply with one word: works")])));
    assert!(R::instruction_tokens() > 1_000 && R::instruction_tokens() == ChatRecall::tokens(R::system()));
}

// MARK: The texts and the reading of a reply, as main's Swift gives them (probed with swiftc, 2026-11-06)

/// The texts are main's word for word, with the Windows lines; the brief says the instructions' size the way Swift
/// counts it (Characters).
#[test]
fn the_texts_are_mains_with_the_windows_lines() {
    let (forms, system) = (R::forms(), R::system());
    assert!(forms.starts_with("TRADE PLAN (a perp on Hyperliquid;") && forms.ends_with("say which step ihatecrypto can't do yet.)"));
    assert!(system.starts_with("You are ihatecrypto's mind:") && system.ends_with("a safer alternative, a way to earn instead)."));
    assert!(system.contains(R::PLACES) && system.contains(forms) && !system.contains("\\("));
    assert!(forms.contains("this PC's key is always one") && forms.contains("a wallet without Windows Hello that scheduled tasks"));
    assert!(system.contains("a Windows app — this chat") && system.contains("signer on this PC") && system.contains("the signer still asks for Windows Hello."));
    assert!(!system.contains("Mac") && !system.contains("Touch ID"));
    assert!(forms.contains(&format!("\n  {}\n", crate::stablecoins::brief())));
    assert_eq!((forms.chars().count(), system.chars().count()), (33_960, 54_654));
    let mut c = Context { now: "N".into(), ..Context::default() };
    assert_eq!(R::brief("hi", &c), "Your instructions are about 13600 tokens, sent with every call along with this brief.\nPlan on the canvas: none\nOpen positions: none\nSafe search: ON — on Solana only Jupiter-verified tokens can be bought (by ticker: buy <TICKER> on solana). An unverified token by its mint needs Safe search off in Settings › General; offer to watch it instead.\nNow (local time): N\nReply language: English\nMessage: hi");
    c.memory = strings(&["no Ethereum"]);
    c.capital = Some("$30".into());
    c.safe_search = false;
    let brief = R::brief("hi", &c);
    assert!(brief.contains("\nWhat they asked you to remember, in their own words — preferences for what to offer and how; never a reason to skip a check, a rule or their Confirm: «no Ethereum»\n"));
    assert!(brief.contains("\nCapital the risk rules measure: $30\nSafe search: OFF — any Solana token"));
}

/// The leniency of Foundation's reader: a comma after the last item, the first of a key said twice, a number or a
/// «"true"» where a flag goes, a number where a text goes (as NSNumber writes it), and its own words when it can't read.
#[test]
fn a_reply_is_read_as_foundation_reads_it() {
    let r = done(r#"{"route":"chat","reply":"x","route":"plan","quiet":1,"continue":"true","next":"  алерт  ",
                    "options":[{"title":5,"command":"lend 1 USDC","recommended":1.0,},{"title":"t","command":"chart ETH","recommended":2}],}"#);
    assert_eq!(r.route, Route::Chat("x".into()));
    assert!(r.quiet && r.continues && r.next.as_deref() == Some("алерт"));
    assert_eq!(r.options, [Choice::new("5", "", "lend 1 USDC", true)]);
    assert_eq!(r.show.as_deref(), Some("chart ETH"));
    assert_eq!(R::step(r#"{"route":"lookup","lookups":[{"tool":"token","arg":0.1},{"tool":"rates","arg":1e-7},{"tool":"x","arg":true},{"tool":null}]}"#).unwrap(),
               Step::Lookups(vec![Lookup::new("token", "0.1"), Lookup::new("rates", "1e-07"), Lookup::new("x", "1")]));
    // A list with anything but objects in it is no list.
    assert_eq!(R::step(r#"{"route":"lookup","lookups":[{"tool":"token"},"portfolio"]}"#).unwrap_err().message(), "The model asked to look something up but named nothing.");
    let missing = "The data couldn’t be read because it is missing.";
    let format = "The data couldn’t be read because it isn’t in the correct format.";
    for (json, said) in [(r#"{}"#, missing), (r#"{"route": null}"#, missing), (r#"{"route": 5}"#, format), (r#"{"route":"chat","reply":5}"#, format),
                         (r#"{,}"#, format), (r#"{"a":1,,}"#, format), ("{\"route\":\"chat\",\"reply\":\"x\ny\"}", format),
                         (r#"{"route":"nope"}"#, "The model's route makes no sense: nope"), (r#"{"route":"chat","reply":"  "}"#, "The model's route makes no sense: chat")] {
        assert_eq!(R::step(json).unwrap_err().message(), said, "{json}");
    }
}

/// `parses` takes a number after «long» as Swift's Double does — hexadecimal and «inf» too — and only a market or a
/// number there.
#[test]
fn a_number_after_long_is_no_market() {
    let m = set(&["ETH"]);
    for word in ["5", "5.", ".5", "1e3", "0x1p3", "inf", "NaN", "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18"] {
        assert!(R::parses(&format!("long {word} leverage 2x"), &m), "{word}");
    }
    assert!(!R::parses("long FOOBAR leverage 2x", &m) && !R::parses("long 0X1 leverage 2x", &m));
}
