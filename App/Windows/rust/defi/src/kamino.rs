//! Kamino Lend (`Kamino.swift`): rates, deposit, withdraw, borrow and repay transactions built by Kamino's own API,
//! positions. The main market lends; the Backpack market takes Backpack's tokenized stocks as collateral and lends
//! USDC against them. The first deposit on a market also creates the user's Kamino accounts there: rent in SOL, shown
//! by simulation.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::STANDARD};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio::task::JoinSet;
use wallet::{Result, WalletError};

use crate::{Object, READ_TIMEOUT, dec, decimal, json, object, objects, str_dec, string, text};

pub const MAIN_MARKET: &str = "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF";
/// Kamino's Backpack market (opened 2026-10-28, risk set by Allez Labs): its own pool, apart from the main market.
pub const BACKPACK_MARKET: &str = "4iRHKGsTq3e4uut6e4PfyV9AEbNuXaMu9JaSP378p9qy";
/// The stocks it takes as collateral, by ticker: Backpack Securities' Token-2022 tokens, each backed one to one by a
/// US-listed share, tradable on Solana (Orca's pools, through Jupiter) by any wallet. Kamino's reserve stats on
/// 2026-10-28: SPCX and SNDK borrow up to 30% of their value and liquidate at 50%, MU and SKHY 40% and 60%.
pub const BACKPACK_STOCKS: [(&str, &str); 4] = [("SPCX", "SpaceX"), ("MU", "Micron"), ("SKHY", "SK Hynix"), ("SNDK", "SanDisk")];
pub const PROGRAM: &str = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD";
/// A position's account on chain (klend's Obligation): its rent comes back when Kamino closes it, everything taken
/// out (0.017633 SOL back, measured live 2026-09-27, = the rent minimum of 3,344 bytes less the fee).
pub const OBLIGATION_BYTES: usize = 3344;
pub const BASE: &str = "https://api.kamino.finance";
const NONE: &str = "11111111111111111111111111111111";

/// `backpackStocks[ticker]`: the company behind a Backpack stock, as the ticker is written.
pub fn backpack_stock(ticker: &str) -> Option<&'static str> { BACKPACK_STOCKS.iter().find(|s| s.0 == ticker).map(|s| s.1) }

/// The markets a loan against `collateral` can be on, in the order «repay» looks: SOL on the main market, a Backpack
/// stock on the Backpack one, unnamed the Backpack market first.
pub fn markets(against: Option<&str>) -> &'static [&'static str] {
    match against { Some("SOL") => &[MAIN_MARKET], None => &[BACKPACK_MARKET, MAIN_MARKET], Some(_) => &[BACKPACK_MARKET] }
}

/// The market a token goes into: Backpack's stocks the Backpack market, everything else the main one.
pub fn market(symbol: &str) -> &'static str { if backpack_stock(&symbol.to_uppercase()).is_none() { MAIN_MARKET } else { BACKPACK_MARKET } }

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reserve {
    pub address: String,
    pub symbol: String,
    pub mint: String,
    /// Supply APY, % a year.
    pub supply_apy: Decimal,
    pub tvl_usd: Decimal,
    /// The token's dollar price: the reserve's supply in dollars over its supply in tokens (0 when empty).
    pub price_usd: Decimal,
    /// What a loan of the token costs, % a year.
    pub borrow_apy: Decimal,
}

/// A reserve's risk numbers, 0…1: how much of its value may be borrowed against it, where liquidation starts, and the
/// factor a debt in it counts with.
#[derive(Debug, Clone, PartialEq)]
pub struct Risk {
    pub max_ltv: Decimal,
    pub liquidation_ltv: Decimal,
    pub borrow_factor: Decimal,
}

/// `Loan.Collateral`: one coin put up.
#[derive(Debug, Clone, PartialEq)]
pub struct Collateral {
    pub reserve: String,
    pub symbol: String,
    pub mint: String,
    /// In the token, at Kamino's price (0 when it gave none).
    pub tokens: Decimal,
    pub value_usd: Decimal,
}

/// `Loan.Debt`: one coin owed.
#[derive(Debug, Clone, PartialEq)]
pub struct Debt {
    pub reserve: String,
    pub symbol: String,
    /// In the token, interest to the last refresh included.
    pub tokens: Decimal,
}

/// The wallet's position on a market as Kamino refreshes it: what it put up, what it owes, and the dollar limits —
/// what it may borrow in all, and the debt (borrow-factor adjusted) at which liquidation starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Loan {
    pub collateral: Vec<Collateral>,
    pub debts: Vec<Debt>,
    pub deposit_usd: Decimal,
    pub debt_usd: Decimal,
    pub borrow_limit_usd: Decimal,
    pub liquidation_limit_usd: Decimal,
    pub adjusted_debt_usd: Decimal,
}

impl Loan {
    /// Liquidation at 1; nil without debt.
    pub fn health(&self) -> Option<Decimal> {
        (self.adjusted_debt_usd > Decimal::ZERO).then(|| self.liquidation_limit_usd / self.adjusted_debt_usd)
    }

    /// The price of the one coin put up at which the health factor reaches 1 (its price over the health factor, the
    /// debt in dollars); nil without debt or with several coins up.
    pub fn liquidation_price(&self) -> Option<Decimal> {
        let [c] = self.collateral.as_slice() else { return None };
        let h = self.health()?;
        (c.tokens > Decimal::ZERO && h > Decimal::ZERO).then(|| c.value_usd / c.tokens / h)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Position {
    pub reserve: String,
    pub symbol: String,
    /// Kamino's collateral shares (raw), not the underlying tokens.
    pub amount: Decimal,
    pub value_usd: Decimal,
    /// The reserve's supply % a year, where the market lists it.
    pub apy: Option<Decimal>,
}

/// A deposit in a Kamino Vault (Earn): the vault's shares, held or staked in its farm, as the vault's token and dollars.
#[derive(Debug, Clone, PartialEq)]
pub struct VaultPosition {
    pub vault: String,
    /// The vault's own name, «Allez USDC».
    pub name: String,
    pub symbol: String,
    pub mint: String,
    /// The vault's shares, held and staked: what a withdrawal asks for.
    pub shares: Decimal,
    pub tokens: Decimal,
    pub value_usd: Decimal,
    /// % a year.
    pub apy: Decimal,
}

/// `KaminoError`: «Kamino: » and the first 240 bytes of the answer.
pub fn error(data: &[u8]) -> WalletError { WalletError(format!("Kamino: {}", String::from_utf8_lossy(&data[..data.len().min(240)]))) }

/// Where Kamino's API is asked: the real one by default, a stand-in in tests.
#[derive(Debug, Clone)]
pub struct Kamino {
    pub base: String,
}

impl Default for Kamino {
    fn default() -> Self { Self { base: BASE.into() } }
}

impl Kamino {
    /// One reserve per token: the main market has several for USDC, the one holding the money is the real one.
    pub async fn reserves(&self, market: &str) -> Result<Vec<Reserve>> {
        parse_reserves(&self.get(&format!("/kamino-market/{market}/reserves/metrics")).await?)
    }

    /// Staking yield of liquid-staking tokens by mint, % a year (Kamino tracks them for its collateral).
    pub async fn staking_yields(&self) -> Result<HashMap<String, Decimal>> { parse_staking_yields(&self.get("/staking-yields").await?) }

    /// Unsigned transaction depositing `amount` (in tokens, e.g. 1.5 USDC) into `reserve` for `wallet`; on the Backpack
    /// market it is collateral to borrow against. Swift's default market is `MAIN_MARKET`.
    pub async fn deposit_transaction(&self, wallet: &str, reserve: &str, amount: Decimal, market: &str) -> Result<Vec<u8>> {
        self.transaction("/ktx/klend/deposit", wallet, reserve, amount, market).await
    }

    pub async fn withdraw_transaction(&self, wallet: &str, reserve: &str, amount: Decimal, market: &str) -> Result<Vec<u8>> {
        self.transaction("/ktx/klend/withdraw", wallet, reserve, amount, market).await
    }

    /// Unsigned transaction borrowing `amount` of `reserve`'s token to `wallet` against its collateral on `market`, and
    /// paying it back. Kamino builds them only once the wallet's position exists (a first deposit makes it).
    pub async fn borrow_transaction(&self, wallet: &str, reserve: &str, amount: Decimal, market: &str) -> Result<Vec<u8>> {
        self.transaction("/ktx/klend/borrow", wallet, reserve, amount, market).await
    }

    pub async fn repay_transaction(&self, wallet: &str, reserve: &str, amount: Decimal, market: &str) -> Result<Vec<u8>> {
        self.transaction("/ktx/klend/repay", wallet, reserve, amount, market).await
    }

    pub async fn risk(&self, reserve: &str) -> Result<Risk> { parse_risk(&self.get(&format!("/reserves/{reserve}/stats")).await?) }

    /// The wallet's plain position on `market` (Kamino's «vanilla» obligation, the one its transactions use); nil before
    /// the first deposit there. The market's reserves are read only when there is one.
    pub async fn loan(&self, wallet: &str, market: &str) -> Result<Option<Loan>> {
        let rows = rows(&self.fresh(&format!("/kamino-market/{market}/users/{wallet}/obligations")).await?)?;
        let Some((state, stats)) = vanilla(&rows) else { return Ok(None) };
        let reserves = self.reserves(market).await?;
        Ok(Some(read_loan(state, stats, &reserves)))
    }

    /// Unsigned transaction redeeming `shares` of a Kamino Vault for its token; shares staked in the vault's farm are
    /// unstaked in the same transaction (checked by simulation: 1 share of «Allez USDC» → 1.0665 USDC).
    pub async fn vault_withdraw_transaction(&self, wallet: &str, vault: &str, shares: Decimal) -> Result<Vec<u8>> {
        self.post("/ktx/kvault/withdraw", json!({"wallet": wallet, "kvault": vault, "amount": text(shares)})).await
    }

    /// The wallet's deposits on the main market; a market whose reserves don't answer leaves the symbols «?».
    pub async fn positions(&self, wallet: &str) -> Result<Vec<Position>> {
        let rows = rows(&self.fresh(&format!("/kamino-market/{MAIN_MARKET}/users/{wallet}/obligations")).await?)?;
        let reserves = self.reserves(MAIN_MARKET).await.unwrap_or_default();
        Ok(read_positions(&rows, &reserves))
    }

    /// The wallet's Kamino Vault deposits. A deposit made on kamino.com lands here, not in the Lend market's obligations
    /// that `positions` reads (5 USDC into «Allez USDC» showed nowhere). Each vault is read at once; the first error
    /// stops the rest.
    pub async fn vault_positions(&self, wallet: &str) -> Result<Vec<VaultPosition>> {
        let held = parse_held(&self.get(&format!("/kvaults/users/{wallet}/positions")).await?)?;
        let mut group = JoinSet::new();
        for (vault, shares) in held {
            let kamino = self.clone();
            group.spawn(async move {
                let metrics = format!("/kvaults/{vault}/metrics");
                let (named, m) = tokio::join!(kamino.vault(&vault), kamino.get(&metrics));
                let m = json(&m?)?;
                let (name, mint) = named?;
                Ok::<_, WalletError>(vault_position(vault, shares, name, mint, m.as_object()))
            });
        }
        let mut out = Vec::new();
        while let Some(done) = group.join_next().await {
            out.push(done.map_err(|e| WalletError(e.to_string()))??);
        }
        out.sort_by_key(|p| std::cmp::Reverse(p.value_usd));
        Ok(out)
    }

    /// A vault's name and token, read for the vault itself each time: a failed read throws, never a nameless vault with
    /// no mint. The list of every vault (≈ 400 KB) was read once per launch, and an error answer stayed «no vaults» to a restart.
    pub(crate) async fn vault(&self, address: &str) -> Result<(String, String)> { parse_vault(&self.get(&format!("/kvaults/vaults/{address}")).await?) }

    async fn transaction(&self, path: &str, wallet: &str, reserve: &str, amount: Decimal, market: &str) -> Result<Vec<u8>> {
        self.post(path, json!({"wallet": wallet, "market": market, "reserve": reserve, "amount": text(amount)})).await
    }

    /// Asked once, whatever the status: the answer is a transaction or Kamino's words.
    async fn post(&self, path: &str, body: Value) -> Result<Vec<u8>> {
        let request = net::HTTP.post(format!("{}{path}", self.base)).header("Content-Type", "application/json").body(body.to_string());
        parse_transaction(&crate::send(request).await?.0)
    }

    /// A wallet's own state, past the caches: Kamino's API sends it with max-age=60, and Cloudflare served the position
    /// as it was before a deposit or a loan for a minute after it landed. A query of its own makes it a miss there.
    async fn fresh(&self, path: &str) -> Result<Vec<u8>> {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let url = format!("{}{path}?env=mainnet-beta&fresh={ms}", self.base);
        Ok(crate::send(net::HTTP.get(url).timeout(READ_TIMEOUT)).await?.0)
    }

    /// Kamino's errors are JSON with a real status (500 `{"error":…}`, 401, 429): a status past 2xx throws, never read as data.
    async fn get(&self, path: &str) -> Result<Vec<u8>> {
        let (data, status) = crate::send(net::HTTP.get(format!("{}{path}", self.base)).timeout(READ_TIMEOUT)).await?;
        if (200..300).contains(&status) { Ok(data) } else { Err(error(&data)) }
    }
}

pub fn parse_reserves(data: &[u8]) -> Result<Vec<Reserve>> {
    let j = json(data)?;
    let rows = objects(Some(&j)).ok_or_else(|| error(data))?;
    let d = |r: &Object, key: &str| dec(r.get(key)).unwrap_or_default();
    // By mint, the first seen of equal pools kept, in the order first seen (Swift's dictionary has none; the sort decides).
    let mut best: Vec<Reserve> = Vec::new();
    for r in rows {
        let o = Some(r);
        let (Some(address), Some(symbol), Some(mint)) = (string(o, "reserve"), string(o, "liquidityToken"), string(o, "liquidityTokenMint")) else { continue };
        let (supplied, usd) = (d(r, "totalSupply"), d(r, "totalSupplyUsd"));
        let reserve = Reserve {
            address: address.into(), symbol: symbol.into(), mint: mint.into(), supply_apy: d(r, "supplyApy") * Decimal::ONE_HUNDRED, tvl_usd: usd,
            price_usd: if supplied > Decimal::ZERO { usd / supplied } else { Decimal::ZERO }, borrow_apy: d(r, "borrowApy") * Decimal::ONE_HUNDRED,
        };
        match best.iter_mut().find(|b| b.mint == reserve.mint) {
            Some(b) => if reserve.tvl_usd > b.tvl_usd { *b = reserve },
            None => if reserve.tvl_usd > -Decimal::ONE { best.push(reserve) },
        }
    }
    best.sort_by_key(|r| std::cmp::Reverse(r.tvl_usd));
    Ok(best)
}

pub fn parse_staking_yields(data: &[u8]) -> Result<HashMap<String, Decimal>> {
    let j = json(data)?;
    let rows = objects(Some(&j)).ok_or_else(|| error(data))?;
    let mut out = HashMap::new();
    for r in rows {
        let Some(mint) = string(Some(r), "tokenMint") else { continue };
        let apy = dec(r.get("apy")).unwrap_or_default();
        if apy > Decimal::ZERO { out.insert(mint.to_string(), apy * Decimal::ONE_HUNDRED); }
    }
    Ok(out)
}

pub fn parse_risk(data: &[u8]) -> Result<Risk> {
    let j = json(data)?;
    let Some(j) = j.as_object().filter(|j| j.contains_key("liqLTV")) else { return Err(error(data)) };
    let d = |key: &str| dec(j.get(key)).unwrap_or_default();
    Ok(Risk { max_ltv: d("maxLTV"), liquidation_ltv: d("liqLTV"), borrow_factor: d("borrowFactor").max(Decimal::ONE) })
}

/// A ktx answer: its `transaction` in base64, else Kamino's words.
pub fn parse_transaction(data: &[u8]) -> Result<Vec<u8>> {
    let j = json(data)?;
    string(j.as_object(), "transaction").and_then(|b64| STANDARD.decode(b64).ok()).ok_or_else(|| error(data))
}

/// A wallet's obligations on a market.
pub fn rows(data: &[u8]) -> Result<Vec<Object>> {
    let j = json(data)?;
    objects(Some(&j)).map(|o| o.into_iter().cloned().collect()).ok_or_else(|| error(data))
}

/// The plain obligation (tag 0) with its state and refreshed stats, if there is one.
pub fn vanilla(rows: &[Object]) -> Option<(&Object, &Object)> {
    let o = rows.iter().find(|o| crate::ns_int(o.get("obligationTag")) == Some(0))?;
    Some((object(Some(o), "state")?, object(Some(o), "refreshedStats")?))
}

/// 2⁶⁰: klend's fixed point.
fn sf_one() -> Decimal { Decimal::from(1u64 << 60) }

/// `dec(v) / pow(2, 60)`. A whole number in a string is split at the point first: a debt of 100,000 USDC in raw units
/// times 2⁶⁰ is past rust_decimal's 28 digits (Swift's Decimal keeps 38).
fn fixed(v: Option<&Value>) -> Decimal {
    if let Some(n) = v.and_then(Value::as_str).and_then(|s| s.trim_start().parse::<u128>().ok()) {
        let whole = Decimal::from_i128_with_scale((n >> 60) as i128, 0);
        return whole + Decimal::from((n & ((1u128 << 60) - 1)) as u64) / sf_one();
    }
    dec(v).unwrap_or_default() / sf_one()
}

fn deposits(state: Option<&Object>) -> Vec<&Object> {
    objects(state.and_then(|s| s.get("deposits"))).unwrap_or_default().into_iter()
        .filter(|d| string(Some(d), "depositReserve").is_some_and(|r| r != NONE)).collect()
}

/// Each deposit's stored value is as fresh as the last refresh; the refreshed total, split in those proportions.
// ponytail: all stored values 0 with several deposits splits evenly; exact would read each reserve's exchange rate.
fn split(total: Decimal, stored: &[Decimal], i: usize) -> Decimal {
    let sum: Decimal = stored.iter().sum();
    if sum > Decimal::ZERO { total * stored[i] / sum } else { total / Decimal::from(stored.len()) }
}

/// The loan from its obligation's state and stats and the market's reserves.
pub fn read_loan(state: &Object, stats: &Object, reserves: &[Reserve]) -> Loan {
    let by_address = |a: &str| reserves.iter().find(|r| r.address == a);
    let d = |o: &Object, key: &str| dec(o.get(key)).unwrap_or_default();
    let deposits = deposits(Some(state));
    let stored: Vec<Decimal> = deposits.iter().map(|d| fixed(d.get("marketValueSf"))).collect();
    let total = d(stats, "userTotalDeposit");
    let collateral = deposits.iter().enumerate().map(|(i, dep)| {
        let reserve = string(Some(dep), "depositReserve").unwrap_or("");
        let r = by_address(reserve);
        let value = split(total, &stored, i);
        // Backpack's stocks are never lent out (Kamino lets nobody borrow them), so a share of one stays one token
        // (6 decimals); anything else is its dollars at the reserve's price.
        let stock = r.is_some_and(|r| backpack_stock(&r.symbol).is_some());
        let price = r.map_or(Decimal::ZERO, |r| r.price_usd);
        let tokens = if stock { d(dep, "depositedAmount") / Decimal::from(1_000_000) } else if price > Decimal::ZERO { value / price } else { Decimal::ZERO };
        Collateral { reserve: reserve.into(), symbol: r.map_or("?", |r| &r.symbol).into(), mint: r.map_or("", |r| &r.mint).into(), tokens, value_usd: value }
    }).collect();
    let debts = objects(state.get("borrows")).unwrap_or_default().into_iter().filter_map(|b| {
        let reserve = string(Some(b), "borrowReserve").filter(|r| *r != NONE)?;
        let r = by_address(reserve);
        // The borrowed amount in the token's smallest units, fixed point 2⁶⁰.
        // ponytail: 6 decimals but SOL's 9 — USDC is what ihatecrypto borrows; read the mint for any other debt.
        let decimals = if r.is_some_and(|r| r.symbol == "SOL") { 9 } else { 6 };
        Some(Debt { reserve: reserve.into(), symbol: r.map_or("?", |r| &r.symbol).into(), tokens: fixed(b.get("borrowedAmountSf")) / Decimal::from(10u64.pow(decimals)) })
    }).collect();
    Loan {
        collateral, debts, deposit_usd: total, debt_usd: d(stats, "userTotalBorrow"), borrow_limit_usd: d(stats, "borrowLimit"),
        liquidation_limit_usd: d(stats, "borrowLiquidationLimit"), adjusted_debt_usd: d(stats, "userTotalBorrowBorrowFactorAdjusted"),
    }
}

/// Every deposit of every obligation. The stored per-deposit value (fixed point, 2⁶⁰) is only as fresh as the
/// obligation's last refresh — 0 right after a first deposit. The API's refreshed total is current, so it is split in
/// the stored proportions. Here only strings are read, as Swift's `positions` reads them.
pub fn read_positions(rows: &[Object], reserves: &[Reserve]) -> Vec<Position> {
    let mut out = Vec::new();
    for o in rows {
        let deposits = deposits(object(Some(o), "state"));
        let stored: Vec<Decimal> = deposits.iter().map(|d| if d.get("marketValueSf").is_some_and(Value::is_string) { fixed(d.get("marketValueSf")) } else { Decimal::ZERO }).collect();
        let stored_sum: Decimal = stored.iter().sum();
        let total = string(object(Some(o), "refreshedStats"), "userTotalDeposit").and_then(decimal).unwrap_or(stored_sum);
        for (i, d) in deposits.iter().enumerate() {
            let reserve = string(Some(d), "depositReserve").unwrap_or("");
            let r = reserves.iter().find(|r| r.address == reserve);
            out.push(Position {
                reserve: reserve.into(), symbol: r.map_or("?", |r| &r.symbol).into(), amount: str_dec(d.get("depositedAmount")).unwrap_or_default(),
                value_usd: split(total, &stored, i), apy: r.map(|r| r.supply_apy),
            });
        }
    }
    out
}

/// The vaults the wallet holds shares of, and how many.
pub fn parse_held(data: &[u8]) -> Result<Vec<(String, Decimal)>> {
    let j = json(data)?;
    let rows = objects(Some(&j)).ok_or_else(|| error(data))?;
    Ok(rows.into_iter().filter_map(|r| {
        let shares = dec(r.get("totalShares")).unwrap_or_default();
        let vault = string(Some(r), "vaultAddress")?;
        (shares > Decimal::ZERO).then(|| (vault.to_string(), shares))
    }).collect())
}

/// A vault's name («Kamino Vault» for the eight without one) and token; no token is an error.
pub fn parse_vault(data: &[u8]) -> Result<(String, String)> {
    let j = json(data)?;
    let state = object(j.as_object(), "state");
    let mint = string(state, "tokenMint").filter(|m| !m.is_empty()).ok_or_else(|| error(data))?;
    Ok((string(state, "name").filter(|n| !n.is_empty()).unwrap_or("Kamino Vault").into(), mint.into()))
}

const SYMBOLS: [(&str, &str); 3] = [("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "USDC"), ("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", "USDT"),
                                     ("So11111111111111111111111111111111111111112", "SOL")];

/// One vault's deposit from its shares, its name and token, and its metrics (a non-object reads as none).
pub fn vault_position(vault: String, shares: Decimal, name: String, mint: String, metrics: Option<&Object>) -> VaultPosition {
    let d = |key: &str| dec(metrics.and_then(|m| m.get(key))).unwrap_or_default();
    let tokens = shares * d("tokensPerShare");
    let symbol = SYMBOLS.iter().find(|s| s.0 == mint).map_or_else(|| format!("{}…", mint.chars().take(4).collect::<String>()), |s| s.1.into());
    VaultPosition { vault, name, symbol, mint, shares, tokens, value_usd: tokens * d("tokenPrice"), apy: d("apy") * Decimal::ONE_HUNDRED }
}
