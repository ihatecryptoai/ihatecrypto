//! What the executor signs with (`LinkTradeSigner.swift`, as the Mac's XPCTradeSigner): every method one SignerRequest
//! over the link, and a refusal said with the signer's own reasons — a guard stop, never routed around.

use std::collections::BTreeMap;
use std::sync::Arc;

use evm::protocols::SponsoredTransfer;
use evm::protocols::batch7702::BatchCall;
use evm::protocols::relay::MulticallPermit;
use evm::protocols::{Permit3009, safe};
use execution::trade_signer::{EvmFees, Signing, TradeError, TradeSigner};
use hyperliquid::MsgPack;
use signer_api::{SignerCall, SignerCall3, SignerReply, SignerRequest};
use wallet::evm::EvmSignature;

use crate::signer_link::SignerLink;

pub struct LinkTradeSigner {
    pub link: Arc<SignerLink>,
    /// A scheduled task's run on the automation wallet: the signer is told which wallet signs, and allows it only a
    /// task's moves.
    pub as_wallet: Option<String>,
    /// While a signature waits (Windows Hello may be up), the window says so.
    pub waiting: Arc<dyn Fn(bool) + Send + Sync>,
}

/// The signer's words for a refusal: its error, then each problem its guard found.
pub fn reason(reply: &SignerReply) -> String {
    std::iter::once(reply.error.clone().unwrap_or_else(|| "The signer refused.".into())).chain(reply.problems.iter().cloned()).collect::<Vec<_>>().join(" ")
}

impl LinkTradeSigner {
    async fn send(&self, request: SignerRequest) -> Result<SignerReply, TradeError> {
        let request = match &self.as_wallet { Some(id) => SignerRequest::AsWallet { id: id.clone(), request: Box::new(request) }, None => request };
        (self.waiting)(true);
        let reply = self.link.send(&request).await;
        (self.waiting)(false);
        reply.map_err(|e| TradeError::Failed(e.0))
    }

    /// One request whose reply carries `pick`'s field; a reply without it is the signer's refusal.
    fn ask<T: Send + 'static>(&self, request: SignerRequest, pick: fn(SignerReply) -> Option<T>) -> Signing<'_, T> {
        Box::pin(async move {
            let reply = self.send(request).await?;
            if !reply.ok { return Err(TradeError::Guard(reason(&reply))) }
            let words = reason(&reply);
            pick(reply).ok_or(TradeError::Guard(words))
        })
    }
}

fn evm_request(fees: EvmFees, to: String, value: u64, data: Vec<u8>, recipients: Vec<String>) -> SignerRequest {
    SignerRequest::SignEvmTransaction {
        chain_id: fees.chain_id, nonce: fees.nonce, max_priority_fee: fees.max_priority_fee, max_fee: fees.max_fee, gas_limit: fees.gas_limit,
        to, value, data, recipients,
    }
}

impl TradeSigner for LinkTradeSigner {
    fn sign_solana(&self, wire: Vec<u8>, accounts: Vec<String>, recipients: Vec<String>, token_owners: BTreeMap<String, String>) -> Signing<'_, Vec<u8>> {
        self.ask(SignerRequest::SignSolana { wire, accounts, recipients, token_owners, extra_programs: vec![], sponsor: None }, |r| r.signed_wire)
    }

    fn sign_hyperliquid(&self, action: MsgPack, nonce: u64) -> Signing<'_, EvmSignature> {
        self.ask(SignerRequest::SignHyperliquidL1 { action, nonce, mainnet: true }, |r| r.signature)
    }

    fn sign_hyperliquid_user(&self, action: MsgPack, recipients: Vec<String>) -> Signing<'_, EvmSignature> {
        self.ask(SignerRequest::SignHyperliquidUser { action, mainnet: true, recipients }, |r| r.signature)
    }

    fn sign_relay_mapping(&self, wallet: String, depositor: String, id: Vec<u8>, nonce: u64) -> Signing<'_, EvmSignature> {
        self.ask(SignerRequest::SignRelayNonceMapping { wallet, depositor, id, nonce }, |r| r.signature)
    }

    fn sign_evm(&self, fees: EvmFees, to: String, data: Vec<u8>, recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        self.ask(evm_request(fees, to, 0, data, recipients), |r| r.signed_raw)
    }

    fn sign_near_intent_deposit(&self, amount: u128, deposit_address: String, nonce: u64, block_hash: Vec<u8>) -> Signing<'_, Vec<u8>> {
        self.ask(SignerRequest::SignNearIntentDeposit { amount: amount.to_string(), deposit_address, nonce, block_hash }, |r| r.signed_raw)
    }

    fn signs_batches(&self) -> bool { true }

    fn sign_evm_batch(&self, fees: EvmFees, calls: Vec<BatchCall>, recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        self.ask(SignerRequest::SignEvmBatch {
            chain_id: fees.chain_id, nonce: fees.nonce, max_priority_fee: fees.max_priority_fee, max_fee: fees.max_fee, gas_limit: fees.gas_limit,
            calls: calls.into_iter().map(|c| SignerCall { to: c.to, data: c.data }).collect(), recipients,
        }, |r| r.signed_raw)
    }

    fn sign_evm_payable(&self, fees: EvmFees, to: String, value: u64, data: Vec<u8>, recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        self.ask(evm_request(fees, to, value, data, recipients), |r| r.signed_raw)
    }

    fn sign_evm_transfer(&self, fees: EvmFees, to: String, value: u64, recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        self.ask(evm_request(fees, to, value, vec![], recipients), |r| r.signed_raw)
    }

    fn sign_near_transfer(&self, amount: u128, receiver: String, recipients: Vec<String>, nonce: u64, block_hash: Vec<u8>) -> Signing<'_, Vec<u8>> {
        self.ask(SignerRequest::SignNearTransfer { amount: amount.to_string(), receiver, recipients, nonce, block_hash }, |r| r.signed_raw)
    }

    fn sign_near_delete_account(&self, beneficiary: String, recipients: Vec<String>, nonce: u64, block_hash: Vec<u8>) -> Signing<'_, Vec<u8>> {
        self.ask(SignerRequest::SignNearDeleteAccount { beneficiary, recipients, nonce, block_hash }, |r| r.signed_raw)
    }

    fn sign_safe_move(&self, m: safe::Move, recipients: Vec<String>) -> Signing<'_, EvmSignature> {
        self.ask(SignerRequest::SignSafeMove { chain_id: m.chain_id, safe: m.safe, to: m.to, value: m.value, data: m.data, nonce: m.nonce, recipients },
                 |r| r.signature)
    }

    /// A Relay order before the deposit that names it: the signer checks whom it pays.
    fn register_relay_order(&self, order: Vec<u8>) -> Signing<'_, ()> {
        self.ask(SignerRequest::RelayOrder { order }, |_| Some(()))
    }

    // Dollars without gas, as the Mac's XPCTradeSigner: someone else pays the fee and the signer reads what it signs.
    fn sign_solana_sponsored(&self, wire: Vec<u8>, accounts: Vec<String>, token_owners: BTreeMap<String, String>, sponsor: String) -> Signing<'_, Vec<u8>> {
        self.ask(SignerRequest::SignSolana { wire, accounts, recipients: vec![], token_owners, extra_programs: vec![], sponsor: Some(sponsor) }, |r| r.signed_wire)
    }

    fn sign_sponsored_transfer(&self, t: SponsoredTransfer, recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        self.ask(SignerRequest::SignSponsoredTransfer { chain_id: t.chain_id, nonce: t.nonce, gas_limit: t.gas_limit, token: t.token, to: t.to, amount: t.amount, recipients },
                 |r| r.signed_raw)
    }

    fn sign_relay_multicall_permit(&self, m: MulticallPermit) -> Signing<'_, Vec<EvmSignature>> {
        self.ask(SignerRequest::SignRelayMulticallPermit {
            chain_id: m.chain_id, token: m.token, value: m.value, valid_before: m.valid_before, relayer: m.relayer, refund_to: m.refund_to,
            nft_recipient: m.nft_recipient, metadata: m.metadata,
            calls: m.calls.into_iter().map(|c| SignerCall3 { target: c.target, allow_failure: c.allow_failure, value: c.value, data: c.call_data }).collect(),
        }, |r| r.signatures.filter(|s| s.len() == 2))
    }

    fn sign_receive_authorization(&self, p: Permit3009) -> Signing<'_, EvmSignature> {
        self.ask(SignerRequest::SignReceiveAuthorization { chain_id: p.chain_id, token: p.token, to: p.to, value: p.value, valid_before: p.valid_before, nonce: p.nonce },
                 |r| r.signature)
    }
}

#[cfg(all(test, not(windows)))]
#[path = "link_signer/tests.rs"]
mod tests;
