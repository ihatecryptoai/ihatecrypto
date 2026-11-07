//! Paying for a service (Payments.swift): «pay GET https://…»: a service that answers 402 is paid after Confirm, then
//! asked again with the proof — the Mac's Payments, over the shared MPP and x402. Its MPP challenge in USDC on Tempo
//! first (`Executor::pay_tempo`), else MPP's NEAR Intents charge when this wallet pays its token (USDC on Solana, Base,
//! Arbitrum or Ethereum: `Executor::pay_intents_charge`, checked by the guard's deposit_charge), else x402 in USDC on
//! Base (an EIP-3009 authorization the signer builds itself, SignX402). Services quoted with the request's content are
//! `State::priced_with_content`.

use std::collections::HashMap;

use execution::{Entry, Kind};
use ihatecrypto_core::foundation::{Date, Rounding, Uuid, rounded};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::mpp::{self, Answer, NearCharge, Refusal};
use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use ihatecrypto_core::scheduled_task::ScheduledTask;
use ihatecrypto_core::x402::{self, Offer};
use protocols::one_click::{self, Token};
use rust_decimal::Decimal;
use signer_api::SignerRequest;
use trade::intent_swap::INTENT_CHAINS;
use trade::{EvmNetwork, Executor};

use crate::automation::{RUN_TASK, task_run};
use crate::blocks::{BlockData, ServiceCard};
use crate::engine::{Engine, Pending};
use crate::from_core::MppChallenge;
use crate::link_signer::reason;
use crate::nodes::host;

/// `throw ExecutionError(…)`.
macro_rules! said {
    ($words:expr) => { Err(trade::Error::Execution($words)) };
}

/// A service's price waiting for Confirm.
#[derive(Debug, Clone, PartialEq)]
pub struct PaymentQuote {
    pub command: WalletCommand,
    /// Tempo's or NEAR Intents' MPP challenge. For an x402 offer: its price and payee, method «x402» (the offer in `x402`).
    pub challenge: MppChallenge,
    /// An x402 offer: USDC on Base by signature.
    pub x402: Option<Offer>,
    pub ask: String,
    pub service: String,
    /// Quoted without the request's content: Confirm sends it, asking the price again with it before paying.
    pub probed: bool,
    /// Asked in a run of this task on the automation wallet: paid, the price is what its later runs may pay by themselves.
    pub task: Option<Uuid>,
    /// A NEAR Intents charge: what the service receives as 1Click lists it («1 USDC») and that asset's 1Click id for the
    /// guard. None when 1Click doesn't list it.
    pub receives: Option<(String, String)>,
}

impl PaymentQuote {
    /// The note the paid move gets: which way it paid.
    pub fn way(&self) -> &'static str {
        if self.x402.is_some() { "x402 · Base" } else if self.challenge.near.is_some() { "MPP · NEAR Intents" } else { "MPP · Tempo" }
    }
}

/// What a service asks, read as its card reads it: its price, or its answer when it asks none.
#[derive(Debug, Clone, PartialEq)]
pub enum PriceAsked {
    Quoted(Box<PaymentQuote>),
    Answered(String),
}

impl Engine {
    /// The USDC on Tempo the balances show.
    pub fn tempo_usdc(&self) -> Decimal {
        let st = self.st();
        st.portfolio.networks.iter().find(|n| n.key == "tempo").and_then(|n| n.rows.iter().find(|r| r.symbol == "USDC")).map_or(Decimal::ZERO, |r| r.value)
    }

    /// What a service asks, read as its card reads it: its price, or its answer when it asks none. Errs with the reason
    /// it can't be paid (said as is).
    pub async fn ask_price(&self, command: &WalletCommand) -> trade::Result<PriceAsked> {
        let not_web = self.say("That isn't a web address ihatecrypto can call.", "Это не веб-адрес, который ihatecrypto может вызвать.");
        let (WalletCommand::Pay { method, url, body }, Some(host)) = (command, match command { WalletCommand::Pay { url, .. } => host(url), _ => None }) else {
            return said!(not_web);
        };
        if mpp::REFUSES_PROOF.contains(&host.to_lowercase().as_str()) {
            return said!(self.say(&format!("{host} takes the payment and then refuses its proof (checked 2026-10-23), so ihatecrypto doesn't pay it. Ask for another service."),
                                 &format!("{host} берёт оплату, а потом не принимает её подтверждение (проверено 2026-10-23), поэтому ihatecrypto его не оплачивает. Попроси другой сервис.")));
        }
        let ask = self.st().messages().iter().rev().find(|m| m.from_user).map(|m| m.text.clone()).unwrap_or_default();
        // Asking a price is a request: only to a service Tempo's directory lists, or a site the user named themselves — so
        // text a lookup slipped into the chat can't have the model send the user's numbers to an address of its choosing.
        let listed = mpp::service_name(url).await;
        if listed.is_none() && !ask.to_lowercase().contains(&host.to_lowercase()) {
            return said!(self.say(&format!("{host} isn't in Tempo's directory of paid services and isn't in your message, so ihatecrypto doesn't call it. Write its address yourself to use it."),
                                 &format!("{host} нет в каталоге платных сервисов Tempo и нет в твоём сообщении, поэтому ihatecrypto его не вызывает. Напиши его адрес сам, чтобы воспользоваться.")));
        }
        let service = listed.unwrap_or_else(|| host.clone());
        let unreached = |e: wallet::WalletError| trade::Error::Execution(self.say(&format!("Couldn't reach {host}: "), &format!("Не достучался до {host}: ")) + &e.0);
        // The price is asked without the request's content (an email's text, a phone number), so a service the user turns
        // down never sees it; one that can't quote without it gets it unpaid, as the protocol has it.
        let mut probed = body.as_deref().is_some_and(|b| !b.is_empty()) && !self.st().priced_with_content.contains(url);
        let mut answer = mpp::call(method, url, if probed { None } else { body.as_deref() }, None).await.map_err(unreached)?;
        if probed && answer.0 != 402 {
            probed = false;
            answer = mpp::call(method, url, body.as_deref(), None).await.map_err(unreached)?;
        }
        let (status, headers, data) = answer;
        if status != 402 {
            if (200..300).contains(&status) { return Ok(PriceAsked::Answered(self.answered(&ask, &service, &data, None, None, "Tempo").await)) }
            return said!(self.say(&format!("{service} answered {status}: "), &format!("{service} ответил {status}: ")) + &mpp::shown(&data, 300));
        }
        match Self::way_asked(&headers, chrono::Utc::now()) {
            Err(refusal) => said!(self.say(&format!("Can't pay {service}: "), &format!("Не могу оплатить {service}: ")) + &refusal.reason),
            Ok((challenge, x402)) => {
                let receives = match &challenge.near { Some(n) => Self::near_receives(n).await, None => None };
                // x402 and NEAR Intents charges always wait for their card: only a Tempo price is one a task pays by itself.
                let task = if x402.is_none() && challenge.near.is_none() { self.automation_task(command).map(|t| t.id) } else { None };
                Ok(PriceAsked::Quoted(Box::new(PaymentQuote { command: command.clone(), challenge, x402, ask, service, probed, task, receives })))
            }
        }
    }

    /// What a 402 asks, read as this wallet pays it: MPP in USDC on Tempo first, then MPP's NEAR Intents charge in USDC
    /// on a network it has, then x402 in USDC on Base (its challenge then the offer's price and payee, method «x402»).
    /// Else why it can't be paid, as MPP says it.
    pub fn way_asked(headers: &HashMap<String, String>, now: Date) -> Result<(MppChallenge, Option<Offer>), Refusal> {
        let mpp = mpp::challenge_paying(headers.get("www-authenticate").map_or("", String::as_str), now, |n| Self::near_origin(n).is_some());
        match (mpp, x402::offer(headers.get("payment-required").map(String::as_str))) {
            (Err(_), Some(offer)) => Ok((MppChallenge { params: HashMap::new(), amount: offer.amount, payee: offer.pay_to.clone(), memo: None, expires: None,
                                                        method: "x402".into(), near: None }, Some(offer))),
            (mpp, _) => mpp.map(|c| (c, None)),
        }
    }

    /// The place a NEAR Intents charge is paid from when its token is USDC there and 1Click takes it: «solana» or an EVM
    /// network's key (the Mac's nearOrigin).
    pub fn near_origin(n: &NearCharge) -> Option<&'static str> {
        if n.on_solana() { return (n.token() == SpotAsset::usdc().mint).then_some("solana") }
        let id = n.evm_chain()?;
        let net = EvmNetwork::all().iter().find(|net| net.rpc.chain_id == id && net.usdc.eq_ignore_ascii_case(n.token()))?;
        one_click::asset::usdc_on_evm(net.key).map(|_| net.key)
    }

    /// Whether a refund of the charge would come back to this wallet: the service names the refund address.
    pub fn near_refund_is_ours(&self, n: &NearCharge, origin: &str) -> bool {
        let st = self.st();
        if origin == "solana" { st.solana_address().is_some_and(|a| a == n.refund_to) } else { st.evm_address().is_some_and(|a| a.eq_ignore_ascii_case(&n.refund_to)) }
    }

    /// What the service receives as 1Click lists it: «1 USDC», and the asset's 1Click id. None when 1Click doesn't list it.
    async fn near_receives(n: &NearCharge) -> Option<(String, String)> { Self::receives_in(&one_click::tokens().await.ok()?, n) }

    pub fn receives_in(tokens: &[Token], n: &NearCharge) -> Option<(String, String)> {
        let a = n.destination_asset.as_str();
        let contract = a.rsplit(':').next().unwrap_or("").to_lowercase();
        let on = |chain: &str| tokens.iter().find(|t| t.blockchain == chain && t.contract.as_deref() == Some(contract.as_str()));
        let token = if let Some(id) = a.strip_prefix("near:mainnet/nep141:") {
            tokens.iter().find(|t| t.asset_id == format!("nep141:{id}"))
        } else if a.starts_with(&format!("{}/", NearCharge::SOLANA_MAINNET)) {
            on("sol")
        } else if let Some(rest) = a.strip_prefix("eip155:") {
            let id: u64 = rest.split('/').next()?.parse().ok()?;
            let net = EvmNetwork::all().iter().find(|net| net.rpc.chain_id == id)?;
            on(INTENT_CHAINS.iter().find(|(p, _)| *p == net.key)?.1)
        } else { None }?;
        let decimals = u32::try_from(token.decimals).ok()?;
        let amount = Decimal::try_from_i128_with_scale(n.amount_out.parse().ok()?, decimals).ok()?.normalize();
        Some((format!("{} {}", Fmt::num(amount, decimals), token.symbol), token.asset_id.clone()))
    }

    /// What to bring to a network for a price there (x402 on Base, a NEAR Intents charge on an EVM network), or None when
    /// it has it: what's short plus 1Click's ≈ 1% and a cent, at least a dollar (its minimum: the next requests need no
    /// bridge). The Mac's x402TopUp.
    pub fn x402_top_up(price: Decimal, there: Decimal) -> Option<Decimal> {
        if there >= price { return None }
        Some(rounded((price - there) * Decimal::new(102, 2) + Decimal::new(1, 2), 2, Rounding::Up).max(Decimal::ONE))
    }

    /// A NEAR Intents charge asked again is paid only to the same receiver, refund address and output, for no more.
    pub fn same_charge(shown: &MppChallenge, fresh: &MppChallenge) -> bool {
        let (Some(n), Some(old)) = (&fresh.near, &shown.near) else { return false };
        n.currency == old.currency && n.destination_recipient == old.destination_recipient && n.destination_asset == old.destination_asset
            && n.amount_out == old.amount_out && n.refund_to == old.refund_to && fresh.amount <= shown.amount
    }

    pub async fn prepare_payment(&self, command: &WalletCommand, note: Option<String>) {
        match self.ask_price(command).await {
            Ok(PriceAsked::Answered(text)) => self.reply(&text, note),
            Ok(PriceAsked::Quoted(q)) => self.show_payment(*q, note),
            Err(e) => self.reply(&e.to_string(), note),
        }
    }

    /// The price on its card for Confirm, said in the chat; in a task's run on the automation wallet, with what that
    /// Confirm lets its later runs pay by themselves.
    fn show_payment(&self, q: PaymentQuote, note: Option<String>) {
        self.st_mut().payment = Some(q.clone());
        let command = &q.command;
        let host = match command { WalletCommand::Pay { url, .. } => host(url), _ => None }.unwrap_or_else(|| q.service.clone());
        let (c, service) = (&q.challenge, &q.service);
        let price = Fmt::num(c.usdc(), 6);
        if q.x402.is_some() {
            let (there, (start, end)) = (self.usdc_free("base").unwrap_or_default(), wallet::address_check::ends(&c.payee));
            let bring = Self::x402_top_up(c.usdc(), there);
            self.show(Pending::Wallet(command.clone()), &self.say(&format!("Pay {price} USDC · {service}"), &format!("Оплатить {price} USDC · {service}")), vec![
                self.say(&format!("x402: a signature that lets {host}'s facilitator move exactly {price} USDC from your Base address to {start} … {end}. It's valid 10 minutes at most, and the facilitator pays the network fee."),
                         &format!("x402: подпись, по которой посредник {host} переведёт ровно {price} USDC с твоего адреса в Base на {start} … {end}. Она действует не дольше 10 минут, а комиссию сети платит посредник.")),
                bring.map_or_else(|| self.say("From your USDC on Base.", "Из твоих USDC в Base."), |b| {
                    let (b, there) = (Fmt::num(b, 2), Fmt::num(there, 2));
                    self.say(&format!("Base has {there} USDC, so {b} USDC come from Solana through NEAR Intents first."),
                             &format!("В Base {there} USDC, поэтому сначала {b} USDC придут с Solana через NEAR Intents."))
                }),
            ], vec![], vec![]);
            let first = if bring.is_some() { self.say(" Base's USDC comes from Solana through NEAR Intents first.", " USDC в Base сначала придут с Solana через NEAR Intents.") } else { String::new() };
            self.reply(&(self.say(&format!("{service} asks {price} USDC by x402 on Base. Press Confirm to sign it, and its facilitator pays the fee, or Cancel."),
                                  &format!("{service} просит {price} USDC по x402 в Base. Нажми «Подтвердить», чтобы подписать, а комиссию заплатит его посредник, или «Отмена».")) + &first), note);
            return;
        }
        if let Some(n) = &c.near {
            let origin = Self::near_origin(n).unwrap_or("solana");
            let net = WalletCommand::place_name(origin);
            let gets = q.receives.as_ref().map_or_else(|| self.say("what it asked for", "то, что просит"), |r| r.0.clone());
            let refund = if self.near_refund_is_ours(n, origin) { self.say("a refund comes back to you", "возврат придёт тебе") } else {
                self.say("a refund goes to the service's address, not yours", "возврат уйдёт на адрес сервиса, а не тебе")
            };
            self.show(Pending::Wallet(command.clone()), &self.say(&format!("Pay {price} USDC · {service} · NEAR Intents"), &format!("Оплатить {price} USDC · {service} · NEAR Intents")), vec![
                self.say(&format!("{price} USDC on {net} to the 1Click deposit address {host} named: {}", c.payee),
                         &format!("{price} USDC в {net} на адрес депозита 1Click, который назвал {host}: {}", c.payee)),
                self.say(&format!("{service} gets {gets} at {}. If the payment doesn't settle, {refund}.", n.destination_recipient),
                         &format!("{service} получит {gets} на {}. Если оплата не пройдёт, {refund}.", n.destination_recipient)),
                c.purpose().map_or_else(|| self.say("For one request.", "За один запрос."), |p| self.say("For: ", "За: ") + p),
            ], vec![], vec![]);
            self.reply(&self.say(&format!("{service} asks {price} USDC on {net} through NEAR Intents and gets {gets} at {}. Press Confirm to pay it, or Cancel. If the payment doesn't settle, {refund}.", n.destination_recipient),
                                 &format!("{service} просит {price} USDC в {net} через NEAR Intents и получит {gets} на {}. Нажми «Подтвердить», чтобы оплатить, или «Отмена». Если оплата не пройдёт, {refund}.", n.destination_recipient)), note);
            return;
        }
        let short = self.tempo_usdc() < c.usdc() + Decimal::new(1, 2);
        let src = self.tempo_need(c.usdc()).and_then(|need| self.tempo_source(need)).map_or("Solana", |(net, _)| net.name);
        self.show(Pending::Wallet(command.clone()), &self.say(&format!("Pay {price} USDC · {service}"), &format!("Оплатить {price} USDC · {service}")), vec![
            self.say(&format!("One Tempo transfer of {price} USDC to the address {host} named: {}", c.payee),
                     &format!("Один перевод {price} USDC на Tempo, на адрес, который назвал {host}: {}", c.payee)),
            if short {
                self.say(&format!("What's short comes from {src} over Relay first: a dollar, so the next requests don't each need a bridge."),
                         &format!("Сначала из {src} через Relay придёт доллар, чтобы следующие запросы не гоняли мост каждый раз."))
            } else {
                self.say("From your USDC on Tempo. The fee, ≈ $0.001, comes out of the USDC.", "Из твоих USDC на Tempo, комиссия ≈ $0.001 — из тех же USDC.")
            },
            c.purpose().map_or_else(|| self.say("For one request.", "За один запрос."), |p| self.say("For: ", "За: ") + p),
        ], vec![], vec![]);
        let then = if q.task.is_none() { String::new() } else {
            self.say(&format!(" After that, this task pays up to {price} USDC for this request by itself. A higher price waits for your Confirm again."),
                     &format!(" Дальше эта задача будет сама платить за этот запрос до {price} USDC. Если цена вырастет, снова спрошу."))
        };
        self.reply(&(self.say(&format!("{service} asks {price} USDC. Press Confirm to pay it in USDC on Tempo, or Cancel."),
                              &format!("{service} просит {price} USDC. Нажми «Подтвердить», чтобы оплатить в USDC на Tempo, или «Отмена».")) + &then), note);
    }

    /// The repeating task this is a run of on the automation wallet (`RUN_TASK`), with the prices Confirmed in it, when the
    /// request is one of its steps (the Mac's automationTask(of:)).
    pub fn automation_task(&self, command: &WalletCommand) -> Option<ScheduledTask> {
        let id = RUN_TASK.try_with(|t| *t).ok().flatten()?;
        self.st().tasks.iter().find(|t| t.id == id && t.steps.iter().any(|s| WalletCommand::parse(s).as_ref() == Some(command))).cloned()
    }

    /// A paid request reached in a running chain with no card that showed its price (a step of a task's run on the
    /// automation wallet), its price just asked: true when the price Confirmed for it in that task covers it, and pay()
    /// takes it with no card. Otherwise its own card shows the price and waits for its own Confirm, the steps after it
    /// (`rest`) held behind it: no price the user hasn't seen is paid (the Mac's paysUnseen).
    pub fn pays_by_itself(&self, q: PaymentQuote, rest: &[String]) -> bool {
        // x402 and NEAR Intents charges always wait for their card: a task pays only a Tempo price it was Confirmed.
        let tempo = q.x402.is_none() && q.challenge.near.is_none();
        if !tempo || !self.automation_task(&q.command).is_some_and(|t| t.approves(q.challenge.amount, &q.command)) {
            self.show_payment(q, None);
            self.hold_after_call(rest);
            return false;
        }
        let (price, service) = (Fmt::num(q.challenge.usdc(), 6), q.service.clone());
        self.st_mut().payment = Some(q);
        self.reply(&self.say(&format!("{service} asks {price} USDC — no more than you Confirmed for this task, so it goes without a card, from the automation wallet."),
                             &format!("{service} просит {price} USDC — не больше, чем ты подтвердил для этой задачи, поэтому без карточки, с кошелька автоматизации.")), None);
        true
    }

    /// After Confirm: the price shown is asked again when it was quoted without the content or is about to expire, and
    /// paid only if it's the same amount to the same payee; then the service is asked again with the payment's proof.
    /// Only a price shown is paid (or one a task's Confirmed price covers, pays_by_itself), never one asked here.
    pub async fn pay(&self, command: &WalletCommand, e: &Executor) -> trade::Result<String> {
        let shown = self.st().payment.clone().filter(|p| &p.command == command);
        let (WalletCommand::Pay { method, url, body }, Some(quote)) = (command, shown) else {
            return said!("The service's price is gone. Ask for it again.".into());
        };
        if let Some(offer) = &quote.x402 { return self.pay_x402(method, url, body.as_deref(), &quote, offer, e).await }
        if quote.challenge.near.is_some() { return self.pay_near(method, url, body.as_deref(), &quote, e).await }
        let mut c = quote.challenge.clone();
        if quote.probed || c.expires.is_some_and(|x| (x - chrono::Utc::now()).num_milliseconds() < 30_000) {
            let (status, headers, _) = mpp::call(method, url, body.as_deref(), None).await?;
            let fresh = (status == 402).then(|| mpp::challenge(headers.get("www-authenticate").map_or("", String::as_str), chrono::Utc::now()).ok()).flatten();
            let Some(fresh) = fresh else { return said!(format!("The service didn't quote again ({status}). Nothing was paid.")) };
            if fresh.amount != c.amount || fresh.payee.to_lowercase() != c.payee.to_lowercase() {
                // Its price depends on the request: from now on it's asked with the content, so the next card shows it.
                if quote.probed { self.st_mut().priced_with_content.insert(url.clone()); }
                return said!(format!("The price changed to {} USDC. Nothing was paid. Ask again to see the new card.", Fmt::num(fresh.usdc(), 6)));
            }
            c = fresh;
        }
        let hash = e.pay_tempo(c.amount, &c.payee, c.memo.as_deref(), &quote.service).await?;
        self.st_mut().payment = None;
        // Asked in a task's run on the automation wallet and paid: its later runs pay up to this by themselves.
        if let Some(id) = quote.task { self.set_tasks(|t| if let Some(t) = t.iter_mut().find(|t| t.id == id) { t.approve(c.amount, command) }) }
        let proof = format!("{}/tx/{hash}", EvmNetwork::tempo().explorer);
        let waiting = self.say(&format!("Waiting for {}'s answer", quote.service), &format!("Жду ответ {}", quote.service));
        if let Some(r) = self.st_mut().running.as_mut() { r.lines.push(waiting) }
        self.changed();
        let payer = self.st().evm_address().unwrap_or_default();
        let (status, _, data) = mpp::call(method, url, body.as_deref(), Some((c.field(), &mpp::credential(&c, &hash, &payer)))).await?;
        if !(200..300).contains(&status) {
            return said!(format!("Paid {} USDC, but {} answered {status}: {}. The payment is on Tempo: {proof}", Fmt::num(c.usdc(), 6), quote.service, mpp::shown(&data, 300)));
        }
        Ok(self.answered(&quote.ask, &quote.service, &data, Some(c.usdc()), Some(&proof), "Tempo").await)
    }

    /// A line on the running move's block.
    fn running_line(&self, line: String) {
        if let Some(r) = self.st_mut().running.as_mut() { r.lines.push(line) }
        self.changed();
    }

    /// After Confirm, x402: the same price asked again, USDC brought to Base through NEAR Intents when it's short, an
    /// EIP-3009 authorization the signer builds and checks itself, and the request again with it. Nothing moves unless
    /// the service's facilitator settles it; the settlement's transaction is the proof.
    async fn pay_x402(&self, method: &str, url: &str, body: Option<&str>, quote: &PaymentQuote, offer: &Offer, e: &Executor) -> trade::Result<String> {
        let service = &quote.service;
        // This wallet's own key signs it, and its Base address pays: never a task's automation wallet (its top-up would
        // land there while the main key is asked) or a Ledger (it can't sign EIP-3009 here).
        if task_run().is_some() || self.st().ledger_active() {
            return said!(self.say("An x402 payment is signed by this wallet's own key, so it can't run from the automation wallet or a Ledger. Nothing was paid.",
                                  "Оплату x402 подписывает собственный ключ этого кошелька, поэтому она не идёт с кошелька автоматизации или с Ledger. Ничего не оплачено."));
        }
        let (status, headers, _) = mpp::call(method, url, body, None).await?;
        let fresh = (status == 402).then(|| x402::offer(headers.get("payment-required").map(String::as_str))).flatten()
            .filter(|f| f.amount == offer.amount && f.pay_to.eq_ignore_ascii_case(&offer.pay_to));
        let Some(fresh) = fresh else {
            return said!(self.say(&format!("{service}'s price changed or it didn't quote again ({status}). Nothing was paid. Ask again to see the new card."),
                                  &format!("Цена {service} изменилась, или он не назвал её снова ({status}). Ничего не оплачено. Спроси снова, чтобы увидеть новую карточку.")));
        };
        let Some(from) = self.st().evm_address() else { return said!(self.say("No EVM address in this wallet.", "В этом кошельке нет EVM-адреса.")) };
        let (price, on_base) = (fresh.usdc(), self.usdc_free("base").unwrap_or_default());
        if let Some(bring) = Self::x402_top_up(price, on_base) {
            self.running_line(self.say("Bringing USDC to Base through NEAR Intents", "Перевожу USDC в Base через NEAR Intents"));
            let arrived = e.usdc_by_intents("base", bring).await?;
            if on_base + arrived < price {
                let got = Fmt::num(arrived, 2);
                return said!(self.say(&format!("Only {got} USDC reached Base. Nothing was paid. Ask again."), &format!("До Base дошло только {got} USDC. Ничего не оплачено. Спроси снова.")));
            }
        }
        let mut nonce = [0u8; 32];
        if getrandom::fill(&mut nonce).is_err() { return said!(self.say("No randomness for the payment. Nothing was paid.", "Нет случайных чисел для оплаты. Ничего не оплачено.")) }
        // As long as the service asks and no longer than 10 minutes: an unsettled signature runs out soon.
        let valid_before = chrono::Utc::now().timestamp().max(0) as u64 + fresh.max_timeout_seconds.clamp(60, 600) as u64;
        let reply = self.signer.send(&SignerRequest::SignX402 { chain_id: fresh.chain_id, token: fresh.asset.clone(), to: fresh.pay_to.clone(), value: fresh.amount,
                                                               valid_before, nonce: nonce.to_vec(), service: service.clone() }).await?;
        let Some(sig) = reply.signatures.as_ref().and_then(|s| s.first()).filter(|_| reply.ok) else { return Err(trade::Error::Guard(reason(&reply))) };
        // r ‖ s ‖ v: the 65 bytes x402 sends.
        let signature = [sig.r.as_slice(), &sig.s, &[sig.v as u8]].concat();
        self.st_mut().payment = None;
        let (field, value) = x402::payment(&fresh, &from, valid_before, &nonce, &signature);
        // Whatever goes wrong now, the signature may still be settled until it runs out: said, so nobody pays twice.
        let (usdc, until) = (Fmt::num(price, 6), chrono::DateTime::from_timestamp(valid_before as i64, 0)
            .map_or_else(String::new, |t| t.with_timezone(&chrono::Local).format("%H:%M").to_string()));
        let pending = self.say(&format!("The signed payment of {usdc} USDC can still be settled until {until}. Check your Base USDC before asking again."),
                               &format!("Подписанная оплата {usdc} USDC ещё может пройти до {until}. Проверь USDC в Base, прежде чем спрашивать снова."));
        // Written down before it's used: a failure from here on reads «status unknown», not «Failed» with Try again, since
        // the signature may still be settled (a second Confirm would sign a second payment).
        let id = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let _ = self.journal().append(&Entry::new("x402", &id, Kind::Signed, &format!("{service} · {usdc} USDC")));
        self.running_line(self.say(&format!("Waiting for {service}'s answer"), &format!("Жду ответ {service}")));
        let (status, headers, data) = mpp::call(method, url, body, Some((field, &value))).await
            .map_err(|err| trade::Error::Execution(self.say(&format!("{service} didn't answer ({}). ", err.0), &format!("{service} не ответил ({}). ", err.0)) + &pending))?;
        if !(200..300).contains(&status) {
            return said!(self.say(&format!("{service} answered {status}: {}. ", mpp::shown(&data, 300)), &format!("{service} ответил {status}: {}. ", mpp::shown(&data, 300))) + &pending);
        }
        let _ = self.journal().append(&Entry::new("x402", &id, Kind::Settled, service));
        let proof = x402::settled(headers.get("payment-response").map(String::as_str)).map(|tx| format!("{}/tx/{tx}", EvmNetwork::base().explorer));
        Ok(self.answered(&quote.ask, service, &data, Some(price), proof.as_deref(), "Base").await)
    }

    /// After Confirm, a NEAR Intents charge: asked again when it's about to run out (its deposit address is single-use, so a
    /// fresh one comes with it: only the same receiver, refund address and output, for no more), USDC brought to the network
    /// first when it's short, the deposit checked with 1Click and sent, then the request again with its hash. The service
    /// answers once 1Click has settled it.
    async fn pay_near(&self, method: &str, url: &str, body: Option<&str>, quote: &PaymentQuote, e: &Executor) -> trade::Result<String> {
        let service = &quote.service;
        let mut c = quote.challenge.clone();
        if quote.probed || c.expires.is_some_and(|x| (x - chrono::Utc::now()).num_seconds() < 60) {
            let (status, headers, _) = mpp::call(method, url, body, None).await?;
            let header = headers.get("www-authenticate").map_or("", String::as_str);
            let fresh = (status == 402).then(|| mpp::challenge_paying(header, chrono::Utc::now(), |n| Self::near_origin(n).is_some()).ok()).flatten()
                .filter(|f| Self::same_charge(&c, f));
            let Some(fresh) = fresh else {
                return said!(self.say(&format!("{service}'s price or receiver changed when asked again ({status}). Nothing was paid. Ask again to see the new card."),
                                      &format!("Цена или получатель {service} изменились при повторном запросе ({status}). Ничего не оплачено. Спроси снова, чтобы увидеть новую карточку.")));
            };
            c = fresh;
        }
        let Some((n, origin)) = c.near.clone().and_then(|n| Self::near_origin(&n).map(|o| (n, o))) else {
            return said!(self.say("ihatecrypto doesn't pay this token. Nothing was paid.", "ihatecrypto не платит этим токеном. Ничего не оплачено."));
        };
        let name = WalletCommand::place_name(origin);
        let payer = if origin == "solana" { self.st().solana_address() } else { self.st().evm_address() };
        let Some(payer) = payer else { return said!(self.say(&format!("No address on {name} in this wallet."), &format!("В этом кошельке нет адреса в {name}."))) };
        let there = self.usdc_free(origin).unwrap_or_default();
        if let Some(bring) = Self::x402_top_up(c.usdc(), there).filter(|_| origin != "solana") {
            self.running_line(self.say(&format!("Bringing USDC to {name} through NEAR Intents"), &format!("Перевожу USDC в {name} через NEAR Intents")));
            let arrived = e.usdc_by_intents(origin, bring).await?;
            if there + arrived < c.usdc() {
                let got = Fmt::num(arrived, 2);
                return said!(self.say(&format!("Only {got} USDC reached {name}. Nothing was paid. Ask again."), &format!("До {name} дошло только {got} USDC. Ничего не оплачено. Спроси снова.")));
            }
            // The top-up took its minutes: a price that ran out meanwhile isn't paid (the USDC stays where it came).
            if c.expires.is_some_and(|x| (x - chrono::Utc::now()).num_seconds() < 60) {
                return said!(self.say(&format!("{service}'s price ran out while the USDC came to {name}. Nothing was paid. Ask again."),
                                      &format!("Цена {service} истекла, пока USDC шли в {name}. Ничего не оплачено. Спроси снова.")));
            }
        }
        let from = (origin != "solana").then_some(origin);
        let hash = e.pay_intents_charge(from, c.amount, &c.payee, &n.destination_recipient, &n.refund_to, &n.amount_out,
                                        quote.receives.as_ref().map(|r| r.1.as_str()), service).await?;
        self.st_mut().payment = None;
        let proof = format!("https://explorer.near-intents.org/transactions/{}", c.payee);
        self.running_line(self.say(&format!("Waiting for {service}'s answer"), &format!("Жду ответ {service}")));
        let credential = mpp::credential(&c, &hash, &payer);
        // The deposit is on the journal as 1Click's (pay_intents_charge): a failure from here on reads «status unknown».
        let data = mpp::paid_on(method, url, body, (c.field(), &credential), c.usdc(), service, &proof, "NEAR Intents", 300).await
            .map_err(|r| trade::Error::Execution(r.reason))?;
        let _ = self.journal().append(&Entry::new("1click", &c.payee, Kind::Settled, &format!("MPP {service}")));
        Ok(self.answered(&quote.ask, service, &data, Some(c.usdc()), Some(&proof), "NEAR Intents").await)
    }

    /// The service's answer in the user's words when a model is connected, its text otherwise; what was paid (on `place`)
    /// under it.
    async fn answered(&self, ask: &str, service: &str, data: &[u8], usdc: Option<Decimal>, proof: Option<&str>, place: &str) -> String {
        let mut text = self.say(&format!("{service} answered:\n"), &format!("{service} ответил:\n")) + &mpp::shown(data, 1500);
        if let Some(client) = self.llm_client(None) {
            let language = self.st().reply_language();
            let model = self.model();
            if let Ok(said) = mpp::reply(ask, service, data, &language, &client, &model).await && !said.is_empty() { text = said }
        }
        let card = ServiceCard { service: service.into(), usdc, proof: proof.map(str::to_string), answer: Answer::new(data), raw: mpp::shown(data, 20_000) };
        self.pin_kind("result", service, text.split('\n').filter(|l| !l.is_empty()).take(20).map(str::to_string).collect(), None, None,
                      Some(BlockData { service: Some(card), ..BlockData::default() }));
        let cost = usdc.map(|u| {
            let paid = Fmt::num(u, 6);
            self.say(&format!("Paid {paid} USDC to {service} on {place}"), &format!("Оплачено {paid} USDC · {service} · {place}"))
        });
        text + &cost.map_or(String::new(), |c| format!("\n\n{c} {}", proof.unwrap_or("")))
    }
}

#[cfg(test)]
#[path = "payments/tests.rs"]
mod tests;
