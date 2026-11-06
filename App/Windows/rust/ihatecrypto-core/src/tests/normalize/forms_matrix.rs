//! FormsMatrixTests: every English form the router tells the model to write (`ChatRouter::forms`), with amounts,
//! networks, tickers, names and casing varied — thousands of commands — each read the way Workspace.runCanonical reads
//! it, in its order. A form that lands on another action, another amount or another place is a money move gone wrong.
//! And JupiterDCATests.jupiterDCAFormsReachTheWalletReaderFirst, which reads through the same `read`.
// pending: the «pnl» family and the schedules that run «profit and loss week» (PnLCommand, round 5's pnl.rs).

use std::collections::HashSet;
use std::sync::LazyLock;

use chrono::Utc;
use rust_decimal::Decimal;

use super::super::d;
use crate::address_book_command::AddressBookCommand;
use crate::alerts::AlertCommand;
use crate::charts::{ChartCommand, Range};
use crate::folders::FolderCommand;
use crate::foundation::{has, trim_set};
use crate::intent::{IntentParser, Side, TradeIntent};
use crate::normalize::ChatRouter;
use crate::position_command::{PositionCommand, SpotAsset, WalletCommand as W};
use crate::scheduled_task::{Every, ScheduledTask};
use crate::take_style::TakeStyle;
use crate::tokens::TokenCommand;

/// Hyperliquid's markets on 2026-10-04, main exchange and xyz, as the app loads them.
static MARKETS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    concat!(
        "BTC,ETH,ATOM,DYDX,SOL,AVAX,BNB,APE,OP,LTC,ARB,DOGE,INJ,SUI,kPEPE,CRV,LDO,LINK,STX,CFX,GMX,SNX,XRP,BCH,APT,AAVE,COMP,WLD,YGG,TRX,kSHIB,UNI,SEI,RUNE,ZRO,DOT,BANANA,TRB,BIGTIME,KAS,BLUR,TIA,BSV,ADA,MINA,POLYX,GAS,PENDLE,FET,NEAR,MEME,ORDI,NEO,ZEN,FIL,PYTH,SUSHI,IMX,kBONK,GMT,SUPER,JUP,kLUNC,RSR,GALA,JTO,ACE,WIF,CAKE,PEOPLE,ENS,ETC,XAI,MANTA,UMA,ONDO,ALT,ZETA,DYM,W,STRK,TAO,AR,kFLOKI,BOME,ETHFI,ENA,MNT,TNSR,SAGA,MERL,HBAR,POPCAT,EIGEN,REZ,NOT,TURBO,BRETT,IO,ZK,RENDER,POL,CELO,HMSTR,kNEIRO,GOAT,MOODENG,GRASS,PURR,PNUT,XLM,SAND,IOTA,ALGO,HYPE,ME,MOVE,VIRTUAL,PENGU,USUAL,FARTCOIN,AIXBT,BIO,GRIFFAIN,SPX,S,MORPHO,TRUMP,MELANIA,ANIME,VVV,BERA,LAYER,KAITO,NIL,PAXG,BABY,WCT,HYPER,ZORA,INIT,NXPC,SOPH,RESOLV,SYRUP,PUMP,PROVE,XPL,WLFI,LINEA,SKY,ASTER,AVNT,STBL,0G,HEMI,APEX,2Z,ZEC,MON,MET,MEGA,CC,ICP,AERO,STABLE,FOGO,LIT,XMR,AXS,DASH,SKR,AZTEC,CHIP,GRAM,CASHCAT,PONS,USELESS,",
        "xyz:XYZ100,xyz:TSLA,xyz:NVDA,xyz:GOLD,xyz:HOOD,xyz:INTC,xyz:PLTR,xyz:COIN,xyz:META,xyz:AAPL,xyz:MSFT,xyz:ORCL,xyz:GOOGL,xyz:AMZN,xyz:AMD,xyz:MU,xyz:SNDK,xyz:MSTR,xyz:CRCL,xyz:NFLX,xyz:COST,xyz:LLY,xyz:SKHX,xyz:TSM,xyz:JPY,xyz:EUR,xyz:SILVER,xyz:RIVN,xyz:BABA,xyz:CL,xyz:COPPER,xyz:NATGAS,xyz:SMSN,xyz:PLATINUM,xyz:USAR,xyz:CRWV,xyz:URNM,xyz:PALLADIUM,xyz:GME,xyz:KR200,xyz:SOFTBANK,xyz:JP225,xyz:HYUNDAI,xyz:KIOXIA,xyz:EWY,xyz:EWJ,xyz:BRENTOIL,xyz:HIMS,xyz:SP500,xyz:DKNG,xyz:LITE,xyz:XLE,xyz:BX,xyz:PURRDAT,xyz:MRVL,xyz:RKLB,xyz:BIRD,xyz:DRAM,xyz:CBRS,xyz:EWZ,xyz:ZM,xyz:EBAY,xyz:ARM,xyz:EWT,xyz:GBP,xyz:SPCX,xyz:ASML,xyz:MINIMAX,xyz:BB,xyz:QNT,xyz:DELL,xyz:IBM,xyz:AVGO,xyz:NOW,xyz:NBIS,xyz:WDC,xyz:NOK,xyz:SMH,xyz:BE,xyz:ZHIPU,xyz:QCOM,xyz:STRC,xyz:BOT,xyz:AMAT,xyz:GIGADEV,xyz:SHAZ,xyz:SKHY,xyz:KSTR,xyz:CXMT,xyz:GEV,xyz:KORU,xyz:UNITREE,xyz:LYTE,xyz:NCLD,xyz:SOXL,xyz:MAGS,xyz:IREN,xyz:NET,xyz:CRWD,xyz:RDDT,xyz:AAOI,xyz:MRNA,xyz:XBI,xyz:SHEIN,xyz:BMNR,xyz:SNXX,xyz:CVX,xyz:TLT,xyz:HO,xyz:OURA",
    )
    .split(',')
    .map(String::from)
    .collect()
});

/// What the app does with a command.
#[derive(Debug, Clone, PartialEq)]
enum Read {
    CancelTasks,
    Task(Option<Every>, Vec<Read>),
    Steps(Vec<Read>),
    App(String),
    Token(TokenCommand),
    Chart(ChartCommand),
    TakeChoice,
    Alert(AlertCommand),
    Folder(FolderCommand),
    Book(AddressBookCommand),
    Wallet(W),
    Position(PositionCommand),
    Plan(TradeIntent),
    Nothing,
}

/// Workspace.runCanonical's order. `position_open`: a position is open on the market the command names (or the only one).
fn read(c: &str, plan: &TradeIntent, position_open: bool) -> Read {
    let m = &*MARKETS;
    if ScheduledTask::cancels_all(c) { return Read::CancelTasks }
    if let Some(t) = ScheduledTask::parse(c, Utc::now()) { return Read::Task(t.every, t.steps.iter().map(|s| read(s, &TradeIntent::default(), false)).collect()) }
    if let Some(w @ W::Pay { .. }) = W::parse(c) { return Read::Wallet(w) }
    if let Some(merged) = ChatRouter::plan_change(c, m) { return read(&merged, plan, position_open) }
    if let Some(steps) = ChatRouter::steps(c) { return Read::Steps(steps.iter().map(|s| read(s, plan, position_open)).collect()) }
    let app = c.to_lowercase();
    let app = trim_set(&app, ".! ");
    if ["new canvas", "clear the plan", "lock", "create an automation wallet"].contains(&app) { return Read::App(app.into()) }
    if let Some(x) = FolderCommand::parse(c) { return Read::Folder(x) }
    if let Some(x) = TokenCommand::parse(c) { return Read::Token(x) }
    if let Some(x) = ChartCommand::parse(c, m) { return Read::Chart(x) }
    // pending: PnLCommand::parse(c, m) → its board (round 5).
    if TakeStyle::asks_for_choice(c) && *plan != TradeIntent::default() { return Read::TakeChoice }
    if let Some(x) = AlertCommand::parse(c, m) { return Read::Alert(x) }
    if let Some(x) = AddressBookCommand::parse(c) { return Read::Book(x) }
    if let Some(x) = W::parse(c) { return Read::Wallet(x) }
    if position_open && let Some(x) = PositionCommand::parse(c) { return Read::Position(x) }
    if has(&c.to_lowercase(), r"^\s*(?:close|cancel\s+orders|move\s+stop\s+to\s+breakeven)\b") { return Read::Nothing }
    IntentParser::apply(c, plan, m).map_or(Read::Nothing, |i| Read::Plan(i.started()))
}

fn n(s: &str) -> Option<Decimal> { Some(d(s)) }

fn strings(items: &[&str]) -> Vec<String> { items.iter().map(|s| s.to_string()).collect() }

/// SplitMix64: the same «random» plans on every run.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn chance(&mut self, p: f64) -> bool { (self.next() % 10_000) as f64 / 10_000.0 < p }
    fn pick<T: Clone>(&mut self, a: &[T]) -> T { a[(self.next() % a.len() as u64) as usize].clone() }
}

#[derive(Clone)]
struct FormCase {
    family: String,
    command: String,
    expected: Read,
    plan: TradeIntent,
    position_open: bool,
}

const CONTRACT: &str = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18";
const EVM_ADDRESS: &str = "0x23618e81E3f5cdF7f54C3d65f7FBc0aBf5B21E8f";
const MINT: &str = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";
const SOL_ADDRESS: &str = "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm";
const NAMES: [&str; 4] = ["Mom", "Bybit", "New wallet", "Cold wallet 2"];

fn amounts() -> Vec<(&'static str, Decimal)> {
    [("1", "1"), ("2.5", "2.5"), ("0.01", "0.01"), ("1000", "1000"), ("1,000", "1000"), ("12.345", "12.345"), ("2,500.5", "2500.5"), ("5.00", "5")]
        .into_iter()
        .map(|(s, v)| (s, d(v)))
        .collect()
}

fn form_cases() -> Vec<FormCase> {
    let mut out: Vec<FormCase> = Vec::new();
    let mut add = |family: &str, c: String, r: Read, plan: Option<&TradeIntent>, position: bool| {
        out.push(FormCase { family: family.into(), command: c, expected: r, plan: plan.cloned().unwrap_or_default(), position_open: position })
    };
    macro_rules! add {
        ($f:expr, $c:expr, $r:expr) => { add($f, $c, $r, None, false) };
    }
    let w = Read::Wallet;
    let amounts = amounts();
    let some_amounts: Vec<(Option<&str>, Option<Decimal>)> = std::iter::once((None, None)).chain(amounts.iter().map(|(s, v)| (Some(*s), Some(*v)))).collect();
    let nn = |a: Option<&str>| a.map_or(String::new(), |a| format!("{a} "));
    let all = SpotAsset::all();

    // EARNING
    for &(a, v) in &some_amounts {
        let a_ = nn(a);
        for s in ["USDC", "SOL", "USDT"] { add!("lend", format!("lend {a_}{s}"), w(W::LendDeposit(s.into(), v))) }
        for s in ["USDC", "SOL"] {
            add!("vault out", format!("withdraw {a_}{s} from the vault"), w(W::LendWithdraw(s.into(), v)));
            add!("kamino", format!("deposit {a_}{s} to kamino"), w(W::Kamino { deposit: true, symbol: s.into(), amount: v }));
            add!("kamino", format!("withdraw {a_}{s} from kamino"), w(W::Kamino { deposit: false, symbol: s.into(), amount: v }));
        }
        add!("hlp", format!("deposit {a_}USDC to HLP"), w(W::Hlp { deposit: true, amount: v }));
        add!("hlp", format!("withdraw {a_}USDC from HLP"), w(W::Hlp { deposit: false, amount: v }));
        add!("aave", format!("lend {a_}USDC on Aave"), w(W::Aave { deposit: true, amount: v, network: "base".into() }));
        add!("aave", format!("withdraw {a_}USDC from Aave"), w(W::Aave { deposit: false, amount: v, network: "base".into() }));
        add!("morpho", format!("deposit {a_}USDC into Morpho"), w(W::Morpho { deposit: true, amount: v, network: "base".into() }));
        add!("morpho", format!("withdraw {a_}USDC from Morpho"), w(W::Morpho { deposit: false, amount: v, network: "base".into() }));
        for net in ["Base", "Arbitrum", "Polygon", "Ethereum"] {
            let key = net.to_lowercase();
            add!("aave", format!("lend {a_}USDC on Aave on {net}"), w(W::Aave { deposit: true, amount: v, network: key.clone() }));
            add!("aave", format!("withdraw {a_}USDC from Aave on {net}"), w(W::Aave { deposit: false, amount: v, network: key.clone() }));
            if net == "Polygon" { continue }
            add!("morpho", format!("deposit {a_}USDC into Morpho on {net}"), w(W::Morpho { deposit: true, amount: v, network: key.clone() }));
            add!("morpho", format!("withdraw {a_}USDC from Morpho on {net}"), w(W::Morpho { deposit: false, amount: v, network: key }));
        }
        let Some(a) = a else { continue };
        add!("stake", format!("stake {a} SOL"), w(W::Stake(v)));
        add!("stake", format!("unstake {a} JitoSOL"), w(W::Unstake(v)));
    }
    add!("yields", "where to keep USDC".into(), w(W::Yields(Some("USDC".into()))));
    add!("yields", "where to keep SOL".into(), w(W::Yields(Some("SOL".into()))));

    // SPREAD and REBALANCE
    let withouts: [(&str, &[&str]); 5] = [("", &[]), (" without kamino", &["kamino"]), (" without kamino, aave", &["kamino", "aave"]),
                                          (" without jupiter, morpho", &["jupiter", "morpho"]), (" without aave", &["aave"])];
    let shares = [("", None), (" max 30% per protocol", n("30")), (" max 50% per protocol", n("50"))];
    let sources = ["Solana", "Base", "Arbitrum", "Polygon", "Ethereum", "Robinhood", "Tempo"];
    for &(a, v) in &some_amounts {
        let a_ = nn(a);
        let froms: Vec<(String, Option<String>)> = std::iter::once((String::new(), None)).chain(sources.iter().map(|s| (format!(" from {s}"), Some(s.to_lowercase())))).collect();
        for (src, from) in &froms {
            for (sh, share) in shares {
                for (wo, keys) in withouts {
                    add!("spread", format!("spread {a_}USDC{src} across the best rates{sh}{wo}"),
                         w(W::Allocate { symbol: "USDC".into(), max_share: share, amount: v, without: strings(keys), from: from.clone() }));
                }
            }
        }
        for (sh, share) in shares {
            for (wo, keys) in withouts {
                add!("spread", format!("spread {a_}SOL across the best rates{sh}{wo}"),
                     w(W::Allocate { symbol: "SOL".into(), max_share: share, amount: v, without: strings(keys), from: None }));
            }
        }
    }
    add!("rebalance", "rebalance USDC".into(), w(W::Rebalance { max_share: None }));
    add!("rebalance", "rebalance USDC max 40% per protocol".into(), w(W::Rebalance { max_share: n("40") }));
    add!("rewards", "claim rewards".into(), w(W::ClaimRewards));
    add!("rewards", "sweep rewards".into(), w(W::SweepRewards { min_usd: None, into: None }));
    for into in ["USDC", "SOL", "JUP", "WETH", "cbBTC"] {
        add!("rewards", format!("sweep rewards into {into}"), w(W::SweepRewards { min_usd: None, into: (into != "USDC").then(|| into.to_uppercase()) }));
    }
    for m in ["5", "10", "12.5", "1,000"] {
        let min = n(&m.replace(',', ""));
        add!("rewards", format!("sweep rewards over ${m}"), w(W::SweepRewards { min_usd: min, into: None }));
        for into in ["USDC", "SOL", "JUP", "WETH"] {
            add!("rewards", format!("sweep rewards over ${m} into {into}"), w(W::SweepRewards { min_usd: min, into: (into != "USDC").then(|| into.into()) }));
        }
    }

    // MOVING MONEY
    let evm = ["Base", "Arbitrum", "Polygon", "Ethereum", "Tempo"];
    for &(a, v) in &some_amounts {
        let a_ = nn(a);
        add!("to solana", format!("withdraw {a_}USDC to Solana"), w(W::WithdrawToSolana(v)));
        for net in evm {
            let back = match net { "Base" => W::BaseToSolana(v), "Arbitrum" => W::ArbitrumToSolana(v), _ => W::EvmToSolana(net.to_lowercase(), v) };
            add!("to solana", format!("move {a_}USDC from {net} to Solana"), w(back));
        }
        add!("to solana", format!("move {a_}USDG from Robinhood to Solana"), w(W::RobinhoodToSolana(v)));
        add!("to solana", format!("move {a_}USDC from Robinhood to Solana"), w(W::RobinhoodToSolana(v)));
        for net in evm.iter().chain(&["Robinhood"]) { add!("from solana", format!("move {a_}USDC from Solana to {net}"), w(W::SolanaTo(net.to_lowercase(), v))) }
        for from in evm.iter().chain(&["Robinhood"]) {
            for to in evm.iter().chain(&["Robinhood", "Hyperliquid"]) {
                if to == from { continue }
                add!("bridge", format!("move {a_}USDC from {from} to {to}"), w(W::Bridge { from: from.to_lowercase(), to: to.to_lowercase(), amount: v }));
            }
        }
    }
    for net in ["Base", "Arbitrum", "Polygon", "Ethereum", "Robinhood"] {
        add!("gas", format!("add gas on {net}"), w(W::Gas { place: net.to_lowercase(), usd: None }));
        for &(a, v) in &amounts { add!("gas", format!("add gas on {net} for {a} USDC"), w(W::Gas { place: net.to_lowercase(), usd: Some(v) })) }
    }

    // SENDING
    let send_nets = ["Ethereum", "Base", "Arbitrum", "Polygon", "Solana", "Robinhood", "Tempo"];
    for name in NAMES {
        for &(a, v) in &amounts {
            add!("send usdc", format!("send {a} USDC to {name}"), w(W::SendTo { name: name.into(), amount: v, network: None }));
            for net in send_nets {
                let send = || w(W::SendTo { name: name.into(), amount: v, network: Some(net.to_lowercase()) });
                add!("send usdc", format!("send {a} USDC to {name} on {net}"), send());
                add!("send usdc", format!("send {a} USDC on {} to {name}", net.to_lowercase()), send());
            }
            add!("send near", format!("send {a} NEAR to {name}"), w(W::SendNear { name: name.into(), amount: Some(v) }));
            add!("send sol", format!("send {a} SOL to {name}"), w(W::SendSol { name: name.into(), amount: v }));
            add!("send sol", format!("send {a} SOL on solana to {name}"), w(W::SendSol { name: name.into(), amount: v }));
        }
        add!("send near", format!("send all NEAR to {name}"), w(W::SendNear { name: name.into(), amount: None }));
        add!("send near", format!("close the NEAR account into {name}"), w(W::CloseNear { name: name.into() }));
        let tokens: [(&str, &[&str]); 12] = [("RAY", &["solana"]), ("AI", &["robinhood"]), ("gtUSDCp", &["base", "arbitrum"]), ("aBasUSDC", &["base"]),
                                             ("jlUSDC", &["solana"]), (CONTRACT, &["robinhood", "base"]), (MINT, &["solana"]), ("USDG", &["robinhood"]),
                                             ("ETH", &["base", "ethereum"]), ("POL", &["polygon"]), ("SOL", &["solana"]), ("PEPE", &["ethereum"])];
        for (token, nets) in tokens {
            for &net in nets {
                let token_send = |amount: Option<Decimal>| w(W::SendToken { name: name.into(), token: token.into(), amount, network: Some(net.into()) });
                add!("send token", format!("send all {token} on {net} to {name}"), token_send(None));
                for x in ["3.5", "1973.719299"] {
                    let r = match token {
                        "SOL" => w(W::SendSol { name: name.into(), amount: d(x) }),
                        "USDG" => w(W::SendTo { name: name.into(), amount: d(x), network: Some(net.into()) }),
                        _ => token_send(n(x)),
                    };
                    add!("send token", format!("send {x} {token} on {net} to {name}"), r);
                }
            }
        }
    }

    // SPOT ON SOLANA, NEAR, STOCKS, TOKENS
    for (i, t) in ["SOL", "BTC", "ETH", "JUP", "JitoSOL"].into_iter().enumerate() {
        let asset = &all[i];
        add!("spot", format!("buy {t}"), w(W::Buy { asset: asset.clone(), usdc: None }));
        for &(a, v) in &amounts { add!("spot", format!("buy {t} for {a} USDC"), w(W::Buy { asset: asset.clone(), usdc: Some(v) })) }
        for (every, seconds) in [("day", 86400), ("week", 604_800), ("3 days", 259_200), ("hour", 3600)] {
            for orders in [2, 4] {
                add!("jupiter dca", format!("Jupiter DCA 200 USDC into {t} in {orders} orders every {every}"),
                     w(W::JupiterDca { asset: asset.clone(), usdc: n("200"), orders: Some(orders), every: seconds }));
            }
        }
        add!("jupiter dca", format!("cancel Jupiter DCA {t}"), w(W::JupiterDcaCancel(Some(asset.clone()))));
        if t == "JitoSOL" { continue }
        add!("spot", format!("sell {t}"), w(W::Sell { asset: asset.clone(), amount: None }));
        for &(a, v) in &amounts { add!("spot", format!("sell {a} {t}"), w(W::Sell { asset: asset.clone(), amount: Some(v) })) }
        for &(a, v) in &amounts {
            for (every, days) in [("day", 1), ("week", 7), ("3 days", 3), ("10 days", 10), ("2 weeks", 14), ("month", 30), ("2 months", 60)] {
                add!("dca", format!("buy {t} for {a} USDC every {every}"), w(W::Schedule { asset: asset.clone(), usdc: v, days }));
            }
        }
    }
    add!("dca", "stop recurring buys".into(), w(W::StopSchedules));
    add!("jupiter dca", "cancel Jupiter DCA".into(), w(W::JupiterDcaCancel(None)));
    add!("jupiter dca", "show Jupiter DCA".into(), w(W::JupiterDcaOrders));
    add!("jupiter dca", "Jupiter DCA 100 USDC into BONK in 2 orders every day".into(),
         w(W::JupiterDca { asset: SpotAsset::by_symbol("BONK"), usdc: n("100"), orders: Some(2), every: 86400 }));
    for t in ["BONK", "WIF", "USDT", "JTO", "PYTH", "POPCAT", "RENDER", "RAY", "TRUMP"] {
        for &(a, v) in &amounts { add!("jupiter token", format!("buy {t} on solana for {a} USDC"), w(W::Buy { asset: SpotAsset::by_symbol(t), usdc: Some(v) })) }
        add!("jupiter token", format!("sell {t} on solana"), w(W::Sell { asset: SpotAsset::by_symbol(t), amount: None }));
        for &(a, v) in &amounts { add!("jupiter token", format!("sell {a} {t} on solana"), w(W::Sell { asset: SpotAsset::by_symbol(t), amount: Some(v) })) }
    }
    for &(a, v) in &amounts {
        add!("near", format!("buy NEAR for {a} USDC"), w(W::BuyNear { usdc: Some(v) }));
        add!("near", format!("sell {a} NEAR"), w(W::SellNear { amount: Some(v), to: None }));
    }
    add!("near", "sell NEAR".into(), w(W::SellNear { amount: None, to: None }));
    let stocks = [("TSLA", "TSLA"), ("Tesla", "TSLA"), ("NVDA", "NVDA"), ("Nvidia", "NVDA"), ("SPCX", "SPCX"), ("SpaceX", "SPCX"), ("AAPL", "AAPL"),
                  ("Apple", "AAPL"), ("GOOGL", "GOOGL"), ("Google", "GOOGL"), ("HOOD", "HOOD"), ("META", "META"), ("Coinbase", "COIN"), ("AMZN", "AMZN"),
                  ("PLTR", "PLTR"), ("Microsoft", "MSFT")];
    for (word, symbol) in stocks {
        for &(a, v) in &amounts { add!("stock", format!("buy {word} stock for {a} USDC"), w(W::BuyStock { symbol: symbol.into(), usdc: Some(v) })) }
        add!("stock", format!("sell {symbol} stock"), w(W::SellStock { symbol: symbol.into(), amount: None }));
        add!("stock", format!("sell 2 {symbol} stock"), w(W::SellStock { symbol: symbol.into(), amount: n("2") }));
        add!("stock", format!("sell 0.132082 {symbol} stock"), w(W::SellStock { symbol: symbol.into(), amount: n("0.132082") }));
    }
    for &(a, v) in &amounts {
        add!("contract", format!("buy token {CONTRACT} for {a} USDC"), w(W::BuyStock { symbol: CONTRACT.into(), usdc: Some(v) }));
        add!("mint", format!("buy token {MINT} for {a} USDC"), w(W::Buy { asset: SpotAsset::by_mint(MINT), usdc: Some(v) }));
        for net in ["base", "arbitrum", "polygon", "ethereum"] {
            add!("contract", format!("buy token {CONTRACT} on {net} for {a} USDC"), w(W::BuyOn { place: net.into(), token: CONTRACT.into(), usdc: Some(v), from: None }));
        }
    }
    add!("contract", format!("sell tokens {CONTRACT}"), w(W::SellStock { symbol: CONTRACT.into(), amount: None }));
    add!("contract", format!("sell 1973 tokens {CONTRACT}"), w(W::SellStock { symbol: CONTRACT.into(), amount: n("1973") }));
    add!("mint", format!("sell tokens {MINT}"), w(W::Sell { asset: SpotAsset::by_mint(MINT), amount: None }));
    add!("mint", format!("sell 1000 tokens {MINT}"), w(W::Sell { asset: SpotAsset::by_mint(MINT), amount: n("1000") }));
    for net in ["base", "arbitrum", "polygon", "ethereum"] {
        add!("contract", format!("sell tokens {CONTRACT} on {net}"), w(W::SellOn { place: net.into(), token: CONTRACT.into(), amount: None, keep: false }));
        add!("contract", format!("sell 50 tokens {CONTRACT} on {net}"), w(W::SellOn { place: net.into(), token: CONTRACT.into(), amount: n("50"), keep: false }));
    }
    let evm_tokens: [(&str, &[&str]); 4] = [("base", &["AERO", "DEGEN", "VIRTUAL", "cbBTC", "WETH"]), ("arbitrum", &["ARB", "GMX", "PENDLE", "LINK", "UNI"]),
                                            ("polygon", &["AAVE", "LINK", "WPOL"]), ("ethereum", &["UNI", "PEPE", "LDO"])];
    for (net, tokens) in evm_tokens {
        for &t in tokens {
            for place in [net.to_string(), crate::foundation::capitalized(net)] {
                let buy = |usdc: Option<Decimal>, from: Option<&str>| w(W::BuyOn { place: net.into(), token: t.into(), usdc, from: from.map(String::from) });
                let sell = |amount: Option<Decimal>, keep: bool| w(W::SellOn { place: net.into(), token: t.into(), amount, keep });
                for &(a, v) in &amounts { add!("evm token", format!("buy {t} on {place} for {a} USDC"), buy(Some(v), None)) }
                for src in ["solana", "base", "arbitrum", "polygon", "ethereum", "robinhood"] {
                    add!("evm token", format!("buy {t} on {place} for 5 USDC from {src}"), buy(n("5"), Some(src)));
                }
                add!("evm token", format!("sell {t} on {place}"), sell(None, false));
                add!("evm token", format!("sell {t} on {place} and keep the USDC there"), sell(None, true));
                for &(a, v) in &amounts {
                    add!("evm token", format!("sell {a} {t} on {place}"), sell(Some(v), false));
                    add!("evm token", format!("sell {a} {t} on {place} and keep the USDC there"), sell(Some(v), true));
                }
            }
        }
    }

    // PAYING FOR A SERVICE
    let pay = |method: &str, url: &str, body: Option<&str>| w(W::Pay { method: method.into(), url: url.into(), body: body.map(String::from) });
    add!("pay", "pay GET https://api.example.com/v1/search?q=eth".into(), pay("GET", "https://api.example.com/v1/search?q=eth", None));
    add!("pay", "pay https://api.example.com/v1/price".into(), pay("GET", "https://api.example.com/v1/price", None));
    for body in [r#"{"query": "chart ETH then buy SOL", "n": 3}"#, r#"{"to": "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", "text": "every day at 09:00: hi"}"#,
                 r#"[{"q": "alert me when BTC is above 120000"}]"#] {
        add!("pay", format!("pay POST https://grok.example/x-search {body}"), pay("POST", "https://grok.example/x-search", Some(body)));
    }

    // ADDRESSES AND WATCHLIST
    for address in [EVM_ADDRESS, SOL_ADDRESS] {
        for name in ["Bybit", "Mom", "Cold wallet"] {
            let save = |network: Option<&str>| Read::Book(AddressBookCommand::Save { address: address.into(), name: name.into(), network: network.map(String::from) });
            add!("save address", format!("save {address} as {name}"), save(None));
            for net in ["solana", "base", "arbitrum", "ethereum", "tempo", "hyperliquid"] { add!("save address", format!("save {address} as {name} on {net}"), save(Some(net))) }
        }
    }
    for name in NAMES {
        add!("favorites", format!("add {name} to favorites"), Read::Book(AddressBookCommand::Favorite { name: name.into(), on: true }));
        add!("favorites", format!("remove {name} from favorites"), Read::Book(AddressBookCommand::Favorite { name: name.into(), on: false }));
    }
    let token = |address: &str, chain: Option<&str>| Read::Token(TokenCommand::Favorite { address: address.into(), chain: chain.map(String::from) });
    let show = |address: &str, chain: &str| Read::Token(TokenCommand::Show { address: address.into(), chain: Some(chain.into()) });
    add!("watch", format!("watch token {CONTRACT}"), token(CONTRACT, None));
    add!("watch", format!("watch token {MINT}"), token(MINT, Some("solana")));
    for chain in ["robinhood", "base", "arbitrum", "ethereum"] {
        add!("watch", format!("watch token {CONTRACT} on {chain}"), token(CONTRACT, Some(chain)));
        add!("token card", format!("chart token {CONTRACT} on {chain}"), show(CONTRACT, chain));
    }
    add!("watch", format!("watch token {MINT} on solana"), token(MINT, Some("solana")));
    add!("token card", format!("chart token {MINT} on solana"), show(MINT, "solana"));
    for s in ["AI", "MOO", "BONK"] { add!("watch", format!("remove token {s} from favorites"), Read::Token(TokenCommand::Unfavorite { symbol: s.into() })) }

    // MARKET (P/L pending)
    let assets = [("BTC", "BTC"), ("ETH", "ETH"), ("SOL", "SOL"), ("HYPE", "HYPE"), ("oil", "xyz:CL"), ("brent", "xyz:BRENTOIL"), ("gold", "xyz:GOLD"),
                  ("silver", "xyz:SILVER"), ("TSLA", "xyz:TSLA"), ("NVDA", "xyz:NVDA"), ("PEPE", "kPEPE"), ("BONK", "kBONK"), ("AAVE", "AAVE"),
                  ("FARTCOIN", "FARTCOIN"), ("SPCX", "xyz:SPCX"), ("SP500", "xyz:SP500"), ("brent oil", "xyz:BRENTOIL"), ("WTI", "xyz:CL"),
                  ("LINK", "LINK"), ("NEAR", "NEAR")];
    let chart = |coin: &str, range: Range| Read::Chart(ChartCommand::Chart { coin: coin.into(), range });
    let set = |coin: &str, price: Decimal, above: Option<bool>, steps: Vec<String>| Read::Alert(AlertCommand::Set { coin: coin.into(), price, above, steps });
    let sides = [("above", Some(true)), ("below", Some(false)), ("at", None)];
    for (word, coin) in assets {
        add!("chart", format!("chart {word}"), chart(coin, Range::Day));
        add!("chart", format!("chart {word} week"), chart(coin, Range::Week));
        add!("chart", format!("chart {word} month"), chart(coin, Range::Month));
        for (side, above) in sides {
            for (p, price) in [("2680", "2680"), ("2,680", "2680"), ("93.5", "93.5"), ("0.000012", "0.000012"), ("117,000", "117000")] {
                add!("alert", format!("alert me when {word} is {side} {p}"), set(coin, d(price), above, vec![]));
            }
        }
    }
    add!("chart", "what's moving".into(), Read::Chart(ChartCommand::Movers));
    add!("alert", "cancel alerts".into(), Read::Alert(AlertCommand::Clear));

    // APP, FOLDERS
    for c in ["new canvas", "clear the plan", "lock", "create an automation wallet"] { add!("app", c.into(), Read::App(c.into())) }
    for folder in ["Research", "Ideas", "Мои идеи", "Q4 plans"] {
        add!("folder", format!("save this canvas to folder {folder}"), Read::Folder(FolderCommand::Save { folder: folder.into() }));
        for text in ["buy ETH below 2500".to_string(), "chart ETH looks bullish, watch 2,700".into(), "alert me when BTC is above 120000".into(),
                     "my pnl this week was bad".into(), format!("token {CONTRACT} looks interesting"), "long SOL then short BTC later".into(),
                     "every day at 09:00 check rates".into(), "remember: rebalance on Monday".into(), "show me gold month".into(),
                     "cancel alerts before the Fed".into(), "add Mom to favorites".into(), format!("save {EVM_ADDRESS} as Bybit")] {
            add!("folder note", format!("write to folder {folder}: {text}"), Read::Folder(FolderCommand::Note { folder: folder.into(), text }));
        }
    }

    // NOW, IN STEPS
    let chainable: Vec<(String, Read)> = vec![
        ("withdraw 5 USDC to Solana".into(), w(W::WithdrawToSolana(n("5")))),
        ("deposit 5 USDC to kamino".into(), w(W::Kamino { deposit: true, symbol: "USDC".into(), amount: n("5") })),
        ("move 50 USDC from Ethereum to Solana".into(), w(W::EvmToSolana("ethereum".into(), n("50")))),
        ("buy SOL for 30 USDC".into(), w(W::Buy { asset: all[0].clone(), usdc: n("30") })),
        ("buy JUP for 20 USDC".into(), w(W::Buy { asset: all[3].clone(), usdc: n("20") })),
        ("lend 300 USDC on Aave on Arbitrum".into(), w(W::Aave { deposit: true, amount: n("300"), network: "arbitrum".into() })),
        ("spread 5 USDC from Solana across the best rates".into(),
         w(W::Allocate { symbol: "USDC".into(), max_share: None, amount: n("5"), without: vec![], from: Some("solana".into()) })),
        ("move 200 USDC from Ethereum to Base".into(), w(W::Bridge { from: "ethereum".into(), to: "base".into(), amount: n("200") })),
        ("deposit 200 USDC into Morpho on Base".into(), w(W::Morpho { deposit: true, amount: n("200"), network: "base".into() })),
        ("sell SOL".into(), w(W::Sell { asset: all[0].clone(), amount: None })),
        ("lend USDC".into(), w(W::LendDeposit("USDC".into(), None))),
        ("send all RAY on solana to Bybit".into(), w(W::SendToken { name: "Bybit".into(), token: "RAY".into(), amount: None, network: Some("solana".into()) })),
        ("withdraw USDC from kamino".into(), w(W::Kamino { deposit: false, symbol: "USDC".into(), amount: None })),
        ("chart ETH".into(), chart("ETH", Range::Day)),
        ("move 5 USDC from Base to Robinhood".into(), w(W::Bridge { from: "base".into(), to: "robinhood".into(), amount: n("5") })),
        (format!("buy token {CONTRACT} for 5 USDC"), w(W::BuyStock { symbol: CONTRACT.into(), usdc: n("5") })),
        ("buy AERO on base for 3 USDC".into(), w(W::BuyOn { place: "base".into(), token: "AERO".into(), usdc: n("3"), from: None })),
        ("withdraw 1 USDC from HLP".into(), w(W::Hlp { deposit: false, amount: n("1") })),
    ];
    for (a, ra) in &chainable {
        for (b, rb) in &chainable {
            if b != a { add!("steps", format!("{a} then {b}"), Read::Steps(vec![ra.clone(), rb.clone()])) }
        }
    }
    let k = chainable.len();
    for i in 0..k {
        let ((a, ra), (b, rb), (c, rc)) = (&chainable[i], &chainable[(i + 3) % k], &chainable[(i + 7) % k]);
        add!("steps", format!("{a} then {b} then {c}"), Read::Steps(vec![ra.clone(), rb.clone(), rc.clone()]));
    }

    // LATER OR ON REPEAT
    let heads = [
        ("in 3 days", None), ("in 1 week", None), ("in 30 minutes", None), ("in 2 hours", None), ("in 1 month", None), ("at 18:30", None), ("tomorrow", None),
        ("tomorrow at 10:00", None), ("on 2027-01-15", None), ("on 2027-01-15 at 08:15", None), ("every hour", Some(Every::Hours(1))),
        ("every 4 hours", Some(Every::Hours(4))), ("every day", Some(Every::Days(1))), ("every day at 09:00", Some(Every::Days(1))),
        ("every week", Some(Every::Days(7))), ("every 2 weeks", Some(Every::Days(14))), ("every month", Some(Every::Months(1))),
        ("every 3 months", Some(Every::Months(3))), ("every monday", Some(Every::Days(7))), ("every friday at 17:45", Some(Every::Days(7))),
        ("every month on the 1st", Some(Every::Months(1))), ("every month on the 15th at 12:00", Some(Every::Months(1))), ("every 3 days", Some(Every::Days(3))),
    ];
    let mut long_eth = TradeIntent::default();
    long_eth.leverage = n("3");
    let bodies: Vec<(&str, Vec<Read>)> = vec![
        ("rebalance USDC", vec![w(W::Rebalance { max_share: None })]),
        ("sell SOL then lend USDC", vec![w(W::Sell { asset: all[0].clone(), amount: None }), w(W::LendDeposit("USDC".into(), None))]),
        ("send 20 USDC to Mom", vec![w(W::SendTo { name: "Mom".into(), amount: d("20"), network: None })]),
        ("sweep rewards over $10 into USDC", vec![w(W::SweepRewards { min_usd: n("10"), into: None })]),
        ("buy SOL for 5 USDC", vec![w(W::Buy { asset: all[0].clone(), usdc: n("5") })]),
        ("spread USDC across the best rates", vec![w(W::Allocate { symbol: "USDC".into(), max_share: None, amount: None, without: vec![], from: None })]),
        ("long ETH leverage 3x", vec![Read::Plan(long_eth.started())]),
        ("move 5 USDC from Base to Solana then deposit 5 USDC to kamino",
         vec![w(W::BaseToSolana(n("5"))), w(W::Kamino { deposit: true, symbol: "USDC".into(), amount: n("5") })]),
        ("claim rewards", vec![w(W::ClaimRewards)]),
        // pending: ("profit and loss week", the P/L board) — PnLCommand (round 5).
    ];
    for (head, every) in heads {
        for (body, steps) in &bodies { add!("task", format!("{head}: {body}"), Read::Task(every, steps.clone())) }
    }
    add!("task", "cancel scheduled tasks".into(), Read::CancelTasks);
    for (word, coin) in [("ETH", "ETH"), ("BTC", "BTC"), ("oil", "xyz:CL"), ("SOL", "SOL"), ("gold", "xyz:GOLD")] {
        for (side, above) in sides {
            for (p, price) in [("2680", "2680"), ("2,680", "2680"), ("93.5", "93.5")] {
                for body in ["long ETH leverage 3x", "buy SOL for 5 USDC", "sell SOL then lend USDC", "close ETH", "send 20 USDC to Mom"] {
                    add!("alert steps", format!("when {word} is {side} {p}: {body}"), set(coin, d(price), above, body.split(" then ").map(String::from).collect()));
                }
            }
        }
    }

    // TRADE PLAN: random combinations of every optional part, in the form's order.
    let mut rng = Seeded(20_261_004);
    let plan_assets = [("BTC", "BTC"), ("ETH", "ETH"), ("SOL", "SOL"), ("HYPE", "HYPE"), ("oil", "xyz:CL"), ("brent", "xyz:BRENTOIL"), ("gold", "xyz:GOLD"),
                       ("silver", "xyz:SILVER"), ("TSLA", "xyz:TSLA"), ("NVDA", "xyz:NVDA"), ("PEPE", "kPEPE"), ("BONK", "kBONK"), ("kPEPE", "kPEPE"),
                       ("AAVE", "AAVE"), ("LINK", "LINK"), ("NEAR", "NEAR"), ("MOVE", "MOVE"), ("FARTCOIN", "FARTCOIN"), ("SPCX", "xyz:SPCX"),
                       ("SP500", "xyz:SP500"), ("ethereum", "ETH"), ("bitcoin", "BTC"), ("2Z", "2Z"), ("0G", "0G"), ("XYZ100", "xyz:XYZ100"),
                       ("brent oil", "xyz:BRENTOIL"), ("WTI", "xyz:CL"), ("NATGAS", "xyz:NATGAS"), ("COPPER", "xyz:COPPER"), ("JPY", "xyz:JPY"),
                       ("ME", "ME"), ("NOW", "xyz:NOW"), ("HOOD", "xyz:HOOD"), ("MSTR", "xyz:MSTR"), ("VIRTUAL", "VIRTUAL"), ("PUMP", "PUMP")];
    let pairs = |items: &[(&'static str, &str)]| items.iter().map(|(s, v)| (*s, d(v))).collect::<Vec<_>>();
    let money = pairs(&[("5", "5"), ("2.5", "2.5"), ("10", "10"), ("1,000", "1000"), ("50", "50"), ("0.5", "0.5")]);
    let risks = pairs(&[("0.5", "0.5"), ("1", "1"), ("2.5", "2.5"), ("10", "10"), ("0.25", "0.25")]);
    let pcts = pairs(&[("1", "1"), ("2.5", "2.5"), ("0.8", "0.8"), ("3", "3"), ("6.5", "6.5"), ("12", "12")]);
    let rs = pairs(&[("2", "2"), ("1.5", "1.5"), ("3", "3")]);
    let levs = pairs(&[("2", "2"), ("3", "3"), ("10", "10"), ("2.5", "2.5"), ("20", "20")]);
    let prices = pairs(&[("2650", "2650"), ("0.1234", "0.1234"), ("95000", "95000"), ("2,650", "2650"), ("67.5", "67.5"), ("117,250.5", "117250.5")]);
    for _ in 0..3_000 {
        let long = rng.chance(0.5);
        let (word, coin) = rng.pick(&plan_assets);
        let mut e = TradeIntent { asset: coin.into(), side: if long { Side::Long } else { Side::Short }, ..TradeIntent::default() };
        let mut parts = vec![format!("{} {word}", if long { "long" } else { "short" })];
        if rng.chance(0.5) { let (s, v) = rng.pick(&money); parts.push(format!("{s} USDC")); e.amount_usdc = Some(v) }
        if rng.chance(0.5) { let (s, v) = rng.pick(&risks); parts.push(format!("risk ${s}")); e.risk_usd = Some(v) }
        if rng.chance(0.5) { let (s, v) = rng.pick(&pcts); parts.push(format!("stop {s}%")); e.stop_pct = Some(v) }
        match rng.next() % 3 {
            1 => { let (s, v) = rng.pick(&pcts); parts.push(format!("take {s}%")); e.take_pct = Some(v) }
            2 => { let (s, v) = rng.pick(&rs); parts.push(format!("take {s}R")); e.take_r = Some(v) }
            _ => {}
        }
        if rng.chance(0.5) { let (s, v) = rng.pick(&levs); parts.push(format!("leverage {s}x")); e.leverage = Some(v) }
        if rng.chance(0.3) { parts.push("by the book".into()); e.auto_rules = Some(true) }
        match rng.next() % 3 {
            1 => { let (s, v) = rng.pick(&pcts); parts.push(format!("on a {s}% pullback")); e.pullback_pct = Some(v) }
            2 => { let (s, v) = rng.pick(&prices); parts.push(format!("limit {s}")); e.entry_px = Some(v) }
            _ => {}
        }
        if rng.chance(0.3) { parts.push("take in parts".into()); e.partial_take = Some(true) }
        if rng.chance(0.3) { let (s, v) = rng.pick(&pcts); parts.push(format!("trailing {s}%")); e.trailing_pct = Some(v) }
        add!("plan", parts.join(" "), Read::Plan(e.started()));
    }

    // A change alone, to a plan already on the canvas.
    let base = TradeIntent { asset: "SOL".into(), risk_usd: n("1"), stop_pct: n("2"), ..TradeIntent::default() };
    let changed = |f: &dyn Fn(&mut TradeIntent)| {
        let mut i = base.clone();
        f(&mut i);
        Read::Plan(i.started())
    };
    let mut changes: Vec<(String, Read)> = vec![];
    for &(s, v) in &money { changes.push((format!("{s} USDC"), changed(&|i| i.amount_usdc = Some(v)))) }
    for &(s, v) in &risks { changes.push((format!("risk ${s}"), changed(&|i| i.risk_usd = Some(v)))) }
    for &(s, v) in &pcts {
        changes.push((format!("stop {s}%"), changed(&|i| i.stop_pct = Some(v))));
        changes.push((format!("take {s}%"), changed(&|i| i.take_pct = Some(v))));
        changes.push((format!("trailing {s}%"), changed(&|i| i.trailing_pct = Some(v))));
        changes.push((format!("on a {s}% pullback"), changed(&|i| i.pullback_pct = Some(v))));
    }
    for &(s, v) in &rs { changes.push((format!("take {s}R"), changed(&|i| i.take_r = Some(v)))) }
    for &(s, v) in &levs { changes.push((format!("leverage {s}x"), changed(&|i| i.leverage = Some(v)))) }
    for &(s, v) in &prices { changes.push((format!("limit {s}"), changed(&|i| i.entry_px = Some(v)))) }
    changes.push(("at market".into(), changed(&|_| {})));
    changes.push(("by the book".into(), changed(&|i| i.auto_rules = Some(true))));
    changes.push(("take in parts".into(), changed(&|i| i.partial_take = Some(true))));
    for (c, r) in changes { add("plan change", c, r, Some(&base), false) }
    for &(s, v) in &money {
        for &(l, x) in &levs {
            let r = changed(&|i| {
                i.amount_usdc = Some(v);
                i.leverage = Some(x)
            });
            add("plan change", format!("{s} USDC leverage {l}x"), r.clone(), Some(&base), false);
            add("plan change", format!("{s} USDC then leverage {l}x"), r, Some(&base), false);
        }
    }
    add("take choice", "take profit options".into(), Read::TakeChoice, Some(&base), false);

    // OPEN POSITION
    let pos = |c: &str, p: PositionCommand| (c.to_string(), Read::Position(p));
    let mut open: Vec<(String, Read)> = vec![];
    for c in ["close", "close ETH", "close oil", "close BTC"] { open.push(pos(c, PositionCommand::Close(Decimal::ONE))) }
    for c in ["close half", "close half ETH", "close half of BTC"] { open.push(pos(c, PositionCommand::Close(d("0.5")))) }
    for (c, f) in [("close 25%", "0.25"), ("close 25% ETH", "0.25"), ("close 33.3% of SOL", "0.333"), ("close 100%", "1"), ("close 10% oil", "0.1")] {
        open.push(pos(c, PositionCommand::Close(d(f))));
    }
    for c in ["move stop to breakeven", "move stop to breakeven on ETH"] { open.push(pos(c, PositionCommand::StopToBreakeven)) }
    for c in ["cancel orders", "cancel orders on ETH", "cancel orders ETH"] { open.push(pos(c, PositionCommand::CancelOrders)) }
    for &(p, v) in &prices {
        open.push(pos(&format!("stop at {p}"), PositionCommand::StopTo(v)));
        open.push(pos(&format!("take at {p}"), PositionCommand::TakeTo(v)));
        open.push(pos(&format!("stop at {p} on ETH"), PositionCommand::StopTo(v)));
    }
    for &(p, v) in &pcts {
        open.push(pos(&format!("trail {p}%"), PositionCommand::Trail(v)));
        open.push(pos(&format!("trail {p}% on ETH"), PositionCommand::Trail(v)));
    }
    for (c, r) in open { add("position", c, r, None, true) }
    out
}

/// The same command as a model may write it: capitalized, or with a full stop.
fn variants(c: FormCase) -> Vec<FormCase> {
    let mut first = c.command.chars();
    let capital = first.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + first.as_str());
    let mut out = vec![c.clone(), FormCase { command: capital, ..c.clone() }];
    if !["pay", "folder note"].contains(&c.family.as_str()) {
        // An alert keeps its steps as written, the full stop with the last one.
        let mut expected = c.expected.clone();
        if let Read::Alert(AlertCommand::Set { steps, .. }) = &mut expected
            && let Some(last) = steps.last_mut()
        {
            last.push('.');
        }
        out.push(FormCase { family: format!("{} .", c.family), command: format!("{}.", c.command), expected, ..c });
    }
    out
}

/// Every 25th command by default (as Swift samples it); `FORMS_MATRIX=full` reads them all.
fn sampled() -> Vec<FormCase> {
    let all: Vec<FormCase> = form_cases().into_iter().flat_map(variants).collect();
    let full = std::env::var("FORMS_MATRIX").is_ok_and(|v| v == "full");
    all.into_iter().enumerate().filter(|(i, _)| full || i % 25 == 0).map(|(_, c)| c).collect()
}

#[test]
fn every_form_reads_as_it_says() {
    let cases = sampled();
    let failed: Vec<String> = cases
        .iter()
        .filter_map(|c| {
            let got = read(&c.command, &c.plan, c.position_open);
            (got != c.expected).then(|| format!("{}: «{}»\n      got      {got:?}\n      expected {:?}", c.family, c.command, c.expected))
        })
        .collect();
    assert!(cases.len() > 1_000);
    assert!(failed.is_empty(), "forms matrix: {} commands, {} misread:\n{}", cases.len(), failed.len(), failed[..failed.len().min(8)].join("\n"));
}

/// Every form ChatRouter.parses accepts, so the router never sends one back to the model as «can't run».
#[test]
fn every_form_parses_for_the_router() {
    let rejected: Vec<String> =
        sampled().into_iter().filter(|c| c.family != "take choice" && !ChatRouter::parses(&c.command, &MARKETS)).map(|c| c.command).collect();
    assert!(rejected.is_empty(), "parses() rejects {}: {:?}", rejected.len(), &rejected[..rejected.len().min(20)]);
}

#[test]
fn jupiter_dca_forms_reach_the_wallet_reader_first() {
    let none = TradeIntent::default();
    let sol = SpotAsset::all()[0].clone();
    for (every, seconds) in [("day", 86400), ("week", 604_800), ("3 days", 259_200), ("hour", 3600), ("month", 2_592_000)] {
        for asset in SpotAsset::all() {
            let c = format!("Jupiter DCA 300 USDC into {} in 3 orders every {every}", asset.symbol);
            assert_eq!(read(&c, &none, false), Read::Wallet(W::JupiterDca { asset: asset.clone(), usdc: n("300"), orders: Some(3), every: seconds }), "{c}");
        }
    }
    assert_eq!(read("Jupiter DCA 300 USDC into SOL over 3 weeks", &none, false),
               Read::Wallet(W::JupiterDca { asset: sol.clone(), usdc: n("300"), orders: Some(3), every: 604_800 }));
    assert_eq!(read("cancel Jupiter DCA SOL", &none, false), Read::Wallet(W::JupiterDcaCancel(Some(sol.clone()))));
    assert_eq!(read("show Jupiter DCA", &none, false), Read::Wallet(W::JupiterDcaOrders));
    // A scheduled task can still hold one.
    match read("every month: Jupiter DCA 100 USDC into SOL in 2 orders every day", &none, false) {
        Read::Task(every, steps) => {
            assert!(every.is_some() && steps == [Read::Wallet(W::JupiterDca { asset: sol, usdc: n("100"), orders: Some(2), every: 86400 })])
        }
        other => panic!("not read as a task: {other:?}"),
    }
}
