//! `PositionCommand.swift`: what the user says about an open position (`PositionCommand`), the coins bought on Solana by
//! fixed mint (`SpotAsset`), every move of the user's own money (`WalletCommand`, its parser and its words), a regular
//! purchase (`Schedule`), and any contract's function the chat writes out (`ContractCall`).

use std::sync::LazyLock;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::address_book_command::AddressBookCommand;
use crate::chat_language::ChatWords;
use crate::foundation::{
    Caps, Date, Rounding, capitalized, chars, count, decimal, first, group, has, number, replacing, rounded, split, split_chars,
    text as decimal_text, trim_set, trim_spaces, whole,
};
use crate::intent::{Fmt, IntentParser};
use crate::stablecoins;
use crate::stock_names;

/// A group that took part in the match, or nil.
fn opt<'t>(c: &Caps<'t>, i: usize) -> Option<&'t str> { c.get(i).map(|m| m.as_str()) }

/// `Decimal(string: s.replacingOccurrences(of: ",", with: "."))`.
fn num(s: &str) -> Option<Decimal> { decimal(&s.replace(',', ".")) }

/// The first group of the first match, as a number.
fn num_of(q: &str, pattern: &'static str) -> Option<Decimal> { first(q, pattern).and_then(|m| num(group(&m, 1))) }

/// «$5», «5 USDC», «5 долларов»: the dollars a message names.
fn dollars(q: &str) -> Option<Decimal> {
    first(q, r"\$\s*(\d+(?:[.,]\d+)?)").or_else(|| first(q, r"(\d+(?:[.,]\d+)?)\s*(?:usdc|usdg|\$|долл|dollars?|bucks|бакс)")).and_then(|m| num(group(&m, 1)))
}

/// What the user can say about a position that is already open («стоп в безубыток», «закрой половину»).
/// Deterministic like the plan parser; only checked while a position or order exists.
#[derive(Debug, Clone, PartialEq)]
pub enum PositionCommand {
    /// Share of the position to close, 0 < f ≤ 1.
    Close(Decimal),
    StopToBreakeven,
    StopTo(Decimal),
    TakeTo(Decimal),
    CancelOrders,
    /// Keep the stop N% behind the best price; ihatecrypto's guardian moves it, only ever tighter.
    Trail(Decimal),
}

impl PositionCommand {
    pub fn parse(text: &str) -> Option<Self> {
        let q = IntentParser::plain_numbers(&text.to_lowercase());
        // «should I close ETH?», «закрыть?», «не закрывай позицию», «don't close it»: a question or a «don't» is answered,
        // never carried out (without a model a close runs at once).
        if IntentParser::is_question(text)
            || has(&q, r"(?:^|[^\p{L}'’])(?:не|нельзя|don['’]?t|dont|do\s+not|never|not)(?![\p{L}'’])|should\s+(?:i|we)\b|стоит\s+ли|(?:а|что)\s+если")
        {
            return None;
        }
        // A TWAP still buying is an entry waiting too: «cancel the TWAP», «останови твап».
        if has(&q, r"(?:отмени|сними|останови|прекрати)\w*\s+(?:лимит|ордер|заявк|twap|твап)|(?:cancel|stop)\s+(?:the\s+|my\s+)?twap|cancel\s+(?:the\s+|my\s+)?(?:limit|order)|remove\s+(?:the\s+)?(?:limit|order)") {
            return Some(Self::CancelOrders);
        }
        if has(&q, r"безубыт|стоп\w*\s+(?:в|на)\s+(?:ноль|бу\b|вход)|break\s*-?\s*even|stop\s+(?:to|at)\s+entry") { return Some(Self::StopToBreakeven) }
        if let Some(v) = num_of(&q, r"(?:(?:перенес\w*|передвин\w*|подвин\w*|постав\w*)?\s*стоп\w*\s+(?:на|в)|(?:move|set|put)?\s*(?:the\s+)?stop(?:[- ]?loss)?\s+(?:to|at))\s+\$?(\d+(?:[.,]\d+)?)(?!\s*%|\d|[.,]\d)") {
            return Some(Self::StopTo(v));
        }
        if let Some(v) = num_of(&q, r"(?:(?:перенес\w*|передвин\w*|подвин\w*|постав\w*)?\s*тейк\w*\s+(?:на|в)|(?:move|set|put)?\s*(?:the\s+)?take(?:[- ]?profit)?\s+(?:to|at))\s+\$?(\d+(?:[.,]\d+)?)(?!\s*%|\d|[.,]\d)") {
            return Some(Self::TakeTo(v));
        }
        // «trail 2%» about the open position; a new plan with a trailing stop («long ETH trailing 2%») is not this.
        if !has(&q, r"лонг|шорт|\blong\b|\bshort\b")
            && let Some(v) = num_of(&q, r"(?:трейл\w*|trail\w*)[^\d]{0,15}?(\d+(?:[.,]\d+)?)\s*%")
            && v > Decimal::ZERO
            && v < Decimal::from(50)
        {
            return Some(Self::Trail(v));
        }
        if !has(&q, r"закр\w*|выйди|выход\w*\s+из|\bclose\b|\bexit\b") { return None }
        if has(&q, r"половин|50\s*%|\bhalf\b") { return Some(Self::Close(Decimal::new(5, 1))) }
        // ponytail: a third to Decimal's 28 digits; Swift's has 38.
        if has(&q, r"треть|\bthird\b") { return Some(Self::Close(Decimal::ONE / Decimal::from(3))) }
        if has(&q, r"четверть|\bquarter\b") { return Some(Self::Close(Decimal::new(25, 2))) }
        if let Some(v) = num_of(&q, r"(\d+(?:[.,]\d+)?)\s*%") && v > Decimal::ZERO && v <= Decimal::ONE_HUNDRED {
            return Some(Self::Close(v / Decimal::ONE_HUNDRED));
        }
        Some(Self::Close(Decimal::ONE))
    }
}

/// Coins to buy and hold on Solana, by fixed verified mint: a search by ticker returns look-alikes. With Safe search
/// off, any token by its mint too: parsed with its mint alone, then looked up (decimals, symbol, token program).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotAsset {
    pub symbol: String,
    pub mint: String,
    pub decimals: i64,
    /// A Token-2022 mint: its accounts derive from that program, not the classic one.
    pub token2022: bool,
    /// Ondo's own token, as Jupiter's verified list vouches for it (Ondo.issued alone is a mint suffix anyone can grind).
    pub ondo: bool,
}

/// Checked 2026-09-25 against Jupiter's verified list (isVerified true).
static ALL: LazyLock<[SpotAsset; 5]> = LazyLock::new(|| {
    [
        SpotAsset::new("SOL", "So11111111111111111111111111111111111111112", 9),
        SpotAsset::new("BTC", "cbbtcf3aa214zXHbiAZQwf4122FBYbraNdFqgw4iMij", 8), // Coinbase Wrapped BTC
        SpotAsset::new("ETH", "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs", 8), // Ether (Portal)
        SpotAsset::new("JUP", "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", 6),
        // Liquid staking: SOL that earns the staking yield and can be sold back any time.
        SpotAsset::new("JitoSOL", "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn", 9),
    ]
});

impl SpotAsset {
    pub fn new(symbol: &str, mint: &str, decimals: i64) -> Self {
        Self { symbol: symbol.into(), mint: mint.into(), decimals, token2022: false, ondo: false }
    }

    /// The fixed verified coins.
    pub fn all() -> &'static [SpotAsset] { &ALL[..] }

    pub fn usdc() -> Self { Self::new("USDC", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", 6) }

    pub fn jito_sol() -> Self { ALL[4].clone() }

    /// A token named by its mint only, until the app looks it up.
    pub fn by_mint(mint: &str) -> Self {
        let c = chars(mint);
        Self::new(&format!("{}…{}", c[..4.min(c.len())].concat(), c[c.len().saturating_sub(4)..].concat()), mint, 0)
    }

    /// A token named by its ticker only («BONK»): the app finds the Jupiter-verified token trading under it.
    pub fn by_symbol(symbol: &str) -> Self {
        let s = trim_set(symbol, "$");
        ALL.iter().find(|a| a.symbol.to_uppercase() == s.to_uppercase()).cloned().unwrap_or_else(|| Self::new(&s.to_uppercase(), "", 0))
    }

    pub fn resolved(&self) -> bool { self.decimals > 0 }

    /// One of the fixed verified coins (or USDC): the only ones bought while Safe search is on.
    pub fn verified(&self) -> bool { *self == Self::usdc() || ALL.iter().any(|a| a.mint == self.mint) }

    pub(crate) fn find(q: &str) -> Option<Self> {
        let i = if has(q, "jitosol|джито") { 4 }
            else if has(q, r"\bsol\b|\bсол\b|\bсолу\b|солан|solana") { 0 }
            else if has(q, r"\bbtc\b|биткоин|битк|bitcoin") { 1 }
            else if has(q, r"\beth\b|эфир|ether") { 2 }
            else if has(q, r"\bjup\b|юпитер|джуп|jupiter") { 3 }
            else { return None };
        Some(ALL[i].clone())
    }
}

/// Moving the user's own money: back to Solana, into the USDC vault and out, spot buys and sells.
#[derive(Debug, Clone, PartialEq)]
pub enum WalletCommand {
    /// Spend `usdc` on the coin, on Solana through Jupiter; nil = ask how much.
    Buy { asset: SpotAsset, usdc: Option<Decimal> },
    /// Native NEAR on the NEAR chain for Solana USDC through NEAR Intents 1Click, to the wallet's own NEAR account; nil =
    /// ask how much.
    BuyNear { usdc: Option<Decimal> },
    /// Native NEAR from the NEAR account for USDC, through 1Click; nil = all it can give. `to`: the EVM network the USDC
    /// arrives on at our own address (Base, Arbitrum, Ethereum: what 1Click pays there), nil = the Solana address.
    SellNear { amount: Option<Decimal>, to: Option<String> },
    /// Sell this much of the coin for USDC; nil = all of it (SOL keeps a fee reserve).
    Sell { asset: SpotAsset, amount: Option<Decimal> },
    /// Regular purchases (DCA): `usdc` of the coin every `days` days.
    Schedule { asset: SpotAsset, usdc: Decimal, days: i64 },
    /// Jupiter's on-chain DCA: `usdc` in all (nil = ask) into the coin, in `orders` swaps (nil = one per 50 USDC, 2 to 10)
    /// `every` seconds apart. The whole amount goes into an order account of the wallet's own at once; Jupiter's keepers
    /// buy, and the coin comes to the wallet after each swap — the app needn't run.
    JupiterDca { asset: SpotAsset, usdc: Option<Decimal>, orders: Option<i64>, every: i64 },
    /// Running Jupiter DCA orders closed, the USDC they haven't spent back; nil = all, else those buying this coin.
    JupiterDcaCancel(Option<SpotAsset>),
    /// The running Jupiter DCA orders, on the canvas.
    JupiterDcaOrders,
    StopSchedules,
    /// nil = everything withdrawable.
    WithdrawToSolana(Option<Decimal>),
    /// Jupiter Lend: symbol («USDC», «SOL», «USDT»), amount in that asset; nil = ask how much.
    LendDeposit(String, Option<Decimal>),
    /// nil = the whole vault position.
    LendWithdraw(String, Option<Decimal>),
    /// Kamino Lend: the main market, and the Backpack market for Backpack's stocks (`KAMINO_STOCKS`), where a deposit is
    /// collateral to borrow against.
    Kamino { deposit: bool, symbol: String, amount: Option<Decimal> },
    /// USDC borrowed on Kamino — on the Backpack market against Backpack's tokenized stocks, or on the main market against
    /// SOL and the deposits there — and paid back (nil = all of it). `against`: what the loan stands on («SPCX», «SOL»);
    /// `adding`: that many of its tokens put up as collateral first (nil = what's there already).
    KaminoBorrow { amount: Option<Decimal>, against: Option<String>, adding: Option<Decimal> },
    /// `against`: the loan to pay back by what it stands on («SOL»: the main market, a stock: the Backpack one); nil = the
    /// Backpack market's first, then the main one's.
    KaminoRepay { amount: Option<Decimal>, against: Option<String> },
    /// SOL into JitoSOL (liquid staking) and back; nil = all.
    Stake(Option<Decimal>),
    Unstake(Option<Decimal>),
    /// Where the asset earns most now; nil = stablecoins and SOL.
    Yields(Option<String>),
    /// Hyperliquid's HLP vault: deposit or withdraw USDC (nil = all).
    Hlp { deposit: bool, amount: Option<Decimal> },
    /// «Разложи USDC по лучшим ставкам»: the asset and the most one protocol may hold, % (nil = 50). `amount`: spread at
    /// most this much of it (nil = all that is free). `without`: protocols left out, as asked («без kamino»): keys of
    /// `SPREAD_PROTOCOLS`. `from`: only this network's free money («с Base»: "base", or "solana"); nil = anywhere.
    Allocate { symbol: String, max_share: Option<Decimal>, amount: Option<Decimal>, without: Vec<String>, from: Option<String> },
    /// «Переложи в лучшие ставки»: USDC already deposited moves to better rates, at most this % per protocol (nil = 50),
    /// only where the extra interest pays the move back within 30 days.
    Rebalance { max_share: Option<Decimal> },
    /// USDC into Aave v3 on `network` («base», «arbitrum», «polygon», «ethereum»; bridged from Solana when short) or out
    /// of it and back to Solana; nil = ask (deposit) / everything (withdraw).
    Aave { deposit: bool, amount: Option<Decimal>, network: String },
    /// USDC borrowed on Aave against what the wallet supplied there, paid back (nil = all of it), and ETH supplied there as
    /// collateral; the network as for `Aave`.
    AaveBorrow { amount: Option<Decimal>, network: String },
    AaveRepay { amount: Option<Decimal>, network: String },
    AaveCollateral { amount: Option<Decimal>, network: String },
    /// The same with a Morpho vault («base», «arbitrum», «ethereum»).
    Morpho { deposit: bool, amount: Option<Decimal>, network: String },
    /// USDC idle on Base back to Solana over Relay; nil = everything.
    BaseToSolana(Option<Decimal>),
    /// The same from Arbitrum.
    ArbitrumToSolana(Option<Decimal>),
    /// The same from «polygon» or «ethereum».
    EvmToSolana(String, Option<Decimal>),
    /// «send 5 USDC to Bybit»: to an entry of the address book, by its name. An address in the text is never a recipient.
    /// `network`: the one the message names («… on ethereum», «… по эфир сети»); nil = where the entry is saved.
    SendTo { name: String, amount: Decimal, network: Option<String> },
    /// Native NEAR from the wallet's NEAR account to an address-book entry on NEAR; nil = all but the account's own
    /// storage and the fee.
    SendNear { name: String, amount: Option<Decimal> },
    /// Deletes the wallet's NEAR account; NEAR sends everything on it to the address-book entry `name`.
    CloseNear { name: String },
    /// Native SOL to an address-book entry on Solana; the amount is always said.
    SendSol { name: String, amount: Decimal },
    /// «send all RAY on solana to Bybit»: any token the wallet holds — by ticker, contract or mint — to an address-book
    /// entry, as it is, on the same network: no swap, no bridge. nil amount = all of it; `network` nil = where the entry is
    /// saved.
    SendToken { name: String, token: String, amount: Option<Decimal>, network: Option<String> },
    /// «send 5 USDC privately to Mom», «отправь приватно 0.001 BTC на Binance»: through NEAR Intents' Confidential Intents,
    /// so the deposit and the payout can't be matched on chain. USDC from Solana or an EVM network, paid as USDC on
    /// Solana, Base, Arbitrum or Ethereum, or as a coin on a network only NEAR Intents reaches; always with an amount.
    SendPrivately { name: String, token: String, amount: Decimal, network: Option<String> },
    /// An agent wallet on NEAR Intents (Agent API) owned by this wallet's NEAR key: «create an agent wallet with $20 a
    /// day» — it may send USDC only to this wallet's own addresses and the saved ones, at most `daily_usd` a day.
    AgentCreate { daily_usd: Decimal },
    /// USDC from Solana into the agent wallet.
    AgentFund { usdc: Decimal },
    /// USDC out of the agent wallet to a saved address (or this wallet's own), by the agent's grant alone: NEAR Intents
    /// checks it against the policy the owner signed, and refuses what it doesn't allow.
    AgentSend { usdc: Decimal, name: String },
    /// Freezes the agent wallet (nothing moves), or lets it act again.
    AgentFreeze(bool),
    /// «задепозить», «пополнить», «my address»: money coming in from outside — show where to send it.
    Receive,
    /// A Robinhood Stock Token (SPCX, TSLA…) bought on Robinhood Chain for USDC from Solana; `symbol` as the user named it,
    /// resolved against Robinhood's registry by the app. nil amount = ask.
    BuyStock { symbol: String, usdc: Option<Decimal> },
    /// A token on an EVM network, by ticker (Uniswap's list) or contract; nil usdc = ask how much. `from`: nil = where the
    /// money is; «solana»; that network itself; or another EVM network, whose USDC comes over first.
    BuyOn { place: String, token: String, usdc: Option<Decimal>, from: Option<String> },
    /// Such a token back to USDC on Solana, the better way again; nil = all of it. `keep`: the USDC stays on that network.
    SellOn { place: String, token: String, amount: Option<Decimal>, keep: bool },
    /// One token for another on that network, straight on Uniswap (v2, v3 or v4, the best), no USDC in the middle; either
    /// side may be the network's own coin (ETH, POL). nil amount = all of `from`.
    SwapOn { place: String, from: String, to: String, amount: Option<Decimal> },
    /// A token on one network for a token on another through Relay, one signature: «swap 0.1 SOL on Solana for ETH on
    /// Arbitrum». Places are «solana» or an EVM network's key; nil amount = all of a token.
    CrossSwap { from: String, on: String, to: String, at: String, amount: Option<Decimal> },
    /// A swap through NEAR Intents' Confidential Intents (asked for privately): the deposit and what arrives can't be
    /// matched on chain. Places as in `CrossSwap`, «near» for the NEAR account; always an amount, always this wallet's own
    /// addresses on both sides. «buy NEAR for 5 USDC privately» = USDC on Solana → NEAR on NEAR.
    SwapPrivately { from: String, on: String, to: String, at: String, amount: Decimal },
    /// Money coming in from a network ihatecrypto has no wallet on — «receive 50 USDC from Sui»: 1Click's deposit address
    /// there, what arrives swapped into USDC on Solana. `network` is the address book's («sui», «bitcoin», «tron», «xrp»,
    /// «dogecoin», «zcash»); nil amount = ask.
    ReceiveAcross { network: String, token: String, amount: Option<Decimal> },
    /// Sell a Stock Token for USDG on Robinhood Chain; nil = all of it.
    SellStock { symbol: String, amount: Option<Decimal> },
    /// USDG on Robinhood Chain back to Solana as USDC; nil = everything.
    RobinhoodToSolana(Option<Decimal>),
    /// USDC from the Solana wallet to our own address on «base», «arbitrum» or «robinhood» (USDG there); nil = ask.
    SolanaTo(String, Option<Decimal>),
    /// USDC from one EVM network straight to another over Relay, at our own address («base» → «robinhood», where it
    /// arrives as USDG), or to our Hyperliquid trading balance («hyperliquid»); nil = everything on `from`.
    Bridge { from: String, to: String, amount: Option<Decimal> },
    /// The same move through Circle's CCTP, asked for by name («… via CCTP»): Ethereum, Arbitrum, Base, Polygon.
    Cctp { from: String, to: String, amount: Option<Decimal> },
    /// USDC through NEAR Intents' 1Click, asked for by name («… via NEAR Intents»), whatever Relay would land: between
    /// Solana («solana»), Base, Arbitrum and Ethereum, at our own addresses.
    Intents { from: String, to: String, amount: Option<Decimal> },
    /// USDT between two of USDT0's networks at our own address: Relay's swap of it, seconds — or, asked for by name («… via
    /// LayerZero»), Tether's own bridge: the same amount on the other side for cents of gas, in minutes to hours. nil =
    /// all the USDT on `from`.
    Usdt { from: String, to: String, amount: Option<Decimal>, layer_zero: bool },
    /// The network's own coin for fees («robinhood», «base», …), `usd` worth of it over Relay; nil = $1.
    Gas { place: String, usd: Option<Decimal> },
    /// A paid API on Tempo (MPP): the request as the service documents it; the price comes from its 402 answer and
    /// is paid in USDC on Tempo only after Confirm. `method` «GET» or «POST»; `body` JSON or nil.
    Pay { method: String, url: String, body: Option<String> },
    /// «claim rewards», «забери награды»: what Merkl owes this wallet on Base, Arbitrum, Ethereum and Polygon, paid to it.
    ClaimRewards,
    /// «sweep rewards over $10 into USDC»: Merkl's rewards, when together they're worth `min_usd` or more (never under $5,
    /// or the gas eats them), claimed, and each sold for USDC on Solana, or `into` a token bought with that USDC.
    SweepRewards { min_usd: Option<Decimal>, into: Option<String> },
    /// «sweep dust», «собери пыль»: the verified tokens worth under `under` dollars (nil = $5) on Base, Arbitrum and
    /// Polygon, each sold for USDC on its own network when the gas takes less than half of it.
    SweepDust { under: Option<Decimal> },
    /// A token on Hyperliquid's spot book (HYPE…) for USDC on HyperCore's spot balance; nil = ask how much.
    HlSpotBuy { symbol: String, usdc: Option<Decimal> },
    /// Sells it there for USDC; nil = all that's free.
    HlSpotSell { symbol: String, amount: Option<Decimal> },
    /// «show approvals», «мои разрешения»: who may still spend which token, on every EVM network — a board, nothing signed.
    ShowApprovals,
    /// Sets one approval to zero: the token's own approve(spender, 0), or Permit2's (`permit2`).
    RevokeApproval { place: String, token: String, spender: String, permit2: bool },
    /// Any contract's function the chat wrote out (ContractCall): checked on the explorer, simulated, then a card.
    ContractCall(ContractCall),
    /// «make a multisig 2 of 3 with Phone on base and arbitrum»: a Safe multisig whose keys are this Mac's (and the
    /// connected Ledger's) plus the saved addresses named, `threshold` of them to move money (nil: a majority); nil `of`
    /// when not said, no network = Base. Its card adjusts all of it; Confirm makes it.
    MultisigCreate { threshold: Option<i64>, of: Option<i64>, keys: Vec<String>, networks: Vec<String> },
    /// «send 200 USDC from the multisig to Alex on base»: a payment out of it, signed by its keys here; nil = all of it.
    MultisigSend { token: String, amount: Option<Decimal>, to: String, network: Option<String> },
    /// «show the multisig»: its card on the canvas.
    MultisigShow,
}

/// A place a spread can leave out: the key a place's name contains, its name, and how it's said.
pub struct SpreadProtocol {
    pub key: &'static str,
    pub name: &'static str,
    pub words: &'static str,
}

pub const SPREAD_PROTOCOLS: [SpreadProtocol; 5] = [
    SpreadProtocol { key: "jupiter", name: "Jupiter Lend", words: r"jupiter|jupyter|юпитер|джупитер|\bjup\b" },
    SpreadProtocol { key: "kamino", name: "Kamino", words: "kamino|камино" },
    SpreadProtocol { key: "aave", name: "Aave", words: r"aave|ааве|\bаав" },
    SpreadProtocol { key: "morpho", name: "Morpho", words: "morpho|морфо" },
    SpreadProtocol { key: "jitosol", name: "JitoSOL", words: "jito|джито|стейк|stak" },
];

/// Backpack's tokenized stocks Kamino's Backpack market takes as collateral, by ticker: their Token-2022 mints
/// (Kamino.backpackStocks has the same four).
pub const KAMINO_STOCK_MINTS: [(&str, &str); 4] = [
    ("SPCX", "SPCXxcqXj6e5dJDVNovHN8744zkbhM2bYudU45BimGb"), ("MU", "MUxEsUKSMACyw5fZf68wxf5FLnZVhtU9CwH8uNNGay1"),
    ("SKHY", "SKHYhSjuRWHgikq8eRKbtBbpABgJSkd7ytQV14i9EQ3"), ("SNDK", "SNDKbwMUQvZhnLnxLduradgLHG5KrPuKwpnrkkGRhfH"),
];
pub const KAMINO_STOCKS: [&str; 4] = ["SPCX", "MU", "SKHY", "SNDK"];

const DEPOSIT_WORDS: &str = r"полож\w*|внес\w*|закин\w*|вкин\w*|кинь|засун\w*|влож\w*|задепозит\w*|депозитн\w*|депни|пополн\w*|инвест\w*|\bdeposit\w*|\bdepo\b|\bput\b|\badd\b|\bplace\b|\bsupply\b|\bstash\b|\bpark\b|\binvest\w*|\btop\s+up\b";
const WITHDRAW_WORDS: &str = r"сним\w*|выв[её]д\w*|забер\w*|верн\w*|\bwithdraw\w*|take\s+out|\bremove\b|\bpull\b|\bredeem\w*";
/// «займёт» is time («сколько займёт покупка»), not a loan.
const BORROW_WORDS: &str = r"\bborrow\w*|\bзайм(?![её]т)\w*|\bзайми\b|занять|\bв\s+долг\b|\bloan\b";
const REPAY_WORDS: &str = r"\brepay\w*|погас\w*|(?:верни|вернуть|закрой|закрыть)\s+(?:весь\s+|мой\s+)?долг";
/// «в камино», «into the vault», «из хранилища».
const INTO_PLACE: &str = r"(?:\bв\b|\bво\b|into|\bto\b|\bin\b)\s+(?:the\s+|my\s+|мо[йеюи]\s+)?(?:vault|хранилищ|jupiter|jupyter|джупитер|юпитер|kamino|камино|aave|ааве|hlp|хлп|лендинг|lend)";
const OUT_OF_PLACE: &str = r"(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b|out\s+of)\s+(?:the\s+|my\s+|мо[йеюи]\s+)?(?:vault|хранилищ|jupiter|jupyter|джупитер|юпитер|kamino|камино|aave|ааве|hlp|хлп|лендинг|lend)";
/// «jupiter dca …», «dca on jupiter», «дса через юпитер».
const JUPITER_DCA_WORDS: &str = r"(?:jupiter|jupyter|джупитер\w*|юпитер\w*)\s*(?:dca|дса|дца)|(?:dca|дса|дца)\s+(?:on|via|through|with|at|через|на|в|с)\s+(?:jupiter|jupyter|джупитер\w*|юпитер\w*)";
/// «1 000», «2 500 tokens»: thousands split by a space, which every reader here takes in part («500 tokens»).
const SPACED_NUMBER: &str = r"\d\s\d{3}(?!\w)";
const AAVE_OR_MORPHO: &str = r"\baave\b|ааве|morpho|морфо";
const BACK_TO_SOLANA: &str = r"(?:выв[её]д\w*|верн\w*|перевед\w*|перенес\w*|withdraw\w*|move|send|bring|bridge)(?:[^.]|\.(?=\d))*(?:solana|солан)";
const ALL_OF_IT: &str = r"\ball\b|\bвесь\b|\bвсё\b|\bвсе\b|полност";
const TRADE_WORDS: &str = r"\b(?:long|short|лонг\w*|шорт\w*|perp\w*)\b";
const BUY_WORDS: &str = r"\bbuy\b|куп\w*|purchase|приобрет\w*|\bget\s+me\b|вложи\w*\s+в";
const SELL_WORDS: &str = r"\bsell\b|прода\w*|\bdump\b";
const AGENT_WALLET: &str = r"agent(?:'s)?\s+wallet|кошел\w*\s+агента|агентск\w*\s+кошел";
/// A word asking for a private send. Whole adverbs only: «Приватный кошелёк» or «Тайный» is a name, not a request.
const PRIVACY_WORD: &str = r"(?i)\b(?:privately|confidentially|anonymously|secretly|приватно|анонимно|конфиденциально|тайно|втайне|скрытно|втихую)\b";
const INTENTS_WORDS: &str = r"near\s*intents|(?:ниар|неар|нир)\s*интент\w*|\b1\s*click\b";
const INTENTS_ROUTE: &str = r"(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b)\s+(?:the\s+|my\s+)?(solana|солан\w*|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|tempo|темпо\w*|hyperliquid|гипер\w*|хайпер\w*)((?:[^.]|\.(?=\d))*?)(?:\bto\b|\binto\b|\bв\b|\bна\b)\s+(?:the\s+)?(solana|солан\w*|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|tempo|темпо\w*|hyperliquid|гипер\w*|хайпер\w*)";

impl WalletCommand {
    /// Reading a text compiles no regex twice here (`foundation` keeps them): Swift's cache of parsed texts isn't needed.
    pub fn parse(text: &str) -> Option<Self> {
        // Anything about the agent wallet is its own command or nothing: never another form's money move.
        if has(&text.to_lowercase(), AGENT_WALLET) { return Self::agent(text) }
        // «send 5 USDC privately to Mom», «переведи приватно 20 USDC маме»: the same send without the word, made private.
        // A request for privacy that isn't such a send reads as nothing, never as a public move (the multisig's too).
        if let Some(w) = first(text, PRIVACY_WORD) {
            let m = w.get(0).expect("a match");
            let plain = replacing(&format!("{}{}", &text[..m.start()], &text[m.end()..]), r"\s{2,}", |_| " ".into());
            return Self::parse(&plain)?.made_private();
        }
        if let Some(call) = ContractCall::parse(text) { return Some(Self::ContractCall(call)) }
        if let Some(multisig) = Self::multisig(text) { return Some(multisig) }
        // «pay POST https://… {"q": 1}»: the URL and the body kept as written.
        if let Some(m) = first(text, r"(?is)^\s*pay\s+(?:(get|post|put|patch|delete)\s+)?(https://[^\s{}\[\]]+)(?:\s+(?:with\s+)?([\{\[].*[\}\]]))?\s*$") {
            let body = opt(&m, 3).map(String::from);
            let method = opt(&m, 1).map_or(if body.is_none() { "GET" } else { "POST" }.into(), str::to_uppercase);
            return Some(Self::Pay { method, url: group(&m, 2).into(), body });
        }
        let text = IntentParser::plain_numbers(text);
        let q = text.to_lowercase();
        // Jupiter's on-chain DCA, before the app's own recurring buys below take «dca».
        if has(&q, JUPITER_DCA_WORDS) { return Self::jupiter_dca(&q) }
        // «add gas on robinhood for 3 USDC», «добавь газа на робингуд на 3 бакса», «докинь газ в полигон».
        if has(&q, r"\bgas\b|\bгаз\w*")
            && has(&q, r"\badd\b|top\s*up|\bbring\b|\bget\b|\bbuy\b|добав\w*|доки\w*|пополн\w*|закин\w*|подкин\w*|купи\w*|нуж\w*")
            && let Some(word) = first(&q, r"\b(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet)\b")
            && let Some(place) = Self::network(group(&word, 1))
        {
            return Some(Self::Gas { place, usd: dollars(&q) });
        }
        // One approval to zero: «revoke the approval of 0x… to 0x… on base [through permit2]».
        if let Some(m) = first(&text, r"(?i)\b(?:revoke|cancel|отзов\w*|отмен\w*)\s+(?:the\s+)?(?:approval|разрешени\w*)\s+(?:of|на|для)?\s*(0x[0-9a-fA-F]{40})\s+(?:to|для|у)\s+(0x[0-9a-fA-F]{40})\s+(?:on|на|в)\s+(base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|robinhood|робинхуд\w*)\b") {
            return Some(Self::RevokeApproval {
                place: Self::network(group(&m, 3)).unwrap_or_else(|| "base".into()),
                token: group(&m, 1).to_lowercase(),
                spender: group(&m, 2).to_lowercase(),
                permit2: q.contains("permit2"),
            });
        }
        // The approvals board: «show approvals», «мои разрешения», «кому я дал разрешения», «revoke approvals».
        if has(&q, r"\b(?:show\s+|my\s+|list\s+|check\s+|revoke\s+)?(?:token\s+)?approvals\b|\ballowances\b|(?:мои|покажи|проверь|какие)\s+(?:\w+\s+)?разрешени\w*|кому\s+(?:я\s+)?(?:дал|выдал)\w*\s+разрешени|апрув\w*") {
            return Some(Self::ShowApprovals);
        }
        // Small leftovers into USDC: «sweep dust», «собери пыль до $3», «convert small balances to USDC».
        if has(&q, r"\bdust\b|\bпыль\b|\bпыли\b|собер\w*\s+мелоч\w*|\bsmall\s+balances?\s+(?:in)?to\s+usdc|\bleftovers?\s+(?:in)?to\s+usdc|остатк\w*\s+(?:в|на)\s+usdc") {
            return Some(Self::SweepDust { under: num_of(&q, r"(?:under|below|less\s+than|<|до|меньше|ниже)\s*\$?\s*(\d+(?:[.,]\d+)?)") });
        }
        // Rewards to claim AND sell: before the plain claim below.
        if has(&q, r"\bsweep\s+(?:the\s+|my\s+)?rewards?|\bsell\s+(?:the\s+|my\s+|all\s+)?(?:claimed\s+)?rewards?|(?:прода|обменя|меня)\w*\s+(?:\w+\s+){0,2}?награ") {
            let min = num_of(&q, r"(?:over|above|at\s+least|≥|>=|от|больше|свыше|когда\s+(?:наберётся|наберется|будет))\s*\$?\s*(\d+(?:[.,]\d+)?)");
            let into = first(&q, r"\b(?:into|to|for)\s+\$?([a-z][a-z0-9]{1,9})\b|(?:\sв|\sна)\s+\$?([a-z][a-z0-9]{1,9})\b")
                .map(|m| opt(&m, 1).or(opt(&m, 2)).unwrap_or("").to_uppercase())
                .filter(|s| !s.is_empty() && !["USDC", "USD", "STABLES", "STABLECOINS"].contains(&s.as_str()));
            return Some(Self::SweepRewards { min_usd: min, into });
        }
        // Rewards to collect, before «claim … from Aave» reads as a withdrawal and «claim yield» as the rates.
        if has(&q, r"\bclaim|клейм|ревард|реворд") || has(&q, r"(?:забер|забра|собер|собра|получ|вывед|выве)\w*\s+(?:\w+\s+){0,2}?(?:награ|бонус)") {
            return Some(Self::ClaimRewards);
        }
        let amount = dollars(&q);
        // Hyperliquid's spot book: «buy HYPE on hyperliquid spot for 10 USDC», «sell all HYPE spot», «купи HYPE на споте за 15$».
        if has(&q, r"\bspot\b|\bспот\w*") && !has(&q, r"\b(?:long|short|perp\w*|лонг\w*|шорт\w*)\b") {
            if let Some(m) = first(&q, r"(?:\bbuy|купи\w*)\s+\$?([a-z][a-z0-9]{1,9})\b") && !["spot", "on", "for"].contains(&group(&m, 1)) {
                return Some(Self::HlSpotBuy { symbol: group(&m, 1).to_uppercase(), usdc: amount });
            }
            if let Some(m) = first(&q, r"(?:\bsell|прода\w*)\s+(?:(\d+(?:[.,]\d+)?|all|все|всё|весь)\s+)?\$?([a-z][a-z0-9]{1,9})\b") && !["spot", "on"].contains(&group(&m, 2)) {
                let count = opt(&m, 1).and_then(num);
                if count.is_none() && amount.is_some() { return None }   // a sale in dollars names no count of coins
                return Some(Self::HlSpotSell { symbol: group(&m, 2).to_uppercase(), amount: count });
            }
        }
        // «положи 1 в джупитер»: a bare number is the amount of a deposit or a withdrawal when nothing else is (not for
        // buys: «buy 0.1 SOL» is coins, not dollars).
        let bare = num_of(&q, r"(?:^|[^\w.,])(\d+(?:[.,]\d+)?)(?![\d.,]*\s*(?:%|x\b|х\b|×|r\b))");
        let jupiter = has(&q, r"jupiter|jupyter|джупитер|юпитер|\bjup\s*(?:lend|vault|earn)|jlusdc");
        let vault = jupiter || has(&q, r"хранилищ|вол[тл]|vault|\blend|депозит|лендинг");
        let into = has(&q, DEPOSIT_WORDS) || has(&q, INTO_PLACE);
        let out_of = has(&q, WITHDRAW_WORDS) || has(&q, OUT_OF_PLACE);
        let coins = num_of(&q, r"(\d+(?:[.,]\d+)?)\s*(?:sol|сол|jitosol|usdt|jupusd|usdc)");
        // «USDC на Солане» is USDC; Solana alone is the network, SOL the coin.
        let symbol = if has(&q, "usdc|юсдс|юсдц") { "USDC" } else if has(&q, r"\bsol\b|\bсол\b") { "SOL" } else if has(&q, "usdt") { "USDT" }
            else if has(&q, "jupusd") { "JupUSD" } else { "USDC" };
        // Money already deposited, moved to better rates: «переложи в лучшие ставки», «rebalance my USDC».
        if has(&q, "перебаланс|ребаланс|rebalanc") || (has(&q, r"перелож|move\s+my\s+deposits") && has(&q, r"ставк|доходн|лучш|rates?|yields?|best")) {
            return Some(Self::Rebalance { max_share: num_of(&q, r"(\d+(?:[.,]\d+)?)\s*%") });
        }
        if has(&q, "разлож|распредел|раскид|размест|spread|allocate|distribute|split") && has(&q, r"ставк|доходн|протокол|лучш|хранилищ|rates?|yields?|protocols?|best|vaults?") {
            let share = num_of(&q, r"(\d+(?:[.,]\d+)?)\s*%");
            // «spread 5 USDC from base», «разложи 1000 с базы»: only that network's money.
            let source = first(&q, r"(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b)\s+(solana|солан\w*|base|бейс\w*|бейз\w*|баз\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфир\w*|mainnet|robinhood|робинхуд\w*|tempo|темпо\w*)");
            let from = source.as_ref().map(|m| {
                let w = group(m, 1);
                if w.starts_with("sol") || w.starts_with("сол") { "solana".into() } else if w.starts_with("баз") { "base".into() } else { Self::network(w).unwrap_or_else(|| w.into()) }
            });
            // «from solana» names the network, not the coin; «USDC» named wins.
            let rest = source.as_ref().and_then(|m| m.get(0)).map_or(q.clone(), |r| format!("{} {}", &q[..r.start()], &q[r.end()..]));
            let is_sol = !has(&rest, "usdc|юсдс|юсдц") && has(&rest, r"\bsol\b|солан|\bсол\b");
            // «… without kamino, aave», «без камино»: the protocols named after the word are left out.
            let tail = first(&q, r"(?:\bwithout\b|\bexcept\b|\bбез\b|\bкроме\b)(.*)").map_or(String::new(), |m| group(&m, 1).into());
            let without = SPREAD_PROTOCOLS.iter().filter(|p| has(&tail, p.words)).map(|p| p.key.to_string()).collect();
            return Some(Self::Allocate {
                symbol: if is_sol { "SOL" } else { "USDC" }.into(), max_share: share, amount: if is_sol { coins } else { amount.or(coins) }, without,
                from: if is_sol { None } else { from },
            });
        }
        if has(&q, r"доходност|где\s+(?:лучше\s+)?держать|куда\s+(?:лучше\s+)?(?:положить|вложить|деть)|\bapy\b|ставк[иа]\s+(?:по\s+)?депозит|where\s+(?:to|should\s+i|can\s+i)\s+(?:keep|put|park|hold)|best\s+(?:rates?|yields?)|\byields?\b") {
            let (sol, usd) = (has(&q, r"\bsol\b|солан|\bсол\b|solana"), has(&q, "usdc|стейбл|доллар|stable"));
            let asset = if sol && usd { None } else if sol { Some("SOL") } else if usd { Some("USDC") } else if has(&q, "usdt") { Some("USDT") } else { None };
            return Some(Self::Yields(asset.map(Into::into)));
        }
        // A token on one network for one on another, through Relay: «swap 0.1 SOL on Solana for ETH on Arbitrum», «swap all
        // AERO on base for SOL on solana», «обменяй 0.1 SOL на ETH в арбитруме» (SOL comes from Solana). USDC for USDC is a
        // bridge; a swap on one network is the rule below.
        if let Some(m) = first(&text, r"(?i)\b(?:swap|exchange|convert|поменя\w*|обменя\w*|свапн\w*|свопн\w*|конверт\w*)\s+(?:(\d+(?:[.,]\d+)?|all|все|всё|весь|всю)\s+)?\$?(0x[0-9a-fA-F]{40}|[A-Za-z][A-Za-z0-9]{1,11}(?:\.e)?)\s+(?:(?:on|from|in|на|в|во|с|со)\s+(solana|солан\w*|near|неар\w*|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|robinhood|робинхуд\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*)(?:\s+chain)?\s+)?(?:for|to|into|на|в)\s+\$?(0x[0-9a-fA-F]{40}|[A-Za-z][A-Za-z0-9]{1,11}(?:\.e)?)\s+(?:on|in|at|на|в|во)\s+(solana|солан\w*|near|неар\w*|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|robinhood|робинхуд\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*)\b") {
            let place = |w: &str| -> String {
                let w = w.to_lowercase();
                if w.starts_with("sol") || w.starts_with("сол") { "solana".into() } else if w.starts_with("near") || w.starts_with("неар") { "near".into() }
                else { Self::network(&w).unwrap_or_else(|| "base".into()) }
            };
            let (a, b, to) = (group(&m, 2), group(&m, 4), place(group(&m, 5)));
            let from = opt(&m, 3).map(place).or_else(|| match a.to_uppercase().as_str() { "SOL" => Some("solana".into()), "NEAR" => Some("near".into()), _ => None });
            if let Some(from) = from && from != to {
                let count = opt(&m, 1).and_then(num);
                if a.to_uppercase() == "USDC" && b.to_uppercase() == "USDC" {
                    return Some(if from == "solana" { Self::SolanaTo(to, count) } else { Self::Bridge { from, to, amount: count } });
                }
                return Some(Self::CrossSwap { from: a.into(), on: from, to: b.into(), at: to, amount: count });
            }
        }
        // One token for another on an EVM network: «swap 0.01 ETH for AERO on base», «swap all DEGEN to AERO on base»,
        // «поменяй 0.01 ETH на AERO на бейсе». USDC on one side is a buy or a sale there, with the money on that network.
        if let Some(m) = first(&text, r"(?i)\b(?:swap|exchange|convert|поменя\w*|обменя\w*|свапн\w*|свопн\w*|конверт\w*)\s+(?:(\d+(?:[.,]\d+)?|all|все|всё|весь|всю)\s+)?\$?(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|[A-Za-z][A-Za-z0-9]{1,11}(?:\.e)?)\s+(?:for|to|into|на|в)\s+\$?(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|[A-Za-z][A-Za-z0-9]{1,11}(?:\.e)?)\s+(?:on|на|в|во)\s+(?:the\s+)?(solana|солан\w*|near|неар\w*|robinhood|робин[гх]уд\w*|hyperliquid(?:\s+spot)?|хайперликвид\w*(?:\s+спот\w*)?|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|tempo|темпо\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*)\b") {
            let (from, to, word) = (group(&m, 2), group(&m, 3), group(&m, 4).to_lowercase());
            let place = if word.starts_with("sol") || word.starts_with("сол") { "solana".into() } else if word.starts_with("near") || word.starts_with("неар") { "near".into() }
                else { Self::network(&word).unwrap_or_else(|| "base".into()) };
            let count = opt(&m, 1).and_then(num);
            let (from_up, to_up) = (from.to_uppercase(), to.to_uppercase());
            let swap = |place: &str, from: &str, to: &str| Some(Self::SwapOn { place: place.into(), from: from.into(), to: to.into(), amount: count });
            // NEAR's account holds NEAR alone: for USDC or back it's 1Click's sale or buy.
            if place == "near" && from_up == "NEAR" && to_up == "USDC" { return Some(Self::SellNear { amount: count, to: None }) }
            if place == "near" && from_up == "USDC" && to_up == "NEAR" { return Some(Self::BuyNear { usdc: count }) }
            // Anything else on NEAR, one token for another there: NEAR Intents.
            if place == "near" { return swap(&place, &from_up, &to_up) }
            // Solana, through Jupiter: USDC on a side is a buy or a sale there; any other two tokens straight, by ticker or mint.
            if place == "solana" {
                let asset = |t: &str| if count_chars(t) >= 32 { SpotAsset::by_mint(t) } else { SpotAsset::by_symbol(t) };
                if from_up == "USDC" { return Some(Self::Buy { asset: asset(to), usdc: count }) }
                if to_up == "USDC" { return Some(Self::Sell { asset: asset(from), amount: count }) }
                let keep = |t: &str| if count_chars(t) >= 32 { t.to_string() } else { t.to_uppercase() };
                return swap(&place, &keep(from), &keep(to));
            }
            // Hyperliquid's spot book trades everything against USDC: one token for another is a sale and a buy.
            if place == "hyperliquid" {
                if from_up == "USDC" { return Some(Self::HlSpotBuy { symbol: to_up, usdc: count }) }
                if to_up == "USDC" { return Some(Self::HlSpotSell { symbol: from_up, amount: count }) }
                return swap(&place, &from_up, &to_up);
            }
            // Robinhood Chain: Stock Tokens and tokens for one another, USDG too, straight on Uniswap there. Tempo's dollars
            // on its own exchange. HyperEVM's money isn't in Uniswap's pools: Relay's own swap on the network.
            if ["robinhood", "tempo", "hyperevm"].contains(&place.as_str()) { return swap(&place, from, to) }
            // The network's own coin (ETH, POL) for its USDC or back stays a swap: Uniswap's ETH pools take it as it is,
            // while a buy or a sale looks the token up on Uniswap's list, where ETH isn't.
            let coin = Self::own_coin(&place);
            if from.to_lowercase() == "usdc" && !coin.contains(&to_up.as_str()) {
                return Some(Self::BuyOn { place: place.clone(), token: to.into(), usdc: count, from: Some(place) });
            }
            if to.to_lowercase() == "usdc" && !coin.contains(&from_up.as_str()) {
                return Some(Self::SellOn { place, token: from.into(), amount: count, keep: true });
            }
            return swap(&place, from, to);
        }
        // A token on a named network: «buy AERO on base for 5 USDC», «sell all DEGEN on base», «buy BONK on solana for 5
        // USDC», «купи ARB на арбитруме на 5$», «buy token 0x… on arbitrum for 3 USDC».
        if let Some(m) = first(&text, r"(?i)\b(buy|купи\w*|sell|прода\w*)\s+(?:(\d+(?:[.,]\d+)?|all|все|всё|весь)\s+)?(?:(?:the\s+)?tokens?\s+|токен\w*\s+)?\$?(0x[0-9a-fA-F]{40}|[A-Za-z][A-Za-z0-9]{1,11})\s+(?:on|на|в)\s+(solana|солан\w*|base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*)\b")
            && !["usdc", "usdg", "tokens", "token"].contains(&group(&m, 3).to_lowercase().as_str())
        {
            let (token, place) = (group(&m, 3), group(&m, 4).to_lowercase());
            let chain = if place.starts_with("sol") || place.starts_with("сол") { "solana".into() } else { Self::network(&place).unwrap_or_else(|| "base".into()) };
            let count = opt(&m, 2).and_then(num);
            let verb = group(&m, 1).to_lowercase();
            let sell = verb.starts_with('s') || verb.starts_with("прод");
            // «sell BONK on solana for 10 USDC» names no count of coins: never «all of it».
            if sell && opt(&m, 2).is_none() && amount.is_some() { return None }
            if chain == "solana" && !token.starts_with("0x") {
                return Some(if sell { Self::Sell { asset: SpotAsset::by_symbol(token), amount: count } } else { Self::Buy { asset: SpotAsset::by_symbol(token), usdc: amount } });
            }
            // «buy ETH on arbitrum for 10 USDC», «sell 0.01 ETH on base»: the network's own coin for USDC is the swap above.
            // With a number only: the swap's «no amount» is all of what it spends, and the gas coin needs one.
            if chain != "solana" && Self::own_coin(&chain).contains(&token.to_uppercase().as_str()) && let Some(n) = (if sell { count } else { amount }) {
                let (from, to) = if sell { (token.to_uppercase(), "USDC".into()) } else { ("USDC".into(), token.to_uppercase()) };
                return Some(Self::SwapOn { place: chain, from, to, amount: Some(n) });
            }
            if chain != "solana" {
                // «… from solana», «… с арбитрума»; «… and keep the USDC there», «… оставь USDC на base».
                let rest = &text[m.get(0).map_or(0, |r| r.end())..];
                let from = first(rest, r"(?i)\b(?:from|с|со|из)\s+(solana|солан\w*|base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфир\w*|mainnet|robinhood|робинхуд\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|hyperevm|хайперевм\w*|гиперевм\w*)")
                    .map(|m| group(&m, 1).to_lowercase())
                    .map(|s| if s.starts_with("sol") || s.starts_with("сол") { "solana".into() } else { Self::network(&s).unwrap_or(s) });
                let keep = has(rest, r"(?i)\bkeep\b|\bstays?\b|остав\w*|остан\w*|не\s+выводи");
                return Some(if sell { Self::SellOn { place: chain, token: token.into(), amount: count, keep } } else { Self::BuyOn { place: chain, token: token.into(), usdc: amount, from } });
            }
        }
        // Any token by its contract («купи 0x… токен на $2»): bought on Robinhood Chain, where ihatecrypto trades by contract.
        if let Some(contract) = first(&text, r"0x[0-9a-fA-F]{40}(?![0-9a-fA-F])").map(|m| group(&m, 0).to_string()) {
            if has(&q, SELL_WORDS) {
                let tokens = Self::sold_count(&q);
                // A sale in dollars, or a number or a share it can't place, is not «all of it».
                if tokens.is_none() && Self::names_other_count(&q.replace(&contract.to_lowercase(), " ")) { return None }
                return Some(Self::SellStock { symbol: contract, amount: tokens });
            }
            if has(&q, BUY_WORDS) { return Some(Self::BuyStock { symbol: contract, usdc: amount }) }
        }
        // Any Solana token by its mint («buy token B5AN… for 2 USDC»): through Jupiter, only with Safe search off.
        if let Some(mint) = AddressBookCommand::address(&text)
            && !mint.starts_with("0x")
            && !mint.ends_with(".near")
            && count(&mint) >= 32
            && ALL.iter().all(|a| a.mint != mint)
        {
            if has(&q, SELL_WORDS) {
                let tokens = Self::sold_count(&q);
                if tokens.is_none() && Self::names_other_count(&q.replace(&mint.to_lowercase(), " ")) { return None }
                return Some(Self::Sell { asset: SpotAsset::by_mint(&mint), amount: tokens });
            }
            if has(&q, BUY_WORDS) { return Some(Self::Buy { asset: SpotAsset::by_mint(&mint), usdc: amount }) }
        }
        // Robinhood Stock Tokens: a stock word or a company ihatecrypto knows, with buy or sell. A loan against stocks isn't
        // one: «займи под акции SpaceX, не продавая» was read as their sale.
        if let Some(symbol) = stock_names::symbol(&text)
            && !has(&q, BORROW_WORDS)
            && has(&q, r"\bstocks?\b|\bshares?\b|акци\w*|токен\w*|robinhood|робинхуд|спейс|spacex|\btesla\b|тесл|нвид|nvidia")
        {
            if has(&q, SELL_WORDS) {
                // «sell 2 TSLA stock for 10 USDC» sells 2; «sell TSLA stock for 10 USDC» names no count: not «all of it»; «sell
                // $50 of TSLA stock» and «50 worth of» are dollars, not 50 tokens (capped at what's held, that sold them all).
                let shares = if has(&q, SPACED_NUMBER) { None } else {
                    num_of(&q, r"(?:^|\s)(\d+(?:[.,]\d+)?)\s*(?:shares?|акци\w*|tokens?|токен\w*|(?!usd|bucks|of\b|worth\b)[a-z]{2,5}\b)").or_else(|| Self::sold_count(&q))
                };
                if shares.is_none() && Self::names_other_count(&q) { return None }
                return Some(Self::SellStock { symbol, amount: shares });
            }
            if has(&q, BUY_WORDS) { return Some(Self::BuyStock { symbol, usdc: amount }) }
        }
        // «move 2 USDC from Base to Solana via NEAR Intents», «переведи 2 USDC с бейза на солану через NEAR Intents»: 1Click,
        // asked for by name. Only USDC, and only where 1Click pays it at our own address.
        if has(&q, INTENTS_WORDS) && has(&q, "usdc|юсдс|юсдц") && !has(&q, r"\baave\b|ааве|morpho|морфо|kamino|камино")
            && let Some(m) = first(&q, INTENTS_ROUTE)
        {
            let place = |w: &str| if w.starts_with("sol") || w.starts_with("сол") { Some("solana".to_string()) } else { Self::network(w) };
            // Between the two networks only words about the wallet itself: «с бейза маме на солане» names a person, and
            // this move pays only our own address — the chat asks instead.
            let gap = group(&m, 2).split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty())
                .all(|w| has(w, r"^(?:my|the|wallet|account|network|chain|over|мо\w*|кошел\w*|аккаунт\w*|сет\w*|сеть)$"));
            let served = ["solana", "base", "arbitrum", "ethereum"];
            return match (gap, place(group(&m, 1)), place(group(&m, 3))) {
                (true, Some(from), Some(to)) if from != to && served.contains(&from.as_str()) && served.contains(&to.as_str()) => {
                    Some(Self::Intents { from, to, amount })
                }
                _ => None,
            };
        }
        // Money leaving Solana («move 5 USDC to Base from Solana»): never a move back to it, whatever network is named.
        let from_solana = has(&q, r"(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b)\s+(?:the\s+|my\s+)?(?:solana|солан)");
        // Out of the Solana wallet: «move 3 USDC from Solana to Robinhood», «переведи 3 USDC из соланы в робингуд», «move 11
        // USDC from Solana to Hyperliquid». «из соланы в aave на арбитрум» is a deposit into Aave there: read further down.
        if !has(&q, AAVE_OR_MORPHO)
            && let Some(m) = first(&q, r"(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b)\s+(?:the\s+|my\s+)?(?:solana|солан\w*)(?:[^.]|\.(?=\d))*?(?:\bto\b|\binto\b|\bв\b|\bна\b)\s+(?:the\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|hyperliquid|хайперликвид\w*|гиперликвид\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|tempo|темпо\w*)")
        {
            return Some(Self::SolanaTo(Self::network(group(&m, 1)).unwrap_or_else(|| "robinhood".into()), amount));
        }
        // Between two EVM networks, or from one to Hyperliquid, straight over Relay: «move 20 USDC from Base to Robinhood»,
        // «из бейза в робингуд», «с бейза на гипер».
        if !has(&q, AAVE_OR_MORPHO)
            && let Some(m) = first(&q, r"(?:\bfrom\b|\bиз\b|\bс\b|\bсо\b)\s+(?:the\s+|my\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|tempo|темпо\w*)(?:[^.]|\.(?=\d))*?(?:\bto\b|\binto\b|\bв\b|\bна\b)\s+(?:the\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|бейз\w*|arbitrum|арбитрум\w*|robinhood|робин[гх]уд\w*|polygon|полигон\w*|ethereum|эфириум\w*|tempo|темпо\w*|hyperliquid|гипер\w*|хайпер\w*|hl\b)")
            && let Some(from) = Self::network(group(&m, 1))
            && let Some(to) = Self::network(group(&m, 2))
            && from != to
        {
            // USDT, not USDC: Tether's own bridge when LayerZero is named, else Relay's swap of it. Optimism's «USDT» is the
            // old bridged token, not USDT0: a plain move from there swaps the one they hold.
            if has(&q, r"\busdt0?\b|usd₮|\btether\b|юсдт|тезер") && !has(&q, "usdc|юсдс|юсдц") && to != "hyperliquid" {
                let layer_zero = has(&q, r"layer\s*zero|л[еэ]й?ер\s*зеро|(?:via|through|over|by|через|по)\s+(?:the\s+)?usdt0|\boft\b");
                let usdt0 = |n: &str| stablecoins::USDT0_NETWORKS.contains(&n);
                if layer_zero || (usdt0(&from) && usdt0(&to) && from != "optimism") { return Some(Self::Usdt { from, to, amount: coins, layer_zero }) }
                return Some(Self::CrossSwap { from: "USDT".into(), on: from, to: "USDT".into(), at: to, amount: coins });
            }
            // «… via CCTP», «… через Circle»: Circle's burn and mint, whatever Relay would land.
            let off_cctp = ["hyperliquid", "robinhood", "tempo"];
            if has(&q, r"\bcctp\b|\bcircle\b|циркл|сиркл") && !off_cctp.contains(&from.as_str()) && !off_cctp.contains(&to.as_str()) {
                return Some(Self::Cctp { from, to, amount });
            }
            return Some(Self::Bridge { from, to, amount });
        }
        if !from_solana && has(&q, "robinhood|робин[гх]уд") && has(&q, BACK_TO_SOLANA) { return Some(Self::RobinhoodToSolana(amount)) }
        // From a network ihatecrypto has no wallet on, through 1Click: «receive 50 USDC from Sui», «получи 0.01 BTC с
        // биткоина», «top up 20 USDT from tron». The coin left unsaid is the network's own (USDT on TRON).
        if let Some(m) = first(&text, r"(?i)\b(?:receive|get|bring|deposit|top\s*up|получи\w*|прими\w*|пополн\w*|заведи|завести)\b(?:\s+(?:me|мне))?(?:\s+(\d+(?:[.,]\d+)?))?(?:\s+(btc|zec|trx|usdt|xrp|doge|sui|usdc))?\s+(?:from|с|со|из)\s+(?:the\s+)?(sui|суи|bitcoin|биткоин\w*|btc|zcash|zec|tron|трон\w*|xrp|ripple|рипл\w*|dogecoin|doge|додж\w*)\b") {
            let w = group(&m, 3).to_lowercase();
            let p = |s: &[&str]| s.iter().any(|s| w.starts_with(s));
            let network = if p(&["su", "су"]) { "sui" } else if p(&["bit", "бит"]) || w == "btc" { "bitcoin" } else if p(&["z"]) { "zcash" }
                else if p(&["tr", "тр"]) { "tron" } else if p(&["do", "до"]) { "dogecoin" } else { "xrp" };
            let own = match network { "sui" => "SUI", "bitcoin" => "BTC", "zcash" => "ZEC", "tron" => "USDT", "dogecoin" => "DOGE", _ => "XRP" };
            let token = opt(&m, 2).map_or(own.into(), str::to_uppercase);
            return Some(Self::ReceiveAcross { network: network.into(), token, amount: opt(&m, 1).and_then(num) });
        }
        // Topping ihatecrypto up from outside: a deposit with no place to put it, or asking for our address.
        let place = has(&q, r"хранилищ|vault|jupiter|jupyter|джупитер|юпитер|kamino|камино|aave|ааве|morpho|морфо|hlp|хлп|\blend|лендинг|стейк|stake");
        if !place && has(&q, r"^\s*(?:(?:хочу|want\s+to|i\s+want\s+to|let'?s|давай)\s+)?(?:задепозит\w*|депозит\w*|пополн\w*|deposit|top\s*up|fund|receive|закинуть\s+деньги)\b|(?:мо[йи]|наш\w*|our|my|ihatecrypto'?s?)\s+(?:\w+\s+)?(?:адрес\w*|address(?:es)?)|deposit\s+address|адрес\w*\s+(?:для\s+)?(?:пополнен|депозит)|куда\s+(?:мне\s+)?(?:от)?(?:прав|слать|кин|закин|переве)|where\s+(?:do\s+i|to|can\s+i|should\s+i)\s+(?:send|deposit|transfer)|how\s+(?:do\s+i|to|can\s+i)\s+(?:top\s+up|deposit|fund|add\s+money)|как\s+(?:мне\s+)?пополн") {
            return Some(Self::Receive);
        }
        if has(&q, r"анстейк|unstake|из\s+стейкинга|продай\w*\s+(?:весь\s+)?jitosol") { return Some(Self::Unstake(coins)) }
        if has(&q, r"застейк|стейкни|\bstake\b|в\s+стейкинг") { return Some(Self::Stake(coins.or(amount))) }
        if has(&q, r"\bhlp\b|хлп") {
            if out_of { return Some(Self::Hlp { deposit: false, amount: coins.or(amount).or(bare) }) }
            if into { return Some(Self::Hlp { deposit: true, amount: coins.or(amount).or(bare) }) }
        }
        // The network the lending command names; Base when it names none.
        let network = Self::network_in(&q).unwrap_or_else(|| "base".into());
        // Borrowing on Kamino and paying it back: «borrow 2 USDC on kamino against 0.05 SPCX», «займи 2 USDC под SPCX на
        // камино», «repay all USDC on kamino», «погаси долг на камино». A Backpack stock named is Kamino, not Aave, and so is
        // a loan against SOL (Kamino's main market): «займи 50 USDC под 1 SOL», «borrow 50 USDC against my SOL».
        let stock = Self::kamino_stock(&q);
        // «солана» is the coin only right after «под»/«залог» («под мои соланы»): further on, «на солане» is the network.
        let against_sol = stock.is_none()
            && has(&q, r"(?:\bagainst\b|\bпод\b|залог\w*|collateral)\s+(?:[\w.,]+\s+){0,3}?(?:\bsol\b|\bсол[аы]?\b)|(?:\bпод\b|залог\w*)\s+(?:мо\w+\s+)?солан\w*|(?:\bsol\b|\bсол[аы]?\b)\s+(?:as\s+collateral|в\s+залог)");
        let against = stock.map(String::from).or_else(|| against_sol.then(|| "SOL".into()));
        if has(&q, "kamino|камино") || ((stock.is_some() || (against_sol && has(&q, BORROW_WORDS))) && !has(&q, r"\baave\b|ааве|\bаав")) {
            if has(&q, BORROW_WORDS) && !has(&q, REPAY_WORDS) {
                let adding = match stock {
                    None if against_sol => num_of(&q, r"(\d+(?:[.,]\d+)?)\s*(?:\bsol\b|\bсол[аы]?\b|солан\w*)"),
                    None => None,
                    Some(_) => num_of(&q, r"(\d+(?:[.,]\d+)?)\s*(?:shares?\s+of\s+|акци\w*\s+)?(?:spcx|sndk|skhy|\bmu\b|space\s*x|спейс\w*|micron|микрон\w*|hynix|хайникс\w*|sandisk|сандиск\w*)"),
                };
                return Some(Self::KaminoBorrow { amount: amount.or(if bare == adding { None } else { bare }), against, adding });
            }
            if has(&q, REPAY_WORDS) {
                return Some(Self::KaminoRepay { amount: if has(&q, ALL_OF_IT) { None } else { amount.or(bare) }, against });
            }
        }
        // Borrowing on Aave and paying it back, and ETH as its collateral: «borrow 10 USDC on aave», «займи 10 USDC в aave»,
        // «repay all USDC on aave», «погаси долг на aave», «supply 0.01 ETH as collateral on aave», «положи 0.01 ETH в залог».
        if has(&q, BORROW_WORDS) && !has(&q, r"\brepay\w*|погас\w*|верн\w*") { return Some(Self::AaveBorrow { amount: amount.or(bare), network }) }
        if has(&q, REPAY_WORDS) { return Some(Self::AaveRepay { amount: if has(&q, ALL_OF_IT) { None } else { amount.or(bare) }, network }) }
        if has(&q, "collateral|залог") && let Some(m) = first(&q, r"(\d+(?:[.,]\d+)?)\s*(?:eth|эфир)") {
            return Some(Self::AaveCollateral { amount: num(group(&m, 1)), network });
        }
        let money = coins.or(amount).or(bare);
        if has(&q, r"\baave\b|ааве|\bаав") {
            if out_of { return Some(Self::Aave { deposit: false, amount: money, network }) }
            // Only with a word for putting money in: «what do I have on Aave?» is a question.
            if into || has(&q, r"\blend\b|\bsupply\b|\bbuy\b|\bкуп\w*|одолж") { return Some(Self::Aave { deposit: true, amount: money, network }) }
        }
        if has(&q, "morpho|морфо|gauntlet") {
            if out_of { return Some(Self::Morpho { deposit: false, amount: money, network }) }
            if into || has(&q, r"\blend\b|\bsupply\b|\bdeposit\b|депозит|полож|одолж") { return Some(Self::Morpho { deposit: true, amount: money, network }) }
        }
        if has(&q, "kamino|камино") {
            // A Backpack stock goes in and out by its count of tokens: «deposit 0.05 SPCX to kamino».
            let tokens = if stock.is_none() { coins } else { bare };
            let symbol = stock.unwrap_or(symbol).to_string();
            if out_of { return Some(Self::Kamino { deposit: false, symbol, amount: tokens.or(amount).or(bare) }) }
            if into { return Some(Self::Kamino { deposit: true, symbol, amount: tokens.or(amount).or(bare) }) }
        }
        if has(&q, r"(?:останов|отмен|прекрат|стоп)\w*\s+(?:(?:регулярн|еже)\w*\s+)?(?:покупк|dca|расписан)|(?:stop|cancel|end)\s+(?:the\s+|all\s+)?(?:recurring|regular|scheduled|daily|weekly|monthly)?\s*(?:buys|purchases|dca|schedules?)") {
            return Some(Self::StopSchedules);
        }
        if has(&q, r"покупай|кажд(?:ый|ую|ые|ое)\b|каждого\s+(?:месяц|года|квартал)|каждой\s+недел|раз\s+в\s+|регулярн|\bdca\b|\bevery\b|\beach\b|once\s+(?:a|per|every)\s+(?:day|week|month|year|quarter|fortnight)|twice|times\s+(?:a|an|per)\b|per\s+day|weekly|daily|monthly|recurring|еже(?:часно|дневно|недельно|месячно|квартально|годно)|hourly|quarterly|yearly|annually|semi-?annual|fortnight")
            && has(&q, r"куп|dca|\bbuy")
            && let Some(asset) = SpotAsset::find(&q)
            && let Some(usdc) = amount
        {
            return Self::schedule(&q, asset, usdc);
        }
        // «закрой NEAR-аккаунт и переведи всё на Bybit», «close the NEAR account into Bybit»: the last «на / to / into» names it.
        if has(&q, r"(?:close|delete|закр\w*|удали\w*)\s+(?:(?:the|my|мой)\s+)?(?:near|неар\w*)[\s-]*(?:account|аккаунт\w*)")
            && let Some(m) = first(&text, r#"(?i).*\s(?:into|to|на|в)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$"#)
        {
            return Some(Self::CloseNear { name: trim_spaces(group(&m, 1)).into() });
        }
        // «send all RAY on solana to Bybit», «отправь 1973 AI в робинхуде на New wallet»: any token, as it is, to a saved
        // address. SOL with a number, NEAR, and USDC with a number but no network keep their own forms below.
        if let Some(m) = first(&text, r#"(?i)^\s*(?:send|transfer|отправь\w*|переведи|перевести|скинь|кинь)\s+(all|весь|всё|все|\d+(?:[.,]\d+)?)\s+(?:(?:the|my|мои|мой|моих)\s+)?\$?(0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}|[A-Za-z][A-Za-z0-9._-]{0,19})(?:\s+tokens?)?(?:\s+(?:on|in|в|на|по)\s+(?:the\s+)?(?:сети\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|solana|солан\w*|base|бейс\w*|бейз\w*|баз[еыау]|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфир\w*|mainnet|robinhood|робин[гх]уд\w*|tempo|темпо\w*|bitcoin|биткоин\w*|zcash|tron|трон\w*|xrp|ripple|dogecoin|sui)(?:\s+(?:network|chain|сети))?)?\s+(?:to|на|для)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$"#) {
            let amount = num(group(&m, 1));
            let (token, name) = (group(&m, 2), trim_spaces(group(&m, 4)));
            let t = token.to_uppercase();
            let network = opt(&m, 3).map(|w| {
                let s = w.to_lowercase();
                let across = [("bitcoin", "bitcoin"), ("биткоин", "bitcoin"), ("zcash", "zcash"), ("tron", "tron"), ("трон", "tron"), ("xrp", "xrp"), ("ripple", "xrp"), ("dogecoin", "dogecoin"), ("sui", "sui")];
                if let Some((_, a)) = across.iter().find(|(k, _)| s.starts_with(k)) { return a.to_string() }
                if s.starts_with("sol") || s.starts_with("сол") { "solana".into() } else if s.starts_with("баз") { "base".into() } else { Self::network(&s).unwrap_or(s) }
            });
            let stable = t == "USDC" || t == "USDG";
            let own_form = (t == "SOL" && amount.is_some()) || t == "NEAR" || (stable && amount.is_some() && network.is_none());
            // Our own places stay what they were; a pasted address is not a name; «send all the money» names no token.
            let place = whole(&name.to_lowercase(), r"(?:the\s+)?(?:solana|солан\w*|hyperliquid|хайперликвид\w*|base|бейс\w*|aave|kamino|hlp|vault|jupiter\w*|morpho)").is_some();
            if !own_form && !place && !["MONEY", "FUNDS", "EVERYTHING", "IT", "THEM", "TOKENS"].contains(&t.as_str())
                && !has(&name.to_lowercase(), "0x[0-9a-f]{6}") && count(name) <= 40
            {
                // USDC on a network only NEAR Intents reaches (Sui) goes as a token through it.
                let across = ["bitcoin", "zcash", "tron", "xrp", "dogecoin", "sui"].contains(&network.as_deref().unwrap_or(""));
                if stable && let Some(amount) = amount && !across { return Some(Self::SendTo { name: name.into(), amount, network }) }
                return Some(Self::SendToken { name: name.into(), token: token.into(), amount, network });
            }
        }
        // «отправь 0.001 SOL на Old wallet», «send 0.01 SOL to Bybit», «send 0.5 SOL on solana to Bybit»: native SOL to a
        // saved Solana address.
        if let Some(m) = first(&text, r#"(?i)(?:send|transfer|отправь\w*|переведи|перевести|скинь|кинь)\s+(\d+(?:[.,]\d+)?)\s*(?:sol|сол\w*)(?:\s+(?:on|in|в|на|по)\s+(?:the\s+)?(?:сети\s+)?(?:solana|солан\w*)(?:\s+(?:network|chain|сети))?)?\s+(?:to|на|для)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$"#)
            && let Some(value) = num(group(&m, 1))
        {
            let name = trim_spaces(group(&m, 2));
            // «send 1 SOL to Jupiter / the vault» is a deposit, not a transfer to someone.
            if whole(&name.to_lowercase(), r"(?:the\s+)?(?:vault|jupiter\w*|kamino|jito\w*|хранилищ\w*|джупитер\w*|камино)").is_none() {
                return Some(Self::SendSol { name: name.into(), amount: value });
            }
        }
        // «отправь 0.05 NEAR на Bybit», «send all NEAR to Bybit»: a number or «all» is required — never guessed.
        if let Some(m) = first(&text, r#"(?i)(?:send|transfer|move|отправь\w*|переведи|перевести|скинь|кинь)\s+((?:all|весь|всё|все)\s+(?:(?:the|my|мой|мои)\s+)?)?(?:(\d+(?:[.,]\d+)?)\s*)?(?:near|неар\w*|ниар\w*)\s+(?:to|на|для)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$"#)
            && (opt(&m, 1).is_some() || opt(&m, 2).is_some())
        {
            return Some(Self::SendNear { name: trim_spaces(group(&m, 3)).into(), amount: opt(&m, 2).and_then(num) });
        }
        // «near» is also a word: «buy ETH near 2500» is a limit on ETH, not the NEAR token.
        if has(&q, r"\bкуп\w*|\bbuy\b") && has(&q, r"\bnear\b(?!\s*\$?\d)|\bнеар\w*|\bниар\w*|\bнир\b") && SpotAsset::find(&q).is_none() && !has(&q, TRADE_WORDS) {
            // «на 3» / «for 3» is dollars; «buy 2 NEAR» is coins and is not guessed.
            return Some(Self::BuyNear { usdc: amount.or_else(|| num_of(&q, r"(?:\bна|\bfor)\s+(\d+(?:[.,]\d+)?)")) });
        }
        // «продай неар», «sell 0.5 NEAR»: native NEAR to USDC on Solana; no number = all of it. Where the USDC lands («for
        // USDC on ethereum», «… on Solana») names no coin to sell: it's left out of the check for one.
        if has(&q, r"продай\w*|продать|\bsell\b") && has(&q, r"\bnear\b|\bнеар\w*|\bниар\w*|\bнир\b")
            && SpotAsset::find(&replacing(&q, r"\b(?:for|в|за|into)\s+usdc\b.*$", |_| String::new())).is_none()
            && !has(&q, TRADE_WORDS)
        {
            let coins = num_of(&q, r"(\d+(?:[.,]\d+)?)\s*(?:near|неар\w*|ниар\w*)");
            if coins.is_none() && amount.is_some() { return None }
            // «… for USDC on base», «продай NEAR в USDC на арбитрум»: the network the USDC arrives on; Solana by default.
            return Some(Self::SellNear { amount: coins, to: Self::network_in(&q) });
        }
        if has(&q, r"\bкуп\w*|\bbuy\b") && let Some(asset) = SpotAsset::find(&q) { return Some(Self::Buy { asset, usdc: amount }) }
        if has(&q, r"продай\w*|продать|\bsell\b") && let Some(asset) = SpotAsset::find(&q) {
            let coins = num_of(&q, r"(\d+(?:[.,]\d+)?)\s*(?:sol|btc|eth|jup|сол|битк|эфир)");
            // «продай сол на 10 баксов» names dollars, not coins: no amount to sell, never «all of it».
            if coins.is_none() && amount.is_some() { return None }
            return Some(Self::Sell { asset, amount: coins });
        }
        if vault && (out_of || has(&q, r"достан\w*")) { return Some(Self::LendWithdraw(symbol.into(), money)) }
        if vault && (into || has(&q, r"^\s*lend\b")) { return Some(Self::LendDeposit(symbol.into(), money)) }
        // «jupiter vault 1 USDC» without a verb.
        if jupiter && let Some(value) = coins.or(amount) { return Some(Self::LendDeposit(symbol.into(), Some(value))) }
        if let Some(m) = first(&text, r#"(?i)(?:send|pay|transfer|отправь\w*|переведи|перевести|скинь|кинь)\s+\$?(\d+(?:[.,]\d+)?)\s*(?:usdc|\$|долл\w*|dollars?|bucks)?\s+(?:to|на|для)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$"#)
            && let Some(value) = num(group(&m, 1))
        {
            let mut name = trim_spaces(group(&m, 2));
            // «… to Mom on ethereum», «… на main wallet по эфир сети»: the network goes, the name stays.
            let mut network = None;
            if let Some(tail) = first(name, r"(?i)\s+(?:on|in|via|по|в|на|через)\s+(?:the\s+)?(?:сети\s+)?(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|ethereum|эфир\w*|mainnet|base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|robinhood|робинхуд\w*|tempo|темпо|solana|солан\w*|hyperliquid|хайперликвид\w*)(?:\s+(?:network|chain|mainnet|сет[иья]))?\s*$") {
                let word = group(&tail, 1).to_lowercase();
                let known = Self::network(&word).filter(|n| ["hyperevm", "optimism", "bnb"].contains(&n.as_str()));
                network = Some(known.unwrap_or_else(|| {
                    let named = [("эфир", "ethereum"), ("бейс", "base"), ("арбитрум", "arbitrum"), ("полигон", "polygon"), ("робинхуд", "robinhood"), ("солан", "solana"), ("хайперликвид", "hyperliquid")];
                    if word == "mainnet" { return "ethereum".into() }
                    if word == "темпо" { return "tempo".into() }
                    named.iter().find(|(p, _)| word.starts_with(p)).map_or(word.clone(), |(_, n)| n.to_string())
                }));
                name = &name[..tail.get(0).map_or(name.len(), |r| r.start())];
            }
            // Our own places stay what they were; a pasted address is not a name.
            let place = whole(&name.to_lowercase(), r"(?:the\s+)?(?:solana|солан\w*|hyperliquid|хайперликвид\w*|base|бейс|aave|kamino|hlp|vault)").is_some();
            if !place && !has(&name.to_lowercase(), "0x[0-9a-f]{6}") && count(name) <= 40 {
                return Some(Self::SendTo { name: name.into(), amount: value, network });
            }
        }
        if !from_solana && has(&q, "arbitrum|арбитрум") && has(&q, BACK_TO_SOLANA) { return Some(Self::ArbitrumToSolana(amount)) }
        if !from_solana
            && let Some(net) = Self::network_in(&q)
            && ["polygon", "ethereum", "tempo", "hyperevm", "optimism", "bnb"].contains(&net.as_str())
            && has(&q, BACK_TO_SOLANA)
        {
            return Some(Self::EvmToSolana(net, amount));
        }
        if !from_solana && has(&q, r"\bbase\b|бейс|бейз|\bбаз[аеуы]\b") && has(&q, BACK_TO_SOLANA) { return Some(Self::BaseToSolana(amount)) }
        // «…to Solana» from Hyperliquid, with a word for taking it out («withdraw», «выведи», «верни», «from Hyperliquid») or
        // Solana as where it goes («send 3 USDC to Solana»). «send 0.5 SOL on solana to Bybit» names Solana as the place, not
        // as where it goes; «из соланы» is the other direction.
        if !vault && !from_solana
            && has(&q, r"(?:выв[её]д\w*|верн\w*|перевед\w*|withdraw\w*|move|send|bring)(?:[^.]|\.(?=\d))*(?:solana|солан)")
            && has(&q, r"выв[её]д\w*|верн\w*|withdraw\w*|hyperliquid|хайпер\w*|гипер\w*|\bhl\b|(?:\bto\b|\binto\b|\bв\b|\bна\b)\s+(?:the\s+|my\s+)?(?:solana|солан[уа]?)\b(?!\s+(?:to|на|для)\b)")
        {
            return Some(Self::WithdrawToSolana(amount));
        }
        None
    }

    /// A recurring buy's period: «every 2 weeks», «каждые 3 месяца», «раз в две недели», «каждую вторую неделю» — a count
    /// and its unit. A month is 30 days, as Jupiter's DCA and the «≈ a month» line count it. Hours and minutes aren't a
    /// recurring buy's (it runs in days): nil, never a week. The count goes first, then day, week, month: «каждую неделю в
    /// течение 2 месяцев» is weekly.
    fn schedule(q: &str, asset: SpotAsset, usdc: Decimal) -> Option<Self> {
        let counts = |w: &str| -> Option<i64> {
            Some(match w {
                "other" | "second" | "two" | "вторую" | "второй" | "два" | "две" => 2, "three" | "три" => 3, "four" | "четыре" => 4,
                "five" | "пять" => 5, "six" | "шесть" => 6, _ => return None,
            })
        };
        let units = |w: &str| -> Option<i64> {
            Some(match chars(w).iter().take(2).copied().collect::<String>().as_str() {
                "дн" | "де" | "су" | "da" => 1, "не" | "we" => 7, "ме" | "mo" => 30, "го" | "ye" => 365, _ => return None,
            })
        };
        // A few buys a day or a week is no period in days: «twice a week», «2 раза в день».
        if has(q, r"twice|thrice|(?:\b\d+|two|three)\s*(?:times|x)\s+(?:a|an|per|every|each)\b|(?:^|\s)(?:[2-9]|\d\d+|два|две|три|четыре|пять|шесть)\s+раза?\s+в\b") { return None }
        let days = if let Some(m) = first(q, r"(?:кажд\w*|every|each|раз\s+в)\s+(\d+|other|second|two|three|four|five|six|вторую|второй|два|две|три|четыре|пять|шесть)\s*(дн|день|сут|days?|нед|weeks?|мес|months?|год|years?|час|hours?|hrs?|h\b|ч\b|мин|min|sec|сек)")
            && let Some(n) = group(&m, 1).parse::<i64>().ok().or_else(|| counts(group(&m, 1)))
        {
            // «every 0 days» never gets past now and buys without end; ScheduledTask.parse refuses it too. Over ten years is
            // no schedule either.
            let unit = units(group(&m, 2))?;
            if !(1..=3650).contains(&n) || n * unit > 3650 { return None }
            n * unit
        } else if has(q, r"bi-?weekly|fortnight") {
            14
        } else if let Some(d) = first(q, r"(?:кажд\w*|раз\s+в|every|each|once\s+(?:a|per)|per)\s+(?:\w+\s+)??(день|сутки|day\b|недел|week|месяц|month)").and_then(|m| units(group(&m, 1))) {
            // The period word right after «каждый / раз в / every» goes before the loose words: «каждый месяц в день
            // зарплаты» and «в первый день каждого месяца» are monthly, «каждый рабочий день» daily.
            d
        } else if has(q, r"\bв\s+(?:день|сутки)|сутк|ежедневн|daily|\ba\s+day\b") {
            1
        } else if has(q, "недел|week") {
            7
        } else if has(q, "квартал|quarter") {
            90
        } else if has(q, r"полгода|пол\s+года|half[\s-]?(?:a\s+)?year|semi-?annual") {
            180
        } else if has(q, "месяц|месяч|month|числ") {
            30
        } else if has(q, r"(?:каждый|раз\s+в)\s+год|ежегодн|\byear|annual") {
            365
        } else if has(q, r"\bhours?\b|hourly|\bчас(?:а|ов)?\b|ежечас|\bminutes?\b|минут") {
            return None;
        } else {
            7
        };
        Some(Self::Schedule { asset, usdc, days })
    }

    /// «jupiter dca 100 USDC into SOL in 2 orders every day», «… over 4 weeks», «cancel jupiter dca SOL», «show jupiter dca».
    fn jupiter_dca(q: &str) -> Option<Self> {
        let rest = replacing(q, r"jupiter|jupyter|джупитер\w*|юпитер\w*", |_| " ".into());
        // «into BONK»: a coin by ticker; «jupiter» itself must not read as JUP.
        let named = first(&rest, r"(?:into|in|for|of|в|на)\s+\$?([a-z][a-z0-9]{1,9})\b").map(|m| group(&m, 1).to_string())
            .filter(|s| !["usdc", "usd", "orders", "order", "parts", "swaps", "total", "all"].contains(&s.as_str()))
            .map(|s| SpotAsset::find(&s).unwrap_or_else(|| SpotAsset::by_symbol(&s)));
        let coin = named.or_else(|| SpotAsset::find(&rest));
        if has(&rest, r"cancel|stop|close|\bend\b|отмен|останов|прекрат|закр|стоп") { return Some(Self::JupiterDcaCancel(coin)) }
        let usdc = first(&rest, r"\$\s*(\d+(?:[.,]\d+)?)").or_else(|| first(&rest, r"(\d+(?:[.,]\d+)?)\s*(?:usdc|\$|долл|dollars?|bucks|бакс)")).and_then(|m| num(group(&m, 1)));
        let Some(coin) = coin.filter(|c| *c != SpotAsset::usdc() && (usdc.is_some() || !has(&rest, r"show|list|status|\bmy\b|покаж|мои|список|статус|какие"))) else {
            return usdc.is_none().then_some(Self::JupiterDcaOrders);
        };
        let unit = |w: &str| -> Option<i64> {
            let p = |s: &[&str]| s.iter().any(|s| w.starts_with(s));
            if p(&["min", "мин"]) { Some(60) } else if p(&["hour", "час"]) { Some(3600) } else if p(&["day", "дн", "день", "сут"]) { Some(86400) }
            else if p(&["week", "недел"]) { Some(604_800) } else if p(&["month", "месяц"]) { Some(2_592_000) } else { None }
        };
        let mut orders = first(&rest, r"(\d+)\s*(?:orders?|buys?|purchases?|swaps?|times|parts?|раз\b|раза|покуп\w*|част\w*|сдел\w*)").and_then(|m| group(&m, 1).parse::<i64>().ok());
        let mut every = None;
        // ponytail: a count so big that seconds overflow is the most an Int holds here; Swift traps on it.
        if let Some(m) = first(&rest, r"(?:every|each|кажд\w*|раз\s+в)\s+(\d+)?\s*(minutes?|hours?|days?|weeks?|months?|мин\w*|час\w*|дн\w*|день|сут\w*|недел\w*|месяц\w*)")
            && let Some(u) = unit(group(&m, 2))
        {
            every = Some(opt(&m, 1).and_then(|n| n.parse::<i64>().ok()).unwrap_or(1).saturating_mul(u));
        } else if let Some(m) = first(&rest, r"\b(hourly|daily|weekly|monthly|ежечасн|ежедневн|еженедельн|ежемесячн)") {
            every = match group(&m, 1) {
                "hourly" | "ежечасн" => Some(3600), "daily" | "ежедневн" => Some(86400), "weekly" | "еженедельн" => Some(604_800),
                "monthly" | "ежемесячн" => Some(2_592_000), _ => None,
            };
        }
        // «over 4 weeks»: 4 swaps a week apart, or the given swaps spread over that time.
        if let Some(m) = first(&rest, r"(?:over|within|during|for|за|в\s+течени\w*|на)\s+(\d+)\s*(minutes?|hours?|days?|weeks?|months?|мин\w*|час\w*|дн\w*|день|сут\w*|недел\w*|месяц\w*)")
            && let Ok(n) = group(&m, 1).parse::<i64>()
            && let Some(u) = unit(group(&m, 2))
            && n > 0
        {
            match orders {
                Some(o) if o > 0 && every.is_none() => every = Some(n.saturating_mul(u) / o),
                None => {
                    orders = Some(n);
                    every = every.or(Some(u));
                }
                _ => {}
            }
        }
        Some(Self::JupiterDca { asset: coin, usdc, orders, every: every.unwrap_or(86400).max(60) })
    }

    /// Swaps when none are named: one per 50 USDC (Jupiter's least swap), 2 to 10.
    pub fn jupiter_dca_orders(usdc: Decimal) -> i64 {
        rounded(usdc / Decimal::from(50), 0, Rounding::Down).to_i64().unwrap_or(if usdc.is_sign_negative() { i64::MIN } else { i64::MAX }).clamp(2, 10)
    }

    /// Jupiter's least DCA, in USDC.
    pub fn jupiter_dca_minimum() -> Decimal { Decimal::ONE_HUNDRED }

    /// Jupiter's own minimums for its DCA, said before a card: nil when the order can be placed.
    pub fn jupiter_dca_problem(usdc: Decimal, orders: i64) -> Option<String> {
        if usdc < Self::jupiter_dca_minimum() { return Some(format!("Jupiter's DCA takes at least 100 USDC in all, 50 a swap: {} USDC is too little.", Fmt::num(usdc, 2))) }
        if orders < 2 { return Some("Jupiter's DCA needs at least 2 swaps.".into()) }
        if usdc / Decimal::from(orders) < Decimal::from(50) {
            let most = rounded(usdc / Decimal::from(50), 0, Rounding::Down).to_i64().unwrap_or(i64::MAX);
            return Some(format!("Jupiter's DCA buys at least 50 USDC a swap: {} USDC makes {most} swaps at most.", Fmt::num(usdc, 2)));
        }
        None
    }

    /// «day», «2 days», «week», «hour»: an interval in seconds as words.
    pub fn every_text(seconds: i64) -> String {
        for (s, w) in [(2_592_000, "month"), (604_800, "week"), (86400, "day"), (3600, "hour"), (60, "minute")] {
            if seconds % s == 0 { return if seconds == s { w.into() } else { format!("{} {w}s", seconds / s) } }
        }
        format!("{seconds} seconds")
    }

    /// A deposit, a buy or a stake that came without an amount: the chat asks, and the next answer fills it.
    pub fn amount_question(&self) -> Option<String> {
        let p = Self::place_name;
        Some(match self {
            Self::LendDeposit(s, None) => format!("How much {s} into Jupiter Lend?"),
            Self::Kamino { deposit: true, symbol, amount: None } => format!("How much {symbol} into Kamino?"),
            Self::Hlp { deposit: true, amount: None } => "How much USDC into HLP? (at least $5)".into(),
            Self::Aave { deposit: true, amount: None, network } => format!("How much USDC into Aave on {}?", p(network)),
            Self::Morpho { deposit: true, amount: None, network } => format!("How much USDC into Morpho on {}?", p(network)),
            Self::Buy { asset, usdc: None } => format!("For how many USDC should I buy {}?", asset.symbol),
            Self::JupiterDca { asset, usdc: None, .. } => format!("How many USDC in all for the Jupiter DCA into {}? (at least 100, 50 a swap)", asset.symbol),
            Self::BuyNear { usdc: None } => "For how many USDC should I buy NEAR?".into(),
            Self::BuyStock { symbol, usdc: None } => format!("For how many USDC should I buy {symbol}?"),
            Self::BuyOn { place, token, usdc: None, .. } => format!("For how many USDC should I buy {token} on {}?", p(place)),
            Self::Stake(None) => "How much SOL to stake?".into(),
            Self::SolanaTo(place, None) => format!("How many USDC should I move from Solana to {}?", p(place)),
            Self::ReceiveAcross { network, token, amount: None } => format!("How much {token} will you send from {}?", network_title(network)),
            Self::AaveBorrow { amount: None, network } => format!("How many USDC to borrow on Aave on {}?", p(network)),
            Self::KaminoBorrow { amount: None, against, .. } => format!("How many USDC to borrow on Kamino{}?", against.as_ref().map_or(String::new(), |s| format!(" against {s}"))),
            Self::AaveCollateral { amount: None, network } => format!("How much ETH as collateral on Aave on {}?", p(network)),
            _ => return None,
        })
    }

    /// The amount `with_amount` changes, as the command holds it: what a route step's details start from.
    pub fn editable_amount(&self) -> Option<Decimal> {
        match self {
            Self::LendDeposit(_, x) | Self::Kamino { amount: x, .. } | Self::Hlp { amount: x, .. } | Self::Aave { amount: x, .. } | Self::Morpho { amount: x, .. }
            | Self::Buy { usdc: x, .. } | Self::BuyNear { usdc: x } | Self::BuyStock { usdc: x, .. } | Self::BuyOn { usdc: x, .. } | Self::Stake(x)
            | Self::SolanaTo(_, x) | Self::JupiterDca { usdc: x, .. } => *x,
            _ => None,
        }
    }

    /// «create an agent wallet with $20 a day», «fund the agent wallet with 5 USDC», «send 1 USDC from the agent wallet
    /// to Mom», «freeze the agent wallet»; «создай кошелёк агента с лимитом 20$ в день», «пополни кошелёк агента на 5
    /// USDC», «переведи 1 USDC с кошелька агента на Маму», «заморозь кошелёк агента».
    pub(crate) fn agent(text: &str) -> Option<Self> {
        let q = IntentParser::plain_numbers(text).to_lowercase();
        if !has(&q, AGENT_WALLET) { return None }
        // A send out of it first: a payee named «Binance deposit» or «Frozen Yogurt» is a name, not another command.
        if let Some(m) = first(text, r#"(?i)(?:send|transfer|pay|отправь\w*|переведи|скинь)\s+(\d+(?:[.,]\d+)?)\s*(?:usdc|\$|долл\w*|бакс\w*)?\s+(?:from|с|со|из)\s+(?:the\s+|my\s+)?(?:agent(?:'s)?\s+wallet|кошел\w*\s+агента|агентск\w*\s+кошел\w*)\s+(?:to|на|для)\s+[«"“]?(.+?)[»"”]?\s*[.!]?$"#)
            && let Some(usdc) = num(group(&m, 1))
            && usdc > Decimal::ZERO
        {
            return Some(Self::AgentSend { usdc, name: group(&m, 2).trim().to_string() });
        }
        if has(&q, r"\bunfreeze|разморо\w*") { return Some(Self::AgentFreeze(false)) }
        if has(&q, r"\bfreeze|замороз\w*|заморож\w*") { return Some(Self::AgentFreeze(true)) }
        if has(&q, r"\b(?:create|make|open|set\s+up)\b|созда\w*|заведи|открой") {
            let daily = num_of(&q, r"\$?\s*(\d+(?:[.,]\d+)?)\s*(?:\$|usd\w*|долл\w*|бакс\w*)?\s*(?:a|per|в|за)\s*(?:day|день|сутки)")?;
            return (daily > Decimal::ZERO).then_some(Self::AgentCreate { daily_usd: daily });
        }
        // Money coming OUT of the agent wallet is never a deposit into it («положи 1 USDC с кошелька агента маме»).
        let out_of = has(&q, r"(?:\bfrom|\bс|\bсо|\bиз)\s+(?:the\s+|my\s+)?(?:agent(?:'s)?\s+wallet|кошел\w*\s+агента|агентск\w*\s+кошел)");
        if !out_of
            && has(&q, r"\b(?:fund|top\s+up|deposit)\b|пополн\w*|закин\w*|положи")
            && let Some(usdc) = num_of(&q, r"(\d+(?:[.,]\d+)?)\s*(?:usdc|\$|долл\w*|бакс\w*)")
        {
            return (usdc > Decimal::ZERO).then_some(Self::AgentFund { usdc });
        }
        None
    }

    /// A payee that is the wallet itself — «me», «my wallet», «Main», «себе», «мой кошелёк» — or the active wallet's own
    /// name: the agent wallet's rules allow its own addresses from the start, no saved entry needed.
    pub fn is_own_wallet(name: &str, label: Option<&str>) -> bool {
        let lowered = name.to_lowercase().replace('ё', "е");
        let n = lowered.trim_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation() || "«»“”„…—–".contains(c));
        if let Some(l) = label && !l.is_empty() && n == l.to_lowercase() { return true }
        whole(n, r"me|myself|main|my(?:\s+own)?(?:\s+main)?\s+(?:wallet|account|address)|себе|мне|основно\w*(?:\s+кошел\w*)?|(?:мой|мо\w+)\s+(?:основно\w*\s+)?(?:кошел\w*|счет\w*|адрес\w*)").is_some()
    }

    /// Whether the user's own words ask for privacy: a send the chat model wrote without «privately» is then not run.
    pub fn asks_privacy(text: &str) -> bool { has(text, PRIVACY_WORD) }

    /// A send that could go privately but doesn't, in a step of a chain or after a schedule's or alert's «…:»: those
    /// are read again later without the user's words. A lone send isn't one: the run makes it private itself.
    pub fn has_public_send(canonical: &str) -> bool {
        let steps = crate::normalize::ChatRouter::steps(canonical);
        if steps.is_none() && !canonical.contains(':') { return false }
        let parts: Vec<String> = steps.unwrap_or_else(|| vec![canonical.to_string()]).into_iter()
            .flat_map(|step| {
                let tail = step.find(':').map(|i| step[i + 1..].trim().to_string());
                std::iter::once(step).chain(tail)
            })
            .collect();
        parts.iter().any(|p| Self::parse(p).is_some_and(|c| c.made_private().is_some_and(|made| made != c)))
    }

    /// This send or NEAR Intents swap made private (`SendPrivately`, `SwapPrivately`), when it has an amount; None for
    /// anything else.
    pub fn made_private(&self) -> Option<Self> {
        let swap = |from: &str, on: &str, to: &str, at: &str, amount: Decimal| Self::SwapPrivately {
            from: from.into(), on: on.into(), to: to.into(), at: at.into(), amount,
        };
        Some(match self {
            Self::SendToken { name, token, amount: Some(x), network } => {
                Self::SendPrivately { name: name.clone(), token: token.clone(), amount: *x, network: network.clone() }
            }
            Self::SendTo { name, amount, network } => Self::SendPrivately { name: name.clone(), token: "USDC".into(), amount: *amount, network: network.clone() },
            Self::SendPrivately { .. } | Self::SwapPrivately { .. } => self.clone(),
            Self::BuyNear { usdc: Some(x) } => swap("USDC", "solana", "NEAR", "near", *x),
            Self::SellNear { amount: Some(x), to } => swap("NEAR", "near", "USDC", to.as_deref().unwrap_or("solana"), *x),
            Self::SwapOn { place, from, to, amount: Some(x) } if place == "near" => swap(from, "near", to, "near", *x),
            Self::CrossSwap { from, on, to, at, amount: Some(x) } => swap(from, on, to, at, *x),
            _ => return None,
        })
    }

    /// A private swap as the same swap in the open: what the switch over Confirm offers beside it.
    pub fn made_public(&self) -> Option<Self> {
        let Self::SwapPrivately { from: a, on: from, to: b, at: to, amount: x } = self else { return None };
        let x = Some(*x);
        Some(if a.to_uppercase() == "USDC" && from == "solana" && b.to_uppercase() == "NEAR" && to == "near" {
            Self::BuyNear { usdc: x }
        } else if a.to_uppercase() == "NEAR" && from == "near" && b.to_uppercase() == "USDC" {
            Self::SellNear { amount: x, to: (to != "solana").then(|| to.clone()) }
        } else if from == "near" && to == "near" {
            Self::SwapOn { place: "near".into(), from: a.clone(), to: b.clone(), amount: x }
        } else {
            Self::CrossSwap { from: a.clone(), on: from.clone(), to: b.clone(), at: to.clone(), amount: x }
        })
    }

    pub fn with_amount(&self, amount: Decimal) -> Self {
        let mut next = self.clone();
        let x = Some(amount);
        match &mut next {
            Self::LendDeposit(_, a) | Self::Kamino { amount: a, .. } | Self::Hlp { amount: a, .. } | Self::Aave { amount: a, .. } | Self::Morpho { amount: a, .. }
            | Self::Buy { usdc: a, .. } | Self::JupiterDca { usdc: a, .. } | Self::BuyNear { usdc: a } | Self::BuyStock { usdc: a, .. } | Self::BuyOn { usdc: a, .. }
            | Self::Stake(a) | Self::SolanaTo(_, a) | Self::ReceiveAcross { amount: a, .. } | Self::AaveBorrow { amount: a, .. }
            | Self::KaminoBorrow { amount: a, .. } | Self::AaveCollateral { amount: a, .. } | Self::Sell { amount: a, .. } | Self::SellOn { amount: a, .. }
            | Self::SwapOn { amount: a, .. } | Self::SendToken { amount: a, .. } | Self::CrossSwap { amount: a, .. } => *a = x,
            Self::SendPrivately { amount: a, .. } | Self::SwapPrivately { amount: a, .. } => *a = amount,
            _ => {}
        }
        next
    }

    /// A network's own coin, the one its fees are paid in: on its network it is swapped for USDC, never bought from a token list.
    pub(crate) fn own_coin(place: &str) -> &'static [&'static str] {
        match place { "polygon" => &["POL", "MATIC"], "hyperevm" => &["HYPE"], "bnb" => &["BNB"], _ => &["ETH"] }
    }

    /// How many tokens a sale names: the number right after «sell»/«продай» («sell 100 0x…», «продай 1,5 B5AN…»), as «sell
    /// 100 AERO on base» reads it, or one before «tokens»/«токенов»/«шт» anywhere. Dollars and percents aren't a count, and
    /// neither is the «0» of «0x…» or the digit a mint starts with. A number it would read only in part («1 000», «5
    /// тысяч», «1/2») is none either: the sale is then refused, not made smaller.
    pub(crate) fn sold_count(q: &str) -> Option<Decimal> {
        if has(q, SPACED_NUMBER) { return None }
        num_of(q, r"(?:\bsell|\bdump|прода\w*)\s+(?:the\s+|my\s+)?(\d+(?:[.,]\d+)?)(?![\w.,%$/])(?!\s*(?:\$|%|usd|долл|бакс|bucks|dollars?|worth|percent|процент|тыс|thousand|млн|million|миллион|млрд|billion))")
            .or_else(|| num_of(q, r"(?:^|\s)(\d+(?:[.,]\d+)?)\s*(?:tokens?|токен\w*|шт)"))
    }

    /// A sale that names a number or a share sold_count didn't read («sell 0x… 100», «sell 50% of …», «продай половину»,
    /// «продай треть», «sell ten», «… for 10 USDC»): never «all of it».
    pub(crate) fn names_other_count(q: &str) -> bool { ChatWords::may_change(q) || has(q, r"\bthird|\bpart|част[ьи]|частичн|\bsome\b|немного|несколько") }

    /// The multisig's three forms, as the chat writes them in English.
    fn multisig(text: &str) -> Option<Self> {
        let t = trim_set(text.trim(), ".!");
        if !t.to_lowercase().contains("multisig") { return None }
        if whole(t, r"(?i)(?:show|open)\s+(?:the\s+|my\s+)?multisig").is_some() { return Some(Self::MultisigShow) }
        let nets = |s: &str| -> Option<Vec<String>> {
            split(s, r"\s*,\s*|\s+and\s+").into_iter().map(|w| Self::network(w).filter(|n| ContractCall::NETWORKS.contains(&n.as_str()))).collect()
        };
        if let Some(m) = whole(t, r"(?i)send\s+(all|\d+(?:\.\d+)?)\s+(\S+)\s+from\s+(?:the\s+|my\s+)?multisig\s+to\s+(.+?)(?:\s+on\s+(\S+(?:\s+chain)?))?") {
            // A network it can't read is none, as Swift's `?? .some(nil)` gives it.
            let network = opt(&m, 4).and_then(|s| nets(s)?.into_iter().next());
            let amount = if group(&m, 1).to_lowercase() == "all" { None } else { decimal(group(&m, 1)) };
            return Some(Self::MultisigSend { token: group(&m, 2).into(), amount, to: group(&m, 3).into(), network });
        }
        if let Some(m) = whole(t, r"(?i)(?:make|create|set\s+up)\s+(?:a\s+|my\s+)?multisig(?:\s+vault)?(?:\s+(\d+)\s+of\s+(\d+))?(?:\s+with\s+(.+?))?(?:\s+on\s+(.+))?") {
            let networks = match opt(&m, 4) { Some(s) => nets(s)?, None => Vec::new() };
            let keys = opt(&m, 3).map_or(Vec::new(), |k| split(k, r"\s*,\s*|\s+and\s+").into_iter().map(String::from).collect());
            let int = |i| opt(&m, i).and_then(|n: &str| n.parse::<i64>().ok());
            return Some(Self::MultisigCreate { threshold: int(1), of: int(2), keys, networks });
        }
        None
    }

    /// «base» → «Base», «bnb» → «BNB Chain»; any other key capitalized.
    pub fn place_name(place: &str) -> String {
        match place {
            "base" => "Base", "arbitrum" => "Arbitrum", "robinhood" => "Robinhood", "polygon" => "Polygon", "ethereum" => "Ethereum",
            "tempo" => "Tempo", "hyperevm" => "HyperEVM", "optimism" => "Optimism", "bnb" => "BNB Chain", "near" => "NEAR",
            _ => return capitalized(place),
        }
        .into()
    }

    /// The EVM network a word names: «арбитрум» → «arbitrum», «полигон» → «polygon», «эфириум» or «mainnet» → «ethereum».
    pub(crate) fn network(word: &str) -> Option<String> {
        let w = word.to_lowercase();
        let p = |s: &[&str]| s.iter().any(|s| w.starts_with(s));
        // HyperEVM before Hyperliquid: both start with «hyper»; BNB Chain before Base: both start with «b».
        let n = if p(&["hyperevm", "хайперевм", "гиперевм"]) || whole(&w, r"hyper\s+evm").is_some() { "hyperevm" }
            else if p(&["optimism", "оптимизм"]) { "optimism" }
            else if p(&["bnb", "bsc", "бнб", "бсц", "binance"]) { "bnb" }
            else if p(&["b", "бейс", "бейз"]) { "base" }
            else if p(&["arb", "арб"]) { "arbitrum" }
            else if p(&["rob", "роб"]) { "robinhood" }
            else if p(&["pol", "пол"]) { "polygon" }
            else if p(&["eth", "эфир", "main", "мейн"]) { "ethereum" }
            else if p(&["temp", "темп"]) { "tempo" }
            else if p(&["hyper", "гипер", "хайпер"]) || w == "hl" { "hyperliquid" }
            else { return None };
        Some(n.into())
    }

    /// The first EVM network the text names, if any.
    pub(crate) fn network_in(q: &str) -> Option<String> {
        first(q, r"\b(hyperevm|hyper\s+evm|хайперевм\w*|гиперевм\w*|optimism|оптимизм\w*|bnb(?:\s+(?:smart\s+)?chain)?|bsc|бнб\w*|бсц\w*|binance\s+smart\s+chain|base|бейс\w*|arbitrum|арбитрум\w*|polygon|полигон\w*|ethereum|эфириум\w*|mainnet|мейннет\w*|tempo|темпо\w*)\b")
            .and_then(|m| Self::network(group(&m, 1)))
    }

    /// The Backpack stock a request names, by ticker or company, in English or Russian.
    pub(crate) fn kamino_stock(q: &str) -> Option<&'static str> {
        if has(q, r"\bspcx\b|space\s*x|спейс\w*") { return Some("SPCX") }
        if has(q, r"\bsndk\b|sandisk|сандиск") { return Some("SNDK") }
        if has(q, r"\bskhy\b|hynix|хайникс|хиникс") { return Some("SKHY") }
        if has(q, r"\bmu\b|micron|микрон") { return Some("MU") }
        None
    }

    /// «1», «1 USDC», «$1.5», «да, 2», «ok 3», «1,000» — a short reply that is only an amount, with the unit it names:
    /// "$", "USDC", "USDT" or "SOL" (nil: none).
    pub(crate) fn amount_reply(text: &str) -> Option<(Decimal, Option<&'static str>)> {
        let q = IntentParser::plain_numbers(text.to_lowercase().trim());
        let m = whole(&q, r"(?:(?:да|yes|ok|окей|давай|го|sure|yep|угу)[,!\s]*)?(?:на\s+|for\s+)?(\$)?\s*(\d+(?:[.,]\d+)?)\s*(usdc|юсдс|юсдц|usdt|usd|\$|долл\w*|dollars?|bucks|бакс\w*|sol|сол)?\s*[.!]?")?;
        let value = num(group(&m, 2)).filter(|v| *v > Decimal::ZERO)?;
        let unit = opt(&m, 3).map(|w| {
            if w.starts_with("usdc") || w.starts_with("юсд") { "USDC" } else if w == "usdt" { "USDT" } else if w == "sol" || w == "сол" { "SOL" } else { "$" }
        });
        // «$5 sol» says both.
        if opt(&m, 1).is_some() && unit == Some("SOL") { return None }
        Some((value, unit.or(opt(&m, 1).map(|_| "$"))))
    }

    /// The amount alone, whatever its unit. Use `amount_answer_for`, which checks the unit against the question.
    pub fn amount_answer(text: &str) -> Option<Decimal> { Self::amount_reply(text).map(|r| r.0) }

    /// The answer to `waiting`'s «How much?»: nil when its unit isn't what was asked for («$5» or «на 5 долларов» to a SOL
    /// stake, «1 sol» to a USDC deposit) or it's under the asset's smallest unit (0.000001 USDC, 0.000000001 SOL), which
    /// would build a «Deposit 0 USDC» card.
    pub fn amount_answer_for(text: &str, waiting: &WalletCommand) -> Option<Decimal> {
        let (value, unit) = Self::amount_reply(text)?;
        let asset = waiting.amount_asset();
        let fits = unit.is_none_or(|u| u == asset || (u == "$" && asset != "SOL"));
        let least = if asset == "SOL" { Decimal::new(1, 9) } else { Decimal::new(1, 6) };
        (fits && value >= least).then_some(value)
    }

    /// What the amount of a command waiting for one counts: SOL for a stake, the deposited coin for Jupiter Lend and
    /// Kamino, else USDC (buys are paid in it).
    pub fn amount_asset(&self) -> String {
        match self {
            Self::LendDeposit(s, _) | Self::Kamino { symbol: s, .. } => s.clone(),
            Self::Stake(_) => "SOL".into(),
            _ => "USDC".into(),
        }
    }

    /// In words, for the confirm button and the reply: «deposit 1 USDC into Jupiter Lend».
    pub fn summary(&self) -> String {
        let n = |d: &Option<Decimal>| d.map_or("all".into(), |d| Fmt::num(d, 6));
        let plain = |d: &Option<Decimal>| d.map_or("all".into(), decimal_text);
        let p = Self::place_name;
        // A mint keeps its case.
        let name = |t: &str| if t.starts_with("0x") || count_chars(t) >= 32 { t.to_string() } else { t.to_uppercase() };
        let ticker = |t: &str, place: &str| stablecoins::find(t, place).map_or(t.to_uppercase(), |c| c.symbol.into());
        match self {
            Self::Buy { asset, usdc } => format!("buy {} for {} USDC", asset.symbol, n(usdc)),
            Self::BuyNear { usdc } => format!("buy native NEAR for {} USDC (to your NEAR account)", n(usdc)),
            Self::SellNear { amount, to } => format!("sell {} NEAR from your NEAR account for USDC on {}", n(amount), to.as_deref().map_or("Solana".into(), p)),
            Self::Sell { asset, amount } => format!("sell {} {}", n(amount), asset.symbol),
            Self::Schedule { asset, usdc, days } => format!("buy {} for {} USDC every {}", asset.symbol, Fmt::num(*usdc, 2), Self::every_text(days.saturating_mul(86_400))),
            Self::StopSchedules => "stop recurring buys".into(),
            // The canonical form: it parses back to the same order.
            Self::JupiterDca { asset, usdc, orders, every } => format!(
                "Jupiter DCA {} USDC into {} in {} orders every {}", n(usdc), asset.symbol,
                orders.or(usdc.map(Self::jupiter_dca_orders)).unwrap_or(2), Self::every_text(*every)
            ),
            Self::JupiterDcaCancel(a) => format!("cancel Jupiter DCA{}", a.as_ref().map_or(String::new(), |a| format!(" {}", a.symbol))),
            Self::JupiterDcaOrders => "show Jupiter DCA".into(),
            Self::WithdrawToSolana(u) => format!("withdraw {} USDC from Hyperliquid to Solana", n(u)),
            Self::LendDeposit(s, x) => format!("deposit {} {s} into Jupiter Lend", n(x)),
            Self::LendWithdraw(s, x) => format!("withdraw {} {s} from Jupiter Lend", n(x)),
            Self::Kamino { deposit, symbol, amount } => {
                if *deposit { format!("deposit {} {symbol} into Kamino", n(amount)) } else { format!("withdraw {} {symbol} from Kamino", n(amount)) }
            }
            Self::Stake(x) => format!("stake {} SOL (→ JitoSOL)", n(x)),
            Self::Unstake(x) => format!("unstake {} JitoSOL", n(x)),
            Self::Yields(a) => format!("rates for {}", a.as_deref().unwrap_or("USDC and SOL")),
            Self::Hlp { deposit, amount } => if *deposit { format!("deposit {} USDC into HLP", n(amount)) } else { format!("withdraw {} USDC from HLP", n(amount)) },
            Self::Allocate { symbol, max_share, amount, without, from } => {
                let names: Vec<&str> = without.iter().filter_map(|k| SPREAD_PROTOCOLS.iter().find(|p| p.key == k).map(|p| p.name)).collect();
                format!(
                    "spread {}{symbol}{} across the best rates{}{}",
                    amount.map_or(String::new(), |x| format!("{} ", Fmt::num(x, 2))),
                    from.as_deref().map_or(String::new(), |f| format!(" from {}", if f == "solana" { "Solana".into() } else { p(f) })),
                    max_share.map_or(String::new(), |s| format!(" (max {}% per protocol)", Fmt::num(s, 2))),
                    if without.is_empty() { String::new() } else { format!(" without {}", names.join(", ")) },
                )
            }
            Self::Rebalance { max_share } => {
                format!("move deposited USDC to the best rates{}", max_share.map_or(String::new(), |s| format!(" (max {}% per protocol)", Fmt::num(s, 2))))
            }
            Self::Aave { deposit, amount, network } => {
                if *deposit { format!("deposit {} USDC into Aave on {}", n(amount), p(network)) } else { format!("withdraw {} USDC from Aave on {} to Solana", n(amount), p(network)) }
            }
            Self::Morpho { deposit, amount, network } => {
                if *deposit { format!("deposit {} USDC into Morpho on {}", n(amount), p(network)) } else { format!("withdraw {} USDC from Morpho on {} to Solana", n(amount), p(network)) }
            }
            Self::BaseToSolana(x) => format!("move {} USDC from Base to Solana", n(x)),
            Self::ArbitrumToSolana(x) => format!("move {} USDC from Arbitrum to Solana", n(x)),
            Self::EvmToSolana(net, x) => format!("move {} USDC from {} to Solana", n(x), p(net)),
            Self::SendTo { name, amount, network } => format!("send {} USDC to {name}{}", Fmt::num(*amount, 2), network.as_ref().map_or(String::new(), |n| format!(" on {n}"))),
            Self::SendNear { name, amount } => format!("send {} NEAR to {name}", n(amount)),
            Self::CloseNear { name } => format!("close your NEAR account and send all its NEAR to {name}"),
            Self::SendSol { name, amount } => format!("send {} SOL to {name}", Fmt::num(*amount, 9)),
            // The amount without grouping («1973.719299», not «1,973.72»): the text parses back to the same transfer.
            Self::SendToken { name, token, amount, network } => {
                format!("send {} {token}{} to {name}", plain(amount), network.as_ref().map_or(String::new(), |n| format!(" on {n}")))
            }
            Self::SendPrivately { name, token, amount, network } => {
                format!("send {} {token} privately{} to {name}", decimal_text(*amount), network.as_ref().map_or(String::new(), |n| format!(" on {n}")))
            }
            Self::AgentCreate { daily_usd } => format!("create an agent wallet with ${} a day", decimal_text(*daily_usd)),
            Self::AgentFund { usdc } => format!("fund the agent wallet with {} USDC", decimal_text(*usdc)),
            Self::AgentSend { usdc, name } => format!("send {} USDC from the agent wallet to {name}", decimal_text(*usdc)),
            Self::AgentFreeze(on) => (if *on { "freeze the agent wallet" } else { "unfreeze the agent wallet" }).into(),
            Self::Receive => "show where to top up".into(),
            Self::BuyStock { symbol, usdc } => format!(
                "buy {} for {} USDC on Robinhood Chain", if symbol.starts_with("0x") { format!("the token {symbol}") } else { format!("{symbol} stock tokens") }, n(usdc)
            ),
            Self::SellStock { symbol, amount } => format!(
                "sell {} {} on Robinhood Chain", n(amount), if symbol.starts_with("0x") { format!("of the token {symbol}") } else { format!("{symbol} stock tokens") }
            ),
            Self::RobinhoodToSolana(x) => format!("move {} USDG from Robinhood Chain to Solana", n(x)),
            Self::BuyOn { place, token, usdc, from } => format!(
                "buy {} on {} for {} USDC{}", if token.starts_with("0x") { format!("the token {token}") } else { ticker(token, place) }, p(place), n(usdc),
                from.as_deref().map_or(String::new(), |f| format!(" from {}", p(f)))
            ),
            Self::SellOn { place, token, amount, keep } => format!(
                "sell {} {} on {} {}", n(amount), if token.starts_with("0x") { format!("of the token {token}") } else { ticker(token, place) }, p(place),
                if *keep { "and keep the USDC there" } else { "for USDC on Solana" }
            ),
            // The amount without grouping, the contracts whole: the text parses back to the same swap.
            Self::SwapOn { place, from, to, amount } => format!(
                "swap {} {} for {} on {}", plain(amount), name(from), name(to), if place == "hyperliquid" { "Hyperliquid spot".into() } else { p(place) }
            ),
            Self::ReceiveAcross { network, token, amount } => {
                format!("receive {}{token} from {} as USDC on Solana", amount.map_or(String::new(), |x| format!("{} ", decimal_text(x))), network_title(network))
            }
            Self::CrossSwap { from, on, to, at, amount } => format!("swap {} {} on {} for {} on {}", plain(amount), name(from), p(on), name(to), p(at)),
            // Both places on NEAR read back as a swap there; across networks, as a swap across.
            Self::SwapPrivately { from, on, to, at, amount } => if on == at {
                format!("swap {} {} privately for {} on {}", decimal_text(*amount), name(from), name(to), p(at))
            } else {
                format!("swap {} {} on {} privately for {} on {}", decimal_text(*amount), name(from), p(on), name(to), p(at))
            },
            Self::SolanaTo(place, x) => format!("move {} USDC from Solana to {}", n(x), p(place)),
            Self::Pay { method, url, .. } => format!("pay for {method} {url} in USDC on Tempo"),
            Self::ContractCall(c) => {
                let k = chars(&c.contract);
                format!("call {} on {}…{} on {}", c.name(), k[..6.min(k.len())].concat(), k[k.len().saturating_sub(4)..].concat(), p(&c.network))
            }
            Self::Gas { place, usd } => format!("add ≈ {} of gas on {}", Fmt::usd(usd.unwrap_or(Decimal::ONE)), p(place)),
            Self::Bridge { from, to, amount } => {
                format!("move {} USDC from {} to {}{}", n(amount), p(from), p(to), if to == "robinhood" { " (arrives as USDG)" } else { "" })
            }
            Self::Cctp { from, to, amount } => format!("move {} USDC from {} to {} via CCTP", n(amount), p(from), p(to)),
            Self::Intents { from, to, amount } => format!("move {} USDC from {} to {} via NEAR Intents", n(amount), p(from), p(to)),
            Self::Usdt { from, to, amount, layer_zero } => {
                format!("move {} USDT from {} to {}{}", n(amount), p(from), p(to), if *layer_zero { " via LayerZero" } else { "" })
            }
            Self::AaveBorrow { amount, network } => format!("borrow {} USDC on Aave on {}", n(amount), p(network)),
            Self::KaminoBorrow { amount, against, adding } => format!(
                "borrow {} USDC on kamino{}", n(amount),
                against.as_ref().map_or(String::new(), |s| format!(" against {}{s}", adding.map_or(String::new(), |a| format!("{} ", n(&Some(a))))))
            ),
            Self::KaminoRepay { amount, against } => format!(
                "{}{}", if amount.is_none() { "repay all USDC on kamino".into() } else { format!("repay {} USDC on kamino", n(amount)) },
                against.as_ref().map_or(String::new(), |s| format!(" against {s}"))
            ),
            Self::AaveRepay { amount, network } => {
                if amount.is_none() { format!("repay all USDC on Aave on {}", p(network)) } else { format!("repay {} USDC on Aave on {}", n(amount), p(network)) }
            }
            Self::AaveCollateral { amount, network } => format!("supply {} ETH as collateral on Aave on {}", n(amount), p(network)),
            Self::ClaimRewards => "claim rewards on Merkl".into(),
            Self::HlSpotBuy { symbol, usdc } => format!("buy {symbol} on Hyperliquid spot for {} USDC", n(usdc)),
            Self::HlSpotSell { symbol, amount } => format!("sell {} {symbol} on Hyperliquid spot", plain(amount)),
            Self::ShowApprovals => "show my token approvals".into(),
            Self::RevokeApproval { place, token, spender, permit2 } => {
                format!("revoke the approval of {token} to {spender} on {}{}", p(place), if *permit2 { " through Permit2" } else { "" })
            }
            Self::SweepDust { under } => format!("sweep dust{} into USDC on Base, Arbitrum and Polygon", under.map_or(String::new(), |u| format!(" under ${}", Fmt::num(u, 2)))),
            Self::SweepRewards { min_usd, into } => {
                let five = Decimal::from(5);
                format!("sweep Merkl rewards once worth {} or more into {}", Fmt::usd(min_usd.unwrap_or(five).max(five)), into.as_deref().unwrap_or("USDC"))
            }
            Self::MultisigCreate { threshold, of, keys, networks } => format!(
                "make a multisig{}{}{}", threshold.map_or(String::new(), |m| format!(" {m} of {}", of.unwrap_or(m))),
                if keys.is_empty() { String::new() } else { format!(" with {}", keys.join(", ")) },
                if networks.is_empty() { String::new() } else { format!(" on {}", networks.join(", ")) }
            ),
            Self::MultisigSend { token, amount, to, network } => {
                format!("send {} {token} from the multisig to {to}{}", plain(amount), network.as_ref().map_or(String::new(), |n| format!(" on {n}")))
            }
            Self::MultisigShow => "show the multisig".into(),
        }
    }
}

/// `String.count`.
fn count_chars(s: &str) -> usize { count(s) }

/// `AddressCheck.Network(rawValue:)?.title ?? net`.
fn network_title(net: &str) -> String {
    serde_json::from_value::<wallet::address_check::Network>(serde_json::Value::String(net.into())).map_or(net.into(), |n| n.title().into())
}

impl IntentParser {
    /// Whether the text can start a new trade plan: it names a market or a direction. «1 USDC» alone does not — it would
    /// otherwise turn into the default «long ETH».
    pub fn starts_plan(text: &str, markets: &std::collections::HashSet<String>) -> bool {
        has(&text.to_lowercase(), r"лонг|шорт|\blong\b|\bshort\b|\bbuy\b|\bsell\b") || Self::named_asset(text, markets).is_some()
    }
}

/// A regular purchase the app carries out while it is open.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub symbol: String,
    #[serde(with = "number")]
    pub usdc: Decimal,
    pub days: i64,
    #[serde(with = "crate::foundation::date")]
    pub next: Date,
    /// Switched off on the Schedule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
}

impl Schedule {
    pub fn new(symbol: &str, usdc: Decimal, days: i64, next: Date) -> Self { Self { symbol: symbol.into(), usdc, days, next, paused: None } }

    pub fn asset(&self) -> Option<SpotAsset> { ALL.iter().find(|a| a.symbol == self.symbol).cloned() }

    /// After a run: the next date strictly in the future, so a long pause does not buy several times at once. A stored
    /// «every 0 days» counts as daily: it would otherwise never get past now.
    pub fn advance(&mut self, now: Date) {
        // A step past what a date holds (a saved file's number) ends at the last date there is, as far as any run goes.
        let step = chrono::TimeDelta::try_days(self.days.max(1)).unwrap_or(chrono::TimeDelta::MAX);
        loop {
            self.next = self.next.checked_add_signed(step).unwrap_or(chrono::DateTime::<chrono::Utc>::MAX_UTC);
            if self.next > now || self.next == chrono::DateTime::<chrono::Utc>::MAX_UTC { break }
        }
    }
}

/// «call arbitrum 0x… deposit(uint256,address) 10e6 me approving 10 USDC»: any contract's function on an EVM network
/// ihatecrypto signs on, from its signature, the arguments as written («me» is this wallet), in the token's smallest
/// units. The app encodes it, checks the contract on the network's explorer, simulates it with its exact approve and
/// shows the card; the signer reads the calldata again and refuses what drains (unlimited approvals, paying someone else).
#[derive(Debug, Clone, PartialEq)]
pub struct ContractCall {
    /// The EVM network's key (base, arbitrum, …).
    pub network: String,
    pub contract: String,
    /// «deposit(uint256,address)»
    pub function: String,
    pub args: Vec<String>,
    /// The network's own coin sent with the call, in whole coins.
    pub value: Option<Decimal>,
    /// An exact approve of this much of the token to the contract, made first.
    pub approve: Option<Approve>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Approve {
    pub amount: Decimal,
    /// The token's ticker on the network (USDC, a stablecoin) or its contract.
    pub token: String,
}

impl ContractCall {
    pub const NETWORKS: [&str; 9] = ["base", "arbitrum", "robinhood", "polygon", "ethereum", "tempo", "hyperevm", "optimism", "bnb"];

    pub fn name(&self) -> &str { self.function.split('(').next().unwrap_or("") }

    pub fn parse(text: &str) -> Option<Self> {
        let m = whole(text, r"(?i)\s*call\s+(\S+)\s+(0x[0-9a-f]{40})\s+(\w+\([a-z0-9,]*\))(.*?)(?:\s+with\s+(\d+(?:\.\d+)?)\s+[a-z]+)?(?:\s+approving\s+(\d+(?:\.\d+)?)\s+(\S+))?\s*[.]?\s*")?;
        let network = WalletCommand::network(group(&m, 1)).filter(|n| Self::NETWORKS.contains(&n.as_str()))?;
        Some(Self {
            network,
            contract: group(&m, 2).into(),
            function: group(&m, 3).into(),
            args: split_chars(group(&m, 4), crate::foundation::is_space).into_iter().map(String::from).collect(),
            value: opt(&m, 5).and_then(decimal),
            approve: opt(&m, 6).and_then(decimal).map(|amount| Approve { amount, token: group(&m, 7).into() }),
        })
    }

    /// The command as the chat writes it.
    pub fn text(&self) -> String {
        format!(
            "call {} {} {}{}{}{}", self.network, self.contract, self.function, self.args.iter().map(|a| format!(" {a}")).collect::<String>(),
            self.value.map_or(String::new(), |v| format!(" with {} coins", decimal_text(v))),
            self.approve.as_ref().map_or(String::new(), |a| format!(" approving {} {}", decimal_text(a.amount), a.token))
        )
    }
}
