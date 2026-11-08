//! A Ledger as the wallet a move signs with (`LedgerSigner.swift`). The same guard the signing process runs checks each
//! request first, then the device shows it and waits for the person's buttons. What comes back is checked against the
//! Ledger's own address before it is used, so a swapped or confused device can't slip a stranger's signature into a move.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use evm::call::Call;
use evm::protocols::{Permit3009, safe};
use evm::{SIGNABLE, Transaction};
use execution::trade_signer::{EvmFees, Signing, TradeError, TradeSigner, TradeWallet};
use guard::{Verdict, hl, sol};
use hyperliquid::{MsgPack, action, builder, signer as hls};
use sha2::{Digest, Sha256};
use solana::message::Message;
use solana::spl;
use wallet::evm::eip712::Typed;
use wallet::evm::{EvmSignature, recover};
use wallet::keys::base58;
use wallet::to_hex;

use crate::{Device, EVM_PATH, LedgerError, SOLANA_PATH, descriptors};

/// The trading key the Ledger approved for its Hyperliquid account, in the signing process: orders, cancels and leverage
/// go there without the device. It answers None for what the device must sign instead (a short, an order past the
/// person's limits).
pub type Agent = Arc<dyn Fn(MsgPack, u64) -> Signing<'static, Option<EvmSignature>> + Send + Sync>;

pub struct LedgerSigner {
    pub device: Device,
    pub wallet: TradeWallet,
    pub evm_path: String,
    pub solana_path: String,
    /// «Nano S Plus», «Flex»…: Ledger's certificates are per model.
    pub model: Option<String>,
    /// What the person has to do on the device now («Open the Solana app…», «Review … on your Ledger»); None when done.
    pub prompt: Arc<dyn Fn(Option<String>) + Send + Sync>,
    /// None while there is no trading key or it is paused.
    pub agent: Option<Agent>,
}

fn device(e: LedgerError) -> TradeError { TradeError::Device { status: e.status, message: e.message } }
fn failed(message: impl Into<String>) -> TradeError { TradeError::Failed(message.into()) }
fn wallet_error(e: wallet::WalletError) -> TradeError { TradeError::Failed(e.0) }
fn refuse(v: &Verdict) -> TradeError { failed(v.problems.join(" ")) }
fn ends(s: &str, head: usize, tail: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    format!("{}…{}", c[..head.min(c.len())].iter().collect::<String>(), c[c.len().saturating_sub(tail)..].iter().collect::<String>())
}

impl LedgerSigner {
    pub fn new(device: Device, wallet: TradeWallet) -> Self {
        LedgerSigner { device, wallet, evm_path: EVM_PATH.into(), solana_path: SOLANA_PATH.into(), model: None, prompt: Arc::new(|_| {}), agent: None }
    }

    fn not_this_ledger(&self, address: &str) -> TradeError { device(LedgerError::new(format!("The Ledger's signature isn't from {address}: is it the Ledger you connected?"))) }

    /// `hash`: what the device shows when it can't show the details (blind signing), so the person can compare it with
    /// the one ihatecrypto checked.
    async fn on_device<T, F: Future<Output = Result<T, TradeError>>>(&self, app: &str, what: &str, hash: &str, body: impl FnOnce() -> F) -> Result<T, TradeError> {
        (self.prompt)(Some(format!("Open the {app} app on your Ledger (allow it on the device if it asks).")));
        let opened = self.device.open(app).await.map_err(device);
        let result = match opened {
            Ok(()) => {
                (self.prompt)(Some(format!("Review on your Ledger and approve: {what}. If it shows a hash, it is {}.", ends(hash, 6, 4))));
                body().await
            }
            Err(e) => Err(e),
        };
        (self.prompt)(None);
        result
    }

    /// The first SPL token transfer in the message and whose account it pays, when the payee is one of `owners` (our own
    /// or an approved address): its associated account for the mint must be exactly the destination.
    pub fn token_transfer(m: &Message, accounts: &[String], owners: &[String]) -> Option<(String, String, String)> {
        for ix in &m.instructions {
            let Some(program) = accounts.get(ix.program_index).filter(|p| [spl::TOKEN_PROGRAM, spl::TOKEN_2022_PROGRAM].contains(&p.as_str())) else { continue };
            // TransferChecked: source, mint, destination, owner.
            if ix.data.first() != Some(&12) || ix.accounts.len() < 4 || ix.accounts[1] >= accounts.len() || ix.accounts[2] >= accounts.len() { continue }
            let (mint, destination) = (&accounts[ix.accounts[1]], &accounts[ix.accounts[2]]);
            if let Some(owner) = owners.iter().find(|o| spl::associated_account(o, mint, program).is_ok_and(|a| &a == destination)) {
                return Some((destination.clone(), owner.clone(), mint.clone()));
            }
        }
        None
    }

    /// Typed data in full, so the Ledger shows each field by name; by its two hashes on an app too old for that. The
    /// signature must recover to the Ledger's address.
    pub async fn typed(&self, typed: &Typed, what: &str) -> Result<EvmSignature, TradeError> {
        let parts = typed.parts().map_err(wallet_error)?;
        let signature = self.on_device("Ethereum", what, &format!("0x{}", to_hex(&parts.message)), || async {
            match self.device.sign_typed_data(typed, &self.evm_path).await {
                Err(e) if matches!(e.status, Some(0x6d00 | 0x6e00)) => self.device.sign_typed_data_hashes(&parts, &self.evm_path).await.map_err(device),
                Err(e) if e.status == Some(0x6a80) => Err(TradeError::Device { status: Some(0x6a80),
                    message: "The Ethereum app won't show this typed data: turn on «Raw messages» in its settings on the Ledger (it then shows every field), or «Blind signing».".into() }),
                other => other.map_err(device),
            }
        }).await?;
        if !recover(&parts.digest(), &signature).is_ok_and(|a| a.eq_ignore_ascii_case(&self.wallet.evm)) { return Err(self.not_this_ledger(&self.wallet.evm)) }
        Ok(signature)
    }

    /// The one approval of a trading key for the Ledger's Hyperliquid account: the device shows the key's address, which
    /// the person compares with the one on the canvas. The key can trade but not withdraw.
    pub async fn approve_agent(&self, agent: &str, name: &str, nonce: u64) -> Result<(MsgPack, EvmSignature), TradeError> {
        let (action, typed) = hls::approve_agent_typed(agent, name, nonce, true);
        let what = format!("let the trading key {} place orders for you on Hyperliquid, and it can't withdraw", ends(agent, 6, 4));
        Ok((action, self.typed(&typed, &what).await?))
    }
}

impl TradeSigner for LedgerSigner {
    fn sign_solana(&self, wire: Vec<u8>, accounts: Vec<String>, recipients: Vec<String>, token_owners: BTreeMap<String, String>) -> Signing<'_, Vec<u8>> {
        Box::pin(async move {
            let message = Message::parse_transaction(&wire).map_err(wallet_error)?;
            let mut policy = sol::Policy::new(&self.wallet.solana);
            policy.recipients = recipients.iter().cloned().collect::<HashSet<_>>();
            let owners: HashMap<String, String> = token_owners.into_iter().collect();
            let verdict = sol::check(&message, &accounts, &policy, &owners);
            if !verdict.ok() { return Err(refuse(&verdict)) }
            let payer = base58::decode(&self.wallet.solana).map_err(wallet_error)?;
            let (unsigned, slot) = solana::unsigned(&wire, &payer, None).map_err(wallet_error)?;
            let hash = base58::encode(&Sha256::digest(&unsigned));
            let transfer = Self::token_transfer(&message, &accounts, &[recipients.as_slice(), &[self.wallet.solana.clone()]].concat());
            let what = match &transfer {
                Some((_, owner, _)) => format!("a token transfer to {}", ends(owner, 4, 4)),
                None => verdict.checks.iter().find(|c| c.contains('→')).cloned().unwrap_or_else(|| "a Solana transaction".into()),
            };
            let certificate = match &transfer { Some(_) => descriptors::trusted_name_certificate(self.model.as_deref()).await.ok(), None => None };
            let signature = self.on_device("Solana", &what, &hash, || async {
                // A USDC transfer shows its amount and recipient when Ledger vouches for whose token account it pays;
                // without that (no network, a token Ledger doesn't know), it stays blind-signed.
                if let (Some((account, owner, mint)), Some(certificate)) = (&transfer, &certificate) {
                    let vouched = async {
                        self.device.load_certificate(certificate, 0x04, "Solana").await?;
                        let challenge = self.device.challenge("Solana").await?;
                        let tlv = descriptors::solana_token_account(owner, mint, account, &challenge).await?;
                        self.device.provide_trusted_name(&tlv, "Solana").await
                    }.await;
                    if let Err(e) = vouched { if e.declined() { return Err(device(e)) } }
                }
                self.device.sign_solana(&unsigned, &self.solana_path).await.map_err(device)
            }).await?;
            let key = <[u8; 32]>::try_from(payer.as_slice()).ok().and_then(|k| VerifyingKey::from_bytes(&k).ok());
            let valid = key.zip(Signature::from_slice(&signature).ok()).is_some_and(|(k, s)| k.verify(&unsigned, &s).is_ok());
            if !valid { return Err(self.not_this_ledger(&self.wallet.solana)) }
            let mut signed = wire;
            signed[slot].copy_from_slice(&signature);
            Ok(signed)
        })
    }

    /// The same calldata rules as the signing process: known networks, no unlimited approvals, tokens only to us or to an
    /// approved address.
    fn sign_evm(&self, fees: EvmFees, to: String, data: Vec<u8>, recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        Box::pin(async move {
            if !SIGNABLE.contains(&fees.chain_id) { return Err(failed(format!("ihatecrypto doesn't sign for network {}.", fees.chain_id))) }
            let call = Call::decode(&data);
            if let Some(refusal) = call.refusal(&self.wallet.evm, Some(fees.chain_id)).or_else(|| Call::target_refusal(&to, &data, &self.wallet.evm)) {
                return Err(failed(refusal));
            }
            let mine: Vec<&String> = recipients.iter().chain(std::iter::once(&self.wallet.evm)).collect();
            if let Some(payee) = call.payees(&to, 0, fees.chain_id).into_iter().find(|p| !mine.iter().any(|m| m.eq_ignore_ascii_case(p))) {
                return Err(failed(format!("Tokens or the right to spend them would go to an unapproved address {payee}.")));
            }
            let tx = Transaction { chain_id: fees.chain_id, nonce: fees.nonce, max_priority_fee_per_gas: fees.max_priority_fee, max_fee_per_gas: fees.max_fee,
                                   gas_limit: fees.gas_limit, to: to.clone(), value: 0, data: data.clone(), authorizations: vec![] };
            let hash = tx.signing_hash().map_err(wallet_error)?;
            let payload = tx.unsigned_payload().map_err(wallet_error)?;
            // A token transfer or approval shows «Send 25 USDC» / «Approve 2 USDC» when Ledger's descriptor for the token
            // comes first; without it the Ethereum app asks for blind signing.
            let token = match call { Call::Transfer { .. } | Call::Approve { .. } => descriptors::erc20(fees.chain_id, &to).await.ok(), _ => None };
            let (v, r, s) = self.on_device("Ethereum", &call.summary(fees.chain_id, &to, 0), &format!("0x{}", to_hex(&hash)), || async {
                if let Some(token) = &token { let _ = self.device.provide_erc20(token).await; }
                self.device.sign_evm_transaction(&payload, &self.evm_path).await.map_err(device)
            }).await?;
            // The device's first byte is the parity for a typed transaction; recovering both settles it either way.
            for parity in [v & 1, (v & 1) ^ 1] {
                let signature = EvmSignature { r: r.clone(), s: s.clone(), v: 27 + parity as i64 };
                if recover(&hash, &signature).is_ok_and(|a| a.eq_ignore_ascii_case(&self.wallet.evm)) {
                    return tx.signed_with(&signature).map(|s| s.0).map_err(wallet_error);
                }
            }
            Err(self.not_this_ledger(&self.wallet.evm))
        })
    }

    /// A move out of a Safe multisig, shown field by field (SafeTx); the same rules as the signing process's.
    fn sign_safe_move(&self, m: safe::Move, recipients: Vec<String>) -> Signing<'_, EvmSignature> {
        Box::pin(async move {
            let approved: HashSet<String> = recipients.iter().chain([&self.wallet.evm, &m.safe]).map(|a| a.to_lowercase()).collect();
            if let Some(why) = safe::refusal(&m, &approved) { return Err(failed(why)) }
            self.typed(&m.typed(), &m.summary()).await
        })
    }

    fn sign_hyperliquid(&self, act: MsgPack, nonce: u64) -> Signing<'_, EvmSignature> {
        Box::pin(async move {
            let verdict = hl::check(&act, &self.wallet.evm, &HashSet::new(), None);
            if !verdict.ok() { return Err(refuse(&verdict)) }
            if let Some(agent) = &self.agent {
                if hls::is_trading(&act) && let Some(signature) = agent(act.clone(), nonce).await? { return Ok(signature) }
            }
            // A TWAP buys on for hours: only the Ledger's trading key can stop it with no one at the device.
            if action::kind(&act) == Some("twapOrder") && self.agent.is_none() {
                return Err(failed("A TWAP needs the Ledger's trading key on: without it, stopping the TWAP waits for the device's buttons. Nothing was signed."));
            }
            let what = if action::kind(&act) == Some("setReferrer") { "ihatecrypto's referral code on Hyperliquid (4% off its fees)" } else { "a Hyperliquid order" };
            let typed = hls::l1_typed(&act, None, nonce, None, true).map_err(wallet_error)?;
            self.typed(&typed, what).await
        })
    }

    fn sign_hyperliquid_user(&self, act: MsgPack, recipients: Vec<String>) -> Signing<'_, EvmSignature> {
        Box::pin(async move {
            let verdict = hl::check(&act, &self.wallet.evm, &recipients.iter().cloned().collect(), None);
            if !verdict.ok() { return Err(refuse(&verdict)) }
            let (ty, fields) = hls::user_action(&act).map_err(wallet_error)?;
            let what = if ty != "ApproveBuilderFee" { "a Hyperliquid transfer".to_string() }
                else if act.text("maxFeeRate") == Some(builder::REVOKE) { "taking back ihatecrypto's fee approval".into() }
                else { format!("ihatecrypto's {} fee on your Hyperliquid trades", builder::RATE) };
            self.typed(&hls::user_typed(&ty, fields, true), &what).await
        })
    }

    fn sign_relay_mapping(&self, w: String, depositor: String, id: Vec<u8>, nonce: u64) -> Signing<'_, EvmSignature> {
        Box::pin(async move {
            if !w.eq_ignore_ascii_case(&self.wallet.evm) || !depositor.eq_ignore_ascii_case(&self.wallet.evm) {
                return Err(failed(format!("The deposit mapping isn't for your Ledger: {w}, {depositor}.")));
            }
            let typed = hls::relay_nonce_mapping_typed(&w, &depositor, &id, nonce).map_err(wallet_error)?;
            self.typed(&typed, "link a withdrawal to a Relay order").await
        })
    }

    /// The same rules as the signing process; the Ledger shows each field of the typed data.
    fn sign_receive_authorization(&self, permit: Permit3009) -> Signing<'_, EvmSignature> {
        Box::pin(async move {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            if let Some(refusal) = permit.refusal(&self.wallet.evm, now) { return Err(failed(refusal)) }
            let typed = permit.typed().map_err(wallet_error)?;
            self.typed(&typed, &permit.summary()).await
        })
    }

    fn sign_near_intent_deposit(&self, _amount: u128, _deposit_address: String, _nonce: u64, _block_hash: Vec<u8>) -> Signing<'_, Vec<u8>> {
        Box::pin(async { Err(failed("NEAR isn't signed on a Ledger in ihatecrypto yet: switch to one of ihatecrypto's own wallets for NEAR.")) })
    }
}

#[cfg(test)]
#[path = "signer_tests.rs"]
mod tests;
