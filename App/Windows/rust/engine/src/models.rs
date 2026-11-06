//! The chat's model and the accounts it can go through (Models.swift): the same providers as the Mac (`llm::catalog`),
//! keys and sign-ins kept with DPAPI (`store::credentials`). A browser sign-in (ChatGPT, OpenRouter's OAuth) is caught
//! on 127.0.0.1; Claude Code and Codex are found where Windows has them (`llm`'s existing_login).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use llm::catalog::{self, Kind, Provider};
use llm::{Client, Credential, Role, Usage};
use serde::Serialize;
use wallet::WalletError;

use crate::engine::Engine;
use crate::store;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub connected: bool,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub takes_key: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_login: Option<String>,
    /// «Sign in with ChatGPT»: a browser sign-in, caught on 127.0.0.1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign_in: Option<String>,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    pub connected: bool,
    /// Its models for the menu (models(of:)) and the one it starts with.
    pub models: Vec<String>,
    pub default_model: String,
    /// The model chosen for this account, kept per account (the model menu's «<model> · connected»).
    pub chosen: String,
    /// Each model's name as the Mac's LLMCatalog.displayName writes it («GPT 5.4», «Qwen3 8B»), the chosen one too.
    pub names: HashMap<String, String>,
}

/// A refreshed login is kept as the first one was.
fn saver(id: &str) -> llm::Save {
    let id = id.to_string();
    Arc::new(move |c| store::set_credential(Some(c), &id))
}

fn failed(e: llm::Error) -> WalletError { WalletError(e.to_string()) }

impl Engine {
    pub fn provider_id(&self) -> String { self.st().settings.provider.clone().unwrap_or_else(|| "anthropic".into()) }

    pub fn provider(&self, id: &str) -> Option<Provider> { catalog::provider(id) }

    /// Swift's `var model`: the chat's account's model.
    pub fn model(&self) -> String { self.model_of(&self.provider_id()) }

    /// Swift's `model(of:)`: the model chosen for an account, kept per account as on the Mac (`modelChoices`): switching
    /// away and back brings it back. A subscription that no longer offers it (a lapsed plan answers 400 for it) has its
    /// first listed model stand in, and the choice comes back once the list has it again.
    /// ponytail: the live list is what the provider listed last; a choice it dropped from a long list is stood in for too.
    pub fn model_of(&self, id: &str) -> String {
        let Some(info) = self.provider(id).map(|p| p.info()) else { return String::new() };
        let st = self.st();
        let chosen = st.settings.models.as_ref().and_then(|m| m.get(id)).cloned()
            .or_else(|| if st.settings.provider.as_deref() == Some(id) { st.settings.model.clone() } else { None })
            .unwrap_or(info.default_model);
        if info.kind == Kind::Subscription && let Some(live) = st.live_models.get(id) && let Some(first) = live.first() && !live.contains(&chosen) {
            return first.clone();
        }
        chosen
    }

    /// The model for an account; None goes back to its default.
    pub fn set_model(&self, model: Option<&str>, id: &str) {
        self.set_settings(|s| {
            let mut all = s.models.take().unwrap_or_default();
            match model.filter(|m| !m.is_empty()) {
                Some(m) => { all.insert(id.into(), m.into()); }
                None => { all.remove(id); }
            }
            s.models = (!all.is_empty()).then_some(all);
            if s.provider.as_deref() == Some(id) { s.model = None }
        });
    }

    /// The model menu's or Settings' «Use»: the chat's account goes on with this model, on this canvas too.
    pub fn choose_model(&self, model: Option<&str>) {
        let id = self.provider_id();
        self.set_model(model, &id);
        self.canvas_talked(&id);
    }

    /// The chat goes on with this account, as the profile menu's and Settings' «Switch» do (the Mac's useAccount): its
    /// model stays as chosen for it, and on this PC the first downloaded chat model stands in for one that isn't there.
    pub fn use_account(&self, id: &str, model: Option<&str>) {
        let Some(info) = self.provider(id).map(|p| p.info()) else { return };
        if info.unavailable.is_some() || !(store::credentials().contains_key(id) || info.kind == Kind::Local) { return }
        self.set_settings(|s| s.provider = Some(id.into()));
        if let Some(m) = model { self.set_model(Some(m), id) }
        let live = self.st().live_models.get(id).cloned();
        if info.kind == Kind::Local && let Some(live) = live && !live.contains(&self.model())
            && let Some(first) = live.iter().find(|m| catalog::chat_refusal(m).is_none()) {
            self.set_model(Some(first), id);
        }
        self.canvas_talked(id);
    }

    /// The account's client, «Think before answering» applied, and its id; None when it isn't connected.
    fn made_client(&self, asked: Option<&str>) -> Option<(Client, String)> {
        let p = self.provider(&asked.map_or_else(|| self.provider_id(), str::to_string))?;
        let info = p.info();
        if info.unavailable.is_some() { return None }
        let credential = store::credentials().remove(&info.id);
        if credential.is_none() && info.kind != Kind::Local { return None }
        let made = p.client(credential.as_ref(), saver(&info.id)).ok()?;
        // Settings › AI model › «Think before answering»: every request reasons first (the Mac's ThinkingClient).
        let thinking = self.st().settings.thinking == Some(true);
        Some((if thinking { made.thinking() } else { made }, info.id))
    }

    /// The model client (the chat's account, or `asked`'s for Settings' Check), or None when it isn't connected: then
    /// the chat reads plain commands only. Every answer says what it read: the chip over the chat's field, as the Mac's
    /// ContextMeter, counted on the engine's thread (Swift's `Task { @MainActor in … counted }`).
    /// ponytail: counted on the canvas open when the answer ends; a scheduled check answering in the background counts there too.
    pub fn llm_client(&self, asked: Option<&str>) -> Option<Client> {
        let (client, id) = self.made_client(asked)?;
        let (tell, mut heard) = tokio::sync::mpsc::unbounded_channel::<(Usage, llm::Request)>();
        let me = Rc::downgrade(&self.rc());
        tokio::task::spawn_local(async move {
            while let Some((usage, request)) = heard.recv().await {
                let Some(me) = me.upgrade() else { return };
                me.counted(usage, request, &id).await;
            }
        });
        Some(client.metered(Arc::new(move |u: &Usage, r: &llm::Request| { let _ = tell.send((u.clone(), r.clone())); })))
    }

    /// A model that reads the chat: connected, big enough for its brief, and, on this PC, its server answering with the
    /// model downloaded (the Mac's hasModel).
    pub fn has_model(&self) -> bool {
        let Some(info) = self.provider(&self.provider_id()).map(|p| p.info()) else { return false };
        let model = self.model();
        if self.made_client(None).is_none() || catalog::chat_refusal(&model).is_some() { return false }
        if info.kind != Kind::Local { return true }
        let st = self.st();
        st.local_servers.get(&info.id) != Some(&false) && st.live_models.get(&info.id).is_none_or(|l| l.contains(&model))
    }

    pub fn model_view(&self) -> ModelView {
        let info = self.provider(&self.provider_id()).map(|p| p.info());
        let model = self.model();
        ModelView {
            provider: info.as_ref().map(|i| i.id.clone()), name: info.as_ref().map(|i| i.name.clone()), model: (!model.is_empty()).then_some(model),
            connected: self.has_model(), models: self.models(info.as_ref().map_or("", |i| i.id.as_str())),
        }
    }

    /// Swift's `models(of:)`: a way in's models for the menu: the live list where there is one (a subscription now, what
    /// a server here has, the suggested ones it hasn't after it), else the checked list; the model in use last when
    /// neither lists it.
    pub fn models(&self, id: &str) -> Vec<String> {
        let Some(info) = self.provider(id).map(|p| p.info()) else { return vec![] };
        let live = self.st().live_models.get(id).cloned();
        let mut listed = live.clone().unwrap_or_else(|| info.models.clone());
        if info.kind == Kind::Local && live.is_some() {
            let more: Vec<String> = info.models.iter().filter(|m| !listed.contains(m)).cloned().collect();
            listed.extend(more);
        }
        let model = self.model();
        if id == self.provider_id() && !model.is_empty() && !listed.contains(&model) { listed.push(model) }
        listed
    }

    pub fn provider_views(&self) -> Vec<ProviderView> {
        let saved = store::credentials();
        catalog::all().into_iter().filter(|p| p.info().id != "apple").map(|p| {
            let info = p.info();
            let (models, chosen) = (self.models(&info.id), self.model_of(&info.id));
            let mut names = HashMap::new();
            for m in models.iter().chain([&chosen]) { names.entry(m.clone()).or_insert_with(|| catalog::display_name(m)); }
            // The catalog is the Mac's: «Ollama on this Mac» runs on this PC here.
            ProviderView {
                name: info.name.replace("this Mac", "this PC"), kind: info.kind.as_str().into(), takes_key: info.takes_key,
                existing_login: if p.existing_login().is_some() { info.existing_login_label.clone() } else { None }, sign_in: info.sign_in_label.clone(),
                note: info.note.replace("this Mac", "this PC"), unavailable: info.unavailable.clone(),
                connected: saved.contains_key(&info.id) || info.kind == Kind::Local, models, default_model: info.default_model.clone(), chosen, names,
                id: info.id,
            }
        }).collect()
    }

    /// A key typed in the window, or a login another tool already has (Claude Code, Codex) when no key is given.
    /// Without a key, an account connected before is switched to as it is: «Switch to OpenRouter» sends the provider only.
    /// A new login doesn't switch the chat to it (the owner, 2026-10-20: «я просто хотел подключить другой аккаунт»),
    /// unless the chat has no model to go on with; a model on this PC picked is a switch.
    pub async fn connect(&self, id: &str, key: Option<&str>, model: Option<&str>) -> Result<(), WalletError> {
        let Some(p) = self.provider(id) else { return Err(WalletError(format!("No model provider «{id}»."))) };
        let info = p.info();
        if let Some(why) = info.unavailable { return Err(WalletError(why)) }
        let typed = key.unwrap_or("").trim();
        // Checked with its one short answer when it was kept: switching back asks the model nothing.
        if typed.is_empty() && store::credentials().contains_key(id) {
            self.use_account(id, model);
            return Ok(());
        }
        let had_model = self.has_model();
        let credential = if !typed.is_empty() {
            Some(Credential::ApiKey(typed.into()))
        } else if let Some(existing) = p.existing_login() {
            Some(existing)
        } else if info.kind == Kind::Local {
            None
        } else if !info.takes_key {
            // A subscription that signs in through its own tool (Claude Code, Codex) with no sign-in on this PC yet: as the
            // Mac's useExistingLogin says it.
            return Err(WalletError(self.say("No sign-in found on this PC.", "Вход на этом ПК не найден.")));
        } else {
            return Err(WalletError(self.say(&format!("Paste the key for {}.", info.name), &format!("Вставь ключ для {}.", info.name))));
        };
        // Checked with one short answer before it's kept: a wrong key fails here, not on the first money message.
        let client = p.client(credential.as_ref(), saver(id)).map_err(failed)?;
        let chosen = model.map_or_else(|| info.default_model.clone(), str::to_string);
        client.complete(&llm::Request::new(chosen, vec![llm::Message::new(Role::User, "Reply with OK.")])).await.map_err(failed)?;
        if let Some(c) = credential { store::set_credential(Some(c), id) }
        if let Some(m) = model { self.set_model(Some(m), id) }
        if !had_model || (info.kind == Kind::Local && typed.is_empty()) { self.use_account(id, None) }
        Ok(())
    }

    /// Settings' «Use <login>»: the login another tool has (Claude Code, Codex) taken again for this account, as the Mac's
    /// useExistingLogin — kept, and the chat stays where it is unless it has no model to go on with («Switch» switches).
    pub fn use_login(&self, id: &str) -> Result<(), WalletError> {
        let Some(login) = self.provider(id).and_then(|p| p.existing_login()) else {
            return Err(WalletError(self.say("No sign-in found on this PC.", "На этом компьютере входа не нашлось.")));
        };
        let had_model = self.has_model();
        store::set_credential(Some(login), id);
        if !had_model { self.use_account(id, None) } else { self.changed() }
        Ok(())
    }

    /// The provider's browser sign-in: the window opens the page (the «openURL» event), the redirect comes back to
    /// 127.0.0.1, and the tokens are kept like a key.
    pub async fn sign_in(&self, id: &str) -> Result<(), WalletError> {
        let Some(p) = self.provider(id).filter(|p| p.info().sign_in_label.is_some()) else {
            return Err(WalletError(format!("«{id}» has no browser sign-in: paste its key.")));
        };
        let had_model = self.has_model();
        let credential = p.sign_in(&self.opener()).await.map_err(failed)?;
        store::set_credential(Some(credential), id);
        if !had_model { self.use_account(id, None) }
        Ok(())
    }

    /// Settings › Second check › Connect: OpenRouter signed in for the second check alone (the Mac's signInJev) — the
    /// chat stays on its own account.
    pub async fn sign_in_jev(&self) -> Result<(), WalletError> {
        let Some(p) = self.provider("openrouter") else { return Ok(()) };
        let credential = p.sign_in(&self.opener()).await.map_err(failed)?;
        store::set_credential(Some(credential), "openrouter");
        self.set_settings(|s| s.jev_open_router = Some(true));
        Ok(())
    }

    /// The window opens a sign-in's page.
    fn opener(&self) -> impl Fn(String) + Send + Sync + use<> {
        let out = self.out.clone();
        move |url| out.event("openURL", &HashMap::from([("url", url)]))
    }

    pub fn second_check(&self) -> bool { self.st().settings.second_check.unwrap_or(true) }

    /// The OpenRouter key Jev may use: when the chat is on OpenRouter, or the user turned Jev on for it.
    pub fn jev_key(&self) -> Option<String> {
        let allowed = self.provider_id() == "openrouter" || self.st().settings.jev_open_router == Some(true);
        if !self.second_check() || !allowed { return None }
        match store::credentials().remove("openrouter") {
            Some(Credential::ApiKey(key)) => Some(key),
            Some(Credential::OAuth(tokens)) => Some(tokens.access),
            _ => None,
        }
    }

    pub fn disconnect(&self, id: &str) {
        store::set_credential(None, id);
        self.changed();
    }
}
