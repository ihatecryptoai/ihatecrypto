//! The agent wallet's offline cases (AgentWallet.swift; the signer's own reading is wallet's agent_owner tests): the rules
//! the owner signs and that the signer accepts them, what the Agent API generated checked against what was asked, the
//! signed_data splice, whom a send pays, the card and its footer, what's said before the card. In-process: this test's
//! own folder, nothing goes to the network or the signer.

use std::collections::HashSet;
use std::sync::Arc;

use protocols::agent_api::{self, parse_generated};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use signer_api::{SignerAddress, SignerReply, SignerWallet};

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;

// Public test data only: Hardhat's accounts, the abandon-about phrase's Solana address, a NEAR implicit account of 0xab.
const ME: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const MOM: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";
const SOL: &str = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk";
const WRAPPED_SOL: &str = "So11111111111111111111111111111111111111112";

fn owner() -> String { "ab".repeat(32) }

fn d(s: &str) -> Decimal { s.parse().expect("a decimal") }

fn address(name: &str, network: &str, address: &str) -> SignerAddress {
    SignerAddress { name: name.into(), network: network.into(), address: address.into(), memo: String::new() }
}

/// An engine on this test's own folder and thread, in `language`, with the wallet «Main» (Solana and EVM) on and a book.
fn with_engine(language: &str, test: impl FnOnce(&Engine)) {
    let _dir = store::test_folder();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    tokio::task::LocalSet::new().block_on(&rt, async {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the tests keep the window locked"));
        e.st_mut().lock.locked = true;
        e.set_settings(|s| s.language = Some(language.into()));
        let main = SignerWallet { id: "1".into(), label: "Main".into(), kind: "phrase".into(), solana: Some(SOL.into()), evm: Some(ME.into()), near: Some(owner()),
                                  test: None, automation: None };
        e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), solana_address: Some(SOL.into()), evm_address: Some(ME.into()),
                                               wallets: Some(vec![main]), active_wallet: Some("1".into()), ..Default::default() });
        e.st_mut().book = vec![address("Mom", "base", MOM), address("Savings", "solana", WRAPPED_SOL), address("Mom Arb", "arbitrum", MOM),
                               address("Kraken", "xrp", "rrrrrrrrrrrrrrrrrrrrrhoLvTp")];
        test(&e);
    });
}

fn record(daily: &str) -> AgentWalletRecord {
    AgentWalletRecord { agent_id: "ag_0123456789".into(), account: Some("agent.near".into()), owner: owner(), daily_usd: d(daily), grant_id: None, frozen: false }
}

/// What generate-intent answers, as the API writes it: `payload` raw, the policy previewed under `hash`.
fn generated(ty: &str, standard: &str, payload: &str, signer: &str, preview: Option<(&Value, &str)>) -> Generated {
    let preview = preview.map_or(String::new(), |(p, h)| format!(r#","preview":{{"policy":{p},"policy_hash":"{h}"}}"#));
    let text = format!(r#"{{"correlation_id":"c1","type":"{ty}","agent_id":"ag_0123456789","intent":{{"standard":"{standard}","payload":{payload}}},"signer":{{"account_id":"{signer}"}}{preview}}}"#);
    parse_generated(text.as_bytes()).expect("a generated intent")
}

fn hash(v: &Value) -> String { agent_api::policy_hash(v.to_string().as_bytes()).expect("a hash") }

#[test]
fn the_rules_pay_only_this_wallets_own_and_saved_addresses_and_the_signer_takes_them() {
    with_engine("en", |e| {
        let policy = e.agent_policy(d("20"), &owner(), false);
        let list: Vec<(String, String)> = policy["destinations"]["list"].as_array().unwrap().iter()
            .map(|x| (x["chain"].as_str().unwrap_or(x["action"].as_str().unwrap()).to_string(), x["address"].as_str().unwrap().to_string())).collect();
        // The owner's NEAR account, its own Solana and Base, the saved Solana one, Mom's 0x once (Base and Arbitrum are
        // the same address), lowercased; no XRP.
        assert_eq!(list, vec![("transfer".into(), owner()), ("sol".into(), SOL.into()), ("base".into(), ME.to_lowercase()),
                              ("base".into(), MOM.to_lowercase()), ("sol".into(), WRAPPED_SOL.into())]);
        assert_eq!(policy["destinations"]["mode"], "only");
        assert_eq!(policy["budget"]["daily_usd"], "20");
        for a in usdc_assets() {
            assert_eq!(policy["limits"]["daily"][a], "20000000");
            assert_eq!(policy["limits"]["per_transaction"][a], "20000000");
        }
        assert_eq!(policy["assets"].as_array().unwrap().len(), 2);
        // The signer's own check of these rules: what it knows is this wallet's own addresses and the book's.
        let own: HashSet<String> = [SOL, ME].map(String::from).into();
        let known: HashSet<String> = e.st().book.iter().map(|b| b.address.clone()).collect();
        assert_eq!(wallet::agent_owner::policy(policy.to_string().as_bytes(), &owner(), &own, &known).unwrap(), "at most $20 a day, only to 5 addresses of yours");
        assert_eq!(e.agent_policy(d("2.5"), &owner(), true)["budget"]["daily_usd"], "2.5");
        assert_eq!(e.agent_policy(d("2.5"), &owner(), true)["frozen"], true);
    });
}

#[test]
fn usdc_units_round_down() {
    assert_eq!(agent_units(d("1.2345678")), 1_234_567);
    assert_eq!(agent_units(d("20")), 20_000_000);
    assert_eq!(agent_units(Decimal::ZERO), 0);
    assert_eq!(agent_units(Decimal::MAX), 0);
}

/// Nothing is signed for a request other than the one asked: another type, owner or agent, a grant for another token,
/// rules other than the ones asked, a freeze that doesn't freeze or raises the cap.
#[test]
fn what_the_api_generated_must_be_what_was_asked() {
    with_engine("en", |e| {
        let owner = owner();
        let policy = e.agent_policy(d("20"), &owner, false);
        let body = json!({"type": "agent_create", "name": "ihatecrypto", "policy": policy});
        let delegate = r#"{"receiverId":"outlayer.near","actions":[]}"#;
        let ok = generated("agent_create", "nep366", delegate, &owner, Some((&policy, &hash(&policy))));
        assert_eq!(e.agent_generated(&body, &ok, &owner, None).unwrap(), Some(policy.to_string().into_bytes()));
        let refused = |g: &Generated, body: &Value, today: Option<Decimal>| matches!(e.agent_generated(body, g, &owner, today), Err(trade::Error::Guard(_)));
        assert!(refused(&generated("agent_freeze", "nep366", delegate, &owner, Some((&policy, &hash(&policy)))), &body, None));
        assert!(refused(&generated("agent_create", "nep366", delegate, "evil.near", Some((&policy, &hash(&policy)))), &body, None));
        // Rules with one more address, previewed under their own hash: not the ones asked.
        let mut wider = policy.clone();
        wider["destinations"]["list"].as_array_mut().unwrap().push(json!({"action": "withdraw", "chain": "sol", "address": "evil"}));
        assert!(refused(&generated("agent_create", "nep366", delegate, &owner, Some((&wider, &hash(&wider)))), &body, None));
        // The asked hash, but a preview that doesn't hash to it.
        assert!(refused(&generated("agent_create", "nep366", delegate, &owner, Some((&wider, &hash(&policy)))), &body, None));
        assert!(refused(&generated("agent_create", "nep366", delegate, &owner, None), &body, None));

        // A grant: its message names this PC's token's hash.
        let grant = |credential: &str| {
            let message = json!({"domain": "near-intents-agent-api.agent-grant.v6", "credential": credential}).to_string();
            generated("grant_issue", "nep413", &json!({"message": message, "recipient": "api.demo.agentsonintents.com", "nonce": "AA=="}).to_string(), &owner, None)
        };
        let asked = json!({"type": "grant_issue", "agent_id": "ag_0123456789", "credential": "c0ffee"});
        assert_eq!(e.agent_generated(&asked, &grant("c0ffee"), &owner, None).unwrap(), None);
        assert!(refused(&grant("deadbeef"), &asked, None));
        assert!(refused(&grant("c0ffee"), &json!({"type": "grant_issue", "agent_id": "ag_other", "credential": "c0ffee"}), None));

        // A freeze: the same rules stored again frozen, within today's cap.
        let frozen = e.agent_policy(d("20"), &owner, true);
        let freeze = json!({"type": "agent_freeze", "agent_id": "ag_0123456789"});
        let shown = |p: &Value, ty: &str| generated(ty, "nep366", delegate, &owner, Some((p, &hash(p))));
        assert_eq!(e.agent_generated(&freeze, &shown(&frozen, "agent_freeze"), &owner, Some(d("20"))).unwrap(), Some(frozen.to_string().into_bytes()));
        assert!(refused(&shown(&policy, "agent_freeze"), &freeze, Some(d("20"))));
        assert!(refused(&shown(&frozen, "agent_freeze"), &freeze, Some(d("10"))));
        assert!(refused(&shown(&frozen, "agent_freeze"), &freeze, None));
        let unfreeze = json!({"type": "agent_unfreeze", "agent_id": "ag_0123456789"});
        assert!(e.agent_generated(&unfreeze, &shown(&policy, "agent_unfreeze"), &owner, Some(d("20"))).is_ok());
        assert!(refused(&shown(&frozen, "agent_unfreeze"), &unfreeze, Some(d("20"))));
    });
}

#[test]
fn signed_data_carries_the_payload_byte_for_byte() {
    let payload = br#"{ "message" : "{\"b\":1,\"a\":\"x/y\"}", "nonce":"AA==" }"#;
    let out = signed_data("nep413", payload, &json!({"public_key": "ed25519:K", "signature": "c2ln"}));
    assert_eq!(String::from_utf8(out.clone()).unwrap(),
               format!(r#"{{"standard":"nep413","payload":{},"public_key":"ed25519:K","signature":"c2ln"}}"#, std::str::from_utf8(payload).unwrap()));
    let v: Value = serde_json::from_slice(&out).expect("JSON");
    assert_eq!(v["standard"], "nep413");
    // And as the Agent API's submit body splices it in.
    let g = generated("grant_issue", "nep413", std::str::from_utf8(payload).unwrap(), &owner(), None);
    let submit: Value = serde_json::from_slice(&agent_api::submit_body(&g, &out)).expect("JSON");
    assert_eq!(submit["signed_data"]["signature"], "c2ln");
    let delegate = signed_data("nep366", b"{}", &json!({"signed_delegate": "AQ=="}));
    assert_eq!(delegate, br#"{"standard":"nep366","payload":{},"signed_delegate":"AQ=="}"#);
}

#[test]
fn a_delegate_is_signed_for_the_next_nonce_and_120_blocks_ahead() {
    assert_eq!(delegate_context(br#"{"jsonrpc":"2.0","id":1,"result":{"nonce":41,"block_height":1000,"permission":"FullAccess"}}"#), Some((42, 1120)));
    assert_eq!(delegate_context(br#"{"jsonrpc":"2.0","id":1,"error":{"name":"HANDLER_ERROR"}}"#), None);
    assert_eq!(delegate_context(br#"{"result":{"nonce":18446744073709551615,"block_height":1}}"#), None);
}

#[test]
fn the_agent_pays_a_saved_solana_or_base_address_or_the_wallet_itself() {
    with_engine("en", |e| {
        let p = e.agent_payee("Mom").unwrap();
        assert_eq!((p.chain, p.address.as_str(), p.own), ("base", MOM.to_lowercase().as_str(), false));
        let p = e.agent_payee("Savings").unwrap();
        assert_eq!((p.chain, p.address.as_str()), ("sol", WRAPPED_SOL));
        for me in ["me", "Main", "my wallet"] {
            let p = e.agent_payee(me).unwrap();
            assert_eq!((p.chain, p.address.as_str(), p.own, p.name.as_str()), ("sol", SOL, true, "Main"), "{me}");
        }
        assert_eq!(e.agent_payee("Kraken").unwrap_err().0, "The agent wallet pays a saved Solana or Base address. “Kraken” is on XRP Ledger.");
        assert!(e.agent_payee("Mom Arb").is_err());
        assert_eq!(e.agent_payee("Nobody").unwrap_err().0, "There's no “Nobody” in Addresses.");
        assert!(e.agent_payee("the multisig").is_err());
    });
}

#[test]
fn each_agent_move_draws_its_route_and_footer() {
    for language in ["en", "ru"] {
        with_engine(language, |e| {
            let commands = [WalletCommand::AgentCreate { daily_usd: d("20") }, WalletCommand::AgentFund { usdc: d("5") },
                            WalletCommand::AgentSend { usdc: d("1.5"), name: "Mom".into() }, WalletCommand::AgentSend { usdc: Decimal::ONE, name: "Nobody".into() },
                            WalletCommand::AgentFreeze(true), WalletCommand::AgentFreeze(false)];
            for c in &commands {
                let steps = e.card_route(c).expect("a route");
                let note = e.route_note(c).expect("a footer");
                for text in steps.iter().flat_map(|s| [s.label.clone(), s.title.clone()].into_iter().chain(s.lines.clone())).chain([note.clone()]) {
                    assert!(!text.contains(';') && !text.contains("Touch ID") && !text.contains("Mac"), "{text}");
                }
            }
            if language == "ru" {
                assert!(e.route_note(&commands[0]).unwrap().ends_with("Нажми «Подтвердить»."));
                return;
            }
            let labels = |c: &WalletCommand| e.card_route(c).unwrap().into_iter().map(|s| s.label).collect::<Vec<_>>();
            assert_eq!(labels(&commands[0]), ["OWNER · YOUR NEAR KEY", "RULES", "GRANT · THIS PC"]);
            assert_eq!(labels(&commands[1]), ["FROM · SOLANA", "TO · AGENT WALLET"]);
            assert_eq!(labels(&commands[2]), ["FROM · AGENT WALLET", "RULES CHECKED BY", "TO · SAVED ADDRESS"]);
            assert_eq!(labels(&commands[3]).last().map(String::as_str), Some("TO"));
            assert_eq!(e.card_route(&commands[0]).unwrap()[1].title, "$20 a day");
            assert!(e.route_note(&commands[0]).unwrap().contains("Windows Hello"));
            assert!(e.route_note(&commands[4]).unwrap().starts_with("Nothing moves from the agent wallet until you unfreeze it."));
            // The rules, once there's an agent wallet, on the send's card and footer.
            assert_eq!(e.card_route(&commands[2]).unwrap()[0].lines, ["no agent wallet yet"]);
            store::save(&record("20"), FILE);
            assert_eq!(e.card_route(&commands[2]).unwrap()[0].lines, ["at most $20 a day · your own and saved addresses only"]);
            assert!(e.route_note(&commands[2]).unwrap().contains("(at most $20 a day · your own and saved addresses only)"));
            let to = e.card_route(&commands[2]).unwrap().pop().unwrap();
            assert_eq!((to.title.as_str(), to.lines[0].ends_with("· Base")), ("Mom", true));
        });
    }
}

/// The partner key, the agent wallet and the payee are said before a card; a key wallet without NEAR is told so.
#[test]
fn an_agent_move_says_whats_short_before_its_card() {
    with_engine("en", |e| {
        let (create, fund, send) = (WalletCommand::AgentCreate { daily_usd: d("20") }, WalletCommand::AgentFund { usdc: d("5") },
                                    WalletCommand::AgentSend { usdc: Decimal::ONE, name: "Nobody".into() });
        // The key is the process's own: this is the only test that sets it, and it's taken away at the end.
        agent_api::set_key(None);
        assert_eq!(e.agent_before_card(&create).as_deref(), Some("Add the Agent API key first: Settings › General › Agent wallet key."));
        agent_api::set_key(Some("naa_test".into()));
        assert_eq!(e.agent_before_card(&create), None);
        assert_eq!(e.agent_before_card(&fund).as_deref(), Some("There's no agent wallet yet: “create an agent wallet with $20 a day”."));
        store::save(&record("20"), FILE);
        assert_eq!(e.agent_before_card(&fund), None);
        assert_eq!(e.agent_before_card(&send).as_deref(), Some("There's no “Nobody” in Addresses."));
        // Made, its grant missing: the same create finishes it. With the grant: there's one already.
        assert_eq!(e.agent_before_card(&create), None);
        set_node_secret(Some("ngt_test".into()), GRANT);
        assert_eq!(e.agent_before_card(&create).as_deref(), Some("There's already an agent wallet (ag_01234…) with $20 a day."));
        agent_api::set_key(None);

        // A key wallet holding only an EVM key has no NEAR owner and no Solana to fund from.
        let key = SignerWallet { id: "k".into(), label: "Key".into(), kind: "evm".into(), solana: None, evm: Some(ME.into()), near: None, test: None, automation: None };
        e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), evm_address: Some(ME.into()), wallets: Some(vec![key]), active_wallet: Some("k".into()),
                                               ..Default::default() });
        assert_eq!(e.missing_key(&create).as_deref(), Some("The wallet «Key» has no NEAR key: it holds only an EVM key. Switch wallets (Settings › Wallet) or import one that has it."));
        assert!(e.missing_key(&fund).unwrap().contains("no Solana key"));
    });
}

#[test]
fn the_record_keeps_swifts_names_and_a_lost_answer_says_dont_send_again() {
    with_engine("en", |e| {
        let swift = r#"{"agentID":"ag_1","owner":"o","dailyUSD":2.5,"frozen":true}"#;
        std::fs::write(store::url(FILE), swift).unwrap();
        let a = e.agent_wallet().expect("the record");
        assert_eq!((a.daily_usd, a.frozen, a.account, a.grant_id), (d("2.5"), true, None, None));
        e.keep_agent_wallet(|a| a.account = Some("agent.near".into()));
        let saved: Value = serde_json::from_slice(&std::fs::read(store::url(FILE)).unwrap()).unwrap();
        assert_eq!(saved, json!({"agentID": "ag_1", "owner": "o", "dailyUSD": 2.5, "frozen": true, "account": "agent.near"}));
        let said = e.may_have_gone("The 5 USDC left Solana for the agent wallet", "", Some("op1"));
        assert_eq!(said, "The 5 USDC left Solana for the agent wallet, but no final answer came back, so it may have gone. Status unknown: don't send it again until the balance shows it (NEAR Intents operation op1).");
    });
}
