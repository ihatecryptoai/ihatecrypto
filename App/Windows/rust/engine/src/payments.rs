//! Paying for a service (Payments.swift): «pay GET https://…»: a service that answers 402 with an MPP challenge is paid
//! in USDC on Tempo after Confirm, then asked again with the payment's hash — the Mac's Payments, over the shared MPP
//! and `Executor::pay_tempo`. Services quoted with the request's content are `State::priced_with_content`.

use ihatecrypto_core::foundation::Uuid;
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::mpp::{self, Answer};
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::scheduled_task::ScheduledTask;
use rust_decimal::Decimal;
use trade::{EvmNetwork, Executor};

use crate::automation::RUN_TASK;
use crate::blocks::{BlockData, ServiceCard};
use crate::engine::{Engine, Pending};
use crate::from_core::MppChallenge;
use crate::nodes::host;

/// `throw ExecutionError(…)`.
macro_rules! said {
    ($words:expr) => { Err(trade::Error::Execution($words)) };
}

/// A service's price waiting for Confirm.
#[derive(Debug, Clone, PartialEq)]
pub struct PaymentQuote {
    pub command: WalletCommand,
    pub challenge: MppChallenge,
    pub ask: String,
    pub service: String,
    /// Quoted without the request's content: Confirm sends it, asking the price again with it before paying.
    pub probed: bool,
    /// Asked in a run of this task on the automation wallet: paid, the price is what its later runs may pay by themselves.
    pub task: Option<Uuid>,
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
            if (200..300).contains(&status) { return Ok(PriceAsked::Answered(self.answered(&ask, &service, &data, None, None).await)) }
            return said!(self.say(&format!("{service} answered {status}: "), &format!("{service} ответил {status}: ")) + &mpp::shown(&data, 300));
        }
        match mpp::challenge(headers.get("www-authenticate").map_or("", String::as_str), chrono::Utc::now()) {
            Err(refusal) => said!(self.say(&format!("Can't pay {service}: "), &format!("Не могу оплатить {service}: ")) + &refusal.reason),
            Ok(challenge) => {
                let task = self.automation_task(command).map(|t| t.id);
                Ok(PriceAsked::Quoted(Box::new(PaymentQuote { command: command.clone(), challenge, ask, service, probed, task })))
            }
        }
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
        if !self.automation_task(&q.command).is_some_and(|t| t.approves(q.challenge.amount, &q.command)) {
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
        Ok(self.answered(&quote.ask, &quote.service, &data, Some(c.usdc()), Some(&proof)).await)
    }

    /// The service's answer in the user's words when a model is connected, its text otherwise; what was paid under it.
    async fn answered(&self, ask: &str, service: &str, data: &[u8], usdc: Option<Decimal>, proof: Option<&str>) -> String {
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
            self.say(&format!("Paid {paid} USDC to {service} on Tempo"), &format!("Оплачено {paid} USDC · {service} · Tempo"))
        });
        text + &cost.map_or(String::new(), |c| format!("\n\n{c} {}", proof.unwrap_or("")))
    }
}

#[cfg(test)]
#[path = "payments/tests.rs"]
mod tests;
