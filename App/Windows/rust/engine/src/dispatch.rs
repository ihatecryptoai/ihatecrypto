//! What the window asks (Engine.swift's `handle(_:)` and `run(_:)`): one request a line, each answered once — its result,
//! or the error the window shows — and followed by one state, unless it only reads. Every op goes to the method its Swift
//! case calls, in that method's module. Also Engine.swift's own `watch(solana:evm:)`.

use std::collections::HashMap;
use std::pin::Pin;
use std::time::Duration;

use ihatecrypto_core::chat_language::TokenMentions;
use ihatecrypto_core::charts::Range;
use ihatecrypto_core::intent::TradeIntent;
use ihatecrypto_core::tokens::TokenCommand;
use signer_api::{SignerAddress, SignerRequest, session};
use wallet::WalletError;
use wallet::address_check::{self, Network};

use crate::browser::BROWSER_WHILE_LOCKED;
use crate::canvas::{Board, LIVE_BOARDS};
use crate::engine::{ANSWERS_ONLY, Engine, Request};
use crate::ledger::LedgerAccount;
use crate::lock::WHILE_LOCKED;
use crate::multisig::MULTISIG_NETWORKS;
use crate::{store, task_seal};

type Answer = Result<HashMap<String, String>, WalletError>;

fn one(key: &str, value: String) -> HashMap<String, String> { HashMap::from([(key.to_string(), value)]) }

/// A method's future made in a frame of its own and awaited boxed: a debug build gives each arm's future its own slot in
/// `run`'s frame, which together outgrew a test thread's 2 MiB (and would Windows' 1 MiB main thread).
#[inline(never)]
fn boxed<'a, F: Future + 'a>(make: impl FnOnce() -> F) -> Pin<Box<dyn Future<Output = F::Output> + 'a>> { Box::pin(make()) }

/// `ChartCommand.Range(rawValue:) ?? .day`.
fn range(text: Option<&str>) -> Range {
    match text { Some("week") => Range::Week, Some("month") => Range::Month, _ => Range::Day }
}

impl Engine {
    /// One request from the window: answered with its result or its error, then a state unless it only reads.
    pub async fn handle(&self, r: Request) {
        match boxed(|| self.run(&r)).await {
            Ok(result) => self.out.answer(r.id, true, &result, None),
            Err(e) => self.out.answer(r.id, false, &HashMap::new(), Some(&e.0)),
        }
        // A question that only reads changes nothing: a new state after it would make the browser's toolbar ask again
        // (each state redraws it, and a redraw asks siteState), six times a second, dropping what's being typed.
        if !ANSWERS_ONLY.contains(&r.op.as_str()) { self.changed() }
    }

    async fn run(&self, r: &Request) -> Answer {
        let op = r.op.as_str();
        if self.st().lock.locked && !WHILE_LOCKED.contains(&op) && !BROWSER_WHILE_LOCKED.contains(&op) {
            return Err(WalletError(self.say("ihatecrypto is locked.", "ihatecrypto заблокирован.")));
        }
        let text = r.text.as_deref().unwrap_or("");
        let wallet = r.wallet.as_deref().unwrap_or("");
        let name = r.name.as_deref().unwrap_or("");
        let network = r.network.as_deref().unwrap_or("");
        let address = r.address.as_deref().unwrap_or("");
        let provider = r.provider.as_deref().unwrap_or("");
        let on = r.test == Some(true);
        match op {
            "unlock" => boxed(|| self.unlock(text)).await,
            "unlockHello" => boxed(|| self.unlock_with_hello(on)).await,
            "lockApp" => boxed(|| self.lock_app()).await,
            "setCode" => boxed(|| self.make_code(text)).await?,
            "clearCode" => boxed(|| self.turn_code_off()).await?,
            "lockAfter" => self.set_lock_after(r.seconds.unwrap_or(300) / 60),
            "hello" => {
                boxed(|| self.start_signer()).await;
                if !self.st().refreshing_every_minute { self.start_every_minute() }
                return Ok(HashMap::new());
            }
            "send" => {
                self.st_mut().composing = false;
                let boards = self.st().boards().clone();
                let find = |id: &str| boards.iter().find(|b| b.id == id).cloned();
                let mut also: Vec<Board> = r.boards.iter().flatten().filter(|id| Some(*id) != r.board.as_ref()).filter_map(|id| find(id)).collect();
                // Each «Ask in chat» chip on something that isn't a block (a coin, an address): a block of its own words.
                also.extend(r.abouts.iter().flatten().map(|a| Board { lines: vec![a.about.clone()], ..Board::new("result", &a.title) }));
                // The model and the parsers read «SOL»: the «$» only made the chip; the chat shows it as typed (the Mac's send).
                let plain = TokenMentions::plain(text);
                let shown = (plain != text).then_some(text);
                let step = r.step.and_then(|i| r.board.as_deref().and_then(find).and_then(|b| self.step_board(&b, i)));
                let about = if let Some(step) = step {
                    also.insert(0, step);
                    None
                } else {
                    r.board.as_deref()
                };
                self.st_mut().also_attached = also;
                boxed(|| self.send(&plain, shown, about)).await;
            }
            "stopModel" => self.stop_model(),
            "retry" => boxed(|| self.retry_move()).await,
            "requote" => self.requote(),
            "refreshModels" => boxed(|| self.refresh_models()).await,
            "downloadModel" => {
                let (me, model) = (self.rc(), r.model.clone().unwrap_or_default());
                tokio::task::spawn_local(async move { me.download_model(&model).await });
            }
            "startOllama" => self.start_ollama(),
            "getOllama" => self.get_ollama(),
            "lookUpContract" => return Ok(boxed(|| self.look_up_contract(address)).await),
            "blockCatalog" => return Ok(self.block_catalog()),
            "confirm" => boxed(|| self.confirm(r.board.as_deref(), r.ticks.as_deref().unwrap_or_default())).await,
            "siteGone" => self.site_gone(address),
            "cancel" => self.cancel_pending(r.board.as_deref()),
            "siteAnswer" => self.answer_site(r.board.as_deref(), on, r.ticks.as_deref().unwrap_or_default())?,
            "pick" => boxed(|| self.pick(r.index)).await,
            "refresh" => boxed(|| self.refresh()).await,
            "createWallet" => boxed(|| self.create_wallet(on)).await?,
            "phraseSaved" => self.st_mut().phrase = None,
            "importPhrase" => return boxed(|| self.import_phrase(r.phrase.as_deref().unwrap_or(""), r.passphrase.as_deref().unwrap_or(""))).await,
            "hlFunded" => return Ok(boxed(|| self.hl_funded()).await),
            "importKey" => boxed(|| self.import_key(r.key.as_deref().unwrap_or(""))).await?,
            "keyAddress" => return self.key_address(r.key.as_deref().unwrap_or("")),
            "phraseAccounts" => return self.phrase_accounts(r.phrase.as_deref().unwrap_or(""), r.passphrase.as_deref().unwrap_or("")),
            "multisigFees" => return Ok(boxed(|| self.multisig_fees()).await),
            "switchWallet" => boxed(|| self.use_wallet(wallet)).await?,
            "connectLedger" => {
                let me = self.rc();
                tokio::task::spawn_local(async move { me.connect_ledger().await });
            }
            "showOnLedger" => boxed(|| self.show_on_ledger(on)).await,
            "renameWallet" => {
                boxed(|| self.signer_call(SignerRequest::RenameWallet { id: wallet.into(), label: r.label.clone().unwrap_or_default() })).await?;
            }
            "removeWallet" => {
                if r.wallet.as_deref() == Some(LedgerAccount::ID) {
                    boxed(|| self.forget_ledger()).await
                } else {
                    boxed(|| self.signer_call(SignerRequest::RemoveWallet { id: wallet.into() })).await?;
                    self.wallet_removed(wallet);
                }
            }
            "lock" => {
                boxed(|| self.signer_call(SignerRequest::Lock {})).await?;
                boxed(|| self.lock_app()).await;
            }
            "sessionLength" => {
                boxed(|| self.signer_call(SignerRequest::SetSessionLength { seconds: r.seconds.unwrap_or(session::STANDARD) })).await?;
            }
            "saveAddress" => boxed(|| self.save_address(self.address_of(r))).await?,
            "removeAddress" => boxed(|| self.delete_address(name, network)).await?,
            "removeAlert" => self.set_alerts(|a| a.retain(|x| r.wallet.as_ref() != Some(&x.id.to_string()))),
            "removeTask" => self.set_tasks(|t| t.retain(|x| r.wallet.as_ref() != Some(&x.id.to_string()))),
            "taskOn" => self.set_task(wallet, on),
            "onboarding" => self.st_mut().onboarding = on,
            "composing" => {
                // The chat's field holds text (or no longer does): a scheduled run waits for it (the Mac's draft.isEmpty).
                self.st_mut().composing = on;
                return Ok(HashMap::new());
            }
            "windowFocus" => {
                let front = r.test.unwrap_or(true);
                let mut st = self.st_mut();
                if st.window_front != front {
                    st.window_front = front;
                    if !front { st.window_left = chrono::Utc::now() }
                }
                return Ok(HashMap::new());
            }
            "taskPays" => self.set_from_active(wallet, on),
            "createAutomationWallet" => boxed(|| self.create_automation_wallet(None)).await,
            "addSkill" => self.add_skill(text, r.name.as_deref().unwrap_or("skill.md"))?,
            "removeSkill" => {
                self.set_added_skills(|s| s.retain(|x| r.wallet.as_ref() != Some(&x.id)));
                self.set_skills_off(|s| { s.remove(wallet); });
            }
            "openStudyFiles" => self.open_study_files(on)?,
            "skillOn" => self.set_skills_off(|s| if on { s.remove(wallet); } else { s.insert(wallet.into()); }),
            "forget" => self.set_memory(|m| m.retain(|x| r.text.as_ref() != Some(x))),
            "removeFavorite" => self.set_favorites(|f| f.retain(|x| r.wallet.as_ref() != Some(&x.id()))),
            "showToken" => self.show_token(address, on),
            "openToken" => return Ok(boxed(|| self.open_token(address, network)).await),
            "tokenRange" => if let Some(id) = &r.board { boxed(|| self.load_token(id, range(r.text.as_deref()))).await },
            "openChart" => return boxed(|| self.open_chart(name, range(r.text.as_deref()), r.network.clone())).await,
            "refreshApprovals" => if let Some(id) = &r.board { boxed(|| self.refresh_approvals(id)).await },
            "checkAddress" => return Ok(self.check_address(address, network, r.label.as_deref())),
            "editAddress" => boxed(|| self.edit_address((r.label.as_deref().unwrap_or(""), text), self.address_of(r))).await?,
            "addressExtra" => self.set_address_extra(name, network, r.key.as_deref(), r.label.as_deref(), r.text.as_deref())?,
            "addressIcon" => self.set_address_extra(name, network, Some(text), None, None)?,
            "transitState" => return Ok(one("state", boxed(|| self.transit_state(name, address)).await)),
            "lookToken" => return Ok(boxed(|| self.look_token(address)).await),
            "addFavorite" => boxed(|| self.token_command(TokenCommand::Favorite { address: address.into(), chain: r.network.clone() }, None)).await,
            "schedulePause" => self.pause_schedule(on),
            "notice" => self.set_notice(name, on),
            "noticeTest" => self.notice_test(),
            "openAtLogin" => self.set_open_at_login(on),
            "thinking" | "checkModel" | "installClaudeCode" | "claudeSignIn" | "installOllama" | "ollamaStatus" | "setUpLocal" | "keepRunning"
            | "keepRunningState" => return boxed(|| self.local_op(r)).await,
            // main.rs, at launch and after each change: how Windows has it now.
            "loginState" => self.set_settings(|s| s.open_at_login = Some(on)),
            "updateReady" => self.update_ready(text),
            "crashed" => self.crashed(name, text, address),
            "openSchedule" => self.open_schedule(name)?,
            "closeSchedule" => self.close_schedule()?,
            "runNow" => boxed(|| self.run_now(name)).await,
            "star" => {
                let found = self.st().book.iter().find(|e| Some(&e.name) == r.name.as_ref() && Some(&e.network) == r.network.as_ref()).cloned();
                let Some(e) = found else { return Ok(HashMap::new()) };
                self.star(&e, on, false);
            }
            "connectModel" => boxed(|| self.connect(provider, r.key.as_deref(), r.model.as_deref())).await?,
            "signIn" => boxed(|| self.sign_in(provider)).await?,
            "useLogin" => self.use_login(provider)?,
            "disconnectModel" => self.disconnect(provider),
            "setModel" => self.choose_model(r.model.as_deref()),
            // Another account's model, kept for it without switching the chat (the Mac's modelChoices[id]).
            "modelFor" => self.set_model(r.model.as_deref(), &r.provider.clone().unwrap_or_else(|| self.provider_id())),
            "useAccount" => self.use_account(&r.provider.clone().unwrap_or_else(|| self.provider_id()), r.model.as_deref()),
            "signInJev" => boxed(|| self.sign_in_jev()).await?,
            "language" => self.set_settings(|s| s.language = r.language.clone().filter(|l| l != "auto")),
            "secondCheck" => self.set_settings(|s| s.second_check = Some(r.test.unwrap_or(true))),
            "jevOpenRouter" => self.set_settings(|s| s.jev_open_router = Some(r.test.unwrap_or(false))),
            "module" => self.set_module(r.name.as_deref(), on)?,
            "policy" => self.set_policy(r.policy.clone())?,
            "safeSearch" => self.set_settings(|s| s.safe_search = (r.test == Some(false)).then_some(false)),
            "batches" => self.set_settings(|s| s.batches = on.then_some(true)),
            "contractCap" => self.set_contract_cap(r.value)?,
            "tradingKey" => {
                let me = self.rc();
                tokio::task::spawn_local(async move { me.turn_on_trading_key().await });
            }
            "revokeFee" => {
                let me = self.rc();
                tokio::task::spawn_local(async move { me.revoke_fee().await });
            }
            "readFee" => boxed(|| self.read_fee_approval()).await,
            "node" => boxed(|| self.set_node(r.network.as_deref(), r.address.as_deref())).await?,
            "oneClickKey" => boxed(|| self.set_one_click_key(r.key.as_deref())).await?,
            "watch" => self.watch(r.solana.as_deref(), r.evm.as_deref())?,
            // From the window's browser, which read the origin from the webview itself.
            "dapp" => {
                let params = r.text.as_deref().unwrap_or("{}");
                return Ok(one("json", boxed(|| self.dapp(name, params, address, r.label.as_deref().unwrap_or(""))).await?));
            }
            "siteState" => return Ok(boxed(|| self.site_state(address)).await),
            "newCanvas" => self.new_canvas(false)?,
            "openCanvas" => self.open_canvas(wallet)?,
            "renameCanvas" => self.rename_canvas(wallet, r.label.as_deref().unwrap_or("")),
            "removeCanvas" => self.remove_canvas(wallet)?,
            "pinCanvas" | "moveCanvas" | "createFolder" | "renameFolder" | "deleteFolder" | "newCanvasIn" | "setIcon" | "siteIcon" => {
                return boxed(|| self.folder_op(r)).await;
            }
            "addBoard" => {
                let Some(kind) = r.name.as_deref().filter(|k| LIVE_BOARDS.contains(k)) else {
                    return Err(WalletError(format!("Unknown block «{name}».")));
                };
                self.pin(Board::new(kind, kind));
            }
            "editStep" => boxed(|| self.edit_step(r.board.as_deref(), r.text.as_deref(), r.name.as_deref())).await?,
            "whyStep" => self.why_step(r.board.as_deref(), r.index)?,
            "planMarkets" | "planMarket" | "planTake" | "planCancel" => return Ok(boxed(|| self.plan_op(r)).await),
            "closeBoard" => self.close_board(r.board.as_deref())?,
            "multisigKey" => self.multisig_key(r.address.as_deref(), r.wallet.as_deref())?,
            "multisigThreshold" => self.update_multisig_draft(|d| d.threshold = r.threshold.unwrap_or(d.threshold)),
            "multisigNetwork" => self.update_multisig_draft(|d| {
                let Some(n) = r.network.as_ref().filter(|n| MULTISIG_NETWORKS.contains(&n.as_str())) else { return };
                if d.networks.contains(n) {
                    if d.networks.len() > 1 { d.networks.retain(|x| x != n) }
                } else {
                    d.networks.push(n.clone());
                }
            }),
            "clearChat" => {
                {
                    let mut st = self.st_mut();
                    st.messages_mut().clear();
                    st.loaded_skills.clear();
                }
                self.cancel_pending(None);
                self.st_mut().intent = TradeIntent::default();
                self.save_chat();
            }
            "topUp" => self.top_up(r.network.as_deref(), r.text.as_deref(), on)?,
            "topUpOpen" => return Ok(one("url", boxed(|| self.top_up_open(r.network.as_deref(), r.text.as_deref(), r.label.as_deref())).await?)),
            "topUpStop" => self.top_up_stop(),
            "historyTabs" => return Ok(one("json", boxed(|| self.history_tabs()).await)),
            "appLanguage" => self.set_settings(|s| s.app_language = r.language.clone().filter(|l| l != "system")),
            // The window lets the signer come forward when Windows Hello asks; only the window may hand that on.
            "signerPid" => return Ok(one("pid", self.signer.pid().map(|p| p.to_string()).unwrap_or_default())),
            _ => return Err(WalletError(format!("Unknown request «{op}»."))),
        }
        Ok(HashMap::new())
    }

    fn address_of(&self, r: &Request) -> SignerAddress {
        let s = |v: &Option<String>| v.clone().unwrap_or_default();
        SignerAddress { name: s(&r.name), network: s(&r.network), address: s(&r.address), memo: s(&r.memo) }
    }

    /// `hello`'s first time: the tasks changed outside said once, the new modules offered, the alerts watched, and the
    /// balances followed while the window is open, every 30 s as the Mac's («Runs within half a minute»).
    fn start_every_minute(&self) {
        let (tampered, all_paused) = {
            let st = self.st();
            let file = store::scoped(task_seal::TASKS, st.wallet_scope_tasks.as_deref());
            (task_seal::load(&file, task_seal::KEY).1, st.tasks.iter().all(|t| t.paused == Some(true)))
        };
        if tampered && all_paused {
            self.reply(&self.say("The scheduled tasks were changed outside ihatecrypto, so every one is paused. Check them in the Schedule and switch on only the ones you made.",
                                 "Задачи по расписанию изменили вне ihatecrypto, поэтому все они на паузе. Проверь их в «Расписании» и включи только свои."), None);
        }
        self.offer_new_modules();
        self.watch_alerts();
        self.st_mut().refreshing_every_minute = true;
        let me = self.rc();
        tokio::task::spawn_local(async move {
            loop {
                me.refresh().await;
                // After the read, as the Mac's watch(): a stop's Windows Hello never holds a refresh.
                me.guard_positions().await;
                me.offer_trading_key();
                me.refresh_live().await;
                me.read_closed().await;
                me.start_scheduled_run();
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });
    }

    fn close_board(&self, board: Option<&str>) -> Result<(), WalletError> {
        let (card, running, kind) = {
            let st = self.st();
            (st.card.as_ref().map(|c| c.id.clone()), st.running_board.clone(), st.boards().iter().find(|b| Some(b.id.as_str()) == board).map(|b| b.kind.clone()))
        };
        // Swift compares the optionals: no board named and no card waiting count as the same.
        if board == card.as_deref() || board == running.as_deref() {
            return Err(WalletError(self.say("Confirm or cancel the move first.", "Сначала подтверди или отмени перевод.")));
        }
        // A move stays on its canvas (the Mac's EarlierBoards): only what the chat drew or a block added by hand closes.
        if kind.as_deref() == Some("move") { return Err(WalletError(self.say("A move stays on its canvas.", "Перевод остаётся на холсте."))) }
        // The Ledger's block closed: its search stops (ledger.rs's loop reads it), as the Mac sets ledgerConnect nil.
        if kind.as_deref() == Some("ledger") { self.st_mut().ledger_connect = None }
        self.set_boards(|b| b.retain(|x| Some(x.id.as_str()) != board));
        Ok(())
    }

    /// «Addresses for balances»: each checked as an address of its chain; an empty one stops watching it.
    pub fn watch(&self, solana: Option<&str>, evm: Option<&str>) -> Result<(), WalletError> {
        let (s, e) = (solana.unwrap_or("").trim(), evm.unwrap_or("").trim());
        if !s.is_empty() && let Some(problem) = address_check::problem(s, Network::Solana) { return Err(WalletError(problem)) }
        if !e.is_empty() && let Some(problem) = address_check::problem(e, Network::Base) { return Err(WalletError(problem)) }
        self.set_settings(|st| {
            st.watch_solana = (!s.is_empty()).then(|| s.to_string());
            st.watch_evm = (!e.is_empty()).then(|| e.to_string());
        });
        self.forget_wallet_reads();
        self.load_equity();
        let me = self.rc();
        tokio::task::spawn_local(async move { me.refresh().await });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
