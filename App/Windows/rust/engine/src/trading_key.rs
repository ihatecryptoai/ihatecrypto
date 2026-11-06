//! Hyperliquid's trading key, as on the Mac (TradingKey.swift; Execution.swift enableAgent, resumeTradingKey,
//! offerTradingKey; Ledger.swift enableLedgerAgent; Settings › Wallet › Hyperliquid). The signer makes the key and signs
//! its approval with the main key (or the Ledger does); only once Hyperliquid accepts it does the signer sign orders,
//! stops and closes with it, without Windows Hello. It can't withdraw: anything that moves money out still takes the
//! main key. Also ihatecrypto's fee: whether the account approved it, and taking it back.

use std::sync::LazyLock;
use std::time::Duration;

use fancy_regex::Regex;
use hyperliquid::{MsgPack, builder};
use ihatecrypto_core::network::Hyperliquid;
use ihatecrypto_core::position_command::WalletCommand;
use rust_decimal::Decimal;
use serde::Serialize;
use signer_api::SignerRequest;
use trade::{Executor, HlReply};
use wallet::evm::EvmSignature;

use crate::canvas::Board;
use crate::engine::Engine;
use crate::from_core::RouterOption;
use crate::ledger::LedgerConnect;
use crate::link_signer;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradingKeyView {
    /// The key's address once made; None before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    pub active: bool,
    /// Approved before and paused by «lock»: on again after one Windows Hello.
    pub paused: bool,
    /// The Ledger's own key (its account), approved on the device.
    pub ledger: bool,
    /// This Hyperliquid account approved ihatecrypto's fee; None until Hyperliquid answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_approved: Option<bool>,
    pub fee_rate: String,
    pub referral: String,
    /// The wallet can have one: an EVM address and a key here.
    pub possible: bool,
}

/// The chat's answers to the trading key's offer.
pub const TRADING_KEY_ON: &str = "approve the hyperliquid trading key";
pub const TRADING_KEY_LATER: &str = "leave the hyperliquid trading key off";

/// «turn on the trading key», «верни торговый ключ».
static TURN_ON: LazyLock<Regex> = LazyLock::new(|| Regex::new(
    r"(?:resume|unpause|turn\s+on|enable|approve|включи|верни|одобри)\s+(?:the\s+)?(?:hyperliquid\s+)?(?:trading\s+key|торгов\w*\s+ключ)",
).expect("a regex"));
static LEDGER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"ledger|л[еэ]дж[еэ]р").expect("a regex"));

/// The first `n` characters of an address (Swift's `prefix`).
fn head(s: &str, n: usize) -> String { s.chars().take(n).collect() }
/// The last `n` characters (Swift's `suffix`).
fn tail(s: &str, n: usize) -> String { let c: Vec<char> = s.chars().collect(); c[c.len().saturating_sub(n)..].iter().collect() }

/// HLExchange.send on mainnet: the signed action to /exchange, its body whatever the status (20 s, as Swift's request).
async fn send_to_exchange(action: &MsgPack, nonce: u64, signature: &EvmSignature) -> Result<Vec<u8>, String> {
    let body = hyperliquid::exchange_body(action, nonce, signature, None);
    let words = |e: reqwest::Error| if e.is_timeout() { "The request timed out.".to_string() } else if e.is_connect() { "Could not connect to the server.".to_string() } else { e.to_string() };
    let response = net::HTTP.post(hyperliquid::exchange_url(true)).header("Content-Type", "application/json").body(body.to_string())
        .timeout(Duration::from_secs(20)).send().await.map_err(words)?;
    Ok(response.bytes().await.map_err(words)?.to_vec())
}

impl Engine {
    /// The key of the wallet that signs: the Ledger's while it's on, else the signer's.
    pub fn agent_address(&self) -> Option<String> {
        let st = self.st();
        if st.ledger_active() { st.ledger_agent.as_ref()?.agent_address.clone() } else { st.status.as_ref()?.agent_address.clone() }
    }

    pub fn agent_active(&self) -> bool {
        let st = self.st();
        (if st.ledger_active() { st.ledger_agent.as_ref() } else { st.status.as_ref() }).and_then(|r| r.agent_active) == Some(true)
    }

    pub fn trading_key_view(&self) -> TradingKeyView {
        let (address, active) = (self.agent_address(), self.agent_active());
        let st = self.st();
        TradingKeyView {
            paused: address.is_some() && !active, address, active, ledger: st.ledger_active(), fee_approved: st.fee_approved,
            fee_rate: builder::RATE.into(), referral: builder::REFERRAL_CODE.into(),
            possible: !st.watching() && st.active_wallet().is_some_and(|w| w.evm.is_some()),
        }
    }

    /// The Ledger's key, read with the signer's status (Workspace.loadSigner).
    pub async fn read_ledger_agent(&self) {
        let evm = { let st = self.st(); if st.ledger_active() { st.ledger.as_ref().and_then(|l| l.evm.clone()) } else { None } };
        let Some(evm) = evm else { self.st_mut().ledger_agent = None; return };
        let read = self.signer.send(&SignerRequest::LedgerAgent { owner: evm, create: false }).await.ok();
        self.st_mut().ledger_agent = read;
    }

    /// «turn on the trading key», «верни торговый ключ»: never approved — approved now; paused — resumed.
    pub async fn trading_key_command(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        if !TURN_ON.is_match(&lower).unwrap_or(false) { return false }
        // «… on my Ledger» with the Ledger not signing: its own account's key, never the main wallet's (the Mac's
        // ledgerCommand answers before the trading-key command, Ledger.swift:77-80).
        if LEDGER.is_match(&lower).unwrap_or(false) && !self.ledger_active() {
            self.reply(&self.say("Switch to your Ledger first («use my Ledger»): the trading key belongs to its Hyperliquid account.",
                                 "Сначала переключись на Ledger («используй мой Ledger»): торговый ключ принадлежит его счёту Hyperliquid."), None);
            return true;
        }
        self.turn_on_trading_key().await;
        true
    }

    /// Settings' «Approve…» and «Resume…», the chat's words and the offer's card.
    pub async fn turn_on_trading_key(&self) {
        let none = self.agent_address().is_none();
        if self.ledger_active() && none { self.enable_ledger_agent().await } else if none { self.enable_agent().await } else { self.resume_trading_key().await }
    }

    /// Whether Hyperliquid has this account, so its key can be approved: asked twice, since an answer that didn't come
    /// (a refused or slow request) isn't an open account. A new one gets what opens it; no answer is said, before any
    /// Windows Hello prompt for a key Hyperliquid would refuse.
    pub async fn hyperliquid_opens(&self, evm: &str) -> bool {
        for attempt in 0..2 {
            if let Ok(new) = Hyperliquid::is_new_account(evm).await {
                if new { self.offer_opening_hyperliquid() }
                return !new;
            }
            if attempt == 0 { tokio::time::sleep(Duration::from_secs(2)).await }
        }
        self.reply(&self.say("Hyperliquid didn't answer, so the trading key wasn't asked for. Try again in a minute.",
                             "Hyperliquid не ответил, поэтому торговый ключ не запрошен. Попробуй ещё раз через минуту."), Some("Hyperliquid".into()));
        false
    }

    /// The signer makes a key and signs its approval with the main key; it goes to Hyperliquid, and only when the exchange
    /// accepts it do orders switch to the key.
    pub async fn enable_agent(&self) {
        let (watching, evm) = { let st = self.st(); (st.watching(), st.active_wallet().and_then(|w| w.evm)) };
        let Some(evm) = evm.filter(|_| !watching) else {
            let why = if watching { self.st().watch_only() } else { self.say("Add a wallet first.", "Сначала добавь кошелёк.") };
            self.reply(&why, None);
            return;
        };
        // Hyperliquid refuses a key for an account that never had a deposit: what opens it, and the way, before any prompt.
        if !self.hyperliquid_opens(&evm).await { return }
        let turned = async {
            let answer = self.signer_call(SignerRequest::EnableAgent {}).await.map_err(|e| e.0)?;
            let (Some(action), Some(signature), Some(nonce), Some(agent)) = (answer.action.clone(), answer.signature.clone(), answer.nonce, answer.agent_address.clone())
            else { return Err(link_signer::reason(&answer)) };
            let result = HlReply::parse(&send_to_exchange(&action, nonce, &signature).await?);
            if result.is_error() { return Err(self.say(&format!("Hyperliquid rejected the key: {result}."), &format!("Hyperliquid не принял ключ: {result}."))) }
            self.signer_call(SignerRequest::SetAgentActive { active: true }).await.map_err(|e| e.0)?;
            Ok(agent)
        };
        match turned.await {
            Ok(agent) => self.reply(&self.say(
                &format!("Trading key on: {}…. Orders, stops and closes now skip Windows Hello — new positions for 7 days, then one confirmation renews them. This key can't withdraw money, withdrawals still go through the main key.", head(&agent, 10)),
                &format!("Торговый ключ включён: {}…. Ордера, стопы и закрытия теперь идут без Windows Hello — новые позиции 7 дней, потом одно подтверждение продлевает. Вывести деньги этот ключ не может, вывод по-прежнему идёт основным ключом.", head(&agent, 10)),
            ), Some("Hyperliquid".into())),
            Err(why) => self.reply(&(self.say("Trading key not turned on: ", "Торговый ключ не включён: ") + &why), Some("Hyperliquid".into())),
        }
    }

    /// The Ledger's account gets a key of its own: the signer makes it, the Ledger approves it on its screen.
    pub async fn enable_ledger_agent(&self) {
        let (wallet, account) = { let st = self.st(); (st.trade_wallet().filter(|_| st.ledger_active()), st.ledger.clone()) };
        let (Some(wallet), Some(account)) = (wallet, account) else {
            self.reply(&self.say("Switch to your Ledger first (“use my Ledger”): the trading key belongs to its Hyperliquid account.",
                                 "Сначала переключись на Ledger («используй мой Ledger»): торговый ключ принадлежит его аккаунту Hyperliquid."), None);
            return;
        };
        if !self.hyperliquid_opens(&wallet.evm).await { return }
        if self.st().ledger_connect.is_none() {
            let status = self.say("Making the trading key…", "Делаю торговый ключ…");
            self.st_mut().ledger_connect = Some(LedgerConnect { model: account.model, evm: account.evm, solana: account.solana, status, problem: None, notes: vec![], busy: true });
        }
        // The Ledger's block follows the approval, as the Mac's LedgerBoard draws whenever ledgerConnect is set.
        self.pin(Board::new("ledger", "ledger"));
        let turned = async {
            let made = self.signer_call(SignerRequest::LedgerAgent { owner: wallet.evm.clone(), create: true }).await.map_err(|e| e.0)?;
            let Some(agent) = made.agent_address.clone() else { return Err(link_signer::reason(&made)) };
            if made.agent_active == Some(true) {
                self.read_ledger_agent().await;
                self.reply(&self.say(&format!("Your Ledger's trading key is already on: {}…{}.", head(&agent, 6), tail(&agent, 4)),
                                     &format!("Торговый ключ Ledger уже включён: {}…{}.", head(&agent, 6), tail(&agent, 4))), None);
                return Ok(());
            }
            let ask = self.say(&format!("Approve the trading key on your Ledger. Its screen must show agentAddress {}…{} and agentName ihatecrypto.", head(&agent, 6), tail(&agent, 4)),
                               &format!("Подтверди торговый ключ на Ledger. На экране должны быть agentAddress {}…{} и agentName ihatecrypto.", head(&agent, 6), tail(&agent, 4)));
            self.reply(&ask, None);
            self.connect_card(|c| c.status = ask);
            let nonce = u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0);
            let (action, signature) = self.ledger_signer(wallet.clone()).approve_agent(&agent, "ihatecrypto", nonce).await.map_err(|e| e.to_string())?;
            let result = HlReply::parse(&send_to_exchange(&action, nonce, &signature).await?);
            if result.is_error() {
                return Err(self.say(&format!("Hyperliquid rejected the key: {result}. Its account needs a deposit first."),
                                    &format!("Hyperliquid не принял ключ: {result}. Сначала аккаунту нужен депозит.")));
            }
            self.signer_call(SignerRequest::SetLedgerAgentActive { owner: wallet.evm.clone(), active: true }).await.map_err(|e| e.0)?;
            self.read_ledger_agent().await;
            let on = self.say("Trading key on: orders go without the Ledger's buttons.", "Торговый ключ включён: ордера идут без кнопок Ledger.");
            self.connect_card(|c| c.status = on);
            self.reply(&self.say(&format!("Trading key on for your Ledger: {}…. Orders, stops, trailing and closes now go without the device. Withdrawals and transfers are still signed on the Ledger. “lock” pauses it.", head(&agent, 10)),
                                 &format!("Торговый ключ для Ledger включён: {}…. Ордера, стопы, трейлинг и закрытия теперь идут без устройства. Выводы и переводы по-прежнему подписываются на Ledger. «Заблокируй» ставит его на паузу.", head(&agent, 10))), None);
            Ok(())
        };
        if let Err(why) = turned.await {
            let off = self.say("The trading key stayed off.", "Торговый ключ остался выключенным.");
            self.connect_card(|c| c.status = off);
            self.reply(&(self.say("Trading key not turned on: ", "Торговый ключ не включён: ") + &why), None);
        }
        self.connect_card(|c| c.busy = false);
        self.changed();
    }

    /// The key «lock» paused, on again; the signer asks for Windows Hello first.
    pub async fn resume_trading_key(&self) {
        let request = if self.ledger_active() {
            SignerRequest::SetLedgerAgentActive { owner: self.st().ledger.as_ref().and_then(|l| l.evm.clone()).unwrap_or_default(), active: true }
        } else {
            SignerRequest::SetAgentActive { active: true }
        };
        match self.signer_call(request).await {
            Ok(r) => {
                self.read_ledger_agent().await;
                let on = r.agent_active == Some(true) || self.agent_active();
                self.reply(&if on { self.say("Trading key on again for 7 days: orders skip Windows Hello, withdrawals still need it.", "Торговый ключ снова включён на 7 дней: ордера идут без Windows Hello, вывод по-прежнему с ним.") }
                           else { self.say("The trading key stayed off.", "Торговый ключ остался выключенным.") }, Some("Hyperliquid".into()));
            }
            Err(e) => self.reply(&(self.say("The trading key stayed off: ", "Торговый ключ остался выключенным: ") + &e.0), Some("Hyperliquid".into())),
        }
    }

    /// Hyperliquid isn't open on this wallet: it opens with the first USDC that lands there.
    fn offer_opening_hyperliquid(&self) {
        let (en, ru) = if self.ledger_active() { ("approval on the Ledger", "на Ledger") } else { ("Windows Hello", "Windows Hello") };
        self.reply(&self.say(&format!("Hyperliquid isn't open on this wallet yet. It opens with the first USDC that lands there: about $1 of it pays the opening fee, and the smallest order is $10, so bring at least $11. Then the trading key turns on with one {en}."),
                             &format!("Hyperliquid на этом кошельке ещё не открыт. Он открывается первыми USDC, которые туда придут: около $1 уходит на открытие, а самый маленький ордер — $10, так что нужно хотя бы $11. После этого торговый ключ включается одним подтверждением {ru}.")),
                   Some("Hyperliquid".into()));
        let solana_usdc = self.st().portfolio.networks.iter().find(|n| n.key == "solana")
            .and_then(|n| n.rows.iter().find(|r| r.symbol == "USDC")).map_or(Decimal::ZERO, |r| r.value);
        let can_move = solana_usdc >= Decimal::from(11);
        let mut options = Vec::new();
        if can_move {
            options.push(RouterOption::new(&self.say("Move 11 USDC to Hyperliquid", "Перевести 11 USDC на Hyperliquid"),
                                           &self.say("From Solana, the card waits for your Confirm", "С Solana, карточка ждёт «Подтвердить»"),
                                           "move 11 USDC from Solana to Hyperliquid", true));
        }
        options.push(RouterOption::new(&self.say("Top up ihatecrypto", "Пополнить ihatecrypto"),
                                       &self.say("From a browser wallet, your phone or an exchange", "Из кошелька в браузере, с телефона или с биржи"),
                                       &WalletCommand::Receive.summary(), !can_move));
        self.set_options(options);
    }

    /// Hyperliquid's first money on this wallet: the chat offers the key, once per address. Nothing signs until the card is
    /// picked and Windows Hello says so.
    pub fn offer_trading_key(&self) {
        if self.agent_address().is_some() || self.agent_active() { return }
        let evm = {
            let st = self.st();
            let hl = st.portfolio.networks.iter().find(|n| n.key == "hyperliquid").map_or(Decimal::ZERO, |n| n.usd);
            if st.watching() || st.onboarding || st.pending.is_some() || st.thinking || st.signing != 0 || hl <= Decimal::ZERO { return }
            let Some(evm) = st.active_wallet().and_then(|w| w.evm) else { return };
            evm.to_lowercase()
        };
        let offered = self.st().settings.trading_key_offered.clone().unwrap_or_default();
        if offered.contains(&evm) { return }
        self.set_settings(|s| s.trading_key_offered = Some([offered, vec![evm]].concat()));
        let ledger = self.ledger_active();
        let (without, one) = if ledger { ("the Ledger's buttons", "approval on the Ledger") } else { ("Windows Hello", "Windows Hello") };
        let (without_ru, one_ru) = if ledger { ("кнопок Ledger", "на Ledger") } else { ("Windows Hello", "Windows Hello") };
        self.reply(&self.say(&format!("Hyperliquid is open on this wallet. Turn on its trading key? Orders, stops and closes then go without {without} — new positions for 7 days, then one confirmation renews them. The key can't withdraw."),
                             &format!("Hyperliquid на этом кошельке открыт. Включить торговый ключ? Тогда ордера, стопы и закрытия идут без {without_ru} — новые позиции 7 дней, потом одно подтверждение продлевает. Вывести деньги ключ не может.")),
                   Some("Hyperliquid".into()));
        self.set_options(vec![
            RouterOption::new(&self.say("Turn on the trading key", "Включить торговый ключ"),
                              &self.say(&format!("One {one} · it can't withdraw"), &format!("Одно подтверждение {one_ru} · вывести не может")), TRADING_KEY_ON, true),
            RouterOption::new(&self.say("Not now", "Не сейчас"), &self.say("Settings › Wallet › Hyperliquid", "Настройки › Кошелёк › Hyperliquid"), TRADING_KEY_LATER, false),
        ]);
    }

    /// The offer's two cards; false for any other.
    pub async fn answer_trading_key(&self, command: &str) -> bool {
        if command == TRADING_KEY_ON { self.turn_on_trading_key().await; return true }
        if command != TRADING_KEY_LATER { return false }
        self.reply(&self.say("Left off. Settings › Wallet › Hyperliquid turns it on any time, or say «turn on the trading key».",
                             "Оставил выключенным. Включить можно в Настройках › Кошелёк › Hyperliquid или словами «включи торговый ключ»."), None);
        true
    }

    // MARK: ihatecrypto's fee

    /// Whether this account approved ihatecrypto's fee, for Settings (Workspace.loadFeeApproval).
    pub async fn read_fee_approval(&self) {
        let Some(user) = self.st().evm_address() else { self.st_mut().fee_approved = None; return };
        let approved = Executor::allowed_fee_of(&user).await.map(|f| f >= builder::FEE);
        if self.st().evm_address().as_deref() == Some(user.as_str()) { self.st_mut().fee_approved = approved }
        self.changed();
    }

    /// Takes back the approval (the signer asks first). The next trade confirmed asks for it again.
    pub async fn revoke_fee(&self) {
        let revoked = match self.executor() {
            Ok(x) => x.set_fee_approval(false).await.map_err(|e| e.to_string()),
            Err(e) => Err(e.0),
        };
        match revoked {
            Ok(()) => self.reply(&self.say("ihatecrypto's fee approval is taken back. New orders go without it, but stops and take-profits already on the exchange keep it until they fill or move. The next Hyperliquid trade you confirm asks for it again.",
                                           "Одобрение комиссии ihatecrypto отозвано. Новые ордера идут без неё, а стопы и тейки, уже стоящие на бирже, сохраняют её, пока не исполнятся или не сдвинутся. Следующая сделка на Hyperliquid, которую ты подтвердишь, спросит снова."),
                                 Some("Hyperliquid".into())),
            Err(why) => self.reply(&(self.say("The fee approval stayed: ", "Одобрение комиссии осталось: ") + &why), Some("Hyperliquid".into())),
        }
        self.read_fee_approval().await;
    }
}

#[cfg(test)]
#[path = "trading_key/tests.rs"]
mod tests;
