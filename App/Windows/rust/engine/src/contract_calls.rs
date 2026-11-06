//! A contract call the chat wrote out (ContractCalls.swift): «call arbitrum 0x… deposit(uint256,address) 10e6 me
//! approving 10 USDC», checked and simulated, waiting for Confirm; a protocol outside ihatecrypto's list held to the cap
//! (Settings › Wallet). After Confirm it's simulated again and signed only if it still does what the card showed.

use std::collections::{HashMap, HashSet};

use evm::abi;
use evm::call::Call;
use evm::erc20;
use evm::simulation::{NATIVE, Simulation};
use ihatecrypto_core::foundation::{Num, Rounding, units};
use ihatecrypto_core::holdings::{self, BLOCKSCOUT, Holding};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::{ContractCall, WalletCommand};
use ihatecrypto_core::stablecoins;
use net::evm_rpc::EvmRpc;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use trade::agent_reads::AgentReads;
use trade::contract_calls::{ContractCallGuard, ContractReputation, ContractTrust, Risk, ScamList, Verdict};
use trade::{EvmNetwork, Executor};

use crate::browser::SIMULATION_RPC;
use crate::canvas::Step;
use crate::engine::{Engine, Pending};
use crate::multisig::{DECIMALS_CALL, SYMBOL_CALL, is_evm};
use crate::portfolio::{Network, Row};

/// A contract call checked and simulated, waiting for Confirm. The model only writes the command: ihatecrypto encodes it
/// from the function's signature, checks the contract on the network's explorer, simulates the exact approve and the
/// call together and draws the card; anything it can't decode, verify or simulate isn't offered (it fails closed). The
/// signer reads the calldata again: no unlimited approvals, nothing paid to someone else, a new spender confirmed in person.
#[derive(Debug, Clone)]
pub struct CallReview {
    pub command: WalletCommand,
    pub call: ContractCall,
    pub net: EvmNetwork,
    pub data: Vec<u8>,
    /// Wei of the network's coin sent with the call.
    pub value: u64,
    pub approve: Option<CallApprove>,
    /// What it does, read from the calldata.
    pub lines: Vec<String>,
    /// The simulated change of each of this wallet's balances.
    pub changes: Vec<String>,
    pub trust: ContractTrust,
    pub warnings: Vec<String>,
    /// The simulated calls and what they did: run again just before signing (`run_contract_call`).
    pub calls: Vec<SimulatedCall>,
    pub simulation: evm::simulation::Simulation,
    /// «First time with this contract», «proxy: its owner can change the code»: on the card, above everything.
    pub badges: Vec<String>,
    /// Each simulated token's symbol and decimals as the card read them: `record_call` books the call with these.
    pub tokens: HashMap<String, (String, Option<u32>)>,
}

/// `CallReview.Approve`: the exact approve made first.
#[derive(Debug, Clone, PartialEq)]
pub struct CallApprove {
    pub token: String,
    pub symbol: String,
    pub amount: Decimal,
    pub units: Decimal,
}

/// One call of the simulation, as it's sent again.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulatedCall {
    pub to: String,
    pub value: u64,
    pub data: Vec<u8>,
}

impl CallReview {
    /// «FluidLending» or «0x1a99…6096».
    pub fn contract_name(&self) -> String {
        self.trust.implementation.clone().or_else(|| self.trust.name.clone()).unwrap_or_else(|| {
            let c: Vec<char> = self.call.contract.chars().collect();
            format!("{}…{}", c.iter().take(6).collect::<String>(), c[c.len().saturating_sub(4)..].iter().collect::<String>())
        })
    }
}

/// `pow(10, decimals)`; None past Decimal's range.
fn scale(decimals: u32) -> Num { Num((decimals <= 28).then(|| Decimal::from_i128_with_scale(10i128.pow(decimals), 0))) }

/// A simulated change in whole tokens: `delta / 10^decimals`; None past Decimal's range.
fn shown(delta: i128, decimals: i64) -> Option<Decimal> { units(&delta.to_string(), decimals) }

/// The exact approve in the token's units. A token that doesn't answer decimals() is refused, never counted in 18.
fn approve_units(token: &str, symbol: &str, network: &str, amount: Decimal, decimals: Option<u32>) -> Result<Decimal, (String, String)> {
    let Some(decimals) = decimals else {
        return Err((format!("{symbol} ({token}) on {network} doesn't answer decimals(), so ihatecrypto can't count an exact approve."),
                    format!("{symbol} ({token}) в {network} не отвечает на decimals(), а без этого ihatecrypto не посчитает точный approve.")));
    };
    // ponytail: an approve past Decimal's range (Swift's Decimal holds it) is refused, not encoded.
    (Num::of(amount) * scale(decimals)).rounded(0, Rounding::Down).0
        .ok_or_else(|| ("A number in the answer is out of range.".into(), "A number in the answer is out of range.".into()))
}

/// What `ContractCallGuard::at_risk` counted, in dollars, or why the call can't be kept under the cap.
fn risk_usd(risk: Risk) -> Result<Decimal, (String, String)> {
    match risk {
        Risk::Usd(usd) => Ok(usd),
        Risk::Unpriced(s) => Err((format!("ihatecrypto can't price the {s} this call would spend or let the contract take, so it can't keep the call under its cap."),
                                  format!("ihatecrypto не может оценить {s}, который вызов потратит или разрешит контракту взять, а значит не может удержать вызов в лимите."))),
        Risk::Uncounted(s) => Err((format!("ihatecrypto can't read the decimals() of the {s} that would leave, so it can't keep the call under its cap."),
                                   format!("у уходящего {s} не читается decimals(), а значит ihatecrypto не может удержать вызов в лимите."))),
    }
}

/// A token's line among the card's balance changes: an NFT by its count, a token in whole tokens. A token that doesn't
/// answer decimals() is refused, never shown in 18.
fn change_line(asset: &str, symbol: &str, delta: i128, decimals: Option<u32>, nft: bool) -> Result<String, (String, String)> {
    let decimals = if nft { 0 } else {
        let Some(d) = decimals else {
            return Err((format!("{symbol} ({asset}) doesn't answer decimals(), so the card couldn't show how much of it moves."),
                        format!("{symbol} ({asset}) не отвечает на decimals(), поэтому карточка не может показать, сколько его уходит или приходит.")));
        };
        i64::from(d)
    };
    Ok(format!("{symbol}: {}", Engine::delta(shown(delta, decimals).unwrap_or(Decimal::MAX))))
}

impl Engine {
    pub async fn prepare_contract_call(&self, command: &WalletCommand, note: Option<String>) {
        let WalletCommand::ContractCall(c) = command else { return };
        let Some(net) = EvmNetwork::named(&c.network) else { return };
        let Some(me) = self.st().evm_address().filter(|a| is_evm(a)) else {
            return self.reply(&self.say("Contract calls need this wallet's EVM key in secure storage.", "Для вызова контракта нужен EVM-ключ этого кошелька в защищённом хранилище."), note);
        };
        let refuse = |en: &str, ru: &str| {
            self.reply(&self.say(&format!("I won't make this call: {en} Nothing was signed."), &format!("Этот вызов делать не буду: {ru} Ничего не подписано.")), note.clone());
        };
        // Addresses the user gave: in their own last message (never a chain's or a task's command text, which the model
        // wrote), their favorites or their address book.
        let (theirs, said) = {
            let st = self.st();
            let theirs: HashSet<String> = std::iter::once(me.clone()).chain(st.favorites.iter().map(|f| f.address.clone())).chain(st.book.iter().map(|e| e.address.clone()))
                .map(|a| a.to_lowercase()).collect();
            (theirs, st.messages().iter().rev().find(|m| m.from_user).map(|m| m.text.to_lowercase()).unwrap_or_default())
        };
        let users_own = |a: &str| theirs.contains(&a.to_lowercase()) || said.contains(&a.to_lowercase());
        let chain_id = net.rpc.chain_id;

        self.st_mut().draft = Some(self.say("Encoding the call", "Собираю вызов"));
        let data = match AgentReads::calldata(&c.function, &c.args, &me) { Ok(d) => d, Err(e) => return refuse(&e.to_string(), &e.to_string()) };
        // The contract, and every other address among the arguments, is the user's own or verified code on the explorer.
        self.st_mut().draft = Some(self.say("Checking the contract on the explorer", "Проверяю контракт в обозревателе"));
        let trust = ContractTrust::check(&c.contract, net.key).await;
        if !trust.unknown && !trust.is_contract {
            return refuse(&format!("{} isn't a contract on {}.", c.contract, net.name), &format!("{} — не контракт в сети {}.", c.contract, net.name));
        }
        // A protocol outside ihatecrypto's own list (Aave, Morpho, Uniswap, Relay…) must look like one: verified code, a
        // month old at least, money in it as DefiLlama prices it, on no scam list. Each of these blocks.
        let listed = Call::known_spenders(chain_id).contains(&c.contract.to_lowercase());
        let mut reputation = ContractReputation::default();
        if !listed {
            if !trust.verified {
                return refuse(&format!("{} on {} has no verified source on the explorer ({}).", c.contract, net.name, trust.line()),
                              &format!("у {} в {} нет проверенного исходника в обозревателе ({}).", c.contract, net.name, trust.line()));
            }
            if trust.days.is_none_or(|d| d < ContractReputation::MIN_DAYS) {
                return refuse(&format!("the contract is {} old. ihatecrypto calls only contracts a month old or more.", trust.days.map_or("of unknown age".into(), |d| format!("{d} days"))),
                              &format!("контракту {}, а ihatecrypto вызывает только контракты старше месяца.", trust.days.map_or("неизвестно сколько дней".into(), |d| format!("{d} дн."))));
            }
            self.st_mut().draft = Some(self.say("Checking its reputation", "Проверяю репутацию"));
            reputation = ContractReputation::check(&c.contract, net, &me).await;
            if let Some(why) = &reputation.scam { return refuse(&format!("{}.", why.en), &format!("{}.", why.ru)) }
            let min = Fmt::usd(ContractReputation::MIN_TVL);
            if reputation.tvl_usd.is_none_or(|t| t < ContractReputation::MIN_TVL) {
                return refuse(&format!("DefiLlama {} — ihatecrypto calls only protocols with {min} or more in them.", reputation.tvl_usd.map_or("doesn't price it".into(), |t| format!("prices only {} in it", Fmt::usd(t)))),
                              &format!("DefiLlama {} — ihatecrypto вызывает только протоколы, где лежит от {min}.", reputation.tvl_usd.map_or("его не оценивает".into(), |t| format!("оценивает в нём только {}", Fmt::usd(t)))));
            }
        }
        // The zero address («no referrer», «none») gives nobody anything; the network's USDC, the big stablecoins there, the
        // token approved and ihatecrypto's own protocols are known; anything else is checked.
        let mut vouched: HashSet<String> = std::iter::once(net.usdc.to_string())
            .chain(stablecoins::TOP.iter().filter_map(|t| stablecoins::find(t, net.key).map(|s| s.address.to_string())))
            .map(|a| a.to_lowercase()).collect();
        vouched.extend(Call::known_spenders(chain_id));
        let known: HashSet<String> = vouched.iter().cloned().chain(c.approve.as_ref().map(|a| a.token.to_lowercase())).collect();
        let is_address = |a: &str| is_evm(a) && a[2..].chars().any(|ch| ch != '0') && a.to_lowercase() != c.contract.to_lowercase();
        // GoPlus Security is asked about every address among the arguments and the token approved, the user's own typing
        // too: a drainer's address pasted from a site is what this is for. This computer's wallets aren't sent.
        let mut own = self.keys_on_this_pc();
        own.insert(me.to_lowercase());
        let asked: Vec<String> = c.args.iter().chain(c.approve.as_ref().map(|a| &a.token))
            .filter(|a| is_address(a) && !vouched.contains(&a.to_lowercase()) && !own.contains(&a.to_lowercase())).cloned().collect();
        let scams = ScamList::shared().verdicts(asked.clone(), chain_id).await;
        for a in &asked {
            let Some(Verdict::Reported(why)) = scams.get(&a.to_lowercase()) else { continue };
            return if c.args.contains(a) { refuse(&format!("the argument {a} — {}.", why.en), &format!("аргумент {a} — {}.", why.ru)) }
                else { refuse(&format!("the token {a} — {}.", why.en), &format!("токен {a} — {}.", why.ru)) };
        }
        reputation.unchecked = reputation.unchecked || scams.values().any(|v| *v == Verdict::Unchecked);
        for a in c.args.iter().filter(|a| is_address(a) && !known.contains(&a.to_lowercase()) && !users_own(a)) {
            let t = ContractTrust::check(a, net.key).await;
            if !(t.is_contract && t.verified) {
                return refuse(&format!("the argument {a} is neither yours nor a verified contract — money could go there."),
                              &format!("аргумент {a} — не твой адрес и не проверенный контракт: туда могут уйти деньги."));
            }
        }
        // Coins sent with it, in wei.
        let mut value = 0u64;
        if let Some(v) = c.value.filter(|v| *v > Decimal::ZERO) {
            let wei = (Num::of(v) * Decimal::from(1_000_000_000_000_000_000u64)).rounded(0, Rounding::Down).0.and_then(|w| w.to_u64());
            let Some(wei) = wei else { return refuse(&format!("that's more {} than one call takes.", net.gas_coin), &format!("это больше {}, чем берёт один вызов.", net.gas_coin)) };
            value = wei;
        }
        // The exact approve: the network's USDC or a big stablecoin by ticker, else a token contract.
        let mut approve = None;
        if let Some(a) = &c.approve {
            let token = if a.token.starts_with("0x") { Some(a.token.clone()) } else if a.token.to_uppercase() == "USDC" { Some(net.usdc.to_string()) }
                else { stablecoins::find(&a.token, net.key).map(|s| s.address.to_string()) };
            let Some(token) = token.filter(|t| is_evm(t)) else {
                return refuse(&format!("I don't know the token «{}» on {}: write its contract.", a.token, net.name), &format!("не знаю токен «{}» в {}: напиши его контракт.", a.token, net.name));
            };
            let (symbol, decimals) = Self::token_meta(&token, net).await;
            let units = match approve_units(&token, &symbol, net.name, a.amount, decimals) { Ok(u) => u, Err((en, ru)) => return refuse(&en, &ru) };
            approve = Some(CallApprove { token, symbol, amount: a.amount, units });
        }
        // What the calldata does, and what the signer would refuse anyway.
        let decoded = Call::decode(&data);
        if let Some(why) = decoded.refusal(&me, Some(chain_id)) { return refuse(&why, &why) }
        let foreign: Vec<String> = decoded.payees(&c.contract, value, chain_id).into_iter().filter(|p| ![me.to_lowercase(), c.contract.to_lowercase()].contains(&p.to_lowercase())).collect();
        if let Some(other) = foreign.first() { return refuse(&format!("money or the right to spend it would go to {other}."), &format!("деньги или право их тратить ушли бы на {other}.")) }
        // Both simulated together: the approve, then the call that spends it. No simulation, no card.
        self.st_mut().draft = Some(self.say(&format!("Simulating it on {}", net.name), &format!("Симулирую в {}", net.name)));
        let mut calls = vec![];
        if let Some(a) = &approve {
            let approved = abi::word_decimal(a.units).ok().and_then(|w| erc20::approve(&c.contract, evm::U256::from_be_bytes(w)).ok()).unwrap_or_default();
            calls.push(SimulatedCall { to: a.token.clone(), value: 0, data: approved });
        }
        calls.push(SimulatedCall { to: c.contract.clone(), value, data: data.clone() });
        let sim = match Self::simulate(net, &me, &calls).await {
            Ok(sim) => sim,
            Err(e) => return refuse(&format!("{}'s node couldn't simulate it ({e}), and ihatecrypto signs nothing it hasn't simulated.", net.name),
                                    &format!("нода {} не смогла это просимулировать ({e}), а без симуляции ihatecrypto ничего не подписывает.", net.name)),
        };
        if let Some(revert) = &sim.revert { return refuse(&format!("the simulation says it would fail — {revert}."), &format!("симуляция говорит, что он не пройдёт — {revert}.")) }
        // A call that moves nothing failed quietly: Compound-style markets (Moonwell…) return an error code instead of
        // reverting — no cash to withdraw, a paused market. Signed, it would only cost gas.
        if sim.changes.is_empty() {
            return refuse("the simulation shows it changes nothing in your wallet: the protocol refused it without failing (a market with no cash left to withdraw, or paused). Signing it would only pay gas.",
                          "симуляция показывает, что он ничего не меняет в кошельке: протокол отказал, не откатив вызов (в рынке нет свободных денег для вывода или он на паузе). Подпись лишь потратила бы газ.");
        }
        // Rights the balances can't show: an approve, an operator, ownership — only a limited approve to the contract passes.
        if let Some(why) = ContractCallGuard::grant_problem(&sim.grants, &c.contract) { return refuse(&format!("{why}."), &format!("{why}.")) }
        // A protocol outside the list may put at most the cap at risk: what leaves, its approve and the coins sent.
        let cap = ContractCallGuard::cap_usd();
        if !listed {
            let outflows: Vec<(&String, i128)> = sim.changes.iter().filter(|(k, v)| **v < 0 && k.as_str() != NATIVE).map(|(k, v)| (k, *v)).collect();
            let asked: Vec<String> = outflows.iter().map(|(k, _)| k.to_string()).chain(approve.as_ref().map(|a| a.token.clone())).collect();
            let priced = ContractReputation::prices(&asked, net).await;
            let mut leaving = vec![];
            for (asset, delta) in &outflows {
                let (symbol, decimals) = Self::token_meta(asset, net).await;
                // An amount past Decimal's range has no count either: refused as uncounted.
                leaving.push((asset.to_string(), symbol, decimals.and_then(|d| shown(delta.saturating_neg(), i64::from(d)))));
            }
            let risk = ContractCallGuard::at_risk(&leaving, approve.as_ref().map(|a| (a.token.as_str(), a.symbol.as_str(), a.amount)), &priced);
            let mut at_risk = match risk_usd(risk) { Ok(usd) => Num::of(usd), Err((en, ru)) => return refuse(&en, &ru) };
            let sent = shown(i128::from(value), 18).unwrap_or_default();
            let spent = shown(-sim.changes.get(NATIVE).copied().unwrap_or(0), 18).map_or(Num(None), |s| Num::of(s) - sent);
            let coins = Num::of(sent) + Num(spent.0.map(|s| s.max(Decimal::ZERO)));
            if coins.0.is_none_or(|c| c > Decimal::ZERO) {
                let price = ContractReputation::coin_usd(net).await;
                if price <= Decimal::ZERO {
                    return refuse(&format!("ihatecrypto can't price {} right now, so it can't keep the call under its cap.", net.gas_coin),
                                  &format!("ihatecrypto сейчас не может оценить {}, а значит не может удержать вызов в лимите.", net.gas_coin));
                }
                at_risk = at_risk + coins * price;
            }
            // A sum past the arithmetic (Swift's NaN, which no cap passes) is over the cap.
            let at_risk = at_risk.0.unwrap_or(Decimal::MAX);
            if at_risk > cap {
                return refuse(&format!("it puts ≈ {} at risk in a protocol outside ihatecrypto's list, over the {} cap (Settings › Wallet).", Fmt::usd(at_risk), Fmt::usd(cap)),
                              &format!("он ставит под риск ≈ {} в протоколе вне списка ihatecrypto — больше лимита {} (Settings › Wallet).", Fmt::usd(at_risk), Fmt::usd(cap)));
            }
        }
        let (mut changes, mut tokens) = (vec![], HashMap::new());
        for (asset, delta) in &sim.changes {
            if asset == NATIVE {
                changes.insert(0, format!("{}: {}", net.gas_coin, Self::delta(shown(*delta, 18).unwrap_or(Decimal::MAX))));
                continue;
            }
            let (symbol, decimals) = Self::token_meta(asset, net).await;
            match change_line(asset, &symbol, *delta, decimals, sim.nfts.contains(asset)) { Ok(line) => changes.push(line), Err((en, ru)) => return refuse(&en, &ru) }
            tokens.insert(asset.clone(), (symbol, decimals));
        }
        let mut warnings = vec![];
        if !sim.changes.is_empty() && sim.changes.values().all(|v| *v < 0) {
            warnings.push(self.say("Nothing comes back to this wallet in this transaction.", "В этой транзакции на кошелёк ничего не возвращается."));
        }
        if reputation.unchecked {
            warnings.push(self.say("The scam check (GoPlus Security) didn't answer this time. The other checks passed.",
                                   "Проверка на скам (GoPlus Security) в этот раз не ответила. Остальные проверки пройдены."));
        }
        let mut badges = vec![];
        if reputation.first_time { badges.push(self.say("First time with this contract", "Первый раз с этим контрактом")) }
        if trust.implementation.is_some() { badges.push(self.say("Proxy: its owner can change the code", "Прокси: владелец может поменять код")) }
        if !listed && let Some(tvl) = reputation.tvl_usd {
            badges.push(self.say(&format!("Outside ihatecrypto's list · {} in it · cap {}", Fmt::usd(tvl), Fmt::usd(cap)), &format!("Вне списка ihatecrypto · в нём {} · лимит {}", Fmt::usd(tvl), Fmt::usd(cap))));
        }
        let changes = if changes.is_empty() { vec![self.say("None of your balances change.", "Балансы не меняются.")] } else { changes };
        let review = CallReview {
            command: command.clone(), call: c.clone(), net: net.clone(), data, value, approve: approve.clone(), lines: vec![decoded.summary(chain_id, &c.contract, value)],
            changes, trust: trust.clone(), warnings: warnings.clone(), calls, simulation: sim, badges: badges.clone(), tokens,
        };
        let name = review.contract_name();
        let simulated = self.say("Simulated: ", "Симуляция: ");
        let lines: Vec<String> = review.lines.iter().cloned().chain([trust.line()]).chain(review.changes.iter().map(|x| format!("{simulated}{x}"))).collect();
        let (call_lines, changes) = (review.lines.clone(), review.changes.clone());
        self.st_mut().call_review = Some(review);
        self.show(Pending::Wallet(command.clone()), &format!("{} · {name} · {}", c.name(), net.name), lines, badges.iter().chain(&warnings).cloned().collect(), vec![]);
        // The card's route: the exact approve, the call, what the simulation showed.
        let mut steps = vec![];
        if let Some(a) = &approve {
            steps.push(step(&self.say("APPROVE · exact", "APPROVE · ровно"), &format!("{} {}", Fmt::num(a.amount, 6), a.symbol),
                            vec![self.say(&format!("exactly this · only {name} may take it"), &format!("ровно столько · взять может только {name}"))]));
        }
        steps.push(step(&format!("CALL · {}", net.name), c.name(), call_lines.iter().cloned().chain([trust.line()]).collect()));
        steps.push(step(&(self.say("SIMULATED · ", "СИМУЛЯЦИЯ · ") + net.name), &changes[0], changes[1..].to_vec()));
        let card = self.st().card.as_ref().map(|c| c.id.clone());
        self.update_board(card.as_deref(), |b| b.steps = Some(steps));
        let all = changes.join(", ");
        let flags = if badges.is_empty() { String::new() } else { format!(" ⚑ {}", badges.join(" · ")) } + &if warnings.is_empty() { String::new() } else { format!(" ⚠ {}", warnings.join(" ")) };
        self.reply(&(self.say(&format!("{} on {name} ({}) is checked and simulated: {all}. The card is on the canvas — press Confirm.", c.name(), net.name),
                              &format!("{} на {name} ({}) проверен и просимулирован: {all}. Карточка на холсте — нажми Confirm.", c.name(), net.name)) + &flags), note);
    }

    /// What the call did to the wallet's tokens, in its cost basis: the shares it got for the dollars it spent, so the
    /// portfolio shows them (and their P/L) before the explorer lists them; shares it gave back, sold.
    pub fn record_call(&self, r: &CallReview) {
        let (mut spent, mut got) = (Num::of(Decimal::ZERO), Vec::<(String, Decimal)>::new());
        for (asset, delta) in r.simulation.changes.iter().filter(|(a, _)| a.as_str() != NATIVE) {
            // The decimals the card read, not a read after the send that a node's hiccup could miss. None (an NFT): no
            // amount, the explorer shows the holding itself.
            let Some((symbol, Some(decimals))) = r.tokens.get(asset) else { continue };
            let Some(amount) = units(&delta.unsigned_abs().to_string(), i64::from(*decimals)) else { continue };
            let key = format!("{}:{}", r.net.key, asset.to_lowercase());
            let dollar = stablecoins::DOLLARS.contains(&symbol.to_uppercase());
            if *delta < 0 {
                if dollar { spent = spent + amount } else { self.set_app_costs(|c| if let Some(cost) = c.get_mut(&key) { cost.sold(amount) }) }
            } else if !dollar {
                got.push((key, amount));
            }
        }
        // One kind of share for the dollars: its cost. Several, or none paid in dollars: kept at no cost, so they still show.
        let spent = spent.0.unwrap_or(Decimal::MAX);
        let paid = if got.len() == 1 && spent > Decimal::ZERO { spent } else { Decimal::new(1, 6) };
        for (key, amount) in got { self.set_app_costs(|c| c.entry(key).or_default().bought(amount, paid, chrono::Utc::now())) }
    }

    /// Shares a contract call brought (kept in the recorded costs) that the network's explorer hasn't listed yet: read from
    /// the token itself and priced by DefiLlama, so the portfolio shows them at once (the Mac's missingDeposits; Blockscout
    /// indexes a new holding minutes late). Called with each portfolio read, before the networks are sorted.
    pub async fn with_missing_deposits(&self, networks: Vec<Network>, owner: &str) -> Vec<Network> {
        if owner.is_empty() { return networks }
        let mut networks = networks;
        let mut keys: Vec<String> = self.st().app_costs.keys().cloned().collect();
        keys.sort();
        for key in keys {
            let parts: Vec<&str> = key.split(':').filter(|p| !p.is_empty()).collect();
            let [chain, contract] = parts[..] else { continue };
            if holdings::lookup(&BLOCKSCOUT, chain).is_none() { continue }
            let Some(net) = EvmNetwork::named(chain) else { continue };
            if networks.iter().any(|n| n.key == net.key && n.rows.iter().chain(&n.hidden).any(|r| r.contract.as_ref().map(|c| c.to_lowercase()).as_deref() == Some(contract))) { continue }
            let Ok(call) = erc20::balance_of(owner) else { continue };
            let Some(raw) = net.rpc.read(contract, &call).await.ok().and_then(|d| crate::multisig::word_decimal(&d)).filter(|u| *u > Decimal::ZERO) else { continue };
            let (symbol, Some(decimals)) = Self::token_meta(contract, net).await else { continue };
            let Some(amount) = units(&raw.to_string(), i64::from(decimals)) else { continue };
            let held = Holding::new(net.key, contract, amount, &symbol);
            let priced = holdings::llama_priced(net.key, vec![held.clone()]).await.into_iter().next().unwrap_or(held);
            let mut row: Row = Self::row_holding(&priced);
            // Bought through a checked call: shown, not tucked away with the airdrops.
            row.real = Some(true);
            let usd = row.usd.unwrap_or_default();
            match networks.iter_mut().find(|n| n.key == net.key) {
                Some(n) => {
                    n.rows.push(row);
                    n.usd = n.usd.saturating_add(usd);
                }
                None => networks.push(Network { key: net.key.into(), name: net.name.into(), usd, rows: vec![row], note: None, hidden: vec![], hidden_more: 0 }),
            }
        }
        networks
    }

    /// After Confirm: the approve and the call exactly as simulated (the executor dry-runs the call again before signing it).
    pub async fn run_contract_call(&self, command: &WalletCommand, e: &Executor) -> trade::Result<String> {
        let review = self.st_mut().call_review.take();
        let Some(r) = review.filter(|r| &r.command == command) else { return Err(trade::Error::Execution("The call's check is gone. Ask for it again.".into())) };
        // Simulated again now: what's signed must still do what the card showed (a proxy upgraded, a pool moved).
        let Some(me) = self.st().evm_address() else { return Err(trade::Error::Execution("No EVM key.".into())) };
        let now = Self::simulate(&r.net, &me, &r.calls).await?;
        if let Some(why) = ContractCallGuard::drift(&r.simulation, &now).or_else(|| ContractCallGuard::grant_problem(&now.grants, &r.call.contract)) {
            return Err(trade::Error::Guard(format!("Nothing was signed: since the card, {why}. Ask for it again to see the new card.")));
        }
        let name = r.contract_name();
        let approve_note = format!("Approve exactly {} {} to {name}", Fmt::num(r.approve.as_ref().map_or(Decimal::ZERO, |a| a.amount), 6), r.approve.as_ref().map_or("", |a| a.symbol.as_str()));
        let done = e.contract_call(&r.net, &r.call.contract, &r.data, r.value, r.approve.as_ref().map(|a| (a.token.as_str(), a.units)), &approve_note,
                                   &format!("{} on {name} on {}", r.call.name(), r.net.name)).await?;
        self.record_call(&r);
        Ok(done)
    }

    /// The node that runs eth_simulateV1 for a network: Arbitrum's own crashes on it.
    pub fn simulator(net: &EvmNetwork) -> EvmRpc {
        SIMULATION_RPC.iter().find(|(id, _)| *id == net.rpc.chain_id).map_or_else(|| net.rpc.clone(), |(id, url)| EvmRpc::pinned(url, *id))
    }

    /// The calls simulated together on the network's simulator.
    async fn simulate(net: &EvmNetwork, from: &str, calls: &[SimulatedCall]) -> wallet::Result<Simulation> {
        let calls: Vec<(String, u64, Vec<u8>)> = calls.iter().map(|c| (c.to.clone(), c.value, c.data.clone())).collect();
        Self::simulator(net).simulate(from, &calls).await
    }

    /// «+1.5», «−0.25», «0».
    pub fn delta(d: Decimal) -> String { format!("{}{}", if d > Decimal::ZERO { "+" } else if d < Decimal::ZERO { "−" } else { "" }, Fmt::num(d.abs(), 8)) }

    /// A token's symbol and decimals, read from it. No decimals when decimals() didn't answer, never 18: an approve counted
    /// in 18 decimals for a 6-decimal token is a trillion times the amount, and the cap would see a trillionth of what leaves.
    pub async fn token_meta(token: &str, net: &EvmNetwork) -> (String, Option<u32>) {
        let symbol = net.rpc.read(token, &SYMBOL_CALL).await.ok().and_then(|d| abi::string(&d)).unwrap_or_else(|| format!("{}…", token.chars().take(6).collect::<String>()));
        (symbol, abi::decimals(net.rpc.read(token, &DECIMALS_CALL).await.ok().as_deref()))
    }
}

/// `Board.Step(label:title:lines:)`.
fn step(label: &str, title: &str, lines: Vec<String>) -> Step { Step { label: label.into(), title: title.into(), lines, money: None, status: None } }

#[cfg(test)]
#[path = "contract_calls/tests.rs"]
mod tests;
