//! The signer's wallets from the window (WalletOps.swift): status, the address book, new wallets. Everything here is
//! the signer's, read and changed only through it; the engine keeps what the signer last said, for the window to draw.
//! Whether a wallet is being made is `State::creating_wallet`.

use std::collections::{BTreeMap, HashMap};

use ihatecrypto_core::foundation::number;
use ihatecrypto_core::network::Hyperliquid;
use rust_decimal::Decimal;
use serde::Serialize;
use signer_api::{SignerAddress, SignerReply, SignerRequest};
use wallet::address_check::{self, Network};
use wallet::keys::{self, path};
use wallet::{WalletError, mnemonic};

use crate::blocks::AddressExtra;
use crate::engine::Engine;
use crate::ledger::LedgerAccount;
use crate::link_signer::reason;

/// Dollars without the fee coin, for the onboarding's Top up.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StuckView {
    pub network: String,
    pub key: String,
    pub coin: String,
    /// What to add of the coin, in dollars, and the dollars there.
    #[serde(with = "number")]
    pub usd: Decimal,
    #[serde(with = "number")]
    pub usdc: Decimal,
    /// Cards the chat offers to move them without the coin; 0: only adding it works there.
    pub ways: i64,
}

/// `AddressCheck.Network(rawValue:)`: «solana», «base», «near»…
pub fn address_network(raw: &str) -> Option<Network> { Network::ALL.into_iter().find(|n| serde_json::to_value(n).ok().as_ref().and_then(|v| v.as_str()) == Some(raw)) }

/// Swift's `defer`: a count the window shows, taken back however the call ends.
struct Busy<'a>(&'a Engine, fn(&mut crate::State));

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        (self.1)(&mut self.0.st_mut());
        self.0.changed();
    }
}

impl Engine {
    pub async fn start_signer(&self) {
        if !self.signer.is_available() {
            let e = self.say("The signer isn't installed next to the engine: reinstall ihatecrypto.", "Подписант не установлен рядом с движком: переустанови ihatecrypto.");
            self.st_mut().signer_error = Some(e);
            return;
        }
        self.read_status().await;
        self.read_book().await;
    }

    pub async fn read_status(&self) {
        match self.signer.send(&SignerRequest::Status {}).await.and_then(|r| if r.ok { Ok(r) } else { Err(WalletError(reason(&r))) }) {
            Ok(reply) => {
                self.status_heard(&reply);
                self.lock_heard(reply.code_set == Some(true));
                self.st_mut().signer_error = None;
                self.read_ledger_agent().await;
            }
            Err(e) => self.st_mut().signer_error = Some(e.0),
        }
        self.changed();
    }

    pub async fn read_book(&self) {
        if let Ok(reply) = self.signer.send(&SignerRequest::AddressBook {}).await && reply.ok {
            self.st_mut().book = reply.addresses.unwrap_or_default();
        }
        self.changed();
    }

    /// A request that changes the signer's state: its refusal becomes the error the window shows, and the status is read
    /// again after it either way.
    pub async fn signer_call(&self, request: SignerRequest) -> Result<SignerReply, WalletError> {
        self.st_mut().signing += 1;
        self.changed();
        let _busy = Busy(self, |s| s.signing -= 1);
        // Windows Hello may ask: the window lets the signer, and Hello's dialog, come forward (as for a move's signature).
        if let Some(pid) = self.signer.pid() { self.out.event("signerForeground", &HashMap::from([("pid", pid.to_string())])) }
        let reply = self.signer.send(&request).await?;
        self.read_status().await;
        if !reply.ok { return Err(WalletError(reason(&reply))) }
        let me = self.rc();
        tokio::task::spawn_local(async move { me.refresh().await });
        Ok(reply)
    }

    /// A new 12-word phrase made inside the signer. The window shows it once, then never again (`phraseSaved`).
    pub async fn create_wallet(&self, test: bool) -> Result<(), WalletError> {
        // One at a time, and never over words not yet written down: a second click made a second wallet whose words
        // replaced the first's, so the first wallet's were never shown (the Mac's Create stays busy while the signer works).
        if self.st().creating_wallet || self.st().phrase.is_some() {
            return Err(WalletError(self.say("A new wallet is being made: write its words down first.", "Новый кошелёк уже создаётся: сначала запиши его слова.")));
        }
        self.st_mut().creating_wallet = true;
        let _busy = Busy(self, |s| s.creating_wallet = false);
        let reply = self.signer_call(SignerRequest::CreateWallet { require_presence: !test }).await?;
        let Some(words) = reply.phrase.map(|p| p.split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>()).filter(|w| w.len() == 12) else {
            return Ok(());
        };
        self.st_mut().phrase = Some(words);
        Ok(())
    }

    /// A phrase, with its BIP-39 passphrase if the wallet has one (taken as typed: spaces count), goes to the signer; the
    /// answer carries the first addresses it makes, to compare with the wallet it came from (the Mac's ImportWalletView).
    pub async fn import_phrase(&self, text: &str, passphrase: &str) -> Result<HashMap<String, String>, WalletError> {
        let words = text.to_lowercase().split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
        mnemonic::validate(&words)?;
        self.signer_call(SignerRequest::ImportMnemonic { phrase: words.clone(), passphrase: passphrase.into(), require_presence: true }).await?;
        let accounts = Self::first_accounts(&words, passphrase, 3)?;
        Ok(HashMap::from([("accounts".into(), serde_json::to_string(&accounts).unwrap_or_else(|_| "[]".into()))]))
    }

    /// The first `count` EVM (m/44'/60'/0'/0/i) and Solana (m/44'/501'/i'/0') addresses a phrase makes, as MetaMask and
    /// Phantom list them. Swift's default `count` is 3.
    pub fn first_accounts(words: &str, passphrase: &str, count: u32) -> Result<Vec<BTreeMap<String, String>>, WalletError> {
        let seed = mnemonic::seed(words, passphrase)?;
        (0..count).map(|i| {
            Ok(BTreeMap::from([
                ("evm".into(), keys::evm_address(&*keys::secp256k1(&*seed, &path::evm(i))?)?),
                ("solana".into(), keys::solana_address(&*keys::ed25519(&*seed, &path::solana(i))?)?),
            ]))
        }).collect()
    }

    /// Whether the wallet's Hyperliquid account has had a deposit: only then does Hyperliquid take a trading key, so the
    /// onboarding shows its trading steps only then (the Mac's hlFunded). Empty when it can't be read.
    pub async fn hl_funded(&self) -> HashMap<String, String> {
        let evm = { let st = self.st(); if st.watching() { None } else { st.active_wallet().and_then(|w| w.evm) } };
        let new = match evm { Some(evm) => Hyperliquid::is_new_account(&evm).await.ok(), None => None };
        HashMap::from([("funded".into(), new.map_or(String::new(), |n| if n { "false" } else { "true" }.into()))])
    }

    /// Dollars where the fee coin is missing (gas_ways.rs), for the onboarding's Top up: the first move there would stop.
    pub fn stuck_view(&self) -> Vec<StuckView> {
        self.stuck_dollars().into_iter()
            .map(|s| StuckView { network: s.network, key: s.key, coin: s.coin, usd: s.usd, usdc: s.usdc, ways: s.ways.len() as i64 }).collect()
    }

    /// The rail's «Switch» and Settings' «Use»: said in the chat, a refusal too, as the Mac's switchWallet. The automation
    /// wallet goes to the signer as on the Mac, so its refusal says why («signs only scheduled tasks' moves»), with the
    /// Ledger left as it was. A refusal throws the chat's own line: Settings, which covers the chat here, shows it as its
    /// toast, and the onboarding's Ledger goes on only after a switch; the rail, beside the chat, leaves it there.
    pub async fn use_wallet(&self, id: &str) -> Result<(), WalletError> {
        let known = self.st().status.as_ref().and_then(|s| s.wallets.as_ref()).is_some_and(|w| w.iter().any(|w| w.id == id));
        if let Some(w) = self.switchable_wallets().into_iter().find(|w| w.id == id) {
            self.switch_wallet(w).await;
        } else if known {
            if let Err(e) = self.signer_call(SignerRequest::SetActiveWallet { id: id.into() }).await {
                self.reply(&(self.say("The wallet wasn't switched: ", "Кошелёк не переключён: ") + &e.0), None);
            }
        } else {
            self.reply(&self.say("That wallet isn't here any more.", "Этого кошелька здесь больше нет."), None);
        }
        let st = self.st();
        let now = if st.ledger_active() { Some(LedgerAccount::ID.to_string()) } else { st.status.as_ref().and_then(|s| s.active_wallet.clone()) };
        if now.as_deref() != Some(id) {
            let said = st.messages().iter().rev().find(|m| !m.from_user).map(|m| m.text.clone());
            return Err(WalletError(said.unwrap_or_else(|| st.say("The wallet wasn't switched.", "Кошелёк не переключён."))));
        }
        Ok(())
    }

    pub async fn import_key(&self, key: &str) -> Result<(), WalletError> {
        self.signer_call(SignerRequest::ImportPrivateKey { key: key.trim().into() }).await.map(|_| ())
    }

    /// A saved address goes into the signer's book after Windows Hello with the whole address on the prompt.
    pub async fn save_address(&self, entry: SignerAddress) -> Result<(), WalletError> {
        let Some(net) = address_network(&entry.network) else { return Err(WalletError(self.say("Pick the address's network.", "Выбери сеть адреса."))) };
        if let Some(problem) = address_check::problem(&entry.address, net) { return Err(WalletError(problem)) }
        let reply = self.signer_call(SignerRequest::SaveAddresses { addresses: vec![entry.clone()] }).await?;
        if let Some(book) = reply.addresses { self.st_mut().book = book }
        // What a save from the chat brought (an ENS name's note, an exchange's logo) goes beside it now that it's saved.
        let key = Self::extra_key(&entry.name, &entry.network);
        let extra = self.st_mut().saving_extras.remove(&key);
        if let Some(extra) = extra && extra != AddressExtra::default() {
            let mut extras = self.address_extras();
            if let std::collections::hash_map::Entry::Vacant(e) = extras.entry(key) {
                e.insert(extra);
                self.set_address_extras(extras);
            }
        }
        Ok(())
    }

    /// Out of the signer's book only: an edit (editAddress) moves the picture, the «from chat» mark and the star to the new
    /// name after this, as the Mac's edit keeps them on the same entry.
    pub async fn remove_address(&self, name: &str, network: &str) -> Result<(), WalletError> {
        let reply = self.signer_call(SignerRequest::RemoveAddress { name: name.into(), network: network.into() }).await?;
        if let Some(book) = reply.addresses { self.st_mut().book = book }
        Ok(())
    }

    /// The book's Delete: the entry, and its picture, category, note and star with it, as the Mac's removal does.
    pub async fn delete_address(&self, name: &str, network: &str) -> Result<(), WalletError> {
        let gone = self.st().book.iter().find(|e| e.name.to_lowercase() == name.to_lowercase() && e.network == network).cloned();
        self.remove_address(name, network).await?;
        let mut extras = self.address_extras();
        extras.remove(&Self::extra_key(name, network));
        self.set_address_extras(extras);
        if let Some(gone) = gone {
            let key = Self::star_key(&gone);
            self.set_starred(|s| { s.remove(&key); });
        }
        Ok(())
    }

    /// The saved entry money may go to by this name, on this network when it's given.
    pub fn saved(&self, name: &str, network: Option<&str>) -> Option<SignerAddress> {
        let st = self.st();
        let named = |e: &&SignerAddress| e.name.to_lowercase() == name.to_lowercase();
        st.book.iter().filter(named).find(|e| network.is_none_or(|n| e.network == n)).or_else(|| st.book.iter().find(named)).cloned()
    }
}

#[cfg(test)]
#[path = "wallet_ops/tests.rs"]
mod tests;
