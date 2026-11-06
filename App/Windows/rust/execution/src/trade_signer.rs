//! Whoever holds the keys (the top of `Execution/Trade/Executor.swift`): the engine's link to the signing process, or a
//! Ledger. Both run the guard before signing, so the executor cannot talk either into more. A signer that can't do a
//! kind of move says so in the words the Swift executor's defaults use.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use evm::protocols::SponsoredTransfer;
use evm::protocols::batch7702::BatchCall;
use evm::protocols::relay::MulticallPermit;
use evm::protocols::{Permit3009, safe};
use hyperliquid::MsgPack;
use wallet::evm::EvmSignature;

/// Why a signature didn't come back. `Guard`: the guard, the signer or a scam check stopped it — something in the move is
/// not what it should be, and nothing is offered around it (the Mac's GuardStop). `Failed`: anything else (no route, no
/// money, the network), which the chat may offer a way around.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeError {
    Guard(String),
    Failed(String),
}

impl std::fmt::Display for TradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { TradeError::Guard(m) | TradeError::Failed(m) => f.write_str(m) }
    }
}

impl std::error::Error for TradeError {}

pub type Signing<'a, T> = Pin<Box<dyn Future<Output = Result<T, TradeError>> + Send + 'a>>;

fn refuse<T: Send + 'static>(message: &str) -> Signing<'static, T> {
    let message = message.to_string();
    Box::pin(async move { Err(TradeError::Failed(message)) })
}

/// The wallet a move signs for: its Solana and EVM addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeWallet {
    pub solana: String,
    pub evm: String,
}

/// An EIP-1559 transaction's network, nonce and fees, as every EVM signing call takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvmFees {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee: u64,
    pub max_fee: u64,
    pub gas_limit: u64,
}

pub trait TradeSigner: Send + Sync {
    /// `accounts`: the message's full account list; `recipients`: addresses the person approved besides their own.
    fn sign_solana(&self, wire: Vec<u8>, accounts: Vec<String>, recipients: Vec<String>, token_owners: BTreeMap<String, String>) -> Signing<'_, Vec<u8>>;
    /// Orders, cancels, leverage.
    fn sign_hyperliquid(&self, action: MsgPack, nonce: u64) -> Signing<'_, EvmSignature>;
    /// Transfers signed by the main key (sendAsset to Relay's depository); `recipients` as approved.
    fn sign_hyperliquid_user(&self, action: MsgPack, recipients: Vec<String>) -> Signing<'_, EvmSignature>;
    /// Relay's binding of a Hyperliquid deposit to an order.
    fn sign_relay_mapping(&self, wallet: String, depositor: String, id: Vec<u8>, nonce: u64) -> Signing<'_, EvmSignature>;
    /// An EIP-1559 transaction; the signer decodes the calldata itself (no unlimited approvals, no transfers to others).
    fn sign_evm(&self, fees: EvmFees, to: String, data: Vec<u8>, recipients: Vec<String>) -> Signing<'_, Vec<u8>>;
    /// Native NEAR into a 1Click deposit: the signer builds the one wrap.near transaction itself and returns it signed.
    fn sign_near_intent_deposit(&self, amount: u128, deposit_address: String, nonce: u64, block_hash: Vec<u8>) -> Signing<'_, Vec<u8>>;

    /// Whether `sign_evm_batch` signs: the signing process does; a Ledger doesn't.
    fn signs_batches(&self) -> bool { false }

    /// `calls` as one EIP-7702 transaction from and to the wallet; each call is held to sign_evm's rules.
    fn sign_evm_batch(&self, _fees: EvmFees, _calls: Vec<BatchCall>, _recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        refuse("This signer doesn't sign batched calls.")
    }

    /// A contract call carrying `value` wei of the network's own coin: only where the coins come back to this wallet, or
    /// to `recipients`.
    fn sign_evm_payable(&self, _fees: EvmFees, _to: String, _value: u64, _data: Vec<u8>, _recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        refuse("This signer doesn't send the network's own coin into a contract: swap from its wrapped token (WETH) instead.")
    }

    /// The network's own coin: `value` wei to `to`, which must be among `recipients`; no calldata.
    fn sign_evm_transfer(&self, _fees: EvmFees, _to: String, _value: u64, _recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        refuse("Sending a network's own coin is signed in the ihatecrypto app.")
    }

    /// A native NEAR transfer of exactly `amount` yocto to `receiver`, which must be among `recipients`.
    fn sign_near_transfer(&self, _amount: u128, _receiver: String, _recipients: Vec<String>, _nonce: u64, _block_hash: Vec<u8>) -> Signing<'_, Vec<u8>> {
        refuse("NEAR transfers are signed in the ihatecrypto app.")
    }

    /// Deletes the wallet's NEAR account; NEAR sends all that is on it to `beneficiary` (one of `recipients`).
    fn sign_near_delete_account(&self, _beneficiary: String, _recipients: Vec<String>, _nonce: u64, _block_hash: Vec<u8>) -> Signing<'_, Vec<u8>> {
        refuse("Closing the NEAR account is signed in the ihatecrypto app.")
    }

    /// This key's owner signature on a move out of a Safe multisig.
    fn sign_safe_move(&self, _move: safe::Move, _recipients: Vec<String>) -> Signing<'_, EvmSignature> {
        refuse("This signer doesn't sign multisig moves.")
    }

    /// A transaction someone else pays the fee for and signs first (Jupiter's gasless swap); our wallet signs second.
    fn sign_solana_sponsored(&self, _wire: Vec<u8>, _accounts: Vec<String>, _token_owners: BTreeMap<String, String>, _sponsor: String) -> Signing<'_, Vec<u8>> {
        refuse("This signer doesn't sign a transaction someone else pays for: add a little SOL instead.")
    }

    /// Dollars Relay takes by signature: its solver pays the network fee.
    fn sign_receive_authorization(&self, _permit: Permit3009) -> Signing<'_, EvmSignature> {
        refuse("This signer doesn't pay by signature: add a little of the network's coin for gas instead.")
    }

    /// The same on Relay's v3.2 proxy: the MulticallAuthorization's signature, then the EIP-3009 one.
    fn sign_relay_multicall_permit(&self, _m: MulticallPermit) -> Signing<'_, Vec<EvmSignature>> {
        refuse("This signer doesn't pay by signature: add a little of the network's coin for gas instead.")
    }

    /// A zero-fee dollar transfer BNB Chain's paymaster pays the gas for; `to` among `recipients`. The raw transaction.
    fn sign_sponsored_transfer(&self, _t: SponsoredTransfer, _recipients: Vec<String>) -> Signing<'_, Vec<u8>> {
        refuse("This signer doesn't send a zero-fee transfer: add a little BNB for gas instead.")
    }

    /// A Relay order handed to the signer before the deposit that names it: the signer checks whom it pays. A signer that
    /// signs on its own screen (a Ledger) or in a test has nothing to check ahead.
    fn register_relay_order(&self, _order: Vec<u8>) -> Signing<'_, ()> { Box::pin(async { Ok(()) }) }
}
