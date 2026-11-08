//! Everything that needs the seed happens here, after the guard (`Signer.swift`, main at bc90340, with the Windows
//! branch's own lines). The engine is not trusted with more than it must supply: approved recipients (the person
//! approved them in the window) and lookup-table contents (the signer has no network). Token-account owners it claims
//! are re-derived here, and Windows Hello's prompt shows what the signer itself parsed, so a lying engine is visible.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::Signer as _;
use evm::call::Call;
use evm::protocols::{Permit3009, SponsoredTransfer, batch7702, relay, safe};
use evm::relay_order::RelayOrder;
use evm::{SIGNABLE, Transaction, network_name};
use guard::{Verdict, hl, sol};
use hyperliquid::action::{self, Opening};
use hyperliquid::{MsgPack, builder, signer as hls};
use signer_api::{LEDGER_DEVICE, SignerAddress, SignerReply, SignerRequest, SignerWallet, passcode, session};
use solana::message::Message;
use solana::spl;
use wallet::address_check::{self, Network};
use wallet::evm::{personal, sign};
use wallet::keys::{self, base58, path};
use wallet::near::{self, Action};
use wallet::typed_data::TypedData;
use wallet::{Result, WalletError, agent_owner, mnemonic, near_owner};
use zeroize::Zeroizing;

use crate::trusted::{Pref, prefs};
use crate::{automation, limits};
use crate::vault::{self, PRESENCE, agents, book, code, own, recipients, seeds};
use crate::wallets;

fn fail<T>(m: impl Into<String>) -> Result<T> { Err(WalletError(m.into())) }

fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) }

pub const PUBLIC_TEST_SOLANA: &str = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk";

fn agent_active_key() -> String { wallets::scoped("agentActive", &wallets::active_id()) }
fn agent_renewed_key() -> String { wallets::scoped("agentRenewed", &wallets::active_id()) }

/// The Hyperliquid trading key opens or adds to positions for the person's days (Settings, 2 unless set) after their
/// last Windows Hello for it (turning it on, an order the main key signed in person); then cancels and reduce-only
/// orders still go without one. Within those days it opens only buys inside the person's dollars (`agent_take`):
/// together they cap how much and how long a program that took over the engine could trade the account down unseen.
/// Only an in-person act renews it, never the engine's word.
fn agent_span() -> i64 { limits::current().agent_days * 86_400 }
fn agent_days_text() -> String { match limits::current().agent_days { 1 => "1 day".into(), d => format!("{d} days") } }

/// Whole seconds, where the Mac keeps a Double: the Swift Windows signer's settings had no Double, so its renewal was
/// never kept and the week never ran — the trading key could only reduce there.
fn agent_fresh(key: &str) -> bool { (now() as i64) - prefs::integer(key) < agent_span() }
fn renew_agent(key: &str) { prefs::set(key, Some(Pref::Int(now() as i64))) }

/// What the trading key may sign on its own: its dollars taken from the day (given back unless kept once it signed), or
/// why it can't.
fn agent_take(opening: &Opening) -> std::result::Result<Option<limits::Hold>, String> {
    match opening {
        Opening::Person(why) => Err(why.clone()),
        Opening::Dollars(d) if d.is_zero() => Ok(None),
        Opening::Dollars(d) => limits::take_agent(*d, &limits::current()).map(Some),
    }
}

fn reply() -> SignerReply { SignerReply::ok() }

/// Every request runs with its recipients and coins as the session's scope (`vault::with_move_scope`).
pub fn handle(request: SignerRequest) -> Result<SignerReply> {
    match move_scope(&request) {
        Some(scope) => vault::with_move_scope(scope, || handle_request(request)),
        None => handle_request(request),
    }
}

/// What a signature moves, for binding the session: the addresses money may go to and the coins it touches (Solana keys
/// its instructions use, EVM token contracts, Hyperliquid markets), each tied to its network — only what the signed bytes
/// say, never the engine's lists of recipients or accounts (which could name everything). None: a request that moves
/// nothing of the kind (a message, settings), or one whose wallet or bytes can't be read.
fn move_scope(request: &SignerRequest) -> Option<HashSet<String>> {
    use SignerRequest as R;
    let keys = |list: Vec<String>, owner: Option<&str>| -> Vec<String> {
        let owner = owner.map(recipients::key);
        list.iter().map(|a| recipients::key(a)).filter(|k| Some(k) != owner.as_ref()).collect()
    };
    // The payees the signer decodes, and the token contract a transfer or an approval spends — not the called contract:
    // an approve and the swap that spends it stay one move.
    let evm = |to: &str, value: u64, data: &[u8], chain_id: u64, owner: &str| -> Vec<String> {
        let call = Call::decode(data);
        let token = match &call {
            Call::Transfer { .. } | Call::Approve { .. } | Call::IncreaseAllowance { .. } | Call::TransferFrom { .. } => vec![to.to_string()],
            Call::Permit2Approve { token, .. } => vec![token.clone()],
            _ => vec![],
        };
        let mut list = keys([call.payees(to, value, chain_id), token].concat(), Some(owner));
        if value > 0 { list.push("native".into()) }
        list
    };
    let tagged = |prefix: String, list: Vec<String>| list.into_iter().map(|a| format!("{prefix}{a}")).collect::<HashSet<_>>();
    Some(match request {
        R::SignSolana { wire, sponsor, .. } => {
            // The message's own keys its instructions use: where money goes must be one of them (the guard), coins too.
            let (owner, m) = (address("solana").ok()?, Message::parse_transaction(wire).ok()?);
            let used: std::collections::BTreeSet<usize> = m.instructions.iter().flat_map(|ix| ix.accounts.iter().copied()).collect();
            let list = used.into_iter().filter_map(|i| m.static_keys.get(i).cloned()).chain(sponsor.clone()).filter(|k| *k != owner).collect();
            tagged("sol:".into(), list)
        }
        R::SignEvmTransaction { chain_id, to, value, data, .. } => tagged(format!("evm:{chain_id}:"), evm(to, *value, data, *chain_id, &address("evm").ok()?)),
        R::SignEvmBatch { chain_id, calls, .. } => {
            let owner = address("evm").ok()?;
            tagged(format!("evm:{chain_id}:"), calls.iter().flat_map(|c| evm(&c.to, 0, &c.data, *chain_id, &owner)).collect())
        }
        R::SignSafeMove { chain_id, to, value, data, .. } => tagged(format!("evm:{chain_id}:"), evm(to, *value, data, *chain_id, &address("evm").ok()?)),
        // Markets only: leverage, the entry and its stop of one trade are one move.
        R::SignHyperliquidL1 { action: act, .. } => action::markets(act).into_iter().map(|m| format!("hl:{m}")).collect(),
        R::SignHyperliquidUser { action: act, .. } => {
            tagged("hl:".into(), [act.text("destination").map(str::to_lowercase), act.text("token").map(str::to_string)].into_iter().flatten().collect())
        }
        R::SignSponsoredTransfer { chain_id, token, to, .. } => tagged(format!("evm:{chain_id}:"), keys(vec![token.clone(), to.clone()], None)),
        R::SignRelayMulticallPermit { chain_id, token, relayer, refund_to, .. } => {
            tagged(format!("evm:{chain_id}:"), keys(vec![token.clone(), relayer.clone(), refund_to.clone()], address("evm").ok().as_deref()))
        }
        R::SignNearTransfer { receiver, .. } => [format!("near:{receiver}")].into(),
        R::SignNearIntentDeposit { deposit_address, .. } => [format!("near:{deposit_address}")].into(),
        _ => return None,
    })
}

fn handle_request(request: SignerRequest) -> Result<SignerReply> {
    use SignerRequest as R;
    match request {
        R::Status {} => {
            let mut r = reply();
            let all = wallets::all();
            // remove() before 2026-11-01 could leave the automation wallet on, its addresses mirrored as «ours».
            let active = wallets::active();
            if prefs::string("activeWallet") != active.as_ref().map(|w| w.id.clone()) { wallets::activate(active.as_ref()) }
            r.has_wallet = Some(!all.is_empty());
            r.wallets = Some(all);
            r.active_wallet = wallets::active().map(|w| w.id);
            r.solana_address = prefs::string("solana");
            r.evm_address = prefs::string("evm");
            r.near_address = prefs::string("near");
            // On Windows the address is read without opening the key.
            r.agent_address = agents::address(&wallets::active_id());
            r.agent_active = Some(prefs::bool(&agent_active_key()) && r.agent_address.is_some());
            r.session_length = Some(seeds::session_length());
            r.limits = Some(limits::current());
            r.code_set = Some(code::is_set());
            Ok(r)
        }

        R::SetCode { code: c } => {
            if let Some(p) = passcode::problem(&c) { return fail(p) }
            // In person every time, even with no wallet yet. A Debug signer that holds only the public test wallet (its
            // self-test) is the one exception.
            let only_public_test = cfg!(debug_assertions) && { let all = wallets::all(); !all.is_empty() && all.iter().all(|w| w.solana.as_deref() == Some(PUBLIC_TEST_SOLANA)) };
            if !only_public_test {
                vault::confirm(&format!("save your new 6-digit passcode: it opens the app{}", if code::CAN_SIGN { " and signs your moves" } else { "" }), None)?;
            }
            let set = code::set(&c);
            vault::end_session(); // a confirmation, not a move: it leaves no session for an unrelated signature
            set?;
            let mut r = reply();
            r.code_set = Some(true);
            Ok(r)
        }

        R::ClearCode {} => {
            if code::is_set() {
                vault::confirm("turn off your passcode", None)?;
                vault::end_session();
                code::clear();
            }
            let mut r = reply();
            r.code_set = Some(false);
            Ok(r)
        }

        R::EnterCode { code: c, .. } => {
            if !code::is_set() {
                let mut r = SignerReply::error(format!("There is no passcode: {} opens ihatecrypto.", code::FALLBACK));
                (r.code_set, r.code_off) = (Some(false), Some(true));
                return Ok(r);
            }
            // The code never signs here (Windows Hello does), so `sign` asks nothing more.
            Ok(match code::enter(&c) {
                code::Entered::Right => SignerReply { code_set: Some(true), ..reply() },
                code::Entered::Wrong { left } => SignerReply { code_set: Some(true), ..SignerReply::error(format!("Wrong code · {left} {} left", if left == 1 { "try" } else { "tries" })) },
                code::Entered::Uncounted => SignerReply { code_set: Some(true), ..SignerReply::error("The signer couldn't count this try, so it didn't check the code: try again.") },
                code::Entered::Off => SignerReply {
                    code_set: Some(false), code_off: Some(true),
                    ..SignerReply::error(format!("{} wrong codes: the code is off. Only {} opens ihatecrypto{} now.", code::TRIES, code::FALLBACK,
                                                 if code::CAN_SIGN { " and signs" } else { "" }))
                },
            })
        }

        R::EnableAgent {} => {
            let owner = address("evm")?;
            let id = wallets::active_id();
            // The key kept for this wallet, by its address (on Windows that doesn't open it), or a new one.
            let kept = agents::address(&id);
            let made = if kept.is_none() { Some(agents::make()?) } else { None };
            let agent = match (&kept, &made) { (Some(a), _) => a.clone(), (None, Some(k)) => keys::evm_address(k)?, _ => unreachable!() };
            if agent.to_lowercase() == owner.to_lowercase() { return fail("The agent key matches the main key.") }
            let nonce = (SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())) as u64;
            // In person every time: an open session from another move doesn't turn the key on.
            confirm_afresh(&format!("approve a Hyperliquid trading key: buys up to ${} an order without {PRESENCE} for {}, it can't withdraw money",
                                    limits::current().agent_order, agent_days_text()))?;
            let (act, signature) = with_key("evm", &format!("approve a trading key: orders without {PRESENCE}, and it can't withdraw money"),
                                            |k| hls::approve_agent(&agent, "ihatecrypto", nonce, k, true))?;
            // Kept only once its approval is signed.
            if let Some(k) = &made { agents::save(k, &id, None)? }
            renew_agent(&agent_renewed_key());
            vault::end_session();
            Ok(SignerReply { agent_address: Some(agent), action: Some(act), signature: Some(signature), nonce: Some(nonce),
                             checks: vec!["Approving an agent key created inside the signer: it can trade but not withdraw.".into()], ..reply() })
        }

        R::SetAgentActive { active: on } => {
            // Resuming a paused trading key, or one whose week is over, is the person's decision: Windows Hello afresh.
            if on && (!prefs::bool(&agent_active_key()) || !agent_fresh(&agent_renewed_key())) && agents::has(&wallets::active_id()) {
                confirm_afresh(&format!("turn the Hyperliquid trading key on for {}: buys up to ${} an order without {PRESENCE}, it can't withdraw money",
                                        agent_days_text(), limits::current().agent_order))?;
                vault::end_session();
                renew_agent(&agent_renewed_key());
            }
            prefs::set(&agent_active_key(), Some(Pref::Bool(on && agents::has(&wallets::active_id()))));
            Ok(SignerReply { agent_active: Some(prefs::bool(&agent_active_key())), ..reply() })
        }

        R::CreateWallet { require_presence } => {
            // Without Windows Hello it can only be a test wallet: a phrase made here and now, wiped once sent (host.rs).
            let phrase = Zeroizing::new(mnemonic::generate(None));
            confirm_in_person(&if require_presence { "add a new wallet to ihatecrypto and sign with it".to_string() }
                              else { format!("add a TEST wallet that signs WITHOUT {PRESENCE} — keep only a few dollars on it") })?;
            let mut r = store(&phrase, "", require_presence, !require_presence, false)?;
            r.phrase = Some(phrase.to_string());
            Ok(r)
        }

        R::CreateAutomationWallet {} => {
            // Next to a Windows Hello wallet only, and one: a budget beside the real money, never the only key.
            let all = wallets::all();
            if all.is_empty() || all.iter().any(SignerWallet::is_automation) {
                return fail(if all.is_empty() { "Add your main wallet first." } else { "There is an automation wallet already." });
            }
            confirm_in_person(&format!("add an AUTOMATION wallet: scheduled tasks sign with it WITHOUT {PRESENCE} — keep only their budget on it"))?;
            store(&mnemonic::generate(None), "", false, true, true)
        }

        R::AsWallet { id, request } => {
            let Some(wallet) = wallets::all().into_iter().find(|w| w.id == id && w.is_automation()) else {
                return fail("Only the automation wallet signs for a scheduled task.");
            };
            match *request {
                R::SignSolana { .. } | R::SignEvmTransaction { .. } | R::SignEvmBatch { .. } | R::SignHyperliquidL1 { .. } | R::SignHyperliquidUser { .. }
                | R::SignRelayNonceMapping { .. } | R::SignReceiveAuthorization { .. } | R::SignRelayMulticallPermit { .. } | R::SignSponsoredTransfer { .. }
                | R::SignNearIntentDeposit { .. } | R::SignNearTransfer { .. } | R::RelayOrder { .. } => {}
                _ => return fail("The automation wallet signs only a task's moves."),
            }
            wallets::as_wallet(wallet, || {
                // Its dollars counted by the signer, against the person's day and week (automation::spend): taken now,
                // given back when the move isn't signed.
                let held = automation::spend(&request)?;
                let reply = handle(*request)?;
                if reply.ok { held.keep()? }
                Ok(reply)
            })
        }

        R::ImportMnemonic { phrase, passphrase, require_presence } => {
            // Wiped once the seed is made.
            let (phrase, passphrase) = (Zeroizing::new(phrase), Zeroizing::new(passphrase));
            drop(Zeroizing::new(mnemonic::validate(&phrase)?)); // a mistyped phrase fails before any prompt
            confirm_in_person("add a wallet from a recovery phrase to ihatecrypto and sign with it")?;
            store(&phrase, &passphrase, require_presence, false, false)
        }

        R::AddressBook {} => Ok(SignerReply { addresses: Some(book::all()), ..reply() }),

        R::SaveAddresses { addresses: entries } => {
            if entries.is_empty() { return fail("Nothing to save.") }
            for e in &entries {
                let Some(net) = network(&e.network).filter(|_| !e.name.trim().is_empty()) else { return fail("An address needs a name and a network.") };
                if let Some(p) = address_check::problem(&e.address, net) { return fail(p) }
                if let Some(p) = address_check::label_problem(&e.name).or_else(|| address_check::label_problem(&e.memo)) { return fail(format!("{}: {p}", e.address)) }
            }
            // The whole address on the prompt, before the name, so no name can stand where an address should be; a twin
            // of an address already known goes under it (E-5, variant B).
            let known = known_addresses();
            let lines: Vec<String> = entries.iter().map(|e| {
                let title = network(&e.network).map_or(e.network.clone(), |n| n.title().to_string());
                format!("{} on {title} as «{}»{}{}", e.address, e.name, if e.memo.is_empty() { String::new() } else { format!(", memo {}", e.memo) }, twin(&e.address, &known))
            }).collect();
            vault::confirm(&format!("save to your address book (check every character):\n{}", lines.join("\n")), None)?;
            let mut saved = book::all();
            for e in &entries {
                saved.retain(|a| !(a.name.to_lowercase() == e.name.to_lowercase() && a.network == e.network));
                saved.push(e.clone());
            }
            let result = book::save(saved.clone());
            vault::end_session();
            result?;
            Ok(SignerReply { addresses: Some(saved), ..reply() })
        }

        R::RemoveAddress { name, network } => {
            let mut saved = book::all();
            saved.retain(|a| !(a.name.to_lowercase() == name.to_lowercase() && a.network == network));
            book::save(saved.clone())?;
            Ok(SignerReply { addresses: Some(saved), ..reply() })
        }

        R::ImportPrivateKey { key } => {
            let imported = keys::ImportedKey::parse(&key)?; // a malformed key fails before any prompt
            let address = imported.address()?;
            confirm_in_person(&format!("add the {} key of {address} to ihatecrypto and sign with it", imported.chain()))?;
            match imported {
                keys::ImportedKey::Solana(secret) => add(&*secret, "solana", Some(address), None, None, true, false, false),
                keys::ImportedKey::Evm(secret) => add(&*secret, "evm", None, Some(address), None, true, false, false),
            }
        }

        R::SetActiveWallet { id } => {
            let Some(wallet) = wallets::all().into_iter().find(|w| w.id == id) else { return fail("No such wallet.") };
            if wallet.is_automation() { return fail("The automation wallet signs only scheduled tasks' moves, so it can't be the wallet that's on.") }
            if Some(&wallet.id) != wallets::active().as_ref().map(|w| &w.id) {
                confirm_in_person(&format!("sign with the wallet «{}» from now on:\n{}", wallet.label, addresses(&wallet)))?;
                vault::end_session();
            }
            wallets::activate(Some(&wallet));
            Ok(SignerReply { wallets: Some(wallets::all()), active_wallet: Some(wallet.id), ..reply() })
        }

        R::RemoveWallet { id } => {
            let Some(wallet) = wallets::all().into_iter().find(|w| w.id == id) else { return fail("No such wallet.") };
            confirm_in_person(&format!("REMOVE the wallet «{}» from ihatecrypto: without its phrase or key its money can't be reached from here\n{}", wallet.label, addresses(&wallet)))?;
            remove(&wallet);
            Ok(SignerReply { wallets: Some(wallets::all()), active_wallet: wallets::active().map(|w| w.id), ..reply() })
        }

        R::RenameWallet { id, label } => {
            let name = label.trim().to_string();
            if name.is_empty() || name.chars().count() > 32 { return fail("A wallet's name is 1 to 32 characters.") }
            // No line breaks or invisible characters: a name shows on Windows Hello prompts beside the addresses.
            if let Some(p) = address_check::label_problem(&name) { return fail(p) }
            let mut list = wallets::all();
            let Some(w) = list.iter_mut().find(|w| w.id == id) else { return fail("No such wallet.") };
            w.label = name;
            wallets::save(&list);
            Ok(SignerReply { wallets: Some(list), active_wallet: wallets::active().map(|w| w.id), ..reply() })
        }

        R::NearAddress {} => {
            if let Some(known) = prefs::string("near") { return Ok(SignerReply { near_address: Some(known), ..reply() }) }
            // A phrase wallet imported before NEAR: derive once (one unlock), then keep it with the wallet.
            let near = with_key("near", "show your NEAR address, derived from your phrase", keys::near_implicit)?;
            prefs::set("near", Some(Pref::String(near.clone())));
            let mut list = wallets::all();
            let active = wallets::active_id();
            if let Some(w) = list.iter_mut().find(|w| w.id == active) {
                w.near = Some(near.clone());
                wallets::save(&list);
            }
            Ok(SignerReply { near_address: Some(near), ..reply() })
        }

        R::LedgerAgent { owner, create } => {
            let id = ledger_agent_id(&owner)?;
            if !agents::has(&id) && create {
                let key = agents::make()?;
                agents::save(&key, &id, None)?;
            }
            let address = agents::address(&id);
            let on = address.is_some() && prefs::string(&format!("agentActive-{id}")).as_deref() == Some("on");
            Ok(SignerReply { agent_address: address, agent_active: Some(on), ..reply() })
        }

        R::SetLedgerAgentActive { owner, active: on } => {
            let id = ledger_agent_id(&owner)?;
            if !agents::has(&id) { return fail("This Ledger has no trading key yet: approve one on the Ledger first.") }
            // Back on after a lock is the person's decision, as for a wallet's own key. The first time too: that the Ledger
            // approved this key is only the engine's word (the 2026-11-08 review). A refusal turns nothing on.
            let state = prefs::string(&format!("agentActive-{id}"));
            let renewed = format!("agentRenewed-{id}");
            if on && (state.is_none() || state.as_deref() == Some("paused") || (state.as_deref() == Some("on") && !agent_fresh(&renewed))) {
                vault::confirm(&format!("turn the Hyperliquid trading key of your Ledger {owner} on for {}: buys up to ${} an order without {PRESENCE}",
                                        agent_days_text(), limits::current().agent_order), None)?;
                renew_agent(&renewed);
                vault::end_session(); // a confirmation, not a move: it leaves no session for an unrelated signature
            }
            prefs::set(&format!("agentActive-{id}"), Some(Pref::String(if on { "on" } else { "paused" }.into())));
            // As kept, not as asked: a change the signer couldn't seal isn't reported as made.
            Ok(SignerReply { agent_active: Some(prefs::string(&format!("agentActive-{id}")).as_deref() == Some("on")), ..reply() })
        }

        R::SignLedgerAgentL1 { owner, action: act, nonce } => {
            let id = ledger_agent_id(&owner)?;
            if !hls::is_trading(&act) {
                return Ok(refused(problem("The Ledger's trading key signs orders, cancels and leverage only. Anything else is signed on the Ledger.")));
            }
            if !action::only_reduces(&act) { agents::ask_again() }
            let on = prefs::string(&format!("agentActive-{id}")).as_deref() == Some("on");
            let Some(agent) = on.then(|| agents::load(&id)).flatten() else {
                if on && agents::has(&id) { return fail(agents::unopened()) }
                return fail("The Ledger's trading key is off: approve it on the Ledger, or turn it back on.");
            };
            if !agent_fresh(&format!("agentRenewed-{id}")) && !action::only_reduces(&act) {
                return fail(format!("The Ledger's trading key opens positions for {} after {PRESENCE} and those are over, stops and closes still go. \
                                     Turn it off and on in Settings › Wallet › Hyperliquid to go on.", agent_days_text()));
            }
            let verdict = hl::check(&act, &owner, &HashSet::new(), None);
            if !verdict.ok() { return Ok(refused(verdict)) }
            // Within the person's limits only: there's no key here to sign the rest in person, so the Ledger signs it.
            let held = match agent_take(&action::opening(&act)) {
                Ok(held) => held,
                Err(why) => return Ok(refused(problem(&format!("The Ledger's trading key: {why}. {LEDGER_DEVICE}")))),
            };
            let signature = hls::sign_l1(&act, &agent, None, nonce, None, true)?;
            if let Some(h) = held { h.keep()? }
            Ok(SignerReply { signature: Some(signature), checks: verdict.checks, ..reply() })
        }

        R::Lock {} => {
            vault::end_session();
            prefs::set(&agent_active_key(), Some(Pref::Bool(false)));
            // A Ledger's trading keys pause too: turning one back on asks Windows Hello.
            for (key, value) in crate::trusted::state().prefs {
                if key.starts_with("agentActive-ledger-") && value == Pref::String("on".into()) { prefs::set(&key, Some(Pref::String("paused".into()))) }
            }
            Ok(SignerReply { agent_active: Some(false), checks: vec![format!("The {PRESENCE} session ended, so the trading key is paused.")], ..reply() })
        }

        R::SetSessionLength { seconds } => {
            if !session::CHOICES.contains(&seconds) { return fail("A confirmation can stay valid for 1, 5, 15 or 30 minutes.") }
            if seconds > seeds::session_length() { vault::confirm(&format!("skip {PRESENCE} for {} min after each confirmation", seconds / 60), None)? }
            prefs::set("sessionLength", Some(Pref::Int(seconds)));
            vault::end_session();
            Ok(SignerReply { session_length: Some(seconds), ..reply() })
        }

        R::SetLimits { limits: wanted } => {
            if let Some(p) = wanted.problem() { return fail(p) }
            let looser = wanted.looser(&limits::current());
            // In person even with no wallet yet: limits set silently then would hold for every wallet added later.
            if !looser.is_empty() && !holds_public_test_wallet() {
                vault::confirm(&format!("loosen your limits:\n{}", looser.join("\n")), None)?;
                vault::end_session(); // a confirmation, not a move: it leaves no session for an unrelated signature
            }
            limits::save(&wanted);
            Ok(SignerReply { limits: Some(limits::current()), ..reply() })
        }

        R::EndSession {} => {
            vault::end_session();
            Ok(reply())
        }

        R::SignNearIntentDeposit { amount, deposit_address, nonce, block_hash } => {
            let Some(yocto) = amount.parse::<u128>().ok().filter(|&y| y > 0) else { return fail(format!("Not a NEAR amount: {amount}.")) };
            if !(deposit_address.len() == 64 && deposit_address.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) {
                return fail(format!("Not a 1Click deposit account: {deposit_address}."));
            }
            if block_hash.len() != 32 { return fail("A NEAR block hash is 32 bytes.") }
            let doing = format!("sell {} NEAR: wrapped and sent to the 1Click deposit {}…{}", near::near(yocto), &deposit_address[..8], &deposit_address[58..]);
            approve(std::slice::from_ref(&deposit_address), false, &doing)?;
            let raw = with_near_key(&doing, |secret, account| {
                Ok(near::sign(near::WRAP_CONTRACT, nonce, &block_hash, &near::intent_deposit(account, &deposit_address, yocto), secret)?.0)
            })?;
            Ok(SignerReply { signed_raw: Some(raw), checks: vec![format!("{} NEAR → wNEAR → {deposit_address} (1Click)", near::near(yocto))], ..reply() })
        }

        R::SignNearTransfer { amount, receiver, recipients, nonce, block_hash } => {
            let Some(yocto) = amount.parse::<u128>().ok().filter(|&y| y > 0) else { return fail(format!("Not a NEAR amount: {amount}.")) };
            check_near_recipient(&receiver, &recipients, &block_hash)?;
            let doing = format!("send {} NEAR to {receiver}", near::near(yocto));
            approve(std::slice::from_ref(&receiver), false, &doing)?;
            let raw = with_near_key(&doing, |secret, account| {
                if receiver == account { return fail("That is this wallet's own NEAR account.") }
                Ok(near::sign(&receiver, nonce, &block_hash, &[Action::Transfer { deposit: yocto }], secret)?.0)
            })?;
            Ok(SignerReply { signed_raw: Some(raw), checks: vec![format!("{} NEAR → {receiver}", near::near(yocto))], ..reply() })
        }

        R::SignNearDeleteAccount { beneficiary, recipients, nonce, block_hash } => {
            check_near_recipient(&beneficiary, &recipients, &block_hash)?;
            approve(std::slice::from_ref(&beneficiary), false, &format!("delete your NEAR account into {beneficiary}"))?;
            // Always a fresh prompt, whatever session is open: this empties and removes the account.
            vault::confirm(&format!("DELETE your NEAR account and send all the NEAR on it to {beneficiary}"), None)?;
            let (raw, account) = with_near_key(&format!("delete your NEAR account into {beneficiary}"), |secret, account| {
                if beneficiary == account { return fail("An account can't be deleted into itself.") }
                Ok((near::sign(account, nonce, &block_hash, &[Action::DeleteAccount { beneficiary: beneficiary.clone() }], secret)?.0, account.to_string()))
            })?;
            Ok(SignerReply { signed_raw: Some(raw), checks: vec![format!("NEAR account {}… deleted into {beneficiary}", &account[..8.min(account.len())])], ..reply() })
        }

        R::SignAgentMessage { payload } => {
            let m = agent_owner::message(&payload)?;
            confirm_afresh(&m.doing)?;
            let raw = with_near_key(&m.doing, |secret, _| near_owner::nep413(&m.message, &m.nonce, &m.recipient, secret))?;
            Ok(SignerReply { signed_raw: Some(raw), checks: vec![format!("NEP-413 to {}: {}", m.recipient, m.doing)], ..reply() })
        }

        R::SignAgentDelegate { payload, nonce, max_block_height, policy } => {
            let d = agent_owner::delegate(&payload, &agent_owner::METHODS)?;
            // Rules are stored only beside the ones checked below (their hash in the call is the custody's own).
            agent_owner::rules_shown(&d.actions, policy.as_deref())?;
            // The rules in words from the signer's own reading: only addresses it knows, the owner being this wallet.
            let rules = match &policy {
                Some(json) => {
                    let owner = address("near")?;
                    let mut known = recipients::all(recipients::List::Money);
                    known.extend(book::all().into_iter().map(|a| a.address));
                    Some(agent_owner::policy(json, &owner, &own::all(), &known)?)
                }
                None => None,
            };
            let doing = format!("agent wallet on NEAR Intents: {}{}", d.doing, rules.map_or(String::new(), |r| format!(" — {r}")));
            confirm_afresh(&doing)?;
            let raw = with_near_key(&doing, |secret, _| near_owner::sign_delegate(&d.receiver, &d.actions, nonce, max_block_height, secret))?;
            Ok(SignerReply { signed_raw: Some(raw), checks: vec![format!("NEP-366 to {}: {}", d.receiver, d.doing)], ..reply() })
        }

        R::Forget {} => {
            let Some(wallet) = wallets::active() else { return Ok(reply()) };
            // Its addresses on the prompt, not only its name: the engine can rename a wallet without asking (the 2026-11-08 review).
            confirm_in_person(&format!("remove the wallet «{}» from ihatecrypto\n{}", wallet.label, addresses(&wallet)))?;
            remove(&wallet);
            Ok(reply())
        }

        R::SignSolanaMessage { message } => {
            // A «message» that parses as a transaction would be a signed transaction once returned.
            let text = std::str::from_utf8(&message).ok()
                .filter(|t| Message::parse(&message).is_err() && !t.chars().any(|c| (c as u32) < 0x20 && !['\n', '\r', '\t'].contains(&c)));
            let Some(text) = text else { return Ok(refused(problem("This isn't text but data that looks like a transaction: message signing refused."))) };
            let signature = with_key("solana", &personal::prompt(text, 160), |k| {
                let secret: [u8; 32] = k.try_into().map_err(|_| WalletError("An ed25519 private key is 32 bytes.".into()))?;
                Ok(ed25519_dalek::SigningKey::from_bytes(&secret).sign(&message).to_bytes().to_vec())
            })?;
            Ok(SignerReply { signature_bytes: Some(signature), checks: vec!["Only text was signed, not a transaction.".into()], ..reply() })
        }

        R::SignPersonal { message } => {
            if personal::is_blind_hash(&message) || message.len() == 32 {
                return Ok(refused(problem("The site asks to sign a hash, not text: such a signature could authorize anything.")));
            }
            let text = std::str::from_utf8(&message).map_or_else(|_| format!("{} bytes", message.len()), str::to_string);
            let signature = with_key("evm", &personal::prompt(&text, 160), |k| sign(&personal::digest(&message), k))?;
            Ok(SignerReply { signature: Some(signature), checks: vec!["Text was signed, not a transaction or an approval.".into()], ..reply() })
        }

        R::SignTypedData { json } => {
            let typed = TypedData::parse(&json)?;
            if let Some(r) = typed.refusal_for(address("evm").ok().as_deref()) { return Ok(refused(problem(&r))) }
            let signature = with_key("evm", &typed.prompt(), |k| sign(&typed.digest, k))?;
            Ok(SignerReply { signature: Some(signature),
                             checks: vec![format!("Signed {} data for «{}». The hash was computed by the signer.", typed.primary_type, typed.domain_name)], ..reply() })
        }

        R::SignEvmTransaction { chain_id, nonce, max_priority_fee, max_fee, gas_limit, to, value, data, recipients } => {
            let owner = address("evm")?;
            let mut v = Verdict::default();
            let call = Call::decode(&data);
            let approved: HashSet<String> = recipients.iter().map(|r| r.to_lowercase()).chain([owner.to_lowercase()]).collect();
            let tx = Transaction { chain_id, nonce, max_priority_fee_per_gas: max_priority_fee, max_fee_per_gas: max_fee, gas_limit, to: to.clone(), value,
                                   data: data.clone(), authorizations: vec![] };
            if !SIGNABLE.contains(&chain_id) { v.problems.push(format!("ihatecrypto doesn't sign for network {chain_id}.")) }
            v.problems.extend(call.refusal(&owner, Some(chain_id)));
            v.problems.extend(Call::target_refusal(&to, &data, &owner));
            v.problems.extend(batch7702::self_call(&to, &data, &owner));
            let relay_payees = match Call::relay_deposit_id(&to, &data) {
                Some(id) => relay_orders::order(&id).map(|o| vec![o.recipient]),
                None => Some(vec![]),
            };
            if relay_payees.is_none() { v.problems.push(relay_orders::UNCHECKED.into()) }
            v.problems.extend(tx.fee_problem());
            // Coins, tokens, NFTs and spending rights go only to this wallet or an address approved in person.
            for payee in call.payees(&to, value, chain_id) {
                if !approved.contains(&payee.to_lowercase()) { v.problems.push(format!("Money or the right to spend it goes to an unapproved address {payee}.")) }
            }
            if !v.ok() { return Ok(refused(v)) }
            // A contract whose call can't be read is asked about in person once, as a call only.
            let doing = format!("{}\nnetwork fee {}", call.summary(chain_id, &to, value), tx.fee_text());
            let target = call.unread_target(&to, value, chain_id);
            let mut money: Vec<String> = recipients.iter().filter(|r| Some(recipients::key(r)) != target).cloned().collect();
            money.extend(relay_payees.unwrap_or_default());
            approve(&money, false, &doing)?;
            if let Some(t) = &target { approve(std::slice::from_ref(t), true, &doing)? }
            let raw = with_key("evm", &doing, |k| Ok(tx.signed(k)?.0))?;
            Ok(SignerReply { signed_raw: Some(raw),
                             checks: vec!["Calldata parsed by the signer: no unlimited approvals, no spending by others, money only to approved addresses.".into()], ..reply() })
        }

        R::SignSafeMove { chain_id, safe: s, to, value, data, nonce, recipients } => {
            let owner = address("evm")?;
            let m = safe::Move { chain_id, safe: s.clone(), to, value, data, nonce };
            let approved: HashSet<String> = recipients.iter().chain([&owner, &s]).map(|a| a.to_lowercase()).collect();
            if let Some(why) = safe::refusal(&m, &approved) { return Ok(refused(problem(&why))) }
            let doing = format!("approve the multisig's move: {}", m.summary());
            approve(&recipients, false, &doing)?;
            let signature = with_key("evm", &doing, |k| sign(&m.hash()?, k))?;
            Ok(SignerReply { signature: Some(signature), checks: vec!["Multisig move hashed by the signer: a plain payment, money only to approved addresses.".into()], ..reply() })
        }

        R::SignEvmBatch { chain_id, nonce, max_priority_fee, max_fee, gas_limit, calls, recipients } => {
            let owner = address("evm")?;
            let calls: Vec<batch7702::BatchCall> = calls.into_iter().map(|c| batch7702::BatchCall { to: c.to, data: c.data }).collect();
            let mut v = Verdict { problems: batch7702::problems(&calls, &owner, chain_id), ..Default::default() };
            if !SIGNABLE.contains(&chain_id) { v.problems.push(format!("ihatecrypto doesn't sign for network {chain_id}.")) }
            let fee = Transaction { chain_id, nonce, max_priority_fee_per_gas: max_priority_fee, max_fee_per_gas: max_fee, gas_limit, to: owner.clone(),
                                    value: 0, data: vec![], authorizations: vec![] };
            v.problems.extend(fee.fee_problem());
            let approved: HashSet<String> = recipients.iter().map(|r| r.to_lowercase()).chain([owner.to_lowercase()]).collect();
            let mut relay_payees = Vec::new();
            for c in &calls {
                for payee in Call::decode(&c.data).payees(&c.to, 0, chain_id) {
                    if !approved.contains(&payee.to_lowercase()) { v.problems.push(format!("Money or the right to spend it goes to an unapproved address {payee}.")) }
                }
                if let Some(id) = Call::relay_deposit_id(&c.to, &c.data) {
                    match relay_orders::order(&id) { Some(o) => relay_payees.push(o.recipient), None => v.problems.push(relay_orders::UNCHECKED.into()) }
                }
            }
            if !v.ok() { return Ok(refused(v)) }
            // The prompt names every call, then that they go as one — and that the wallet keeps running the batch
            // account's code on that network afterwards.
            let lines: Vec<String> = calls.iter().map(|c| Call::decode(&c.data).summary(chain_id, &c.to, 0)).collect();
            let doing = format!("{}\nas one transaction: this wallet runs the audited batch account on {} (EIP-7702)\nnetwork fee {}",
                                lines.join("\nthen "), network_name(chain_id), fee.fee_text());
            let targets: Vec<String> = calls.iter().filter_map(|c| Call::decode(&c.data).unread_target(&c.to, 0, chain_id)).collect();
            let payees: HashSet<String> = calls.iter().flat_map(|c| Call::decode(&c.data).payees(&c.to, 0, chain_id)).map(|p| recipients::key(&p)).collect();
            let mut money: Vec<String> = recipients.iter().filter(|r| !targets.contains(&recipients::key(r)) || payees.contains(&recipients::key(r))).cloned().collect();
            money.extend(relay_payees);
            approve(&money, false, &doing)?;
            if !targets.is_empty() { approve(&targets, true, &doing)? }
            let raw = with_key("evm", &doing, |k| Ok(batch7702::signed(chain_id, nonce, max_priority_fee, max_fee, gas_limit, &owner, &calls, k)?.0))?;
            Ok(SignerReply { signed_raw: Some(raw),
                             checks: vec!["Each call parsed by the signer and held to the one-call rules. The wallet runs only the audited batch account's code.".into()], ..reply() })
        }

        R::SignSolana { wire, accounts, recipients, token_owners, extra_programs, sponsor } => {
            let owner = address("solana")?;
            let sponsor_key = match &sponsor {
                Some(s) => match base58::decode(s) { Ok(k) if k.len() == 32 => Some(k), _ => return fail("The fee payer isn't a Solana address.") },
                None => None,
            };
            let message = Message::parse_transaction(&wire)?;
            // Lookup tables are resolved by the engine; the static part must be exactly what is in the transaction.
            if accounts.len() < message.static_keys.len() || accounts[..message.static_keys.len()] != message.static_keys[..] {
                return fail("The account list doesn't match the transaction.");
            }
            let mut programs = sol::known_programs();
            for p in &extra_programs {
                programs.entry(p.clone()).or_insert_with(|| format!("program {}… (approved by you)", &p[..6.min(p.len())]));
            }
            let policy = sol::Policy { recipients: recipients.iter().cloned().collect(), programs, trust_lookup_tables: false, sponsor: sponsor.clone(),
                                       max_slippage_bps: limits::current().slippage_bps, ..sol::Policy::new(&owner) };
            let verdict = sol::check(&message, &accounts, &policy, &verified_owners(&token_owners, &accounts));
            if !verdict.ok() { return Ok(refused(verdict)) }
            let relay_payees: Vec<String> = sol::relay_deposit_ids(&message).iter().map(|id| relay_orders::require(id).map(|o| o.recipient)).collect::<Result<_>>()?;
            // The programs the person allowed for this one transaction are named in full: they run with the wallet's authority.
            let known = sol::known_programs();
            let extra: Vec<String> = extra_programs.iter().filter(|p| !known.contains_key(*p) && base58::decode(p).is_ok_and(|k| k.len() == 32))
                .map(|p| format!("runs {p}, not on ihatecrypto's list")).collect();
            let payer: Vec<String> = sponsor.iter().map(|s| format!("the network fee is paid by {}…{}, not by you", &s[..4.min(s.len())], &s[s.len().saturating_sub(4)..])).collect();
            let doing = [vec![prompt("sign a Solana transaction", &verdict)], extra.clone(), payer].concat().join("\n");
            // The fee payer may get rent back from this wallet: then it's a payee like any other.
            let payee: Vec<String> = sponsor.iter().filter(|s| sol::refunds_sponsor(&message, &accounts, &owner, s)).cloned().collect();
            approve(&[recipients, payee, relay_payees].concat(), false, &doing)?;
            // A program not on the list runs with this wallet's authority: every such transaction asks in person afresh.
            if !extra.is_empty() { confirm_afresh(&format!("{doing}\nit can move anything in this wallet: sign only if you trust this site"))? }
            // A swap a person signs gets a prompt of its own, whatever session is open: its rate is theirs to judge (the
            // automation wallet's swaps are held to its dollars instead). Past the person's limits (slippage, a site's
            // fee): in person afresh, even for the automation wallet.
            if !verdict.ask_afresh.is_empty() || (verdict.swaps && wallets::signing().is_none()) {
                confirm_afresh(&std::iter::once(doing.clone()).chain(verdict.ask_afresh.iter().cloned()).collect::<Vec<_>>().join("\n"))?;
            }
            let signed = with_key("solana", &doing, |k| Ok(solana::sign(&wire, k, sponsor_key.as_deref())?.1))?;
            Ok(SignerReply { signed_wire: Some(signed), checks: verdict.checks, ..reply() })
        }

        R::SignReceiveAuthorization { .. } => {
            // Relay's v3 proxy alone: nothing this signature covers names the order. Off (the owner, 2026-10-24).
            Ok(refused(problem("A payment by signature that names no Relay order: whom it pays can't be checked, so ihatecrypto doesn't sign it.")))
        }

        R::SignX402 { chain_id, token, to, value, valid_before, nonce, service } => {
            let owner = address("evm")?;
            let a = x402::Authorization { chain_id, token, from: owner.clone(), to, value, valid_before, nonce };
            if let Some(r) = a.refusal(&owner, now()) { return Ok(refused(problem(&r))) }
            // Each payment confirmed in person with its amount and payee: the payee isn't saved as a recipient, so it can't
            // receive anything else later without its own prompt.
            let name: String = service.chars().filter(|&c| !address_check::hidden(c)).take(60).collect();
            let doing = format!("{} · {name}", a.summary());
            confirm_afresh(&doing)?;
            let digest = a.typed()?.parts()?.digest();
            let signature = with_key("evm", &doing, |k| sign(&digest, k))?;
            Ok(SignerReply { signatures: Some(vec![signature]),
                             checks: vec!["The signer built the x402 authorization itself: a dollar it knows, from this wallet, at most $5, valid an hour.".into()], ..reply() })
        }

        R::SignSponsoredTransfer { chain_id, nonce, gas_limit, token, to, amount, recipients } => {
            let owner = address("evm")?;
            let t = SponsoredTransfer { chain_id, nonce, gas_limit, token, to: to.clone(), amount };
            let mut v = Verdict::default();
            v.problems.extend(t.refusal());
            let approved: HashSet<String> = recipients.iter().map(|r| recipients::key(r)).chain([recipients::key(&owner)]).collect();
            if !approved.contains(&recipients::key(&to)) { v.problems.push(format!("Money goes to an unapproved address {to}.")) }
            if !v.ok() { return Ok(refused(v)) }
            let ask: Vec<String> = recipients.iter().filter(|r| recipients::key(r) == recipients::key(&to) && recipients::key(r) != recipients::key(&owner)).cloned().collect();
            approve(&ask, false, &t.summary())?;
            let raw = with_key("evm", &t.summary(), |k| Ok(t.signed(k)?.0))?;
            Ok(SignerReply { signed_raw: Some(raw), checks: vec!["The signer built the transfer itself: a dollar it knows, to an approved address, gas price zero.".into()], ..reply() })
        }

        R::SignRelayMulticallPermit { chain_id, token, value, valid_before, relayer, refund_to, nft_recipient, metadata, calls } => {
            let owner = address("evm")?;
            let m = relay::MulticallPermit {
                chain_id, token, from: owner.clone(), value, valid_before, relayer, refund_to, nft_recipient, metadata,
                calls: calls.into_iter().map(|c| relay::Call { target: c.target, allow_failure: c.allow_failure, value: c.value, call_data: c.data }).collect(),
            };
            let Some(id) = m.deposit_id() else { return fail(relay_orders::UNCHECKED) };
            let order = relay_orders::require(&id)?;
            approve(std::slice::from_ref(&order.recipient), false, &order.summary())?;
            if let Some(r) = m.refusal(&owner, now()) { return Ok(refused(problem(&r))) }
            let permit: Permit3009 = m.permit()?;
            let digests = [m.digest()?, permit.typed()?.parts()?.digest()];
            let signatures = with_key("evm", &format!("{}\ninto Relay's depository for this wallet", permit.summary()), |k| {
                digests.iter().map(|d| sign(d, k)).collect::<Result<Vec<_>>>()
            })?;
            Ok(SignerReply { signatures: Some(signatures),
                             checks: vec!["The signer read Relay's calls itself (one deposit into its depository for this wallet) and computed the nonce they bind.".into()], ..reply() })
        }

        R::SignHyperliquidL1 { action: act, nonce, mainnet } => {
            let verdict = hl::check(&act, &address("evm")?, &HashSet::new(), None);
            if !verdict.ok() { return Ok(refused(verdict)) }
            // Orders, cancels and leverage go through the agent when it is active: it cannot move money out, so no
            // prompt. Past its days it still cancels and reduces; what opens or adds takes the main key in person.
            let id = wallets::active_id();
            let agent_on = hls::is_trading(&act) && prefs::bool(&agent_active_key());
            let reduces = action::only_reduces(&act);
            // Windows keeps the key under Windows Hello, opened once a run: an order that opens or adds comes from a card
            // just confirmed, so it may ask at once.
            if agent_on && !reduces { agents::ask_again() }
            let fresh = agent_fresh(&agent_renewed_key()) || reduces;
            // Within the person's limits (Settings): an opening buy up to its price × size, under one order's and the day's
            // dollars. An opening sell or TWAP, or more, takes the main key in person (the 2026-11-08 review).
            // Taken from the day in the same step as the check, before signing; given back if it isn't signed.
            let mut over = None;
            if agent_on && fresh && agents::has(&id) {
                match agent_take(&action::opening(&act)) {
                    Err(why) => over = Some(why),
                    Ok(held) => {
                        // A key that's there but wasn't opened: refused, never handed to the main key.
                        let Some(agent) = agents::load(&id) else { return fail(agents::unopened()) };
                        let signature = hls::sign_l1(&act, &agent, None, nonce, None, mainnet)?;
                        if let Some(h) = held { h.keep()? }
                        return Ok(SignerReply { signature: Some(signature), checks: verdict.checks, ..reply() });
                    }
                }
            }
            let key_on = agent_on && agents::has(&id);
            let stale = key_on && !fresh;
            let kind = action::kind(&act).unwrap_or("");
            // A TWAP buys on for hours: started only with a trading key on, the one thing that can stop it unattended.
            if kind == "twapOrder" && !key_on {
                return Ok(refused(problem("A TWAP needs the Hyperliquid trading key on: locked or off, nothing could stop it unattended.")));
            }
            let what = match kind {
                "setReferrer" => "use ihatecrypto's referral code on Hyperliquid: 4% off its fees".to_string(),
                "vaultTransfer" => "move money between your Hyperliquid account and a vault".into(),
                _ if stale => format!("sign a Hyperliquid order: the trading key's {} are over, and this renews them", agent_days_text()),
                _ => over.as_ref().map_or("sign a Hyperliquid order".into(), |why| format!("sign a Hyperliquid order: {why}")),
            };
            // Renewing the trading key's days, an order past its limits, and a vault deposit (money locked for days), ask
            // in person afresh.
            let deposit = kind == "vaultTransfer" && act.get("isDeposit") == Some(&MsgPack::Bool(true));
            if stale || over.is_some() || deposit { confirm_afresh(&prompt(&what, &verdict))? }
            let signature = with_key("evm", &prompt(&what, &verdict), |k| hls::sign_l1(&act, k, None, nonce, None, mainnet))?;
            if stale { renew_agent(&agent_renewed_key()) }
            Ok(SignerReply { signature: Some(signature), checks: verdict.checks, ..reply() })
        }

        R::RelayOrder { order: json } => {
            let order = RelayOrder::read(&json, address("evm").ok().as_deref(), address("solana").ok().as_deref(), now())
                .map_err(|r| WalletError(r.reason))?;
            // Held, not yet approved: its payee is asked about with the deposit that names it, after Confirm.
            let check = format!("Relay order checked by the signer: it pays {} on {}, and refunds come back to this wallet.", order.recipient, order.output_chain);
            relay_orders::add(order);
            Ok(SignerReply { checks: vec![check], ..reply() })
        }

        R::SignRelayNonceMapping { wallet: w, depositor, id, nonce } => {
            let order = relay_orders::require(&id)?;
            approve(std::slice::from_ref(&order.recipient), false, &order.summary())?;
            let owner = address("evm")?.to_lowercase();
            if w.to_lowercase() != owner || depositor.to_lowercase() != owner {
                return Ok(refused(problem(&format!("The deposit mapping isn't for your wallet: {w}, {depositor}."))));
            }
            let digest = hls::relay_nonce_mapping_digest(&w, &depositor, &id, nonce)?;
            let signature = with_key("evm", "link a Hyperliquid withdrawal to a Relay order", |k| sign(&digest, k))?;
            Ok(SignerReply { signature: Some(signature), checks: vec!["Deposit mapping: the wallet and depositor are yours.".into()], ..reply() })
        }

        R::SignHyperliquidUser { action: act, mainnet, recipients } => {
            let verdict = hl::check(&act, &address("evm")?, &recipients.iter().cloned().collect(), None);
            if !verdict.ok() { return Ok(refused(verdict)) }
            let (kind, fields) = hls::user_action(&act)?;
            let revoke = act.text("maxFeeRate") == Some(builder::REVOKE);
            let what = if kind != "ApproveBuilderFee" { "sign a Hyperliquid transfer" }
                       else if revoke { "take back ihatecrypto's fee approval" } else { "approve ihatecrypto's fee on your Hyperliquid trades" };
            approve(&recipients, false, &prompt(what, &verdict))?;
            // Approving ihatecrypto's fee is the person's own opt-in: a fresh prompt, never an open session.
            if kind == "ApproveBuilderFee" && !revoke { confirm_afresh(&prompt(what, &verdict))? }
            let signature = with_key("evm", &prompt(what, &verdict), |k| hls::sign_user_action(&kind, fields.clone(), k, mainnet))?;
            Ok(SignerReply { signature: Some(signature), checks: verdict.checks, ..reply() })
        }
    }
}

fn network(raw: &str) -> Option<Network> { serde_json::from_value(serde_json::Value::String(raw.into())).ok() }

fn problem(p: &str) -> Verdict { Verdict { problems: vec![p.into()], ..Default::default() } }

/// A phrase wallet: its seed and the public addresses derived from it.
fn store(phrase: &str, passphrase: &str, require_presence: bool, test: bool, automation: bool) -> Result<SignerReply> {
    let seed = mnemonic::seed(phrase, passphrase)?;
    let solana = keys::solana_address(&*keys::ed25519(&*seed, &path::solana(0))?)?;
    let evm = keys::evm_address(&*keys::secp256k1(&*seed, &path::evm(0))?)?;
    let near = keys::near_implicit(&*keys::ed25519(&*seed, &path::near(0))?)?;
    add(&*seed, "phrase", Some(solana), Some(evm), Some(near), require_presence, test, automation)
}

/// Adds a wallet (or re-seals the same one: same kind and addresses keep its id, trading key and approved addresses)
/// and signs with it from now on — except the automation wallet, which signs only a task's moves.
#[allow(clippy::too_many_arguments)]
fn add(secret: &[u8], kind: &str, solana: Option<String>, evm: Option<String>, near: Option<String>, require_presence: bool,
       test: bool, automation: bool) -> Result<SignerReply> {
    let mut list = wallets::all();
    let same = list.iter().find(|w| w.kind == kind && w.solana == solana && w.evm.as_ref().map(|e| e.to_lowercase()) == evm.as_ref().map(|e| e.to_lowercase())).cloned();
    let id = same.as_ref().map(|w| w.id.clone()).unwrap_or_else(|| {
        if list.is_empty() { SignerWallet::FIRST_ID.into() } else { wallet::to_hex(&rand::<4>()) }
    });
    seeds::save(secret, require_presence, &id, test)?;
    if solana.as_deref() != Some(PUBLIC_TEST_SOLANA) { own::add(&[solana.clone(), evm.clone()].into_iter().flatten().collect::<Vec<_>>()) }
    let tests = list.iter().filter(|w| w.is_test()).count();
    let label = same.map(|w| w.label).unwrap_or_else(|| {
        if automation { "Automation".into() } else if test { if tests == 0 { "Test".into() } else { format!("Test {}", tests + 1) } }
        else if list.is_empty() { "Main".into() } else { format!("Wallet {}", list.len() + 1) }
    });
    let wallet = SignerWallet { id: id.clone(), label, kind: kind.into(), solana: solana.clone(), evm: evm.clone(), near: near.clone(),
                                test: (test && !automation).then_some(true), automation: automation.then_some(true) };
    match list.iter_mut().find(|w| w.id == id) { Some(w) => *w = wallet.clone(), None => list.push(wallet.clone()) }
    wallets::save(&list);
    if automation {
        return Ok(SignerReply { has_wallet: Some(true), wallets: Some(list), active_wallet: wallets::active().map(|w| w.id), ..reply() });
    }
    wallets::activate(Some(&wallet));
    Ok(SignerReply { has_wallet: Some(true), solana_address: solana, evm_address: evm, near_address: near, wallets: Some(list), active_wallet: Some(id), ..reply() })
}

fn rand<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the system's randomness");
    b
}

/// Removes a wallet with everything kept for it; signing moves to the first one left that isn't the automation wallet.
fn remove(wallet: &SignerWallet) {
    let was_active = wallets::active().is_some_and(|w| w.id == wallet.id);
    seeds::delete(&wallet.id);
    agents::delete(&wallet.id);
    recipients::delete(&wallet.id);
    own::remove(&[wallet.solana.clone(), wallet.evm.clone()].into_iter().flatten().collect::<Vec<_>>());
    prefs::remove(&wallets::scoped("agentActive", &wallet.id));
    let rest: Vec<SignerWallet> = wallets::all().into_iter().filter(|w| w.id != wallet.id).collect();
    wallets::save(&rest);
    if was_active { wallets::activate(SignerWallet::on(&rest, None)) }
}

/// Relay orders this signer checked (relayOrder), by id, until their deadline: a deposit naming any other id isn't
/// signed, so whom a bridge pays is never only the engine's word.
pub mod relay_orders {
    use super::*;

    static HELD: LazyLock<Mutex<HashMap<Vec<u8>, RelayOrder>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    pub const UNCHECKED: &str = "A Relay deposit for an order the signer hasn't checked: nothing was signed.";

    pub fn add(order: RelayOrder) {
        let t = now();
        let mut held = HELD.lock().expect("relay");
        held.retain(|_, o| o.deadline > t);
        held.insert(order.id.to_vec(), order);
    }

    pub fn order(id: &[u8]) -> Option<RelayOrder> { HELD.lock().expect("relay").get(id).filter(|o| o.deadline > now()).cloned() }

    pub fn require(id: &[u8]) -> Result<RelayOrder> { order(id).ok_or_else(|| WalletError(UNCHECKED.into())) }
}

/// A paid API's price by x402, the `exact` scheme on EVM (`X402Authorization`, Chains/EVM/Permit3009.swift): EIP-3009
/// TransferWithAuthorization of a dollar the signer knows by its own domain (Permit3009's tokens), from this wallet to the
/// service's `payTo`, which the service's facilitator sends and pays the fee for. The signer builds the typed data itself
/// and refuses more than `CAP` or longer than an hour.
pub mod x402 {
    use evm::U256;
    use wallet::evm::eip712::{Field, Typed, Value};

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Authorization {
        pub chain_id: u64,
        pub token: String,
        pub from: String,
        pub to: String,
        pub value: u64,
        pub valid_before: u64,
        pub nonce: Vec<u8>,
    }

    /// At most 5 dollars a payment: x402 prices an API call, not a transfer.
    pub const CAP: u64 = 5_000_000;

    fn dollars(units: u64) -> String { evm::units(&U256::from(units), 6) }

    impl Authorization {
        fn known(&self) -> Option<(&'static str, &'static str, &'static str)> { Permit3009::token_info(self.chain_id, &self.token) }

        /// Why the signer won't sign it, or None. `now`: Unix seconds.
        pub fn refusal(&self, owner: &str, now: u64) -> Option<String> {
            if self.known().is_none() || self.chain_id != 8453 {
                return Some(format!("{} on network {} isn't a dollar ihatecrypto pays an API with (USDC on Base only).", self.token, self.chain_id));
            }
            if self.from.to_lowercase() != owner.to_lowercase() { return Some("The payment isn't from this wallet.".into()) }
            let address = self.to.len() == 42 && self.to.starts_with("0x") && self.to[2..].bytes().all(|b| b.is_ascii_hexdigit());
            if !address || self.to.to_lowercase() == owner.to_lowercase() { return Some(format!("Not a service's address: {}.", self.to)) }
            if self.value == 0 || self.value > CAP {
                return Some(format!("An API payment is at most {} dollars. This one asks {}.", dollars(CAP), dollars(self.value)));
            }
            if self.nonce.len() != 32 { return Some("The payment by signature is incomplete.".into()) }
            if self.valid_before <= now || self.valid_before > now + 3600 { return Some("The signature would stay valid for more than an hour, or has run out.".into()) }
            None
        }

        pub fn typed(&self) -> Result<Typed> {
            let Some((name, version, _)) = self.known() else { return fail(format!("Unknown dollar token {} on network {}.", self.token, self.chain_id)) };
            Ok(Typed {
                domain: vec![Field::new("name", "string", Value::String(name.into())), Field::new("version", "string", Value::String(version.into())),
                             Field::new("chainId", "uint256", Value::Uint(self.chain_id)), Field::new("verifyingContract", "address", Value::Address(self.token.clone()))],
                ty: "TransferWithAuthorization".into(),
                message: vec![Field::new("from", "address", Value::Address(self.from.clone())), Field::new("to", "address", Value::Address(self.to.clone())),
                              Field::new("value", "uint256", Value::Uint(self.value)), Field::new("validAfter", "uint256", Value::Uint(0)),
                              Field::new("validBefore", "uint256", Value::Uint(self.valid_before)), Field::new("nonce", "bytes32", Value::Bytes32(self.nonce.clone()))],
            })
        }

        pub fn summary(&self) -> String {
            format!("pay {} {} on {} to {} for an API (x402, its facilitator pays the fee)", dollars(self.value), self.known().map_or("dollars", |k| k.2),
                    network_name(self.chain_id), self.to)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_small_payments_from_this_wallet() {
            let (me, service, usdc) = ("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", "0x70997970C51812dc3A010C7d01b50e0d17dc79C8", "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913");
            let now = 1_800_000_000;
            let a = |to: &str, value: u64, before: u64, from: &str, token: &str| Authorization {
                chain_id: 8453, token: token.into(), from: from.into(), to: to.into(), value, valid_before: before, nonce: vec![1; 32] };
            let ok = a(service, 1000, 1_800_000_300, me, usdc);
            assert_eq!(ok.refusal(me, now), None);
            assert!(a(service, 5_000_001, 1_800_000_300, me, usdc).refusal(me, now).unwrap().ends_with("This one asks 5.000001."));
            assert!(a(service, 1000, now + 7200, me, usdc).refusal(me, now).is_some());
            assert!(a(service, 1000, 1_800_000_300, service, usdc).refusal(me, now).is_some());
            assert!(a(me, 1000, 1_800_000_300, me, usdc).refusal(me, now).is_some());
            assert!(a(service, 1000, 1_800_000_300, me, "0x0000000000000000000000000000000000000001").refusal(me, now).is_some());
            assert!(Authorization { chain_id: 42161, token: "0xaf88d065e77c8cc2239327c5edb3a432268e5831".into(), ..ok.clone() }.refusal(me, now).is_some());
            assert_eq!(ok.summary(), format!("pay 0.001 USDC on Base to {service} for an API (x402, its facilitator pays the fee)"));
            // The digest the signer signs is EIP-3009's TransferWithAuthorization under USDC's own domain on Base.
            let parts = ok.typed().unwrap().parts().unwrap();
            assert_ne!(parts.digest(), Permit3009 { chain_id: 8453, token: usdc.into(), from: me.into(), to: service.into(), value: 1000,
                                                    valid_before: 1_800_000_300, nonce: vec![1; 32] }.typed().unwrap().parts().unwrap().digest());
        }
    }
}

/// Where a Ledger's trading key is kept: by its account, which must be an EVM address.
fn ledger_agent_id(owner: &str) -> Result<String> {
    let ok = owner.len() == 42 && owner.starts_with("0x") && owner[2..].bytes().all(|b| b.is_ascii_hexdigit());
    if !ok { return fail(format!("Not an EVM address: {owner}.")) }
    Ok(format!("ledger-{}", owner.to_lowercase()))
}

/// A wallet's addresses for a prompt: its name is the engine's to set, its addresses are not.
fn addresses(w: &SignerWallet) -> String {
    [w.solana.as_ref().map(|a| format!("Solana {a}")), w.evm.as_ref().map(|a| format!("EVM {a}")), w.near.as_ref().map(|a| format!("NEAR {a}"))]
        .into_iter().flatten().collect::<Vec<_>>().join("\n")
}

/// The active wallet's key for one chain («solana», «evm», «near»), after Windows Hello or within its session: derived
/// from its phrase, or its one imported key. The key lives only while `body` signs with it, then it and the seed are wiped.
fn with_key<T>(chain: &str, reason: &str, body: impl FnOnce(&[u8]) -> Result<T>) -> Result<T> {
    let Some(wallet) = wallets::active() else { return fail("No wallet imported.") };
    if let Some(m) = wallet.missing(chain) { return fail(m) }
    let secret = seeds::load(reason, &wallet.id, wallets::signing().is_none())?;
    if wallet.kind != "phrase" { // a key wallet holds that chain's key itself
        vouch(chain, &secret, &wallet)?;
        return body(&secret);
    }
    vouch_seed(&secret, &wallet)?;
    let key = match chain {
        "solana" => keys::ed25519(&secret, &path::solana(0))?,
        "evm" => keys::secp256k1(&secret, &path::evm(0))?,
        _ => keys::ed25519(&secret, &path::near(0))?,
    };
    body(&*key)
}

/// The address `key` signs for must be the one the checks took as «ours»; then it's kept among ours. The public test
/// wallet stays out: anyone holds its key.
fn vouch(chain: &str, key: &[u8], wallet: &SignerWallet) -> Result<()> {
    let a = match chain { "solana" => keys::solana_address(key).ok(), "evm" => keys::evm_address(key).ok(), _ => None };
    let Some(a) = a else { return Ok(()) };
    if address(chain).is_ok_and(|claimed| recipients::key(&claimed) != recipients::key(&a)) {
        return fail(format!("This wallet's saved {chain} address isn't the one its key signs for: nothing was signed."));
    }
    if wallet.solana.as_deref() != Some(PUBLIC_TEST_SOLANA) { own::add(&[a]) }
    Ok(())
}

/// A phrase wallet's every saved address, not only the one this signature is for: one changed in the settings is caught
/// at the first signature of any kind.
fn vouch_seed(seed: &[u8], wallet: &SignerWallet) -> Result<()> {
    for chain in ["solana", "evm", "near"] {
        let own_address = match chain {
            "evm" => keys::evm_address(&*keys::secp256k1(seed, &path::evm(0))?)?,
            "solana" => keys::solana_address(&*keys::ed25519(seed, &path::solana(0))?)?,
            _ => keys::near_implicit(&*keys::ed25519(seed, &path::near(0))?)?,
        };
        let listed = match chain { "solana" => wallet.solana.clone(), "evm" => wallet.evm.clone(), _ => wallet.near.clone() };
        let saved: Vec<String> = [address(chain).ok(), listed].into_iter().flatten().collect();
        if saved.iter().any(|s| recipients::key(s) != recipients::key(&own_address)) {
            return fail(format!("This wallet's saved {chain} address isn't the one its key signs for: nothing was signed."));
        }
        if chain != "near" && wallet.solana.as_deref() != Some(PUBLIC_TEST_SOLANA) { own::add(&[own_address]) }
    }
    Ok(())
}

/// The public BIP-39 test phrase («abandon … about»): anyone can sign for it, so the self-test runs without prompts.
/// Its address in the settings isn't enough: the wallet's seed must open without a prompt and derive it.
fn holds_public_test_wallet() -> bool {
    address("solana").ok().as_deref() == Some(PUBLIC_TEST_SOLANA) && wallets::active().is_some_and(|w| signs_without_prompt(&w, Some(PUBLIC_TEST_SOLANA)))
}

/// The wallet was sealed without Windows Hello — its seed opens with no prompt — and, given `solana`, its key is that one.
fn signs_without_prompt(wallet: &SignerWallet, solana: Option<&str>) -> bool {
    let Some(secret) = seeds::without_prompt(&wallet.id) else { return false };
    let Some(solana) = solana else { return true };
    if wallet.kind != "phrase" { return keys::solana_address(&secret).ok().as_deref() == Some(solana) }
    keys::ed25519(&secret, &path::solana(0)).ok().and_then(|k| keys::solana_address(&*k).ok()).as_deref() == Some(solana)
}

/// Adding, switching or removing a wallet once there is one is the person's act, in person.
fn confirm_in_person(reason: &str) -> Result<()> {
    if wallets::all().is_empty() || holds_public_test_wallet() { return Ok(()) }
    vault::confirm(reason, None)
}

fn test_wallet_signing() -> bool {
    wallets::active().is_some_and(|w| w.is_test() && !w.is_automation() && wallets::signing().is_none() && signs_without_prompt(&w, None))
}

/// An act that asks in person afresh, whatever session is open. A test wallet asks nothing; the automation wallet's
/// prompt closes by itself after a minute.
pub(crate) fn confirm_afresh(reason: &str) -> Result<()> {
    if holds_public_test_wallet() || test_wallet_signing() { return Ok(()) }
    vault::confirm(reason, if wallets::signing().is_none() { None } else { Some(60) })
}

fn twin(address: &str, known: &[(String, String)]) -> String {
    address_check::lookalike(address, known).map_or(String::new(), |k| format!("\n  looks like {}: same start and end, a different middle", k.0))
}

/// Money to an address outside the wallet: each new address is confirmed in person once, with the address on the
/// prompt, and remembered in the signer's own settings. `calls`: contracts whose calls the signer can't read, allowed as
/// calls only. `doing`: what the signature that follows does — this prompt opens the session that signature then uses.
pub fn approve(addresses: &[String], calls: bool, doing: &str) -> Result<()> {
    if holds_public_test_wallet() || test_wallet_signing() { return Ok(()) }
    let ours: HashSet<String> = [address("solana").ok(), address("evm").ok().map(|a| a.to_lowercase())].into_iter().flatten().collect();
    // An address saved in the book was confirmed in person, whole, when it was saved; another wallet here is the person's own.
    let mut known: HashSet<String> = recipients::all(recipients::List::Money);
    known.extend(book::all().iter().map(|a| recipients::key(&a.address)));
    known.extend(own::all());
    if calls { known.extend(recipients::all(recipients::List::Calls)) }
    let new: Vec<String> = addresses.iter().map(|a| recipients::key(a)).filter(|a| !known.contains(a) && !ours.contains(a))
        .collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    if new.is_empty() { return Ok(()) }
    let what = if calls { "allow ihatecrypto to call a contract whose call it can't read:\n" } else { "allow ihatecrypto to send money to a new address:\n" };
    let others = known_addresses();
    let lines: Vec<String> = new.iter().map(|a| format!("{a}{}", twin(a, &others))).collect();
    vault::confirm(&format!("{what}{}\nfor this move: {doing}", lines.join("\n")), if wallets::signing().is_none() { None } else { Some(60) })?;
    recipients::add(&new, if calls { recipients::List::Calls } else { recipients::List::Money })
}

/// What a new address is compared with for a twin: the saved book, this wallet, the other wallets and every address money
/// went to from this wallet — the signer's own lists, not the engine's.
fn known_addresses() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = book::all().into_iter().map(|a: SignerAddress| (format!("«{}»", a.name), a.address)).collect();
    out.extend([address("solana").ok(), address("evm").ok()].into_iter().flatten().map(|a| ("this wallet".to_string(), a)));
    out.extend(wallets::all().into_iter().flat_map(|w| [w.solana, w.evm]).flatten().map(|a| ("one of your wallets".to_string(), a)));
    out.extend(own::all().into_iter().map(|a| ("one of your wallets".to_string(), a)));
    out.extend(recipients::all(recipients::List::Money).into_iter().map(|a| ("an address you sent money to".to_string(), a)));
    out
}

/// A NEAR receiver ihatecrypto may pay: a valid account, among the saved addresses the engine passed, with a real block hash.
fn check_near_recipient(receiver: &str, recipients: &[String], block_hash: &[u8]) -> Result<()> {
    if address_check::problem(receiver, Network::Near).is_some() { return fail(format!("Not a NEAR account: {receiver}.")) }
    if !recipients.iter().any(|r| r == receiver) { return fail(format!("{receiver} isn't a saved address: NEAR goes only to one.")) }
    if block_hash.len() != 32 { return fail("A NEAR block hash is 32 bytes.") }
    Ok(())
}

/// The wallet's NEAR key (m/44'/397'/0') after Windows Hello, and its implicit account, which must be the one stored.
fn with_near_key<T>(reason: &str, body: impl FnOnce(&[u8], &str) -> Result<T>) -> Result<T> {
    with_key("near", reason, |secret| {
        let account = keys::near_implicit(secret)?;
        if address("near").is_ok_and(|k| k != account) { return fail("The NEAR key doesn't match this wallet's NEAR account.") }
        body(secret, &account)
    })
}

/// The signing wallet's own address on `chain`: the active wallet's (mirrored in the settings), or the automation
/// wallet's while it signs a task's move.
pub fn address(chain: &str) -> Result<String> {
    if let Some(w) = wallets::signing() {
        let a = match chain { "solana" => w.solana.clone(), "evm" => w.evm.clone(), _ => w.near.clone() };
        return a.ok_or_else(|| WalletError(w.missing(chain).unwrap_or_else(|| format!("No {chain} address."))));
    }
    if let Some(a) = prefs::string(chain) { return Ok(a) }
    if let Some(m) = wallets::active().and_then(|w| w.missing(chain)) { return fail(m) }
    fail("No wallet imported.")
}

fn refused(verdict: Verdict) -> SignerReply {
    SignerReply { checks: verdict.checks, problems: verdict.problems, ..SignerReply::error("The guard refused to sign.") }
}

/// The prompt text: the money lines the signer parsed itself.
fn prompt(what: &str, verdict: &Verdict) -> String {
    std::iter::once(what.to_string()).chain(verdict.checks.iter().filter(|c| c.contains('→')).cloned()).collect::<Vec<_>>().join("\n")
}

/// Keeps only claims the signer can prove: the account is the associated token account of that owner for one of the
/// transaction's own accounts as the mint. A made-up "this is your account" is dropped.
pub(crate) fn verified_owners(claimed: &BTreeMap<String, String>, mints: &[String]) -> HashMap<String, String> {
    claimed.iter().filter(|(account, owner)| {
        mints.iter().any(|mint| [spl::TOKEN_PROGRAM, spl::TOKEN_2022_PROGRAM].iter().any(|p| spl::associated_account(owner, mint, p).is_ok_and(|a| &a == *account)))
    }).map(|(a, o)| (a.clone(), o.clone())).collect()
}

/// For the zeroizing of seeds the vault hands out.
#[allow(dead_code)]
type Secret = Zeroizing<Vec<u8>>;
