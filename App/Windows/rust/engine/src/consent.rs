//! A site's request and a doubted trade waiting for the person's word (Consent.swift): what they need ticked, the
//! request as its own block, and declining it (here because the lock declines one).
//! Layer 2: answering a site's request (answer_site, site_gone, decline_site_on_canvas, expire_site), the ticks a site's
//! request and a doubted trade need, another wallet signing (status_heard, wallet_changed), a card picked in the chat
//! (pick), and what Settings › Modules refuses at Confirm (confirm_refusal).

use std::time::Duration;

use ihatecrypto_core::modules::ModuleNetwork;
use ihatecrypto_core::position_command::WalletCommand;
use serde::Serialize;
use signer_api::SignerReply;
use wallet::WalletError;

use crate::canvas::Board;
use crate::chains::ChainState;
use crate::engine::{Engine, Message, Pending};
use crate::moves::{LEAVE_COSTLY, SEND_ANYWAY};

/// What a site's request still needs the person's word for before it can be approved (the Mac's ApprovalSheet `ask`):
/// a lookalike address, programs off the list, a payee or contract that isn't theirs, the second check's concern.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteTick {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub toggle: String,
}

/// A site's request as the window draws it: apart from the person's own card, with its whole origin, and answered by
/// its id (`siteAnswer`), never by the move card's Confirm.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteAskView {
    pub id: String,
    pub origin: String,
    pub host: String,
    /// «known», «lookalike» (of `real`), «unknown», «local».
    pub check: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real: Option<String>,
    pub title: String,
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    /// Every one must be ticked for Approve to count.
    pub ticks: Vec<SiteTick>,
    /// «Connect» or «Sign».
    pub approve: String,
    /// The canvas its block is on: «Review on the canvas» opens that one.
    pub canvas: String,
    /// A Solana transaction as the Mac's sheet reads it: «You send 5 USDC to Kamino Vault», each balance change with its
    /// ≈ $, the checks folded to one line, and what happens after signing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
    pub balances: Vec<SiteBalance>,
    pub checks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One balance a site's transaction changes, simulated: «−5» USDC ≈ $5.00; SOL that only pays the fee and rent says so.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SiteBalance {
    pub symbol: String,
    pub amount: String,
    pub up: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The first and last characters of `s`, by Character as Swift's prefix/suffix.
fn short(s: &str, head: usize, tail: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    format!("{}…{}", c.iter().take(head).collect::<String>(), c[c.len().saturating_sub(tail)..].iter().collect::<String>())
}

impl Engine {
    // MARK: A site's request (A1, A2)

    /// The person's answer to the site's request with this id, the one its card showed; any other is left alone. An
    /// approval counts only with every tick the request needs (the Mac's canApprove).
    pub fn answer_site(&self, id: Option<&str>, yes: bool, ticks: &[String]) -> Result<(), WalletError> {
        let missing = {
            let st = self.st();
            let Some(ask) = st.site_ask.as_ref().filter(|a| Some(a.view.id.as_str()) == id) else { return Ok(()) };
            if yes { ask.view.ticks.iter().find(|t| !ticks.contains(&t.id)).map(|t| t.toggle.clone()) } else { None }
        };
        if let Some(toggle) = missing {
            return Err(WalletError(self.say(&format!("Tick «{toggle}» first, or decline."), &format!("Сначала отметь «{toggle}» или откажи."))));
        }
        let Some(mut ask) = self.st_mut().site_ask.take() else { return Ok(()) };
        let declined = self.say("Declined: nothing was signed.", "Отказано: ничего не подписано.");
        self.update_board(Some(&ask.view.id), |b| {
            b.state = Some(if yes { "done" } else { "cancelled" }.into());
            if !yes { b.lines.push(declined) }
        });
        ask.answer(yes);
        self.changed();
        Ok(())
    }

    /// The page that asked is gone (its webview closed or hidden for good, another origin loaded, the app locked): its
    /// request is declined, as the Mac's sheet goes with its view. From the window (main.rs, `siteGone`).
    pub fn site_gone(&self, origin: &str) {
        let bare = |o: &str| o.to_lowercase().trim_matches('/').to_string();
        let same = self.st().site_ask.as_ref().is_some_and(|a| bare(&a.view.origin) == bare(origin));
        if !same { return }
        self.decline_site(&self.say("The page closed before an answer: nothing was signed.", "Страница закрылась до ответа: ничего не подписано."));
    }

    /// A canvas going away with the site's request on it: declined, since its ticks were only there to answer.
    pub fn decline_site_on_canvas(&self, boards: &[Board]) {
        let on = self.st().site_ask.as_ref().is_some_and(|a| boards.iter().any(|b| b.id == a.view.id));
        if !on { return }
        self.decline_site(&self.say("Its canvas was deleted: nothing was signed.", "Его холст удалён: ничего не подписано."));
    }

    /// The window waits 900 s for an answer to the page; the request is declined a little before, so an Approve never
    /// signs for a page that already got its error.
    pub async fn expire_site(&self, id: &str) {
        tokio::time::sleep(Duration::from_secs(880)).await;
        if self.st().site_ask.as_ref().map(|a| a.view.id.as_str()) != Some(id) { return }
        self.decline_site(&self.say("No answer for 15 minutes: declined, nothing was signed.", "15 минут без ответа: отклонено, ничего не подписано."));
    }

    // MARK: A trade the second check doubts (S2)

    /// The Mac's Execute sheet toggle: Sign stays grey until it's on.
    pub fn trade_tick(concern: &str, say: impl Fn(&str, &str) -> String) -> SiteTick {
        SiteTick { id: "doubt".into(), title: say("The second check has a concern", "У второй проверки есть сомнение"), detail: concern.into(),
                   toggle: say("I understand the second check's warning and execute anyway", "Понимаю предупреждение второй проверки и выполняю всё равно") }
    }

    /// What the waiting card still needs ticked, said; None when Confirm may go.
    pub fn unticked_on_card(&self, ticks: &[String]) -> Option<String> {
        let toggle = self.st().card.as_ref()?.ticks.iter().find(|t| !ticks.contains(&t.id))?.toggle.clone();
        Some(self.say(&format!("Tick «{toggle}» on the card first, or cancel it."), &format!("Сначала отметь на карточке «{toggle}» или отмени её.")))
    }

    /// The site's request waiting is answered no, its block cancelled with `why`.
    pub fn decline_site(&self, why: &str) {
        let Some(mut ask) = self.st_mut().site_ask.take() else { return };
        self.update_board(Some(&ask.view.id), |b| {
            b.state = Some("cancelled".into());
            b.lines.push(why.into());
        });
        ask.answer(false);
        self.changed();
    }

    pub fn lookalike_tick(real: &str, say: impl Fn(&str, &str) -> String) -> SiteTick {
        SiteTick { id: "lookalike".into(),
                   title: say(&format!("The address looks like {real}, but it's a different site — a common phishing trick."),
                              &format!("Адрес похож на {real}, но это другой сайт — частый приём фишинга.")),
                   detail: say("Check the address bar letter by letter.", "Проверь адрес в строке по буквам."),
                   toggle: say(&format!("I understand this isn't {real}"), &format!("Понимаю, что это не {real}")) }
    }

    pub fn jev_tick(text: &str, say: impl Fn(&str, &str) -> String) -> SiteTick {
        SiteTick { id: "jev".into(), title: say("The second check has a concern", "У второй проверки есть сомнение"), detail: text.into(),
                   toggle: say("I understand the second check's warning", "Понимаю предупреждение второй проверки") }
    }

    /// An EVM transaction's payee or contract that isn't the user's (the Mac's evmTransaction sheet): a contract it can't
    /// read when that contract is all it pays, else the addresses money or the right to spend it goes to.
    pub fn payee_tick(to: &str, recipients: &[String], unread: bool, say: impl Fn(&str, &str) -> String) -> Option<SiteTick> {
        if unread && recipients == [to.to_lowercase()] {
            return Some(SiteTick {
                id: "contract".into(), title: say("ihatecrypto can't read what this contract does", "ihatecrypto не может прочитать, что делает этот контракт"),
                detail: short(to, 10, 6) + &say(" — check it on the site. The signer asks for Windows Hello once for a contract it hasn't seen.",
                                                " — проверь его на сайте. Для контракта, которого он ещё не видел, подписант один раз спросит Windows Hello."),
                toggle: say("Allow this contract", "Разрешить этот контракт"),
            });
        }
        if recipients.is_empty() { return None }
        Some(SiteTick {
            id: "payee".into(), title: say("Money, or the right to spend it, goes to an address that isn't yours", "Деньги или право их тратить уходят на чужой адрес"),
            detail: recipients.iter().map(|a| short(a, 10, 6)).collect::<Vec<_>>().join(", ")
                + &say(" — check it on the site. The signer asks for Windows Hello for a new one.", " — проверь на сайте. Для нового адреса подписант спросит Windows Hello."),
            toggle: say("Send to this address", "Отправить на этот адрес"),
        })
    }

    pub fn programs_tick(unknown: &[String], say: impl Fn(&str, &str) -> String) -> Option<SiteTick> {
        if unknown.is_empty() { return None }
        let names = unknown.iter().map(|p| short(p, 4, 4)).collect::<Vec<_>>().join(", ");
        let one = unknown.len() == 1;
        Some(SiteTick {
            id: "programs".into(),
            title: say(&format!("{names} {} on ihatecrypto's list", if one { "isn't" } else { "aren't" }), &format!("{names}: нет в списке ihatecrypto")),
            detail: say(&format!("Signing calls {} the way the site built the transaction; the balances above are the simulated result.", if one { "it" } else { "them" }),
                        &format!("Подпись вызовет {} так, как сайт собрал транзакцию; балансы выше — результат симуляции.", if one { "её" } else { "их" })),
            toggle: say(&format!("Allow {} for this one transaction", if one { "it" } else { "them" }), "Разрешить для этой транзакции"),
        })
    }

    // MARK: Another wallet signs (A4)

    /// What the signer said of itself: when another wallet signs now (switched, made, imported, removed), nothing built
    /// for the last one carries over.
    pub fn status_heard(&self, reply: &SignerReply) {
        let active = reply.active_wallet.clone();
        let before = self.st_mut().status.replace(reply.clone());
        // Each wallet's own tasks and recorded costs, at launch too (the Mac's Wallets.swift).
        self.load_wallet_state();
        if let Some(before) = before && before.active_wallet != active { self.wallet_changed() }
    }

    /// Nothing built for the last wallet carries over (the Mac's walletChanged): the waiting card, cards parked on other
    /// canvases, a chain holding its steps, a «How much?», a «Send anyway» and a site's request go; balances are read again.
    pub fn wallet_changed(&self) {
        let gone = self.say("Built for the other wallet: nothing was signed.", "Собрано для другого кошелька: ничего не подписано.");
        let card = self.st().card.as_ref().map(|c| c.id.clone());
        if card.is_some() {
            let gone = gone.clone();
            self.update_board(card.as_deref(), |b| {
                b.state = Some("cancelled".into());
                b.lines.push(gone);
            });
        }
        {
            let mut st = self.st_mut();
            if matches!(st.pending, Some(Pending::Wallet(WalletCommand::MultisigCreate { .. }))) { st.multisig_draft = None }
            st.pending = None;
            st.card = None;
            st.parked.clear();
            st.chain_state = ChainState::default();
            st.awaiting_amount = None;
            st.costly = None;
            st.retry = None;
            st.call_review = None;
            st.multisig_payment = None;
            st.route_way = None;
        }
        self.set_way_quotes(None);
        self.decline_site(&gone);
        self.forget_wallet_reads();
        self.load_equity();
        self.changed();
    }

    // MARK: A card picked (A8)

    /// A card picked in the chat (the Mac's pick): said by its title and carried out as the command it is, against the
    /// words of the message it answered — no model reads it again. A card with no command is the user's answer to the
    /// model, and goes to it as typed. Like any new request it drops a card that waited for Confirm.
    pub async fn pick(&self, index: Option<i64>) {
        let i = {
            let st = self.st();
            let Some(i) = index.and_then(|i| usize::try_from(i).ok()).filter(|&i| i < st.options.len()) else { return };
            if st.thinking { return }
            i
        };
        self.mark_picked(i);
        let (o, user_text) = {
            let st = self.st();
            let Some(o) = st.options.get(i).cloned() else { return };
            // The words of the message these cards answered; cards offered some other way bring none of the user's words.
            (o, st.offered_for.as_ref().filter(|f| f.options == st.options).map(|f| f.text.clone()).unwrap_or_default())
        };
        self.st_mut().offered_for = None;
        self.set_options(vec![]);
        if self.answer_new_modules(&o.command) { return }
        if self.answer_trading_key(&o.command).await { return }
        if o.command == SEND_ANYWAY {
            self.send_anyway().await;
            return;
        }
        if o.command == LEAVE_COSTLY {
            self.st_mut().costly = None;
            self.reply(&self.say("Left as it was: nothing was signed.", "Оставил как есть: ничего не подписано."), None);
            return;
        }
        if o.command.is_empty() {
            self.send(&o.title, None, None).await;
            return;
        }
        if self.st().card.is_some() { self.cancel_pending(None) }
        // Like any new message, it drops what waited: steps held behind a card, a «How much?» (the Mac's pick).
        {
            let mut st = self.st_mut();
            st.chain_state.after_call = None;
            st.chain_state.held = None;
            st.awaiting_amount = None;
            st.messages_mut().push(Message::new(true, &o.title, None));
            st.agent_goal = None;
        }
        self.name_canvas(&o.title);
        self.save_chat();
        self.st_mut().thinking = true;
        self.changed();
        if !self.opening_route_command(&o.command).await && !self.run_canonical(&o.command, &user_text).await {
            self.reply(&self.say(&format!("ihatecrypto can't do “{}” yet — nothing moved.", o.command), &format!("ihatecrypto пока не умеет «{}» — ничего не ушло.", o.command)), None);
        }
        // Swift's defer.
        self.st_mut().thinking = false;
        self.changed();
    }

    // MARK: Modules at Confirm (A9)

    /// What Settings › Modules switched off since the card was drawn: refused at Confirm too (the Mac's prepareExecution
    /// and transfer). None when it may go.
    pub fn confirm_refusal(&self, waiting: &Pending) -> Option<String> {
        let ru = self.st().russian();
        match waiting {
            Pending::Trade(p) => trade::modules().refusal_trade(p.market.dex.is_some(), p.bridge.is_some() || p.fund_from.is_some() || p.open_via_1click.is_some(), ru),
            Pending::Transfer(entry, _) => ModuleNetwork::from_place(&entry.network).and_then(|n| trade::modules().refusal_network(n, ru)),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "consent/tests.rs"]
mod tests;
