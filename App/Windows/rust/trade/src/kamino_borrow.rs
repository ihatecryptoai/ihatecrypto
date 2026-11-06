//! Borrowing USDC on Kamino against what the wallet put up there — Backpack's tokenized stocks on the Backpack market, or
//! SOL (and its other deposits) on the main market — and paying it back (KaminoBorrow.swift). A borrow keeps the health
//! factor at `Executor::MINIMUM_HEALTH` or more, the same line as on Aave, and never past Kamino's own borrow limit; the
//! card says at which price of the collateral the position would be liquidated.

use std::collections::HashMap;

use chrono::Utc;
use defi::kamino::{self, Kamino, Loan, Reserve, Risk};
use execution::Kind;
use ihatecrypto_core::foundation::{Num, Rounding, rounded, text};
use ihatecrypto_core::intent::Fmt;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use solana::spl;

use crate::wallet::{WSOL, ata, d, i, millionths, owners, pct, sleep, sol_of, units_of, usd};
use crate::{Executor, Result, fail, num};

/// What a borrow on Kamino would do, read now; nothing signed.
#[derive(Debug, Clone, PartialEq)]
pub struct KaminoLoanPreview {
    pub market: String,
    /// What the loan stands on, when one coin does («SPCX», «SOL»), and how much of it is (or will be) collateral.
    pub stock: Option<String>,
    pub stock_tokens: Decimal,
    /// Dollars, after the deposit the borrow brings in first.
    pub collateral_usd: Decimal,
    /// What Kamino lets the wallet borrow more, after that deposit and before this borrow.
    pub available_usd: Decimal,
    /// Owed after the borrow.
    pub debt_usd: Decimal,
    pub health_now: Option<Decimal>,
    pub health_after: Option<Decimal>,
    /// The stock's price at which the health factor reaches 1, the rest of the collateral held steady; None without one.
    pub liquidation_price: Option<Decimal>,
    /// The USDC loan's rate, % a year.
    pub borrow_apy: Decimal,
    /// The first deposit on a market opens the wallet's accounts there: SOL it costs beyond what goes in (rent and fee,
    /// from a simulation of the deposit Kamino builds now), and the position's rent that comes back when everything is
    /// taken out. None with a position there already, or when it couldn't be read.
    pub first_rent_sol: Option<Decimal>,
    pub rent_back_sol: Option<Decimal>,
    /// Tokens of the collateral named that carry this loan alone at a health factor of 2.
    pub for_health_two: Option<Decimal>,
}

/// The position after a deposit and a borrow (Executor::kamino_after): collateral, what may be borrowed more, the debt,
/// the health factor and the collateral's liquidation price.
pub type KaminoAfter = (Decimal, Decimal, Decimal, Option<Decimal>, Option<Decimal>);

impl Executor {
    /// Tokens of a collateral that may leave a Kamino position with debt: what's left must still carry the debt at the
    /// borrow limit (Kamino's own rule for a withdrawal) and at a health factor of `MINIMUM_HEALTH` (ihatecrypto's, as for
    /// a borrow), with 1% to spare for the price moving meanwhile. 0 when Kamino's numbers are past range.
    pub fn kamino_free_collateral(loan: &Loan, max_ltv: Decimal, liquidation_ltv: Decimal, price_usd: Decimal) -> Decimal {
        if max_ltv <= Decimal::ZERO || liquidation_ltv <= Decimal::ZERO || price_usd <= Decimal::ZERO { return Decimal::ZERO }
        let by_limit = (Num::of(loan.borrow_limit_usd) - loan.adjusted_debt_usd) / max_ltv;
        let by_health = (Num::of(loan.liquidation_limit_usd) - Num::of(Self::MINIMUM_HEALTH) * loan.adjusted_debt_usd) / liquidation_ltv;
        let (Some(by_limit), Some(by_health)) = (by_limit.0, by_health.0) else { return Decimal::ZERO };
        (Num::of(by_limit.min(by_health)) / price_usd * d("0.99")).0.map_or(Decimal::ZERO, |f| f.max(Decimal::ZERO))
    }

    /// The position after putting up `adding` of a stock at `price` (its reserve's `risk`) and borrowing `usdc` (a debt
    /// counted `borrow_factor` times): its limits, health, and the stock's liquidation price. `held`: that stock's tokens
    /// already in the position. (Swift's doesn't throw; a number past range here stops the borrow.)
    pub fn kamino_after(loan: Option<&Loan>, adding: Decimal, held: Decimal, price: Decimal, risk: Option<&Risk>, usdc: Decimal, borrow_factor: Decimal)
        -> Result<KaminoAfter> {
        let zero = Decimal::ZERO;
        let added = Num::of(adding) * price;
        let collateral = num(Num::of(loan.map_or(zero, |l| l.deposit_usd)) + added)?;
        let limit = num(Num::of(loan.map_or(zero, |l| l.borrow_limit_usd)) + added * risk.map_or(zero, |r| r.max_ltv))?;
        let liquidation_limit = num(Num::of(loan.map_or(zero, |l| l.liquidation_limit_usd)) + added * risk.map_or(zero, |r| r.liquidation_ltv))?;
        let adjusted = loan.map_or(zero, |l| l.adjusted_debt_usd);
        let debt = num(Num::of(adjusted) + Num::of(usdc) * borrow_factor)?;
        let health = if debt > zero { Some(num(Num::of(liquidation_limit) / debt)?) } else { None };
        // The stock's share of the liquidation limit moves with its price; the rest stays.
        let tokens = num(Num::of(held) + adding)?;
        let ltv = risk.map_or(zero, |r| r.liquidation_ltv);
        let mut liquidation = None;
        if tokens > zero && ltv > zero && debt > zero {
            let rest = Num::of(liquidation_limit) - Num::of(tokens) * price * ltv;
            let p = num((Num::of(debt) - rest) / (Num::of(tokens) * ltv))?;
            liquidation = (p > zero).then_some(p);
        }
        Ok((collateral, num(Num::of(limit) - adjusted)?.max(zero), num(Num::of(loan.map_or(zero, |l| l.debt_usd)) + usdc)?, health, liquidation))
    }

    /// The mint's token program, SPL Token or Token-2022: its owner on chain.
    pub(crate) async fn token_program(&self, mint: &str) -> Result<String> {
        let info = self.rpc.call("getAccountInfo", json!([mint, {"encoding": "base64", "commitment": "finalized"}])).await?;
        match info.pointer("/value/owner").and_then(Value::as_str) {
            Some(program) if [spl::TOKEN_PROGRAM, spl::TOKEN_2022_PROGRAM].contains(&program) => Ok(program.into()),
            _ => fail(format!("{mint} isn't a token mint on Solana.")),
        }
    }

    /// What borrowing `usdc` would do, read now; `adding` of `stock` goes in as collateral first. Nothing is signed.
    /// A Backpack stock borrows on the Backpack market, SOL on the main market; none named, on the Backpack market when
    /// something is up there, else on the main one when SOL is.
    pub async fn kamino_borrow_preview(&self, usdc: Decimal, stock: Option<&str>, adding: Option<Decimal>) -> Result<KaminoLoanPreview> {
        let sol = self.wallet.solana.as_str();
        let k = Kamino::default();
        let (market, loan): (&str, Option<Loan>) = if let Some(stock) = stock {
            let market = kamino::market(stock);
            (market, k.loan(sol, market).await?)
        } else {
            let (main, backpack) = tokio::join!(k.loan(sol, kamino::MAIN_MARKET), k.loan(sol, kamino::BACKPACK_MARKET));
            let backpack = backpack?;
            // The main market's USDC savings alone aren't something to borrow USDC against.
            if backpack.as_ref().map_or(Decimal::ZERO, |b| b.deposit_usd) <= Decimal::ZERO
                && let Some(m) = main?
                && m.collateral.iter().any(|c| c.symbol == "SOL") {
                (kamino::MAIN_MARKET, Some(m))
            } else {
                (kamino::BACKPACK_MARKET, backpack)
            }
        };
        let reserves = k.reserves(market).await?;
        let Some(usdc_reserve) = reserves.iter().find(|r| r.symbol == "USDC") else { return fail("Kamino lends no USDC on that market.") };
        // The stock the loan stands on: the one named, else the biggest collateral there (the first of equals).
        let symbol = stock.map(str::to_string)
            .or_else(|| loan.as_ref().and_then(|l| l.collateral.iter().reduce(|a, b| if a.value_usd < b.value_usd { b } else { a })).map(|c| c.symbol.clone()));
        let reserve = symbol.as_ref().and_then(|s| reserves.iter().find(|r| r.symbol.to_uppercase() == s.to_uppercase()));
        if let (Some(symbol), None) = (&symbol, reserve) { return fail(format!("Kamino takes no {symbol} as collateral.")) }
        let usdc_risk = k.risk(&usdc_reserve.address);
        let risk = match reserve { Some(r) => Some(k.risk(&r.address).await?), None => None };
        let held = loan.as_ref().and_then(|l| l.collateral.iter().find(|c| Some(&c.reserve) == reserve.map(|r| &r.address))).map_or(Decimal::ZERO, |c| c.tokens);
        let after = Self::kamino_after(loan.as_ref(), adding.unwrap_or_default(), held, reserve.map_or(Decimal::ZERO, |r| r.price_usd), risk.as_ref(), usdc,
                                       usdc_risk.await?.borrow_factor)?;
        let mut p = KaminoLoanPreview {
            market: market.into(), stock: reserve.map(|r| r.symbol.clone()), stock_tokens: num(Num::of(held) + adding.unwrap_or_default())?,
            collateral_usd: after.0, available_usd: after.1, debt_usd: after.2, health_now: loan.as_ref().and_then(Loan::health), health_after: after.3,
            liquidation_price: after.4, borrow_apy: usdc_reserve.borrow_apy, first_rent_sol: None, rent_back_sol: None, for_health_two: None,
        };
        if let (Some(reserve), Some(ltv)) = (reserve, risk.as_ref().map(|r| r.liquidation_ltv)) && ltv > Decimal::ZERO && reserve.price_usd > Decimal::ZERO {
            p.for_health_two = (Num::of(usdc) * 2 / ltv / reserve.price_usd).rounded(4, Rounding::Up).0;
        }
        // The main market's card names them (the Backpack market's first deposit and borrow have their own measured words).
        // The coin's first deposit is the executor's «first time» too, a position there or not. Too little SOL for the
        // whole deposit and its rent: a small one's simulation still says the rent.
        if held.is_zero() && market == kamino::MAIN_MARKET && let (Some(reserve), Some(adding)) = (reserve, adding) && adding > Decimal::ZERO {
            p.first_rent_sol = self.kamino_deposit_cost(reserve, adding, market).await;
            if p.first_rent_sol.is_none() { p.first_rent_sol = self.kamino_deposit_cost(reserve, d("0.001"), market).await }
            if loan.is_none() { p.rent_back_sol = self.rpc.rent_minimum(kamino::OBLIGATION_BYTES).await.ok().map(|l| sol_of(l as i128)) }
        }
        Ok(p)
    }

    /// SOL a deposit of `amount` into `reserve` costs beyond itself — the fee and, the first time on a market, the rent of
    /// the accounts it opens — from a simulation of the deposit Kamino builds now. Nothing is signed; None when it can't
    /// be read (too little SOL to run it).
    pub(crate) async fn kamino_deposit_cost(&self, reserve: &Reserve, amount: Decimal, market: &str) -> Option<Decimal> {
        let sol = self.wallet.solana.as_str();
        let wire = Kamino::default().deposit_transaction(sol, &reserve.address, amount, market).await.ok()?;
        let before = self.rpc.balance(sol).await.ok()?;
        let run = self.rpc.simulate(&wire, &[sol.to_string()], false).await.ok().filter(|r| r.error.is_none())?;
        let after = run.after.get(sol)?.0;
        let spent = sol_of(before as i128 - after as i128);
        if reserve.mint == WSOL { (Num::of(spent) - amount).0 } else { Some(spent) }
    }

    /// The position once Kamino's API shows the move just made (`done`): it lags behind the chain — a first deposit read
    /// as no position at all, and a repaid debt as still owed for over 10 s (live, 2026-10-29). 30 seconds at most.
    pub(crate) async fn kamino_loan(&self, market: &str, done: impl Fn(Option<&Loan>) -> bool) -> Result<Option<Loan>> {
        let k = Kamino::default();
        let mut now = k.loan(&self.wallet.solana, market).await?;
        for _ in 0..30 {
            if done(now.as_ref()) { break }
            sleep(1).await;
            now = k.loan(&self.wallet.solana, market).await?;
        }
        Ok(now)
    }

    pub fn usdc_owed(loan: Option<&Loan>) -> Decimal { loan.and_then(|l| l.debts.iter().find(|d| d.symbol == "USDC")).map_or(Decimal::ZERO, |d| d.tokens) }

    pub fn kamino_market_name(market: &str) -> &'static str { if market == kamino::BACKPACK_MARKET { "Kamino's Backpack market" } else { "Kamino" } }
    /// «Backpack» or «main»: the journal's note of a borrow names its market with it.
    pub fn kamino_market_word(market: &str) -> &'static str { if market == kamino::BACKPACK_MARKET { "Backpack" } else { "main" } }

    /// Signed a borrow on `market` in the last minute, by this app's own journal. Kamino's API, which the 1.5 line on a
    /// withdrawal reads, shows a borrow only 10–30 s after it lands: a withdrawal waits that out.
    /// ponytail: the API's word, not the chain's; read the obligation account on chain if a minute's wait isn't enough.
    pub(crate) fn kamino_borrowed_just_now(&self, market: &str) -> bool {
        let on = format!("on the {} market", Self::kamino_market_word(market));
        self.journal.entries().ok()
            .and_then(|e| e.into_iter().rev().find(|e| e.kind == Kind::Signed && e.note.starts_with("Kamino borrow") && e.note.contains(&on)))
            .is_some_and(|e| (Utc::now() - e.at).num_milliseconds() < 60_000)
    }

    /// Borrows `usdc` on Kamino to this wallet; `adding` of `stock` goes in as collateral first (its own transaction, the
    /// first one on the market pays the position's rent). Refused when the health factor would fall under
    /// `MINIMUM_HEALTH` or past Kamino's borrow limit.
    pub async fn kamino_borrow(&self, usdc: Decimal, stock: Option<&str>, adding: Option<Decimal>) -> Result<String> {
        let sol = self.wallet.solana.as_str();
        let units = units_of(usdc, 6);
        if units == 0 { return fail("Say how many USDC to borrow.") }
        let p = self.kamino_borrow_preview(usdc, stock, adding).await?;
        let place = Self::kamino_market_name(&p.market);
        if p.collateral_usd <= Decimal::ZERO {
            let put = p.stock.as_ref().map_or_else(|| "0.05 SPCX".to_string(), |s| format!("{} {s}", Fmt::num(p.for_health_two.unwrap_or_default(), 4)));
            return fail(format!("Nothing on Kamino to borrow against: put {} up first — «borrow {} USDC on kamino against {put}» puts {put} in as collateral and \
                                 borrows against it{}. Nothing was signed.", p.stock.as_deref().unwrap_or("SOL or a stock"), Fmt::num(usdc, 2),
                                if p.stock.is_none() { "" } else { " at a health factor of 2" }));
        }
        if usdc > p.available_usd {
            return fail(format!("{place} lets this wallet borrow {} more against {} of collateral, and {} USDC is past it. Borrow less, or put more up. \
                                 Nothing was signed.", usd(p.available_usd), usd(p.collateral_usd), Fmt::num(usdc, 2)));
        }
        let Some(health) = p.health_after.filter(|h| *h >= Self::MINIMUM_HEALTH) else {
            return fail(format!("That would leave a health factor of {}, while ihatecrypto borrows only down to {} (Kamino liquidates at 1.0). Borrow less, \
                                 or put more up. Nothing was signed.", Fmt::num(p.health_after.unwrap_or_default(), 2), text(Self::MINIMUM_HEALTH)));
        };
        let k = Kamino::default();
        let Some(usdc_reserve) = k.reserves(&p.market).await?.into_iter().find(|r| r.symbol == "USDC") else {
            return fail("Kamino lends no USDC on that market just now. Nothing was signed.");
        };
        let mut deposited = false;
        // The card showed what a first deposit's rent is: no question about it here.
        if let (Some(stock), Some(adding)) = (stock, adding) && adding > Decimal::ZERO {
            self.kamino(stock, Some(adding), true, false, true).await?;
            deposited = true;
        }
        let borrowed: Result<()> = async {
            // Kamino builds a borrow only once it sees the position a deposit just opened: a few seconds after it lands.
            let mut wire = None;
            for attempt in 0..10 {
                match k.borrow_transaction(sol, &usdc_reserve.address, usdc, &p.market).await {
                    Ok(w) => {
                        wire = Some(w);
                        break;
                    }
                    Err(e) if attempt < 9 && e.0.contains("NOT_FOUND") => sleep(2).await,
                    Err(e) => return Err(e.into()),
                }
            }
            let Some(wire) = wire else { return fail("Kamino didn't build the borrow.") };
            let my_usdc = ata(sol, &usdc_reserve.mint)?;
            let last_valid = self.rpc.latest_blockhash().await?.1;
            self.report(&format!("{place}: {} of collateral, health factor {} → {}{}.", usd(p.collateral_usd),
                                 p.health_now.map_or_else(|| "∞".to_string(), |h| Fmt::num(h, 2)), Fmt::num(health, 2),
                                 p.liquidation_price.map_or(String::new(), |l| format!(", liquidation if {} ≤ {}", p.stock.as_deref().unwrap_or("the collateral"), usd(l)))));
            // The first borrow also opens the position's account in Kamino's farm for the debt: ≈ 0.0053 SOL of rent on top
            // of the fee, so the floor is 0.012 SOL, not the usual 0.005. What arrives is what the card said, give or take 1%.
            self.sign_and_send(&wire, last_valid, &owners(&[(&my_usdc, sol)]), &[(my_usdc.clone(), "USDC".into(), true, pct(i(units), 99))],
                               &format!("Kamino borrow {} USDC on the {} market", Fmt::num(usdc, 2), Self::kamino_market_word(&p.market)), -12_000_000,
                               &HashMap::from([(my_usdc.clone(), pct(i(units), 101))]), false, None).await?;
            Ok(())
        }.await;
        if let Err(e) = borrowed {
            // The stock is up already: the same words again would put it up twice.
            if deposited {
                let named = stock.unwrap_or("");
                return fail(format!("The {} {named} is up as collateral on {place} now, but the borrow didn't go through: {e} «borrow {} USDC on kamino against \
                                     {named}» borrows against it without putting more up.", Fmt::num(adding.unwrap_or_default(), 6), Fmt::num(usdc, 2)));
            }
            return Err(e);
        }
        let owed = Self::usdc_owed(k.loan(sol, &p.market).await?.as_ref());
        let was = num(Num::of(p.debt_usd) - usdc)?;
        let half = usdc / Decimal::TWO;
        let now = self.kamino_loan(&p.market, |l| Self::usdc_owed(l) > owed || (Num::of(was) + half).0.is_some_and(|w| l.map_or(Decimal::ZERO, |l| l.debt_usd) > w)).await?;
        let line = format!("Borrowed {} USDC on {place} at {}% a year: health factor {}{}, owed {} USDC. {}Say «repay all USDC on kamino{}» to pay it back.",
                           Fmt::num(usdc, 2), Fmt::num(rounded(p.borrow_apy, 2, Rounding::Plain), 2),
                           Fmt::num(now.as_ref().and_then(Loan::health).unwrap_or(health), 2),
                           p.liquidation_price.map_or(String::new(), |l| format!(", liquidation if {} ≤ {}", p.stock.as_deref().unwrap_or("the collateral"), usd(l))),
                           Fmt::num(Self::usdc_owed(now.as_ref()).max(usdc), 6),
                           if p.market == kamino::BACKPACK_MARKET { "Stock prices stand still while US markets are closed. A gap at the open can reach that line at once. " }
                           else { "" },
                           if p.market == kamino::MAIN_MARKET { " against SOL" } else { "" });
        self.report(&line);
        Ok(line)
    }

    /// Pays back `usdc` of the USDC debt on Kamino (None = all of it, interest included) from the USDC on Solana, on the
    /// market that holds the debt. Kamino takes no more than is owed. Swift's `against` defaults to None.
    pub async fn kamino_repay(&self, usdc: Option<Decimal>, against: Option<&str>) -> Result<String> {
        let sol = self.wallet.solana.as_str();
        let k = Kamino::default();
        // One market a time, the Backpack market first; a loan on the other one too is named in the reply.
        let mut found: Option<(&str, Loan, Decimal, Reserve)> = None;
        let mut other: Option<(&str, Decimal)> = None;
        for &m in kamino::markets(against) {
            let Some(loan) = k.loan(sol, m).await? else { continue };
            let Some(debt) = loan.debts.iter().find(|d| d.symbol == "USDC").filter(|d| d.tokens > Decimal::ZERO) else { continue };
            let debt = debt.tokens;
            let Some(r) = k.reserves(m).await?.into_iter().find(|r| r.symbol == "USDC") else { continue };
            // The stored amount stands still between the position's refreshes on chain; Kamino's refreshed dollars carry the
            // interest since (USDC at its price there).
            let refreshed = if loan.debts.len() == 1 && r.price_usd > Decimal::ZERO { num(Num::of(loan.debt_usd) / r.price_usd)? } else { Decimal::ZERO };
            if found.is_none() { found = Some((m, loan, debt.max(refreshed), r)) } else { other = Some((m, debt.max(refreshed))) }
        }
        let Some((market, loan, owed, usdc_reserve)) = found else {
            return fail(if against == Some("SOL") { "No USDC debt against SOL on Kamino's main market." } else { "No USDC debt on Kamino." });
        };
        let place = Self::kamino_market_name(market);
        // All of it: the interest of the next minutes too, a tenth of a percent over; Kamino takes only what's owed.
        let mut asked = match usdc { Some(u) => u.min(owed), None => num((Num::of(owed) * d("1.001")).rounded(6, Rounding::Up))? };
        let held = millionths(self.rpc.token_balance(sol, &usdc_reserve.mint).await?);
        if usdc.is_none() && held < asked && held >= owed { asked = held } // exactly the debt is enough
        let units = crate::wallet::times_pow10(asked, 6).map_or(0, |x| crate::executor::uint64(rounded(x, 0, Rounding::Up)));
        if held < asked {
            return fail(format!("Paying back {} USDC needs it on Solana. The wallet has {} USDC there. Nothing was signed.", Fmt::num(asked, 6), Fmt::num(held, 2)));
        }
        let wire = k.repay_transaction(sol, &usdc_reserve.address, asked, market).await?;
        let my_usdc = ata(sol, &usdc_reserve.mint)?;
        let last_valid = self.rpc.latest_blockhash().await?.1;
        self.sign_and_send(&wire, last_valid, &owners(&[(&my_usdc, sol)]), &[(my_usdc.clone(), "USDC".into(), true, -i(units))],
                           &format!("Kamino repay {} USDC", Fmt::num(asked, 6)), -5_000_000, &HashMap::new(), false, None).await?;
        let stored = loan.debts.iter().find(|d| d.symbol == "USDC").map_or(owed, |d| d.tokens);
        let now = self.kamino_loan(market, |l| Self::usdc_owed(l) < stored).await?;
        // What left the wallet, not what was asked: Kamino takes only what's owed. The chain says it before the API.
        let paid = num(Num::of(held) - millionths(self.rpc.token_balance(sol, &usdc_reserve.mint).await?))?;
        let caught_up = Self::usdc_owed(now.as_ref()) < stored;
        // «All» asked a tenth of a percent over what's owed, and Kamino takes no more: nothing is left then.
        let left = if caught_up {
            Self::usdc_owed(now.as_ref())
        } else if usdc.is_none() && paid > Decimal::ZERO {
            Decimal::ZERO
        } else {
            num(Num::of(stored) - paid)?.max(Decimal::ZERO)
        };
        let stocks: Vec<String> = now.as_ref().map(|n| n.collateral.iter().filter(|c| c.tokens > Decimal::ZERO).map(|c| c.symbol.clone()).collect()).unwrap_or_default();
        let shown_paid = if paid > Decimal::ZERO { paid } else { num(Num::of(owed) - left)?.max(Decimal::ZERO) };
        let rest = if left > d("0.00001") {
            format!("{} USDC still owed{}", Fmt::num(left, 6),
                    if caught_up { format!(", health factor {}.", now.as_ref().and_then(Loan::health).map_or_else(|| "∞".to_string(), |h| Fmt::num(h, 2))) }
                    else { ".".into() })
        } else {
            format!("no debt left.{}", if stocks.is_empty() { String::new() } else {
                format!(" The {} stays as collateral: «withdraw {} from kamino» takes it back.", stocks.join(", "), stocks[0])
            })
        };
        let elsewhere = other.map_or(String::new(), |(m, o)| format!(" {} USDC is still owed on Kamino's {} pays it.", Fmt::num(o, 6),
            if m == kamino::BACKPACK_MARKET { "Backpack market too: «repay all USDC on kamino» again" } else { "main market too: «repay all USDC on kamino against SOL»" }));
        let line = format!("Paid back {} USDC on {place}: {rest}{elsewhere}", Fmt::num(shown_paid, 6));
        self.report(&line);
        Ok(line)
    }
}
