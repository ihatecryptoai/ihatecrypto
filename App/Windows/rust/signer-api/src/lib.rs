//! What the engine asks the signer and what it answers (`Signer/API/SignerAPI.swift`): one JSON request, one JSON
//! reply, exactly as Swift's `JSONEncoder` writes them, so a Rust signer serves the Swift engine and back. Swift's
//! synthesized `Codable` for an enum: `{"case":{"label":…}}`, `{"case":{}}` with no values, `_0` for an unlabeled
//! value; a nil optional is left out, every other field is required (tests/golden.json is real Swift output).
//! The frame on the pipe: a 4-byte little-endian length, then the JSON (`Signer/Windows/Main/main.swift`).

use std::collections::BTreeMap;
use std::io::{self, Read, Write};

use hyperliquid::MsgPack;
use serde::{Deserialize, Serialize};
use wallet::b64;
use wallet::evm::EvmSignature;

/// Swift's `Data?`: base64 when present, left out when nil.
mod b64opt {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => s.serialize_str(&STANDARD.encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        Option::<String>::deserialize(d)?.map(|s| STANDARD.decode(s).map_err(serde::de::Error::custom)).transpose()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SignerRequest {
    Status {},
    /// Validates and stores the seed; the phrase itself is not kept. `require_presence`: Windows Hello on every use.
    ImportMnemonic { phrase: String, passphrase: String, require_presence: bool },
    /// A new 12-word phrase made inside the signer; the reply carries it once, for the paper backup.
    CreateWallet { require_presence: bool },
    /// Removes the active wallet; asks for the person first.
    Forget {},
    /// The address book's names and addresses as they were confirmed in person.
    AddressBook {},
    /// Saves entries to that book, the person first, every entry's whole address on the prompt.
    SaveAddresses { #[serde(rename = "_0")] addresses: Vec<SignerAddress> },
    /// Takes an entry out of the book (no prompt: removing one can't send money anywhere).
    RemoveAddress { name: String, network: String },
    /// A single private key from another wallet (`ImportedKey`), a wallet that signs only on that key's chain.
    ImportPrivateKey { key: String },
    SetActiveWallet { id: String },
    RemoveWallet { id: String },
    RenameWallet { id: String, label: String },
    /// A Solana transaction after the guard. `accounts`: the full account list the engine resolved. `extra_programs`:
    /// programs the person allowed for this one transaction. `sponsor`: someone else pays the fee and signs first.
    SignSolana {
        #[serde(with = "b64")] wire: Vec<u8>,
        accounts: Vec<String>,
        recipients: Vec<String>,
        token_owners: BTreeMap<String, String>,
        extra_programs: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")] sponsor: Option<String>,
    },
    /// Sign-in text from a site; refused when the bytes are a Solana transaction message in disguise.
    SignSolanaMessage { #[serde(with = "b64")] message: Vec<u8> },
    /// EVM personal_sign (EIP-191); a bare 32-byte hash is refused.
    SignPersonal { #[serde(with = "b64")] message: Vec<u8> },
    /// eth_signTypedData_v4: the signer parses and hashes the JSON itself.
    SignTypedData { #[serde(with = "b64")] json: Vec<u8> },
    /// An EIP-1559 transaction from a site; the signer decodes the calldata itself.
    #[serde(rename = "signEVMTransaction")]
    SignEvmTransaction {
        chain_id: u64, nonce: u64, max_priority_fee: u64, max_fee: u64, gas_limit: u64,
        to: String, value: u64, #[serde(with = "b64")] data: Vec<u8>, recipients: Vec<String>,
    },
    /// ihatecrypto's own calls as one EIP-7702 transaction.
    #[serde(rename = "signEVMBatch")]
    SignEvmBatch {
        chain_id: u64, nonce: u64, max_priority_fee: u64, max_fee: u64, gas_limit: u64,
        calls: Vec<SignerCall>, recipients: Vec<String>,
    },
    /// One owner's signature on a move out of a Safe multisig. Reply: signature.
    SignSafeMove { chain_id: u64, safe: String, to: String, value: u64, #[serde(with = "b64")] data: Vec<u8>, nonce: u64, recipients: Vec<String> },
    /// Hyperliquid order/cancel (L1); the signer hashes the action itself.
    SignHyperliquidL1 { action: MsgPack, nonce: u64, mainnet: bool },
    /// A Hyperliquid transfer-type action signed by the main key, its EIP-712 schema read from the action itself.
    SignHyperliquidUser { action: MsgPack, mainnet: bool, recipients: Vec<String> },
    /// Relay's binding of our Hyperliquid deposit to an order.
    SignRelayNonceMapping { wallet: String, depositor: String, #[serde(with = "b64")] id: Vec<u8>, nonce: u64 },
    /// A Relay order (its quote's protocol.v2.orderData, JSON) for the signer to check before a deposit names it.
    RelayOrder { #[serde(with = "b64")] order: Vec<u8> },
    /// EIP-3009 ReceiveWithAuthorization for Relay's approval proxy. Reply: signature.
    SignReceiveAuthorization { chain_id: u64, token: String, to: String, value: u64, valid_before: u64, #[serde(with = "b64")] nonce: Vec<u8> },
    /// A paid API's price by x402: EIP-3009 TransferWithAuthorization the signer builds itself (`x402::Authorization`) —
    /// a dollar it knows, from this wallet to `to`, at most 5 dollars, valid an hour at most, with `service` named on the
    /// prompt. Reply: signatures[0].
    SignX402 { chain_id: u64, token: String, to: String, value: u64, valid_before: u64, #[serde(with = "b64")] nonce: Vec<u8>, service: String },
    /// Relay's v3.2 MulticallAuthorization and its EIP-3009 authorization. Reply: signatures, the MulticallAuthorization's first.
    SignRelayMulticallPermit {
        chain_id: u64, token: String, value: u64, valid_before: u64, relayer: String, refund_to: String,
        nft_recipient: String, #[serde(with = "b64")] metadata: Vec<u8>, calls: Vec<SignerCall3>,
    },
    /// A dollar transfer on BNB Chain at gas price 0 (MegaFuel). Reply: signedRaw.
    SignSponsoredTransfer { chain_id: u64, nonce: u64, gas_limit: u64, token: String, to: String, amount: String, recipients: Vec<String> },
    /// Creates (once) a Hyperliquid agent key inside the signer; returns approveAgent signed by the main key.
    EnableAgent {},
    /// Whether Hyperliquid accepted the approval; only then do orders use the agent key.
    SetAgentActive { #[serde(rename = "_0")] active: bool },
    /// A Hyperliquid trading key for a Ledger's account, made once when `create`. Reply: agentAddress, agentActive.
    LedgerAgent { owner: String, create: bool },
    SetLedgerAgentActive { owner: String, active: bool },
    /// An order, cancel or leverage change for the Ledger's account, signed by its trading key; nothing else.
    SignLedgerAgentL1 { owner: String, action: MsgPack, nonce: u64 },
    /// The NEAR implicit account of the same phrase (m/44'/397'/0').
    NearAddress {},
    /// Ends the session and pauses the trading key.
    Lock {},
    /// One of `session::CHOICES`, in seconds; a longer time asks the person first.
    SetSessionLength { seconds: i64 },
    /// Ends the session only (the screen locked, the computer slept).
    EndSession {},
    /// Sells native NEAR through 1Click: the signer builds the one wrap.near transaction itself. Reply: signedRaw.
    SignNearIntentDeposit { amount: String, deposit_address: String, nonce: u64, #[serde(with = "b64")] block_hash: Vec<u8> },
    /// A native NEAR transfer the signer builds itself; the receiver must be one of `recipients`.
    SignNearTransfer { amount: String, receiver: String, recipients: Vec<String>, nonce: u64, #[serde(with = "b64")] block_hash: Vec<u8> },
    /// DeleteAccount of the wallet's NEAR account into `beneficiary` (one of `recipients`).
    SignNearDeleteAccount { beneficiary: String, recipients: Vec<String>, nonce: u64, #[serde(with = "b64")] block_hash: Vec<u8> },
    /// NEAR Intents' Agent API, as an agent wallet's owner (this wallet's NEAR key): a NEP-413 message the signer reads
    /// itself (`agent_owner::message`): only to the Agent API's host, only owner requests it can say, confirmed in person
    /// each time. Reply: signedRaw = the 64-byte ed25519 signature.
    SignAgentMessage { #[serde(with = "b64")] payload: Vec<u8> },
    /// The same owner's NEP-366 delegate action to the agent wallets' custody contract (`agent_owner::delegate`: create,
    /// freeze, a policy change), which the API's sponsor sends on chain. `nonce`, `max_block_height`: from the owner's
    /// access key. `policy`: the complete policy the call carries (or hashes), checked by the signer itself
    /// (`agent_owner::policy`). Reply: signedRaw = the borsh SignedDelegateAction.
    SignAgentDelegate {
        #[serde(with = "b64")] payload: Vec<u8>,
        nonce: u64,
        max_block_height: u64,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "b64opt")] policy: Option<Vec<u8>>,
    },
    /// A new automation wallet (one at most), signing without a prompt for scheduled tasks' budget.
    CreateAutomationWallet {},
    /// A 6-digit passcode (`passcode::problem` must pass), confirmed in person first.
    SetCode { code: String },
    ClearCode {},
    /// The passcode typed in the app. `sign`: it also opens a signing session.
    EnterCode { code: String, sign: bool },
    /// `request` signed by the automation wallet `id` while another wallet stays active: a scheduled task's move.
    AsWallet { id: String, request: Box<SignerRequest> },
}

/// One call Relay's proxy runs (Multicall3's Call3Value).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignerCall3 {
    pub target: String,
    pub allow_failure: bool,
    pub value: u64,
    #[serde(with = "b64")]
    pub data: Vec<u8>,
}

/// One call of a batch: a contract and its calldata, no coins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignerCall {
    pub to: String,
    #[serde(with = "b64")]
    pub data: Vec<u8>,
}

/// A saved address as the signer keeps it: the name money goes to and the address confirmed for it in person.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SignerAddress {
    pub name: String,
    /// AddressCheck's network raw value: «solana», «base», «near»…
    pub network: String,
    pub address: String,
    /// An exchange's memo or tag, confirmed with the address ("" for none; always on the wire).
    pub memo: String,
}

impl SignerAddress {
    /// The same entry: the name in any case, a 0x address in any case, a Solana or NEAR one exactly.
    pub fn same(&self, name: &str, network: &str, address: &str) -> bool {
        self.name.to_lowercase() == name.to_lowercase() && self.network == network
            && if self.address.starts_with("0x") { self.address.to_lowercase() == address.to_lowercase() } else { self.address == address }
    }
}

/// One wallet the signer holds, public parts only: its name, what it can sign for, its addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignerWallet {
    pub id: String,
    pub label: String,
    /// "phrase": every chain from a recovery phrase; "solana" or "evm": one private key, that chain only.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solana: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near: Option<String>,
    /// A test wallet: it signs without a prompt (None in older registries: no).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<bool>,
    /// The automation wallet: signs without a prompt, only what a task asks for by its id (`AsWallet`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation: Option<bool>,
}

impl SignerWallet {
    /// Wallet «1» is the one held before several wallets: its stored items keep their old names.
    pub const FIRST_ID: &str = "1";

    pub fn is_test(&self) -> bool { self.test == Some(true) }

    pub fn is_automation(&self) -> bool { self.automation == Some(true) }

    /// The registry the signer keeps. Before several wallets there was one sealed seed and its addresses: that becomes
    /// wallet «1», «Main», active — nothing is re-sealed and no prompt appears.
    pub fn migrated(registry: Option<Vec<SignerWallet>>, seed_present: bool, solana: Option<String>, evm: Option<String>, near: Option<String>) -> Vec<SignerWallet> {
        if let Some(r) = registry { return r }
        if !seed_present || (solana.is_none() && evm.is_none()) { return vec![] }
        vec![SignerWallet { id: Self::FIRST_ID.into(), label: "Main".into(), kind: "phrase".into(), solana, evm, near, test: None, automation: None }]
    }

    /// The wallet that's on: the chosen one, else the first left — never the automation wallet, which signs only a
    /// task's moves (`AsWallet`). None: none signs until the person picks one.
    pub fn on<'a>(list: &'a [SignerWallet], chosen: Option<&str>) -> Option<&'a SignerWallet> {
        let mut can = list.iter().filter(|w| !w.is_automation());
        can.clone().find(|w| Some(w.id.as_str()) == chosen).or_else(|| can.next())
    }

    /// «this wallet has no Solana key» when a key wallet is asked to sign on another chain.
    pub fn missing(&self, chain: &str) -> Option<String> {
        let has = match chain {
            "solana" => self.solana.is_some(),
            "evm" => self.evm.is_some(),
            _ => self.near.is_some(),
        };
        if has { return None }
        let name = match chain { "solana" => "Solana", "evm" => "EVM (Hyperliquid, Base…)", "near" => "NEAR", other => other };
        let holds = match self.kind.as_str() { "solana" => "a Solana", "evm" => "an EVM", _ => "a" };
        Some(format!("The wallet «{}» has no {name} key: it holds only {holds} key. Switch wallets (Settings › Wallet) or import one that has it.", self.label))
    }
}

/// The app's 6-digit passcode, checked the same way by the engine and the signer.
pub mod passcode {
    pub const LENGTH: usize = 6;

    /// Why `code` can't be one: not six digits, or one guessed first (all the same digit, a straight run up or down) —
    /// with five tries before the code turns off, those would be the five.
    pub fn problem(code: &str) -> Option<String> {
        // Swift: six characters, each a whole number, all ASCII — six ASCII digits.
        if code.len() != LENGTH || !code.bytes().all(|b| b.is_ascii_digit()) { return Some(format!("The code is {LENGTH} digits.")) }
        let d: Vec<i32> = code.bytes().map(|b| (b - b'0') as i32).collect();
        let mut steps: Vec<i32> = d.windows(2).map(|w| (w[1] - w[0] + 10) % 10).collect();
        steps.sort();
        steps.dedup();
        match steps.as_slice() {
            [0] => Some("All the same digit is the first code anyone tries.".into()),
            [1] | [9] => Some("A straight run like 123456 is guessed first.".into()),
            _ => None,
        }
    }
}

pub mod session {
    /// 1, 5, 15 or 30 minutes; 1 minute until the person picks another, and with that minute the engine also ends the
    /// session when the move is done (`EndSession`), so one prompt covers one move.
    pub const CHOICES: [i64; 4] = [60, 300, 900, 1800];
    pub const STANDARD: i64 = 60;
}

/// The signer's answer. Every optional is left out when None; `checks` and `problems` are always there (Swift's
/// decoder requires them).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignerReply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_wallet: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solana_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evm_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near_address: Option<String>,
    /// Only in the reply to CreateWallet: the new phrase, for the person's paper backup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phrase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64opt")]
    pub signed_wire: Option<Vec<u8>>,
    /// A signed raw EVM transaction (or NEAR's borsh SignedTransaction).
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64opt")]
    pub signed_raw: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<EvmSignature>,
    /// Several signatures from one request, in its order (SignRelayMulticallPermit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signatures: Option<Vec<EvmSignature>>,
    /// A raw ed25519 signature (signed messages).
    #[serde(default, skip_serializing_if = "Option::is_none", with = "b64opt")]
    pub signature_bytes: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_active: Option<bool>,
    /// Every wallet the signer holds (public parts) and the id of the one in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallets: Option<Vec<SignerWallet>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_wallet: Option<String>,
    /// Seconds one confirmation keeps signing open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_length: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<MsgPack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<u64>,
    /// The address book as the signer keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addresses: Option<Vec<SignerAddress>>,
    /// A passcode is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_set: Option<bool>,
    /// The request waits for the passcode instead of the system prompt: `error` says what it signs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_code: Option<bool>,
    /// Five wrong codes: the code is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_off: Option<bool>,
    pub checks: Vec<String>,
    pub problems: Vec<String>,
}

impl SignerReply {
    pub fn ok() -> Self { Self { ok: true, ..Default::default() } }

    pub fn error(message: impl Into<String>) -> Self { Self { ok: false, error: Some(message.into()), ..Default::default() } }
}

/// The largest frame either side takes: 16 MiB.
pub const MAX_FRAME: usize = 16 << 20;

/// One frame: a 4-byte little-endian length, then the JSON.
pub fn write_frame(out: &mut impl Write, body: &[u8]) -> io::Result<()> {
    out.write_all(&(body.len() as u32).to_le_bytes())?;
    out.write_all(body)?;
    out.flush()
}

/// The next frame's body; None when the pipe closed or the length is past `MAX_FRAME` (the other side is gone or
/// broken, so the loop ends).
pub fn read_frame(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header).ok()?;
    let length = u32::from_le_bytes(header) as usize;
    if length > MAX_FRAME { return None }
    let mut body = vec![0u8; length];
    input.read_exact(&mut body).ok()?;
    Some(body)
}

#[cfg(test)]
mod tests;
