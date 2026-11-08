//! What an EVM transaction's calldata asks for, as far as token safety goes (`EVMCall` in EVM.swift). The drainers'
//! tools — unlimited approvals, operator rights over all NFTs, permit calls, pulling tokens out of the sender — are
//! refused; every other call names who gets money from it, for the signer to hold to the wallet's own and approved.

use std::collections::HashSet;
use std::sync::LazyLock;

use wallet::evm::keccak;
use wallet::to_hex;

use crate::abi::{address_in, u256_in, uint_in, bytes_array};
use crate::protocols::{self, cctp, kyber, merkl, relay, safe, uniswap, usdt0, v4, RELAY_PROXIES};
use crate::{U256, coin, dollar, dollar_decimals, network_name, tempo, units, word_value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// Empty data: a plain transfer of the native coin.
    Plain,
    Approve { spender: String, amount: Vec<u8> },
    IncreaseAllowance { spender: String, amount: Vec<u8> },
    Transfer { to: String, amount: Vec<u8> },
    TransferFrom { from: String, to: String, amount: Vec<u8> },
    SetApprovalForAll { operator: String, approved: bool },
    Permit,
    /// A Uniswap swap (v3's SwapRouter02, v2's Router02): who gets what comes out.
    Swap { recipient: String },
    /// An ERC-4626 vault: who gets the shares or the assets, and whose shares are spent (None for a deposit).
    Vault { receiver: String, owner: Option<String> },
    /// A lending pool (Aave's supply, deposit or withdraw): who gets the aTokens or the withdrawn asset.
    Lending { beneficiary: String },
    /// Merkl's Distributor.claim: whom each reward is paid to.
    Claim { recipients: Vec<String> },
    /// Permit2.approve: lets `spender` pull up to `amount` of `token` until `expiration` (unix seconds).
    Permit2Approve { token: String, spender: String, amount: Vec<u8>, expiration: u64 },
    /// The Universal Router's v4 swap in exactly this app's shape: it pays the caller.
    V4Swap(v4::Swap),
    /// KyberSwap's router swap: who gets what comes out, and who else takes a fee.
    KyberSwap(kyber::Swap),
    /// ERC-721 or ERC-1155 safeTransferFrom / safeBatchTransferFrom.
    NftTransfer { from: String, to: String },
    /// Aave's approveDelegation, Morpho Blue's setAuthorization, Compound III's allow, ERC-6909's setOperator.
    Delegation { to: String, granted: bool },
    /// WETH9.deposit(): the coins sent with it come back to the sender as the wrapped token.
    Wrap,
    /// WETH9.withdraw(amount).
    Unwrap { amount: Vec<u8> },
    /// Circle's CCTP burn: `amount` of USDC minted on `domain` to `recipient` (None: not an EVM address).
    CctpBurn { recipient: Option<String>, domain: u32, amount: Vec<u8> },
    /// A LayerZero OFT's send (USDT0); `plain`: no options, compose message or command with it.
    OftSend { recipient: Option<String>, eid: u32, amount: Vec<u8>, plain: bool, refund: String },
    /// Aave's borrow and repay.
    Borrow { asset: String, amount: Vec<u8>, on_behalf_of: String },
    Repay { asset: String, amount: Vec<u8>, on_behalf_of: String },
    /// Safe's execTransaction: the multisig at the call's `to` makes this move, signed by its owners.
    MultisigExec { to: String, value: Vec<u8>, data: Vec<u8>, operation: u8 },
    /// Any other contract function, by selector.
    Contract { selector: String },
}

/// SwapRouter02's stand-ins for a recipient: MSG_SENDER is the caller, ADDRESS_THIS the router itself.
pub const MSG_SENDER: &str = "0x0000000000000000000000000000000000000001";
pub const ADDRESS_THIS: &str = "0x0000000000000000000000000000000000000002";
const ZERO: &str = "0x0000000000000000000000000000000000000000";

fn short(a: &str) -> String {
    let chars: Vec<char> = a.chars().collect();
    if chars.len() > 12 { format!("{}…{}", chars[..6].iter().collect::<String>(), chars[chars.len() - 4..].iter().collect::<String>()) } else { a.into() }
}

fn nonzero(w: &[u8]) -> bool { w.iter().any(|&b| b != 0) }

static RELAY_DEPOSITS: LazyLock<Vec<[u8; 4]>> = LazyLock::new(|| {
    ["depositNative(address,bytes32)", "depositErc20(address,address,uint256,bytes32)", "depositErc20(address,address,bytes32)"]
        .iter().map(|s| keccak(s.as_bytes())[..4].try_into().expect("4")).collect()
});

impl Call {
    pub fn decode(data: &[u8]) -> Self {
        // Calldata shorter than a selector still runs a contract's fallback: only none at all is a plain send.
        if data.len() < 4 { return if data.is_empty() { Self::Plain } else { Self::Contract { selector: format!("0x{}", to_hex(data)) } } }
        if let Some((to, value, data, operation)) = safe::read_exec(data) { return Self::MultisigExec { to, value, data, operation } }
        let selector = to_hex(&data[..4]);
        let args = &data[4..];
        let word = |i: usize| (args.len() >= (i + 1) * 32).then(|| args[i * 32..(i + 1) * 32].to_vec());
        let address = |i: usize| word(i).map(|w| format!("0x{}", to_hex(&w[12..])));
        // A swap paying word i's address. `literal`: a router that reads 0x…01 as address 1, not as the caller.
        let payee = |i: usize, literal: bool| address(i).filter(|r| !literal || r.to_lowercase() != MSG_SENDER).map(|recipient| Self::Swap { recipient });
        let found = match selector.as_str() {
            "095ea7b3" => address(0).zip(word(1)).map(|(spender, amount)| Self::Approve { spender, amount }),
            // increaseAllowance, and increaseApproval of older tokens.
            "39509351" | "d73dd623" => address(0).zip(word(1)).map(|(spender, amount)| Self::IncreaseAllowance { spender, amount }),
            // Tempo's transferWithMemo pays like transfer.
            "a9059cbb" | "95777d59" => address(0).zip(word(1)).map(|(to, amount)| Self::Transfer { to, amount }),
            "23b872dd" => match (address(0), address(1), word(2)) {
                (Some(from), Some(to), Some(amount)) => Some(Self::TransferFrom { from, to, amount }),
                _ => None,
            },
            "a22cb465" => address(0).zip(word(1)).map(|(operator, b)| Self::SetApprovalForAll { operator, approved: b[31] == 1 }),
            "d505accf" | "8fcbaf0c" => Some(Self::Permit),
            "04e45aaf" | "5023b4df" => payee(3, false),
            "414bf389" | "db3e2198" => payee(3, true),
            // exactInput / exactOutput: the tuple read only where the app's own encoder puts it (0x20).
            "b858183f" | "09b81346" => if uint_in(args, 0) == Some(32) { payee(2, false) } else { None },
            "c04b8d59" | "f28c0498" => if uint_in(args, 0) == Some(32) { payee(2, true) } else { None },
            "38ed1739" | "5c11d795" | "18cbafe5" | "791ac947" | "8803dbee" | "4a25d94a" => payee(3, true),
            "472b43f3" | "42712a67" => payee(3, false),
            "7ff36ab5" | "fb3bdb41" | "b6f9de95" => payee(2, true),
            "ac9650d8" | "5ae401dc" | "1f0464d1" => {
                multicall_recipient(args, if selector == "ac9650d8" { 0 } else { 1 }).map(|recipient| Self::Swap { recipient })
            }
            "6e553f65" => address(1).map(|receiver| Self::Vault { receiver, owner: None }),
            "b460af94" | "ba087652" => address(1).zip(address(2)).map(|(receiver, o)| Self::Vault { receiver, owner: Some(o) }),
            "617ba037" | "e8eda9df" | "69328dec" => address(2).map(|beneficiary| Self::Lending { beneficiary }),
            "71ee95c0" => merkl::recipients(data).map(|recipients| Self::Claim { recipients }),
            "87517c45" => match (address(0), address(1), word(2), uint_in(args, 3)) {
                (Some(token), Some(spender), Some(amount), Some(expiration)) => Some(Self::Permit2Approve { token, spender, amount, expiration }),
                _ => None,
            },
            "3593564c" => v4::decode_execute(data).map(Self::V4Swap),
            "e21fd0e9" => kyber::decode(data).map(Self::KyberSwap),
            "42842e0e" | "b88d4fde" | "f242432a" | "2eb2c2d6" => address(0).zip(address(1)).map(|(from, to)| Self::NftTransfer { from, to }),
            "c04a8a10" | "eecea000" | "110496e5" | "558a7297" => address(0).zip(word(1)).map(|(to, w)| Self::Delegation { to, granted: nonzero(&w) }),
            "d0e30db0" => Some(Self::Wrap),
            "2e1a7d4d" => word(0).map(|amount| Self::Unwrap { amount }),
            "8e0250ee" | "779b432d" => cctp::read_burn(data).map(|(recipient, domain, amount)| Self::CctpBurn { recipient, domain, amount }),
            // A send that can't be read is still a send: it's refused, not signed as some contract's call.
            "c7c7f5b3" => Some(match usdt0::read_send(data) {
                Some(s) => Self::OftSend { recipient: s.recipient, eid: s.eid, amount: s.amount, plain: s.plain, refund: s.refund },
                None => Self::OftSend { recipient: None, eid: 0, amount: vec![0; 32], plain: false, refund: String::new() },
            }),
            "a415bcad" => match (address(0), word(1), address(4)) {
                (Some(asset), Some(amount), Some(on_behalf_of)) => Some(Self::Borrow { asset, amount, on_behalf_of }),
                _ => None,
            },
            "573ade81" => match (address(0), word(1), address(3)) {
                (Some(asset), Some(amount), Some(on_behalf_of)) => Some(Self::Repay { asset, amount, on_behalf_of }),
                _ => None,
            },
            _ => None,
        };
        found.unwrap_or(Self::Contract { selector: format!("0x{selector}") })
    }

    /// What the transaction does, in words for the signing prompt, which reads «ihatecrypto is trying to <this>».
    /// Read from the calldata by the signer itself, never from the app's description of it.
    pub fn summary(&self, chain_id: u64, to: &str, value: u64) -> String { self.summary_wei(chain_id, to, &U256::from(value).to_be_bytes::<32>()) }

    /// `wei` as a 32-byte word: a multisig's move carries a uint256.
    fn summary_wei(&self, chain_id: u64, to: &str, wei: &[u8]) -> String {
        let paid = nonzero(wei);
        let net = format!("on {}", network_name(chain_id));
        let coin = coin(chain_id);
        let u = |w: &[u8], d: usize| units(&word_value(w), d);
        let coins = format!("{} {coin}", u(wei, 18));
        if *self == Self::Plain { return format!("send {coins} to {} {net}", short(to)) }
        // A token the signer can't name shows its raw units: an amount on the prompt, never none.
        let amount = |w: &[u8], token: &str| match dollar(token) {
            Some(symbol) => format!("{} {symbol}", u(w, dollar_decimals(token))),
            None => format!("{} units of the token {}", u(w, 0), short(token)),
        };
        match self {
            Self::Wrap => return format!("wrap {coins} into W{coin} {net}"),
            Self::V4Swap(_) if paid => return format!("swap {coins} on Uniswap v4 {net}, paying this wallet"),
            _ => {}
        }
        let what = match self {
            Self::Transfer { to: recipient, amount: a } => format!("send {} to {} {net}", amount(a, to), short(recipient)),
            Self::Approve { spender, amount: a } => format!("let {} spend exactly {} {net}", short(spender), amount(a, to)),
            Self::IncreaseAllowance { spender, amount: a } => format!("let {} spend {} more {net}", short(spender), amount(a, to)),
            Self::Swap { .. } => format!("swap on Uniswap {net}, paying this wallet"),
            Self::V4Swap(_) => format!("swap on Uniswap v4 {net}, paying this wallet"),
            Self::KyberSwap(k) => format!("swap {} on KyberSwap {net} for at least {}, paying this wallet",
                                          amount(&k.amount.to_be_bytes::<32>(), &k.src_token), amount(&k.min_return.to_be_bytes::<32>(), &k.dst_token)),
            Self::Permit2Approve { token, spender, amount: a, .. } => format!("let {} spend exactly {} through Permit2 {net}", short(spender), amount(a, token)),
            Self::Unwrap { amount: a } => format!("unwrap {} W{coin} of {} into {coin} {net}", u(a, 18), short(to)),
            Self::Vault { owner, .. } => format!("{} the vault {} {net}", if owner.is_none() { "deposit into" } else { "withdraw from" }, short(to)),
            Self::Lending { .. } => format!("use the lending pool {} {net}, for this wallet", short(to)),
            Self::NftTransfer { to: recipient, .. } => format!("send NFTs of {} to {} {net}", short(to), short(recipient)),
            Self::CctpBurn { recipient, domain, amount: a } => format!("move {} USDC {net} to {} on {} through Circle's CCTP",
                                                                       u(a, 6), recipient.as_deref().map_or("?".into(), short), cctp::chain_name(*domain)),
            Self::OftSend { recipient, eid, amount: a, .. } => {
                let what = if usdt0::on(chain_id).is_some_and(|d| d.oft == to.to_lowercase()) { format!("{} USDT", u(a, 6)) }
                           else { format!("{} units of the token of {}", u(a, 0), short(to)) };
                format!("move {what} {net} to {} on {} through LayerZero", recipient.as_deref().map_or("?".into(), short), usdt0::network_name(*eid))
            }
            Self::Borrow { asset, amount: a, .. } => format!("borrow {} from the lending pool {} {net}, a debt of this wallet's", amount(a, asset), short(to)),
            Self::Repay { asset, amount: a, .. } => if a.iter().all(|&b| b == 0xff) {
                format!("repay all of this wallet's {} debt to {} {net}", dollar(asset).map_or_else(|| short(asset), str::to_string), short(to))
            } else {
                format!("repay {} of this wallet's debt to {} {net}", amount(a, asset), short(to))
            },
            Self::MultisigExec { to: target, value: v, data: d, .. } => {
                format!("carry out the multisig {}'s signed move: {}", short(to), Call::decode(d).summary_wei(chain_id, target, v))
            }
            Self::Contract { .. } if to.to_lowercase() == tempo::FEE_MANAGER => "pay Tempo's fees in USDC from now on".into(),
            Self::Contract { .. } if to.to_lowercase() == tempo::EXCHANGE => "swap dollars on Tempo's exchange, paying this wallet".into(),
            _ => format!("call the contract {} {net}", short(to)),
        };
        if paid { format!("{what}, paying {coins}") } else { what }
    }

    /// An allowance none of ihatecrypto's own approvals reaches: ≥ 2⁹⁶ units.
    pub fn unlimited(amount: &[u8]) -> bool { amount.iter().take(20).any(|&b| b != 0) }

    /// Contracts ihatecrypto itself lets spend its tokens on a network; an approve to any other spender needs it
    /// approved in person, like a recipient. Lowercase.
    pub fn known_spenders(chain_id: u64) -> HashSet<String> {
        let mut s: Vec<&str> = vec![relay::DEPOSITORY];
        s.extend(RELAY_PROXIES);
        s.extend(protocols::aave_pools(chain_id));
        s.extend(protocols::morpho_vaults(chain_id));
        if cctp::domain(chain_id).is_some() { s.push(cctp::TOKEN_MESSENGER) }
        if let Some(d) = usdt0::on(chain_id).filter(|d| d.approves) { s.push(d.oft) }
        if chain_id == 4217 { s.push(tempo::EXCHANGE) }
        if let Some((_, router, _)) = uniswap::v3(chain_id) { s.push(router) }
        if let Some((router, _)) = uniswap::v2(chain_id) { s.push(router) }
        if chain_id == 4663 { s.push(uniswap::ROBINHOOD_ROUTER) }
        let v4 = v4::on(chain_id);
        if let Some(d) = &v4 { s.extend([d.router, v4::PERMIT2]) }
        if kyber::chain(chain_id).is_some() { s.push(kyber::ROUTER) }
        s.into_iter().map(str::to_lowercase).collect()
    }

    /// Contracts ihatecrypto's own moves call with calldata the signer doesn't read.
    fn own_unread_callee(target: &str) -> bool {
        [relay::DEPOSITORY, tempo::EXCHANGE, tempo::FEE_MANAGER, safe::FACTORY].contains(&target) || RELAY_PROXIES.contains(&target)
    }

    /// The contract of a call the signer can't read, to be allowed in person once: None when the call is read, pays
    /// that contract (then it's a recipient), or is one of ihatecrypto's own.
    pub fn unread_target(&self, to: &str, value: u64, chain_id: u64) -> Option<String> {
        let target = to.to_lowercase();
        if self.payees(to, value, chain_id).iter().any(|p| p.to_lowercase() == target) { return None }
        // A protocol's call read by its selector alone is that protocol's only at its own contract.
        let by_protocol = matches!(self, Self::Swap { .. } | Self::V4Swap(_) | Self::KyberSwap(_) | Self::Vault { .. } | Self::Lending { .. }
            | Self::Claim { .. } | Self::Borrow { .. } | Self::Repay { .. } | Self::CctpBurn { .. } | Self::OftSend { .. } | Self::Permit2Approve { .. });
        if by_protocol { return (!Self::protocol_contracts(chain_id).contains(&target)).then_some(target) }
        (matches!(self, Self::Contract { .. }) && !Self::own_unread_callee(&target)).then_some(target)
    }

    /// The contracts whose calls ihatecrypto reads by protocol.
    fn protocol_contracts(chain_id: u64) -> HashSet<String> {
        let mut s = Self::known_spenders(chain_id);
        s.insert(merkl::DISTRIBUTOR.into());
        if let Some(d) = usdt0::on(chain_id) { s.insert(d.oft.into()); }
        s
    }

    /// What the contract called makes wrong with this call, beyond the call itself.
    pub fn target_refusal(to: &str, data: &[u8], sender: &str) -> Option<String> {
        let target = to.to_lowercase();
        if target == kyber::ROUTER && kyber::decode(data).is_none() {
            return Some("A call to KyberSwap's router that isn't the swap ihatecrypto reads. ihatecrypto only swaps there.".into());
        }
        if target == relay::DEPOSITORY {
            if data.len() < 4 || !RELAY_DEPOSITS.iter().any(|s| data.starts_with(s)) {
                return Some("A call to Relay's depository that isn't a deposit. ihatecrypto only deposits there.".into());
            }
            if address_in(&data[4..], 0).map(|a| a.to_lowercase()) != Some(sender.to_lowercase()) {
                return Some("The Relay deposit names someone else as the depositor.".into());
            }
        }
        safe::exec_refusal(data, to)
    }

    /// The Relay order id a call into Relay's depository deposits for: its last word.
    pub fn relay_deposit_id(to: &str, data: &[u8]) -> Option<Vec<u8>> {
        (to.to_lowercase() == relay::DEPOSITORY && data.len() >= 4 + 64 && (data.len() - 4) % 32 == 0 && RELAY_DEPOSITS.iter().any(|s| data.starts_with(s)))
            .then(|| data[data.len() - 32..].to_vec())
    }

    /// Who gets money, or the right to spend it, from this call. The signer signs only when each is the wallet's own or
    /// approved in person.
    pub fn payees(&self, to: &str, value: u64, chain_id: u64) -> Vec<String> {
        let mut out = if value > 0 && !self.returns_coins(to, value, chain_id) { vec![to.to_string()] } else { vec![] };
        match self {
            Self::Transfer { to: r, .. } | Self::NftTransfer { to: r, .. } | Self::TransferFrom { to: r, .. } => out.push(r.clone()),
            Self::CctpBurn { recipient: Some(r), .. } => out.push(r.clone()),
            Self::OftSend { recipient: Some(r), refund, .. } => out.extend([r.clone(), refund.clone()]),
            Self::MultisigExec { to: target, value: v, data: d, .. } => {
                if uint_in(v, 0) != Some(0) { out.push(target.clone()) }
                out.extend(Call::decode(d).payees(target, 0, chain_id));
            }
            Self::Approve { spender, .. } => {
                if spender.to_lowercase() != ZERO && !Self::known_spenders(chain_id).contains(&spender.to_lowercase()) { out.push(spender.clone()) }
            }
            Self::IncreaseAllowance { spender, amount } | Self::Permit2Approve { spender, amount, .. } => {
                if nonzero(amount) && !Self::known_spenders(chain_id).contains(&spender.to_lowercase()) { out.push(spender.clone()) }
            }
            _ => {}
        }
        out
    }

    /// Coins sent with this call that come back to the sender, so `to` isn't paid.
    pub fn returns_coins(&self, to: &str, value: u64, chain_id: u64) -> bool {
        let to = to.to_lowercase();
        match self {
            Self::Wrap => uniswap::v2(chain_id).is_some_and(|(_, weth)| weth == to),
            Self::V4Swap(s) => v4::on(chain_id).is_some_and(|d| d.router == to) && s.currency_in() == v4::NATIVE
                && s.amount_in == U256::from(value) && s.settle_max == s.amount_in,
            Self::OftSend { plain, .. } => *plain && usdt0::on(chain_id).is_some_and(|d| d.oft == to),
            _ => false,
        }
    }

    /// Why this must not be signed, or None. `sender` is the wallet's own address; `chain_id` lets a v4 swap run a hook
    /// Uniswap's registry vouches for there.
    pub fn refusal(&self, sender: &str, chain_id: Option<u64>) -> Option<String> {
        self.refusal_at(sender, chain_id, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()))
    }

    pub fn refusal_at(&self, sender: &str, chain_id: Option<u64>, now: u64) -> Option<String> {
        let me = sender.to_lowercase();
        let other = |a: &str| a.to_lowercase() != me;
        Some(match self {
            Self::Swap { recipient } if other(recipient) && recipient.to_lowercase() != MSG_SENDER => format!("The swap would pay {recipient}, not this wallet."),
            Self::Lending { beneficiary } if other(beneficiary) => format!("The lending pool would credit {beneficiary}, not this wallet."),
            Self::V4Swap(s) => return v4::refusal(s, chain_id),
            Self::KyberSwap(s) => return kyber::refusal(s, sender),
            Self::MultisigExec { operation, .. } if *operation != 0 => "A multisig move by DELEGATECALL runs another contract's code as the multisig. Refused.".into(),
            Self::CctpBurn { recipient: None, .. } => "The CCTP burn mints to an account that isn't an EVM address. ihatecrypto mints only to EVM addresses.".into(),
            Self::OftSend { recipient: None, .. } => "The LayerZero send can't be read, or pays an account that isn't an EVM address. ihatecrypto sends only to EVM addresses.".into(),
            Self::OftSend { plain: false, .. } => "The LayerZero send carries options or a message, which can pay someone else on the other network. ihatecrypto sends only plain ones.".into(),
            Self::Borrow { on_behalf_of, .. } if other(on_behalf_of) => format!("The borrow would put the debt on {on_behalf_of}, not this wallet."),
            Self::Repay { on_behalf_of, .. } if other(on_behalf_of) => format!("The repay would pay {on_behalf_of}'s debt, not this wallet's."),
            Self::Permit2Approve { amount, .. } if Self::unlimited(amount) => "Unlimited spending through Permit2 — the drainers' main trick. ihatecrypto only approves exact amounts.".into(),
            Self::Permit2Approve { expiration, .. } if *expiration > now + 86_400 => "A Permit2 approval for longer than a day. ihatecrypto approves for the one swap only.".into(),
            Self::Claim { recipients } => match recipients.iter().find(|r| other(r)) {
                Some(r) => format!("The rewards claim would pay {r}, not this wallet."),
                None if recipients.is_empty() => "The rewards claim would pay nobody, not this wallet.".into(),
                None => return None,
            },
            Self::Vault { receiver, .. } if other(receiver) => format!("The vault would credit {receiver}, not this wallet."),
            Self::Vault { owner: Some(o), .. } if other(o) => format!("The vault call spends shares of {o}, not this wallet's."),
            Self::Approve { amount, .. } | Self::IncreaseAllowance { amount, .. } if Self::unlimited(amount) => {
                "Unlimited token spending approval — the drainers' main trick. ihatecrypto only approves exact amounts.".into()
            }
            Self::SetApprovalForAll { approved: true, .. } => "Rights to all your NFTs in this contract (setApprovalForAll) — an NFT drainer trick. Refused.".into(),
            Self::Delegation { to, granted: true } => format!("Lets {to} manage your positions or move your tokens in this contract (credit delegation, an authorization, an operator). Refused."),
            Self::Permit => "A permit call grants approval to spend your tokens. Refused.".into(),
            Self::TransferFrom { from, .. } if !other(from) => "transferFrom from your address in your name — this is how tokens are pulled through an existing approval. Refused.".into(),
            _ => return None,
        })
    }
}

/// Whom a SwapRouter02 multicall pays: the first payee that isn't the caller (so the swap check refuses it), else the
/// caller. None when a call in it is one this decoder doesn't know, or when anything is left in the router.
fn multicall_recipient(args: &[u8], array_word: usize) -> Option<String> {
    let calls = bytes_array(args, array_word * 32).filter(|c| !c.is_empty())?;
    let mut payees: Vec<String> = Vec::new();
    for call in &calls {
        if call.len() < 4 { return None }
        let inner = &call[4..];
        let address = |i: usize| (inner.len() >= (i + 1) * 32).then(|| format!("0x{}", to_hex(&inner[i * 32 + 12..(i + 1) * 32])));
        match to_hex(&call[..4]).as_str() {
            // unwrapWETH9(min, recipient), sweepToken(token, min, recipient): pay out what the router holds.
            "49404b7c" => payees.push(address(1)?),
            "df2ab5bb" => payees.push(address(2)?),
            // unwrapWETH9(min), sweepToken(token, min), refundETH(): to the caller.
            "49616997" | "e90a182f" | "12210e8a" => payees.push(MSG_SENDER.into()),
            // wrapETH(value), pull(token, value): into the router, from the caller.
            "1c58db4f" | "f2d5d56b" => return None,
            _ => match Call::decode(call) {
                Call::Swap { recipient } if recipient.to_lowercase() != ADDRESS_THIS => payees.push(recipient),
                _ => return None,
            },
        }
    }
    // Read whole: a multicall paying this wallet in one swap and someone else in the next names two payees besides the
    // caller, and comes back as «several payees» — never this wallet — so the swap check refuses it.
    let named: HashSet<String> = payees.iter().map(|p| p.to_lowercase()).filter(|p| p != MSG_SENDER).collect();
    if named.len() > 1 { return Some("several payees".into()) }
    Some(named.into_iter().next().unwrap_or_else(|| MSG_SENDER.into()))
}

/// Word `i` of `data` as a number, for callers outside the crate.
pub fn word_u256(data: &[u8], i: usize) -> Option<U256> { u256_in(data, i) }
