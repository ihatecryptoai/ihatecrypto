//! Safe multisigs made in ihatecrypto (Multisig.swift): kept in multisigs.json, for the user, not per wallet. The chain
//! is the truth — a multisig's owners and threshold are read from it before every move. Here too: the set-up card
//! (multisig_key, update_multisig_draft, its candidates), making one, a payment out of one and its signatures, and
//! reading what each holds.

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use evm::abi::{self, u256_in, uint_in};
use evm::erc20;
use evm::protocols::safe;
use execution::trade_signer::{TradeSigner, TradeWallet};
use ihatecrypto_core::foundation::{Num, Rounding, number, shifted, units};
use ihatecrypto_core::holdings::{self, Holding};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::stablecoins;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use signer_api::SignerAddress;
use trade::{EvmNetwork, Executor};
use wallet::WalletError;

use crate::canvas::Board;
use crate::engine::{Engine, Pending};
use crate::moves::EVM_PLACES;
use crate::store;

/// A Safe multisig: where it is, its keys and how many must sign. The chain is the truth — its owners and threshold are
/// read from it before every move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Multisig {
    pub address: String,
    pub keys: Vec<MultisigKey>,
    pub threshold: i64,
    pub salt: u64,
    /// The networks it's made on (EvmNetwork keys): the same address on each.
    pub networks: Vec<String>,
}

impl Multisig {
    pub fn id(&self) -> &str { &self.address }
}

/// `Multisig.Key`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MultisigKey {
    pub name: String,
    /// Lowercase.
    pub address: String,
    /// «pc»: the signer's key on this computer (Windows Hello); «ledger»: the connected Ledger; «address»: a saved
    /// address (a phone's wallet, a person) that signs in Safe's own app.
    pub kind: String,
}

impl MultisigKey {
    pub fn id(&self) -> &str { &self.address }
}

/// What a multisig holds on one of its networks, for its block (the Mac's MultisigBoard «By network»): its coins as a
/// line and their dollars; both None while the network's node hasn't answered.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MultisigNetHolding {
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coins: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usd: Option<Decimal>,
}

/// The set-up card's choices, before Confirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultisigDraft {
    pub keys: Vec<MultisigKey>,
    pub threshold: i64,
    pub networks: Vec<String>,
    pub salt: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

impl MultisigDraft {
    /// More than half: lose fewer than half the devices and it still opens; a thief needs more than half of them.
    pub fn majority(&self) -> i64 { self.keys.len() as i64 / 2 + 1 }
}

/// A payment out of a multisig, read for its card: what, to whom, and which of its keys sign it here.
#[derive(Debug, Clone, PartialEq)]
pub struct MultisigPayment {
    pub command: WalletCommand,
    pub multisig: Multisig,
    pub net: String,
    pub to: SignerAddress,
    pub symbol: String,
    pub token: Option<String>,
    pub amount: Decimal,
    pub units: Decimal,
    pub threshold: i64,
    pub signers: Vec<MultisigKey>,
    /// What the multisig holds of it now, for the card's balance box (the Mac's MultisigPayment.held).
    pub held: Decimal,
}

/// Where Safe's contracts run among ihatecrypto's networks.
pub const MULTISIG_NETWORKS: [&str; 9] = ["base", "arbitrum", "optimism", "ethereum", "polygon", "bnb", "hyperevm", "robinhood", "tempo"];

/// symbol() and decimals(), a token's own answers.
pub const SYMBOL_CALL: [u8; 4] = [0x95, 0xd8, 0x9b, 0x41];
pub const DECIMALS_CALL: [u8; 4] = [0x31, 0x3c, 0xe5, 0x67];

/// `ABI.decimal(in:word:0)`: the first word as a number; None when it isn't there.
// ponytail: up to Decimal's 28 digits; Swift's holds 38, so a balance past 7.9e28 base units reads as unanswered.
pub fn word_decimal(data: &[u8]) -> Option<Decimal> { units(&u256_in(data, 0)?.to_string(), 0) }

/// «0x» and 40 hex digits, whole.
pub fn is_evm(s: &str) -> bool { s.len() == 42 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit()) }

/// «ledger», «my Ledger», «леджер»: the Ledger by what it is.
static LEDGER_WORD: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"^(?:my\s+|the\s+)?(?:ledger|л[еэ]дж[еэ]р\w*)$").expect("a regex"));

/// `pow(10, decimals)`; None past Decimal's range.
fn scale(decimals: i64) -> Num { Num(u32::try_from(decimals).ok().filter(|d| *d <= 28).map(|d| Decimal::from_i128_with_scale(10i128.pow(d), 0))) }

fn place_names(networks: &[String]) -> String { networks.iter().map(|n| WalletCommand::place_name(n)).collect::<Vec<_>>().join(", ") }

impl Engine {
    pub fn save_multisigs(&self) { store::save(&self.st().multisigs, "multisigs.json") }

    /// Each multisig's money on each of its networks, read with the balances.
    pub async fn refresh_multisigs(&self) {
        let multisigs = self.st().multisigs.clone();
        for m in multisigs {
            let held = self.multisig_holdings(&m).await;
            self.st_mut().multisig_held.insert(m.address.clone(), held);
        }
    }

    /// As the Mac's MultisigBoard.holdings: the network's coin and its USDC read from the chain itself (an explorer lists
    /// a new address minutes late), the other tokens from the explorer; dust that no market vouches for stays out.
    pub async fn multisig_holdings(&self, m: &Multisig) -> Vec<MultisigNetHolding> {
        let mut out = vec![];
        for key in &m.networks {
            let Some(net) = EvmNetwork::named(key) else { continue };
            let usdc = match erc20::balance_of(&m.address) { Ok(call) => net.rpc.read(net.usdc, &call).await.ok().and_then(|d| word_decimal(&d)), Err(_) => None };
            let Some(usdc) = usdc else {
                out.push(MultisigNetHolding { network: key.clone(), coins: None, usd: None });
                continue;
            };
            let skip = HashSet::from([net.usdc.to_lowercase()]);
            let mut list: Vec<Holding> = holdings::evm(key, &m.address, false, &skip, &[]).await.unwrap_or_default().into_iter().filter(|h| !h.native).collect();
            if usdc > Decimal::ZERO && let Some(amount) = shifted(usdc, i64::from(net.usdc_decimals)) {
                list.insert(0, holdings::dollar(key, net.usdc, amount, "USDC", None));
            }
            if net.fee_token.is_none() && let Ok(wei) = net.rpc.balance(&m.address).await && wei > 0 && let Some(amount) = units(&wei.to_string(), 18) {
                let price = self.st().markets.get(net.gas_coin).map(|m| m.mark_px);
                list.insert(0, Holding { price, verified: true, native: true, ..Holding::new(key, "native", amount, net.gas_coin) });
            }
            let shown: Vec<&Holding> = list.iter().filter(|h| h.looks_real() || h.usd().unwrap_or_default() >= Decimal::new(1, 2)).collect();
            out.push(MultisigNetHolding {
                network: key.clone(),
                coins: Some(shown.iter().map(|h| format!("{} {}", Fmt::num(h.amount, 6), h.symbol)).collect::<Vec<_>>().join(" · ")),
                usd: Some(shown.iter().filter_map(|h| h.usd()).fold(Num::of(Decimal::ZERO), |s, u| s + u).0.unwrap_or(Decimal::MAX)),
            });
        }
        out
    }

    /// The signer's own EVM key, whichever wallet is shown: one computer is one key of a multisig.
    pub fn pc_key(&self) -> Option<String> { self.st().status.as_ref()?.evm_address.as_ref().map(|a| a.to_lowercase()) }

    /// The connected Ledger as a key: its own device, signed with its buttons.
    pub fn ledger_key(&self) -> Option<MultisigKey> {
        let st = self.st();
        let l = st.ledger.as_ref()?;
        l.evm.as_ref().map(|a| MultisigKey { name: l.label.clone(), address: a.to_lowercase(), kind: "ledger".into() })
    }

    /// The other EVM keys on this computer (the test or automation wallet, another phrase): they make one key, not several.
    pub fn keys_on_this_pc(&self) -> HashSet<String> {
        self.st().status.as_ref().and_then(|s| s.wallets.as_ref()).map(|w| w.iter().filter_map(|w| w.evm.as_ref().map(|a| a.to_lowercase())).collect()).unwrap_or_default()
    }

    pub fn command(d: &MultisigDraft) -> WalletCommand {
        WalletCommand::MultisigCreate {
            threshold: Some(d.threshold), of: Some(d.keys.len() as i64), keys: d.keys.iter().filter(|k| k.kind != "pc").map(|k| k.name.clone()).collect(),
            networks: d.networks.clone(),
        }
    }

    /// Saved EVM addresses the card can add: confirmed with Windows Hello when saved, none of this computer's.
    pub fn multisig_candidates(&self) -> Vec<MultisigKey> {
        let Some(draft) = self.st().multisig_draft.clone() else { return vec![] };
        let have: HashSet<String> = draft.keys.iter().map(|k| k.address.clone()).chain(self.keys_on_this_pc()).collect();
        let mut out: Vec<MultisigKey> = self.ledger_key().filter(|l| !have.contains(&l.address)).into_iter().collect();
        let st = self.st();
        for e in st.book.iter().filter(|e| (EVM_PLACES.contains(&e.network.as_str()) || e.network == "hyperliquid") && is_evm(&e.address)) {
            let a = e.address.to_lowercase();
            if st.multisigs.iter().any(|m| m.address == a) || have.contains(&a) || out.iter().any(|k| k.address == a) { continue }
            out.push(MultisigKey { name: e.name.clone(), address: a, kind: "address".into() });
        }
        out
    }

    /// «1 key», «3 keys»; «1 ключ», «2 ключа», «5 ключей».
    pub fn keys_word(&self, n: i64) -> String {
        if !self.st().russian() { return format!("{n} key{}", if n == 1 { "" } else { "s" }) }
        let (tens, ones) = (n % 100, n % 10);
        format!("{n} {}", if tens / 10 == 1 { "ключей" } else if ones == 1 { "ключ" } else if (2..=4).contains(&ones) { "ключа" } else { "ключей" })
    }

    pub fn is_evm(&self, s: &str) -> bool { is_evm(s) }

    /// «make a multisig 2 of 3 with Phone on base»: the keys (this computer's, the saved addresses named), how many sign and
    /// where, as a card the person adjusts before Confirm.
    pub async fn prepare_multisig(&self, command: &WalletCommand, note: Option<String>) {
        let WalletCommand::MultisigCreate { threshold, of, keys: names, networks } = command else { return };
        let Some(pc) = self.pc_key() else {
            return self.reply(&self.say("A multisig needs this computer's EVM key: add a wallet with a phrase first.", "Для мультисига нужен EVM-ключ этого компьютера: сначала добавь кошелёк с фразой."), note);
        };
        let mut keys = vec![MultisigKey { name: self.say("This PC", "Этот ПК"), address: pc.clone(), kind: "pc".into() }];
        // The connected Ledger: when named, when no count was said, or when the count needs it beside the names.
        let ledger = self.ledger_key();
        let is_ledger = |name: &str| {
            let n = name.to_lowercase();
            Some(&n) == ledger.as_ref().map(|l| l.name.to_lowercase()).as_ref() || LEDGER_WORD.is_match(&n).unwrap_or(false)
        };
        let others: Vec<&String> = names.iter().filter(|n| !is_ledger(n)).collect();
        if let Some(l) = ledger.clone() && l.address != pc && (names.iter().any(|n| is_ledger(n)) || of.is_none() || of.unwrap_or(0) > others.len() as i64 + 1) {
            keys.push(l);
        }
        let here = self.keys_on_this_pc();
        for name in others {
            let Some(entry) = self.entry(name, Some("base")).filter(|e| is_evm(&e.address)) else {
                return self.reply(&self.say(&format!("There's no EVM address «{name}» in Addresses. Save it first («save 0x… as {name} on base»), then make the multisig."),
                                            &format!("В адресной книге нет EVM-адреса «{name}». Сначала сохрани его («сохрани 0x… как {name} на base»), потом делай мультисиг.")), note);
            };
            let address = entry.address.to_lowercase();
            if here.contains(&address) {
                return self.reply(&self.say(&format!("«{}» is another wallet on this computer: one computer is one device, so it's one key of the multisig. Name a phone's wallet, a person or a Ledger.", entry.name),
                                            &format!("«{}» — ещё один кошелёк на этом компьютере: один компьютер — одно устройство и один ключ мультисига. Назови кошелёк телефона, человека или Ledger.", entry.name)), note);
            }
            if !keys.iter().any(|k| k.address == address) { keys.push(MultisigKey { name: entry.name, address, kind: "address".into() }) }
        }
        let nets: Vec<String> = networks.iter().filter(|n| MULTISIG_NETWORKS.contains(&n.as_str())).cloned().collect();
        let count = keys.len() as i64;
        let m = threshold.unwrap_or(count / 2 + 1).max(1).min(count);
        let mut draft = MultisigDraft { keys, threshold: m, networks: if nets.is_empty() { vec!["base".into()] } else { nets }, salt: chrono::Utc::now().timestamp().max(0) as u64, address: None };
        // The same card again («Try again» after a failure): the same salt, so it finds the multisig a first try made.
        if let Some(last) = self.st().multisig_draft.as_ref() && last.keys == draft.keys && last.threshold == draft.threshold { draft.salt = last.salt }
        draft.address = safe_address(&draft);
        let on = place_names(&draft.networks);
        self.st_mut().multisig_draft = Some(draft);
        self.show_multisig_card();
        let short = if of.unwrap_or(0) > count {
            let asked = of.unwrap_or(0);
            self.say(&format!(" You asked for {asked} keys: add the others on the card (a saved address)."), &format!(" Ты просил {asked} ключа: добавь остальные на карточке (сохранённый адрес)."))
        } else { String::new() };
        let words = self.keys_word(count);
        self.reply(&(self.say(&format!("The multisig's card is on the canvas: {words}, any {m} move its money, on {on}. Add keys, change how many must sign or pick the networks there, then press Confirm."),
                              &format!("Карточка мультисига на холсте: {words}, деньги двигают любые {m}, сеть: {on}. Там можно добавить ключи, поменять, сколько нужно подписей, выбрать сети — потом нажми «Подтвердить».")) + &short), note);
    }

    /// The card follows the draft: Confirm and the chat see what the card shows.
    fn show_multisig_card(&self) {
        let Some(d) = self.st().multisig_draft.clone() else { return };
        let mut lines: Vec<String> = d.keys.iter().map(|k| format!("{} · {}", k.name, k.address)).collect();
        lines.push(self.say("Address: ", "Адрес: ") + d.address.as_deref().unwrap_or("—"));
        lines.push(self.say("One transaction per network from this computer to Safe's factory (Safe 1.4.1), and ihatecrypto never changes its keys.",
                            "По одной транзакции на сеть с этого компьютера в фабрику Safe (Safe 1.4.1), и ihatecrypto никогда не меняет его ключи."));
        let words = self.keys_word(d.keys.len() as i64);
        let title = self.say(&format!("Multisig · any {} of {words} on ", d.threshold), &format!("Мультисиг · любые {} из {words} · ", d.threshold)) + &place_names(&d.networks);
        let waiting = {
            let st = self.st();
            st.card.as_ref().map(|c| c.id.clone()).filter(|_| matches!(st.pending, Some(Pending::Wallet(WalletCommand::MultisigCreate { .. }))))
        };
        let pending = Pending::Wallet(Self::command(&d));
        match waiting {
            Some(id) => {
                {
                    let mut st = self.st_mut();
                    st.pending = Some(pending.clone());
                    if let Some(c) = st.card.as_mut() { (c.title, c.lines) = (title.clone(), lines.clone()) }
                }
                let route = self.steps(&pending, &title, &lines);
                self.update_board(Some(&id), |b| { b.title = title; b.steps = Some(route) });
            }
            None => self.show(pending, &title, lines, vec![], vec![]),
        }
    }

    pub fn update_multisig_draft(&self, change: impl FnOnce(&mut MultisigDraft)) {
        let draft = {
            let st = self.st();
            st.multisig_draft.clone().filter(|_| matches!(st.pending, Some(Pending::Wallet(WalletCommand::MultisigCreate { .. }))))
        };
        let Some(mut d) = draft else { return };
        change(&mut d);
        d.threshold = d.threshold.max(1).min((d.keys.len() as i64).max(1));
        d.address = safe_address(&d);
        self.st_mut().multisig_draft = Some(d);
        self.show_multisig_card();
    }

    /// A key added from the card's candidates (by address) or removed; this computer's key stays.
    pub fn multisig_key(&self, add: Option<&str>, remove: Option<&str>) -> Result<(), WalletError> {
        if let Some(a) = add.map(str::to_lowercase) {
            let Some(c) = self.multisig_candidates().into_iter().find(|k| k.address == a) else {
                return Err(WalletError(self.say("That address can't be a key here.", "Этот адрес не может быть ключом.")));
            };
            self.update_multisig_draft(|d| { d.keys.push(c); d.threshold = d.majority() });
        }
        if let Some(r) = remove.map(str::to_lowercase) {
            self.update_multisig_draft(|d| {
                if d.keys.len() <= 1 { return }
                d.keys.retain(|k| !(k.address == r && k.kind != "pc"));
                d.threshold = d.majority();
            });
        }
        Ok(())
    }

    /// After Confirm: the multisig on each network the card named, kept here, and saved in Addresses so money can go to it.
    pub async fn run_multisig_create(&self) -> trade::Result<String> {
        let Some(d) = self.st().multisig_draft.clone() else { return Err(trade::Error::Execution("The multisig's card is gone. Ask for it again.".into())) };
        let owners: Vec<String> = d.keys.iter().map(|k| k.address.clone()).collect();
        let threshold = usize::try_from(d.threshold).unwrap_or(0);
        if let Some(problem) = safe::owners_problem(&owners, threshold) { return Err(trade::Error::Execution(problem + " Nothing was signed.")) }
        for key in d.keys.iter().filter(|k| k.kind == "address") {
            for net in d.networks.iter().filter_map(|n| EvmNetwork::named(n)) {
                if Executor::is_contract(&key.address, net).await? {
                    return Err(trade::Error::Execution(format!("«{}» is a contract on {}, not a wallet a person signs with. Nothing was signed.", key.name, net.name)));
                }
            }
        }
        let e = self.executor()?;
        let (mut made, mut address) = (Vec::<String>::new(), String::new());
        for key in &d.networks {
            let Some(net) = EvmNetwork::named(key) else { continue };
            address = e.create_multisig(net, &owners, threshold, d.salt).await?;
            made.push(key.clone());
            // Kept as soon as it exists on one network: a later one failing doesn't lose it.
            {
                let mut st = self.st_mut();
                st.multisigs.retain(|m| m.address != address);
                st.multisigs.push(Multisig { address: address.clone(), keys: d.keys.clone(), threshold: d.threshold, salt: d.salt, networks: made.clone() });
            }
            self.save_multisigs();
            let me = self.rc();
            tokio::task::spawn_local(async move { me.refresh_multisigs().await });
        }
        self.st_mut().multisig_draft = None;
        self.pin(Board::new("multisig", "multisig"));
        // Not saved in the book, as on the Mac: «Multisig» answers through multisigEntry, only on a network it's made on, and
        // the signer asks Windows Hello with the whole address the first time money goes there.
        let (nets, first, (m, n)) = (place_names(&made), made.first().map_or("base", String::as_str), (d.threshold, d.keys.len()));
        Ok(self.say(&format!("The multisig {address} is made on {nets}: any {m} of its {n} keys move its money. Put money in with «send 10 USDC on {first} to Multisig». It's on the canvas."),
                    &format!("Мультисиг {address} создан в {nets}: деньги двигают любые {m} из {n} ключей. Положить деньги: «отправь 10 USDC на {first} на Multisig». Он на холсте.")))
    }

    /// «send 200 USDC from the multisig to Alex on base»: read from the chain for its card.
    pub async fn prepare_multisig_send(&self, command: &WalletCommand, note: Option<String>) {
        let WalletCommand::MultisigSend { token, amount, to: name, network } = command else { return };
        let Some(m) = self.st().multisigs.last().cloned() else {
            return self.reply(&self.say("There's no multisig yet: say «make a multisig».", "Мультисига пока нет: скажи «сделай мультисиг»."), note);
        };
        let key = network.clone().or_else(|| m.networks.first().cloned()).unwrap_or_else(|| "base".into());
        let Some(net) = EvmNetwork::named(&key).filter(|_| m.networks.contains(&key)) else {
            let (here, on) = (WalletCommand::place_name(&key), place_names(&m.networks));
            return self.reply(&self.say(&format!("The multisig isn't on {here}: it's on {on}."), &format!("Мультисига нет в {here}: он в {on}.")), note);
        };
        let to = match self.token_recipient(name, Some(&key)) {
            Ok((to, _)) if is_evm(&to.address) => to,
            Ok((to, _)) => return self.reply(&format!("“{}” isn't an EVM address.", to.name), note),
            Err(e) => return self.reply(&e.to_string(), note),
        };
        let coin = token.to_uppercase() == net.gas_coin.to_uppercase();
        let contract = if coin { Some(String::new()) } else if token.starts_with("0x") { Some(token.clone()) } else if token.to_uppercase() == "USDC" { Some(net.usdc.into()) }
            else { stablecoins::find(token, &key).map(|c| c.address.to_string()) };
        let Some(contract) = contract else {
            return self.reply(&self.say(&format!("I don't know the token «{token}» on {}: write its contract.", net.name), &format!("Не знаю токен «{token}» в {}: напиши его контракт.", net.name)), note);
        };
        let symbol = if coin { net.gas_coin.to_string() } else { net.rpc.read(&contract, &SYMBOL_CALL).await.ok().and_then(|d| abi::string(&d)).unwrap_or_else(|| token.to_uppercase()) };
        let decimals = if coin { 18 } else { net.rpc.read(&contract, &DECIMALS_CALL).await.ok().and_then(|d| uint_in(&d, 0)).unwrap_or(18) };
        let owners = net.rpc.read(&m.address, &safe::owners_call()).await.ok().and_then(|d| safe::owners(&d));
        let threshold = match owners { Some(_) => net.rpc.read(&m.address, &safe::threshold_call()).await.ok().and_then(|d| uint_in(&d, 0)), None => None };
        let raw = match threshold {
            None => None,
            Some(_) if coin => net.rpc.balance(&m.address).await.ok().and_then(|w| units(&w.to_string(), 0)),
            Some(_) => match erc20::balance_of(&m.address) { Ok(call) => net.rpc.read(&contract, &call).await.ok().and_then(|d| word_decimal(&d)), Err(_) => None },
        };
        let (Some(owners), Some(threshold), Some(raw)) = (owners, threshold, raw) else {
            return self.reply(&self.say(&format!("{}'s node didn't answer for the multisig. Try again in a moment.", net.name), &format!("Нода {} не ответила по мультисигу, попробуй через минуту.", net.name)), note);
        };
        // A token's decimals are its own answer: an amount past the arithmetic is more than the multisig holds.
        let decimals = i64::try_from(decimals).unwrap_or(i64::MAX);
        let held = shifted(raw, decimals).unwrap_or_default();
        let units_out = match amount { Some(a) => (Num::of(*a) * scale(decimals)).rounded(0, Rounding::Down).0, None => Some(raw) };
        let Some(units_out) = units_out.filter(|u| *u > Decimal::ZERO && *u <= raw) else {
            let less = if amount.is_none() { (String::new(), String::new()) } else { (", less than that".into(), " — меньше, чем нужно".into()) };
            return self.reply(&self.say(&format!("The multisig holds {} {symbol} on {}{}.", Fmt::num(held, 6), net.name, less.0),
                                        &format!("В мультисиге {} {symbol} в {}{}.", Fmt::num(held, 6), net.name, less.1)), note);
        };
        if coin && units_out > Decimal::from(u64::MAX) {
            return self.reply(&self.say(&format!("That's more {} than one move sends.", net.gas_coin), &format!("Это больше {}, чем уходит за раз.", net.gas_coin)), note);
        }
        let owned: HashSet<String> = owners.iter().map(|o| o.to_lowercase()).collect();
        let (pc, ledger) = (self.pc_key(), self.ledger_key().map(|l| l.address));
        let mut here: Vec<MultisigKey> = m.keys.iter().filter(|k| owned.contains(&k.address) && (Some(&k.address) == pc.as_ref() || Some(&k.address) == ledger.as_ref())).cloned().collect();
        here.sort_by_key(|k| k.kind != "pc");
        if (here.len() as u64) < threshold {
            let elsewhere = m.keys.iter().filter(|k| !here.contains(k)).map(|k| k.name.clone()).collect::<Vec<_>>().join(", ");
            return self.reply(&self.say(&format!("The multisig needs {threshold} signatures and only {} of its keys sign here. The others ({elsewhere}) sign in Safe's own app: app.safe.global.", here.len()),
                                        &format!("Мультисигу нужно {threshold} подписи, а здесь подписывают только {} его ключа. Остальные ({elsewhere}) подписывают в приложении Safe: app.safe.global.", here.len())), note);
        }
        let signers: Vec<MultisigKey> = here.into_iter().take(threshold as usize).collect();
        let amount_out = shifted(units_out, decimals).unwrap_or_default();
        let shown = format!("{} {symbol}", Fmt::num(amount_out, 6));
        let after = (Num::of(held) - amount_out).0.unwrap_or_default();
        let names = (signers.iter().map(|k| k.name.clone()).collect::<Vec<_>>().join(" and "), signers.iter().map(|k| k.name.clone()).collect::<Vec<_>>().join(" и "));
        self.st_mut().multisig_payment = Some(MultisigPayment {
            command: command.clone(), multisig: m, net: key, to: to.clone(), symbol: symbol.clone(), token: (!coin).then_some(contract), amount: amount_out, units: units_out,
            threshold: threshold as i64, signers, held,
        });
        self.show(Pending::Wallet(command.clone()), &self.say(&format!("{shown} from the multisig to {} on {}", to.name, net.name), &format!("{shown} из мультисига на {} в {}", to.name, net.name)), vec![
            self.say(&format!("The multisig holds {} {symbol} → {} after", Fmt::num(held, 6), Fmt::num(after, 6)), &format!("В мультисиге {} {symbol} → {} после", Fmt::num(held, 6), Fmt::num(after, 6))),
            self.say(&format!("Signed here by {}, this computer pays the gas", names.0), &format!("Подписывает здесь {}, газ платит этот компьютер", names.1)),
            to.address.clone(),
        ], vec![], vec![]);
        self.reply(&self.say("The card is on the canvas — press Confirm.", "Карточка на холсте — нажми «Подтвердить»."), Some(note.unwrap_or_else(|| "Safe · multisig".into())));
    }

    pub async fn run_multisig_send(&self, command: &WalletCommand) -> trade::Result<String> {
        let payment = self.st().multisig_payment.clone().filter(|p| &p.command == command);
        let Some((p, net)) = payment.and_then(|p| EvmNetwork::named(&p.net).map(|n| (p, n))) else {
            return Err(trade::Error::Execution("The multisig payment's check is gone. Ask for it again.".into()));
        };
        self.st_mut().multisig_payment = None;
        let e = self.executor()?;
        let ledger = self.st().ledger.clone();
        let cosigners: Vec<Arc<dyn TradeSigner>> = p.signers.iter().map(|k| match ledger.as_ref().filter(|_| k.kind == "ledger").and_then(|l| l.evm.clone().map(|evm| (l, evm))) {
            Some((l, evm)) => Arc::new(self.ledger_signer(TradeWallet { solana: l.solana.clone().unwrap_or_default(), evm })) as Arc<dyn TradeSigner>,
            None => Arc::new(self.trade_signer(None)) as Arc<dyn TradeSigner>,
        }).collect();
        let amount = format!("{} {}", Fmt::num(p.amount, 6), p.symbol);
        let data = match &p.token { Some(_) => erc20::transfer(&p.to.address, evm::U256::from_be_bytes(abi::word_decimal(p.units)?))?, None => vec![] };
        let value = if p.token.is_none() { p.units.to_u64().unwrap_or(0) } else { 0 };
        let hash = e.multisig_pay(net, &p.multisig.address, p.token.as_deref().unwrap_or(&p.to.address), value, &data, &cosigners, std::slice::from_ref(&p.to.address),
                                  &format!("{amount} from the multisig to {} on {}", p.to.name, net.name)).await?;
        Ok(self.say(&format!("Sent {amount} from the multisig to {} on {}: {}/tx/{hash}", p.to.name, net.name, net.explorer),
                    &format!("Отправлено {amount} из мультисига на {} в {}: {}/tx/{hash}", p.to.name, net.name, net.explorer)))
    }

    pub fn show_multisig(&self, note: Option<String>) {
        let Some(m) = self.st().multisigs.last().cloned() else {
            return self.reply(&self.say("There's no multisig yet: say «make a multisig».", "Мультисига пока нет: скажи «сделай мультисиг»."), note);
        };
        self.pin(Board::new("multisig", "multisig"));
        let (a, t, n, on) = (&m.address, m.threshold, m.keys.len(), place_names(&m.networks));
        self.reply(&self.say(&format!("The multisig is on the canvas: {a}, any {t} of its {n} keys, on {on}."), &format!("Мультисиг на холсте: {a}, любые {t} из {n} ключей, сети: {on}.")), note);
    }
}

/// `try? Safe.address(…)` of a draft.
fn safe_address(d: &MultisigDraft) -> Option<String> {
    safe::address(&d.keys.iter().map(|k| k.address.clone()).collect::<Vec<_>>(), usize::try_from(d.threshold).ok()?, d.salt).ok()
}

#[cfg(test)]
#[path = "multisig/tests.rs"]
mod tests;
