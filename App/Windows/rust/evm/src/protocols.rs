//! The protocols whose calls the signer reads and builds offline (`Chains/EVM/*.swift`): their addresses per network,
//! their calls encoded and read back, and why one isn't signed. What they need from the network is the engine's.

use wallet::evm::eip712::{self, Field, Typed, Value};
use wallet::evm::{EvmSignature, keccak, sign};
use wallet::{Result, from_hex, to_hex};

use crate::abi::{self, address_in, bytes_array, padded, u256_in, uint_in};
use crate::call::Call;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};

use crate::{Rlp, Transaction, U256, Authorization, fail, network_name, SIGNABLE};

fn selector(signature: &str) -> [u8; 4] { keccak(signature.as_bytes())[..4].try_into().expect("4 bytes") }

fn is_address(a: &str) -> bool { a.len() == 42 && a.starts_with("0x") && a[2..].bytes().all(|b| b.is_ascii_hexdigit()) }

const ZERO: &str = "0x0000000000000000000000000000000000000000";

/// Circle's CCTP v2 for USDC between EVM networks (`CCTP.swift`).
pub mod cctp {
    use super::*;

    pub const TOKEN_MESSENGER: &str = "0x28b5a0e9c621a5badaa536219b3a228c8168cf5d";
    pub const MESSAGE_TRANSMITTER: &str = "0x81d40f21f12a8f0e3252bccb954d722d4c464b64";
    pub const IRIS: &str = "https://iris-api.circle.com";
    /// «cctp-forward», version 0, no more data: Circle mints on the destination.
    pub const FORWARD_HOOK: &str = "636374702d666f72776172640000000000000000000000000000000000000000";
    /// Fast transfer: attested once the burn is in a block, seconds.
    pub const FAST: u32 = 1000;

    /// CCTP's domain for an EVM chain id; None where ihatecrypto doesn't use it.
    pub fn domain(chain_id: u64) -> Option<u32> { Some(match chain_id { 1 => 0, 10 => 2, 42161 => 3, 8453 => 6, 137 => 7, _ => return None }) }

    pub fn chain_name(domain: u32) -> String {
        match domain { 0 => "Ethereum", 2 => "Optimism", 3 => "Arbitrum", 6 => "Base", 7 => "Polygon", 5 => "Solana", _ => return format!("domain {domain}") }.into()
    }

    /// depositForBurnWithHook(amount, destinationDomain, mintRecipient, burnToken, destinationCaller 0, maxFee, fast, hook).
    pub fn burn(units: u64, domain: u32, recipient: &str, usdc: &str, max_fee: u64) -> Result<Vec<u8>> {
        let hook = from_hex(FORWARD_HOOK).expect("hex");
        Ok([&[0x77, 0x9b, 0x43, 0x2d][..], &abi::uint(units), &abi::uint(domain as u64), &abi::address(recipient)?, &abi::address(usdc)?, &[0; 32],
            &abi::uint(max_fee), &abi::uint(FAST as u64), &abi::uint(8 * 32), &abi::uint(hook.len() as u64), &hook].concat())
    }

    /// Who a burn's mint pays and on which domain, read from depositForBurn or depositForBurnWithHook; the recipient is
    /// None when its bytes32 isn't an EVM address (a Solana account).
    pub fn read_burn(data: &[u8]) -> Option<(Option<String>, u32, Vec<u8>)> {
        if !(data.starts_with(&[0x8e, 0x02, 0x50, 0xee]) || data.starts_with(&[0x77, 0x9b, 0x43, 0x2d])) { return None }
        let args = &data[4..];
        let domain = uint_in(args, 1).filter(|&d| args.len() >= 7 * 32 && d <= u32::MAX as u64)?;
        let word = &args[64..96];
        let recipient = word[..12].iter().all(|&b| b == 0).then(|| format!("0x{}", to_hex(&word[12..])));
        Some((recipient, domain as u32, args[..32].to_vec()))
    }
}

/// Tether's USDT0, a LayerZero OFT (`USDT0.swift`).
pub mod usdt0 {
    use super::*;

    pub struct Deployment {
        /// LayerZero's endpoint id of the network.
        pub eid: u32,
        /// The OFT `send` goes to: the adapter on Ethereum, which pulls USDT by an approve; elsewhere it burns by itself.
        pub oft: &'static str,
        pub token: &'static str,
        pub approves: bool,
        /// How long a transfer from here takes to land (LayerZero Scan medians, 2026-10-13).
        pub minutes: u32,
    }

    pub const DEPLOYMENTS: [(u64, Deployment); 5] = [
        (1, Deployment { eid: 30101, oft: "0x6c96de32cea08842dcc4058c14d3aaad7fa41dee", token: "0xdac17f958d2ee523a2206206994597c13d831ec7", approves: true, minutes: 20 }),
        (42161, Deployment { eid: 30110, oft: "0x14e4a1b13bf7f943c8ff7c51fb60fa964a298d92", token: "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9", approves: false, minutes: 20 }),
        (10, Deployment { eid: 30111, oft: "0xf03b4d9ac1d5d1e7c4cef54c2a313b9fe051a0ad", token: "0x01bff41798a0bcf287b996046ca68b395dbc1071", approves: false, minutes: 35 }),
        (137, Deployment { eid: 30109, oft: "0x6ba10300f0dc58b7a1e4c0e41f5dabb7d7829e13", token: "0xc2132d05d31c914a87c6611c10748aeb04b58e8f", approves: false, minutes: 2 }),
        (999, Deployment { eid: 30367, oft: "0x904861a24f30ec96ea7cfc3be9ea4b476d237e98", token: "0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb", approves: false, minutes: 710 }),
    ];

    pub fn on(chain_id: u64) -> Option<&'static Deployment> { DEPLOYMENTS.iter().find(|(c, _)| *c == chain_id).map(|(_, d)| d) }

    /// The network an endpoint id names, for what the signer says it signs.
    pub fn network_name(eid: u32) -> String {
        DEPLOYMENTS.iter().find(|(_, d)| d.eid == eid).and_then(|(c, _)| crate::name(*c)).map_or_else(|| format!("LayerZero's network {eid}"), str::to_string)
    }

    /// «≈ 20 min», «≈ 11 h 50 min».
    pub fn wait(minutes: u32) -> String { if minutes < 60 { format!("≈ {minutes} min") } else { format!("≈ {} h {} min", minutes / 60, minutes % 60) } }

    /// SendParam(dstEid, to, amountLD, minAmountLD, extraOptions, composeMsg, oftCmd), the last three empty.
    fn param(eid: u32, recipient: &str, units: u64, min: u64) -> Result<Vec<u8>> {
        Ok([&abi::uint(eid as u64)[..], &abi::address(recipient)?, &abi::uint(units), &abi::uint(min),
            &abi::uint(7 * 32), &abi::uint(8 * 32), &abi::uint(9 * 32), &[0; 96]].concat())
    }

    /// quoteOFT(SendParam).
    pub fn quote_oft(eid: u32, recipient: &str, units: u64) -> Result<Vec<u8>> {
        Ok([&[0x0d, 0x35, 0xb4, 0x15][..], &abi::uint(32), &param(eid, recipient, units, 0)?].concat())
    }

    /// quoteSend(SendParam, payInLzToken false).
    pub fn quote_send(eid: u32, recipient: &str, units: u64, min: u64) -> Result<Vec<u8>> {
        Ok([&[0x3b, 0x6f, 0x74, 0x3b][..], &abi::uint(64), &abi::uint(0), &param(eid, recipient, units, min)?].concat())
    }

    /// send(SendParam, MessagingFee(nativeFee, 0), refundAddress).
    pub fn send(eid: u32, recipient: &str, units: u64, min: u64, fee: u64, refund: &str) -> Result<Vec<u8>> {
        Ok([&[0xc7, 0xc7, 0xf5, 0xb3][..], &abi::uint(4 * 32), &abi::uint(fee), &abi::uint(0), &abi::address(refund)?,
            &param(eid, recipient, units, min)?].concat())
    }

    /// A read send: who gets the tokens on which network (None: not an EVM address), how many, whether it is plain,
    /// and who gets the unused fee back.
    pub struct Send {
        pub recipient: Option<String>,
        pub eid: u32,
        pub amount: Vec<u8>,
        pub plain: bool,
        pub refund: String,
    }

    /// An OFT's send read back, wherever its encoder put the pieces; None when it isn't a send or can't be read.
    pub fn read_send(data: &[u8]) -> Option<Send> {
        if !data.starts_with(&[0xc7, 0xc7, 0xf5, 0xb3]) { return None }
        let args = &data[4..];
        let at = uint_in(args, 0).filter(|&a| a % 32 == 0 && a <= args.len() as u64)? as usize;
        let refund = address_in(args, 3)?;
        let p = &args[at..];
        let eid = uint_in(p, 0).filter(|&e| p.len() >= 7 * 32 && e <= u32::MAX as u64)?;
        let empty = |word: usize| -> Option<bool> {
            let o = uint_in(p, word).filter(|&o| o % 32 == 0 && p.len() >= 32 && o <= (p.len() - 32) as u64)? as usize;
            Some(uint_in(&p[o..], 0).map_or(false, |n| n == 0))
        };
        let (options, compose, command) = (empty(4)?, empty(5)?, empty(6)?);
        let to = &p[32..64];
        Some(Send {
            recipient: to[..12].iter().all(|&b| b == 0).then(|| format!("0x{}", to_hex(&to[12..]))),
            eid: eid as u32,
            amount: p[64..96].to_vec(),
            plain: options && compose && command && uint_in(args, 2) == Some(0),
            refund,
        })
    }
}

/// Merkl's Distributor (`Merkl.swift`): the calls the signer reads and builds. The API and sweeps are the engine's.
pub mod merkl {
    use super::*;

    /// The same address on every chain Merkl pays out on.
    pub const DISTRIBUTOR: &str = "0x3ef3d8ba38ebe18db133cec108f4d14ce00dd9ae";
    /// Base, Arbitrum, Ethereum, Polygon.
    pub const CHAINS: [u64; 4] = [8453, 42161, 1, 137];

    #[derive(Debug, Clone, PartialEq)]
    pub struct Reward {
        pub chain_id: u64,
        pub token: String,
        pub symbol: String,
        pub decimals: u32,
        /// Everything ever earned, in the token's smallest units: the amount `claim` takes.
        pub amount: Decimal,
        /// Already claimed, in the same units.
        pub claimed: Decimal,
        pub proofs: Vec<String>,
        /// The root the proofs are for.
        pub root: String,
        pub price: Option<Decimal>,
    }

    impl Reward {
        /// What claiming pays now, in tokens.
        pub fn claimable(&self) -> Decimal { (0..self.decimals).fold(self.amount.saturating_sub(self.claimed), |x, _| x / Decimal::TEN) }
        /// None without a price, or past Decimal's range (Merkl's numbers are outside data: no panic on them).
        pub fn worth_usd(&self) -> Option<Decimal> { self.price.and_then(|p| self.claimable().checked_mul(p)) }
    }

    /// A sweep's floor: what the person said, never under $5 — below it the claim and the sale cost most of it in gas.
    pub fn sweep_floor(asked: Option<Decimal>) -> Decimal { asked.unwrap_or(Decimal::from(5)).max(Decimal::from(5)) }

    /// What a sweep sells: each reward worth a dollar or more that isn't already the coin it ends in. Dust stays.
    pub fn to_sell(owed: &[Reward], into: Option<&str>) -> Vec<Reward> {
        owed.iter().filter(|r| {
            let s = r.symbol.to_uppercase();
            r.worth_usd().unwrap_or_default() >= Decimal::ONE && s != "USDC" && s != "USDG" && Some(s) != into.map(str::to_uppercase)
        }).cloned().collect()
    }

    /// The API's answer: `[{chain, rewards: [{root, recipient, amount, claimed, proofs, token: {chainId, address,
    /// decimals, symbol, price}}]}]`. Only rewards paid to `user` with something left to claim.
    pub fn parse(data: &[u8], user: &str) -> Result<Vec<Reward>> {
        #[derive(serde::Deserialize)]
        struct Chain { rewards: Vec<Entry> }
        #[derive(serde::Deserialize)]
        struct Entry { root: String, recipient: String, amount: String, claimed: String, proofs: Vec<String>, token: Token }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Token { chain_id: u64, address: String, decimals: u32, symbol: String, price: Option<f64> }
        let chains: Vec<Chain> = serde_json::from_slice(data).map_err(|e| wallet::WalletError(format!("Merkl's answer can't be read: {e}")))?;
        Ok(chains.into_iter().flat_map(|c| c.rewards).filter_map(|e| {
            let (amount, claimed) = (e.amount.parse::<Decimal>().ok()?, e.claimed.parse::<Decimal>().ok()?);
            (e.recipient.to_lowercase() == user.to_lowercase() && amount > claimed).then(|| Reward {
                chain_id: e.token.chain_id, token: e.token.address, symbol: e.token.symbol, decimals: e.token.decimals, amount, claimed,
                proofs: e.proofs, root: e.root, price: e.token.price.and_then(Decimal::from_f64),
            })
        }).collect())
    }

    /// claim(address[] users, address[] tokens, uint256[] amounts, bytes32[][] proofs): every reward in `rewards` paid
    /// to `user`.
    pub fn claim(user: &str, rewards: &[Reward]) -> Result<Vec<u8>> {
        let array = |words: Vec<[u8; 32]>| -> Vec<u8> { [abi::uint(words.len() as u64).to_vec(), words.concat()].concat() };
        let users = array(rewards.iter().map(|_| abi::address(user)).collect::<Result<_>>()?);
        let tokens = array(rewards.iter().map(|r| abi::address(&r.token)).collect::<Result<_>>()?);
        let amounts = array(rewards.iter().map(|r| abi::word_decimal(r.amount)).collect::<Result<_>>()?);
        let inner: Vec<Vec<u8>> = rewards.iter().map(|r| {
            Ok(array(r.proofs.iter().map(|p| match from_hex(p) {
                Some(b) if b.len() == 32 => Ok(b.try_into().expect("32")),
                _ => fail(format!("A Merkl proof isn't 32 bytes: {p}")),
            }).collect::<Result<_>>()?))
        }).collect::<Result<_>>()?;
        // bytes32[][]: its length, an offset per proof (counted from right after the length), then each proof.
        let (mut offsets, mut at) = (Vec::new(), 32 * inner.len() as u64);
        for p in &inner {
            offsets.extend(abi::uint(at));
            at += p.len() as u64;
        }
        let proofs = [abi::uint(inner.len() as u64).to_vec(), offsets, inner.concat()].concat();
        let (mut head, mut tail, mut start) = (Vec::new(), Vec::new(), 4 * 32u64);
        for part in [users, tokens, amounts, proofs] {
            head.extend(abi::uint(start));
            start += part.len() as u64;
            tail.extend(part);
        }
        Ok([&[0x71, 0xee, 0x95, 0xc0][..], &head, &tail].concat())
    }

    /// The users a `claim` pays, read back from its calldata; None when it isn't one.
    pub fn recipients(data: &[u8]) -> Option<Vec<String>> {
        if data.len() < 36 || !data.starts_with(&[0x71, 0xee, 0x95, 0xc0]) { return None }
        let args = &data[4..];
        let at = uint_in(args, 0).filter(|&a| a % 32 == 0 && a < args.len() as u64)? as usize;
        let n = uint_in(args, at / 32).filter(|&n| n <= 256)? as usize;
        let users: Vec<String> = (0..n).filter_map(|i| address_in(args, at / 32 + 1 + i)).collect();
        (users.len() == n).then_some(users)
    }

    /// getMerkleRoot().
    pub const ROOT_CALL: [u8; 4] = [0x49, 0x59, 0x06, 0x57];
}

/// A multisig on Safe v1.4.1 (`Safe.swift`).
pub mod safe {
    use super::*;

    /// SafeProxyFactory 1.4.1.
    pub const FACTORY: &str = "0x4e1dcf7ad4e460cfd30791ccc4f9c8a4f820ec67";
    /// SafeL2 1.4.1, the code every multisig runs.
    pub const SINGLETON: &str = "0x29fcb43b46531bca003ddc8fcb67ffe91900c762";
    /// CompatibilityFallbackHandler 1.4.1.
    pub const FALLBACK_HANDLER: &str = "0xfd0732dc9e303f09fcef3a7388ad10a83459ec99";
    /// The factory's proxyCreationCode(), read from it on Base (the same on every network).
    const PROXY_CREATION_CODE: &str = "608060405234801561001057600080fd5b506040516101e63803806101e68339818101604052602081101561003357600080fd5b8101908080519060200190929190505050600073ffffffffffffffffffffffffffffffffffffffff168173ffffffffffffffffffffffffffffffffffffffff1614156100ca576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004018080602001828103825260228152602001806101c46022913960400191505060405180910390fd5b806000806101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff1602179055505060ab806101196000396000f3fe608060405273ffffffffffffffffffffffffffffffffffffffff600054167fa619486e0000000000000000000000000000000000000000000000000000000060003514156050578060005260206000f35b3660008037600080366000845af43d6000803e60008114156070573d6000fd5b3d6000f3fea264697066735822122003d1488ee65e08fa41e58e888a9865554c535f2c77126a82cb4c0f917f31441364736f6c63430007060033496e76616c69642073696e676c65746f6e20616464726573732070726f7669646564";

    pub fn exec_selector() -> [u8; 4] { selector("execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)") }
    pub fn approve_hash_call() -> [u8; 4] { selector("approveHash(bytes32)") }
    /// nonce(), getOwners(), getThreshold(): what a multisig says of itself.
    pub fn nonce_call() -> [u8; 4] { selector("nonce()") }
    pub fn owners_call() -> [u8; 4] { selector("getOwners()") }
    pub fn threshold_call() -> [u8; 4] { selector("getThreshold()") }

    /// Why these can't own a multisig, or None: at least one owner, each a distinct address, and `threshold` of them.
    pub fn owners_problem(owners: &[String], threshold: usize) -> Option<String> {
        if owners.is_empty() { return Some("A multisig needs at least one key.".into()) }
        if !owners.iter().all(|o| is_address(o) && o.to_lowercase() != ZERO && o.to_lowercase() != "0x0000000000000000000000000000000000000001") {
            return Some("Every key of a multisig is an EVM address.".into());
        }
        let distinct: std::collections::HashSet<String> = owners.iter().map(|o| o.to_lowercase()).collect();
        if distinct.len() != owners.len() { return Some("The same key is in the list twice.".into()) }
        if threshold < 1 || threshold > owners.len() {
            return Some(format!("A multisig needs 1 to {} signatures, not {threshold}.", owners.len()));
        }
        None
    }

    /// setup(owners, threshold, to, data, fallbackHandler, paymentToken, payment, paymentReceiver).
    pub fn setup(owners: &[String], threshold: usize) -> Result<Vec<u8>> {
        if let Some(p) = owners_problem(owners, threshold) { return fail(p) }
        let n = owners.len() as u64;
        let mut out = selector("setup(address[],uint256,address,bytes,address,address,uint256,address)").to_vec();
        for w in [abi::uint(8 * 32), abi::uint(threshold as u64), abi::address(ZERO)?, abi::uint(8 * 32 + 32 * (1 + n)),
                  abi::address(FALLBACK_HANDLER)?, abi::address(ZERO)?, abi::uint(0), abi::address(ZERO)?, abi::uint(n)] {
            out.extend(w);
        }
        for o in owners { out.extend(abi::address(o)?) }
        out.extend(abi::uint(0)); // the empty `data`
        Ok(out)
    }

    /// createProxyWithNonce(singleton, initializer, saltNonce).
    pub fn create(owners: &[String], threshold: usize, salt: u64) -> Result<Vec<u8>> {
        let init = setup(owners, threshold)?;
        Ok([&selector("createProxyWithNonce(address,bytes,uint256)")[..], &abi::address(SINGLETON)?, &abi::uint(0x60), &abi::uint(salt),
            &abi::uint(init.len() as u64), &padded(&init)].concat())
    }

    /// Where `create` puts the multisig, before it exists (CREATE2). Lowercase.
    pub fn address(owners: &[String], threshold: usize, salt: u64) -> Result<String> {
        let init = setup(owners, threshold)?;
        let s = keccak(&[keccak(&init).as_slice(), &abi::uint(salt)].concat());
        let code = keccak(&[from_hex(PROXY_CREATION_CODE).expect("hex"), abi::address(SINGLETON)?.to_vec()].concat());
        Ok(format!("0x{}", to_hex(&keccak(&[&[0xff][..], &from_hex(FACTORY).expect("hex"), &s, &code].concat())[12..])))
    }

    /// One move out of a multisig, as its owners sign it: a plain CALL of `to` with `value` wei and `data`, at the
    /// multisig's `nonce`. Operation, gas refund and refund receiver are always zero here.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Move {
        pub chain_id: u64,
        pub safe: String,
        pub to: String,
        pub value: u64,
        pub data: Vec<u8>,
        pub nonce: u64,
    }

    impl Move {
        /// The SafeTx typed data (EIP-712): a Ledger shows it field by field.
        pub fn typed(&self) -> Typed {
            Typed {
                domain: vec![Field::new("chainId", "uint256", Value::Uint(self.chain_id)), Field::new("verifyingContract", "address", Value::Address(self.safe.clone()))],
                ty: "SafeTx".into(),
                message: vec![
                    Field::new("to", "address", Value::Address(self.to.clone())), Field::new("value", "uint256", Value::Uint(self.value)),
                    Field::new("data", "bytes", Value::Bytes(self.data.clone())), Field::new("operation", "uint8", Value::Uint(0)),
                    Field::new("safeTxGas", "uint256", Value::Uint(0)), Field::new("baseGas", "uint256", Value::Uint(0)),
                    Field::new("gasPrice", "uint256", Value::Uint(0)), Field::new("gasToken", "address", Value::Address(ZERO.into())),
                    Field::new("refundReceiver", "address", Value::Address(ZERO.into())), Field::new("nonce", "uint256", Value::Uint(self.nonce)),
                ],
            }
        }

        /// What each owner signs: the multisig's getTransactionHash for this move.
        pub fn hash(&self) -> Result<[u8; 32]> { Ok(self.typed().parts()?.digest()) }

        /// «send 200 USDC to 0x3b9C…e21A on Base from the multisig 0x7a3F…91c2»: read from the calldata, for a prompt.
        pub fn summary(&self) -> String {
            format!("{} from the multisig {}…{}", Call::decode(&self.data).summary(self.chain_id, &self.to, self.value),
                    &self.safe[..6.min(self.safe.len())], &self.safe[self.safe.len().saturating_sub(4)..])
        }

        /// execTransaction(…, signatures): the move with its owners' signatures, sorted by owner as the contract wants.
        pub fn exec(&self, signatures: &[(String, EvmSignature)]) -> Result<Vec<u8>> {
            let mut sorted: Vec<&(String, EvmSignature)> = signatures.iter().collect();
            sorted.sort_by_key(|s| s.0.to_lowercase());
            let mut packed = Vec::new();
            for (_, s) in sorted {
                if s.r.len() > 32 || s.s.len() > 32 || !(s.v == 27 || s.v == 28) { return fail("A malformed signature for the multisig.") }
                packed.extend(vec![0; 32 - s.r.len()]);
                packed.extend(&s.r);
                packed.extend(vec![0; 32 - s.s.len()]);
                packed.extend(&s.s);
                packed.push(s.v as u8);
            }
            let data_part = [abi::uint(self.data.len() as u64).to_vec(), padded(&self.data)].concat();
            let mut out = exec_selector().to_vec();
            for w in [abi::address(&self.to)?, abi::uint(self.value), abi::uint(10 * 32), abi::uint(0), abi::uint(0), abi::uint(0), abi::uint(0),
                      abi::address(ZERO)?, abi::address(ZERO)?, abi::uint(10 * 32 + data_part.len() as u64)] {
                out.extend(w);
            }
            out.extend(data_part);
            out.extend(abi::uint(packed.len() as u64));
            out.extend(padded(&packed));
            Ok(out)
        }
    }

    /// An execTransaction's call read back: (to, value word, data, operation). None when the calldata isn't one.
    pub fn read_exec(calldata: &[u8]) -> Option<(String, Vec<u8>, Vec<u8>, u8)> {
        if calldata.len() < 4 + 10 * 32 || calldata[..4] != exec_selector() { return None }
        let args = &calldata[4..];
        let to = address_in(args, 0)?;
        let offset = uint_in(args, 2)?;
        let op = uint_in(args, 3).filter(|&o| o <= 255)?;
        // Subtracted, not added: an offset or length near 2⁶⁴ would overflow.
        if offset % 32 != 0 || offset > args.len() as u64 - 32 { return None }
        let length = uint_in(args, (offset / 32) as usize).filter(|&l| l <= args.len() as u64 - 32 - offset)?;
        let start = offset as usize + 32;
        Some((to, args[32..64].to_vec(), args[start..start + length as usize].to_vec(), op as u8))
    }

    /// An execTransaction this wallet sends itself, held to the same rules: no call into the multisig, only a payout,
    /// no gas refund; approveHash never. None: neither call, or a plain payout.
    pub fn exec_refusal(calldata: &[u8], safe: &str) -> Option<String> {
        if calldata.starts_with(&approve_hash_call()) {
            return Some("The call approves a multisig move ahead of time (Safe's approveHash): anyone could then carry it out. ihatecrypto doesn't sign that.".into());
        }
        if !calldata.starts_with(&exec_selector()) { return None }
        let Some((to, value, data, _)) = read_exec(calldata) else {
            return Some("A multisig move ihatecrypto can't read: its parts aren't where Safe's own encoding puts them. ihatecrypto doesn't sign that.".into());
        };
        let args = &calldata[4..];
        if args.len() >= 7 * 32 && args[192..224].iter().any(|&b| b != 0) {
            return Some("The multisig move pays a gas refund to an address it doesn't show. ihatecrypto doesn't sign that.".into());
        }
        if to.to_lowercase() == safe.to_lowercase() {
            return Some("The move changes the multisig itself (its keys, modules or guard). ihatecrypto doesn't sign that.".into());
        }
        match Call::decode(&data) {
            Call::Plain if data.is_empty() => None,
            Call::Transfer { .. } if value.iter().all(|&b| b == 0) => None,
            _ => Some(format!("A multisig move ihatecrypto signs is a plain payment of a coin or a token. This one calls the contract {to}.")),
        }
    }

    /// getOwners()'s answer: a dynamic address array.
    pub fn owners(data: &[u8]) -> Option<Vec<String>> {
        let offset = uint_in(data, 0).filter(|o| o % 32 == 0)? as usize;
        let n = uint_in(data, offset / 32).filter(|&n| n <= 1000)? as usize;
        (0..n).map(|i| address_in(data, offset / 32 + 1 + i)).collect()
    }

    /// Why an owner mustn't sign this move, or None: only the multisig paying out, to `approved` (lowercase).
    pub fn refusal(m: &Move, approved: &std::collections::HashSet<String>) -> Option<String> {
        if !SIGNABLE.contains(&m.chain_id) { return Some(format!("ihatecrypto doesn't sign for network {}.", m.chain_id)) }
        if !is_address(&m.safe) || !is_address(&m.to) { return Some("Not an EVM address in the multisig's move.".into()) }
        if m.to.to_lowercase() == m.safe.to_lowercase() {
            return Some("The move changes the multisig itself (its keys, modules or guard). ihatecrypto doesn't sign that.".into());
        }
        let call = Call::decode(&m.data);
        match call {
            Call::Plain if m.data.is_empty() => {}
            Call::Transfer { .. } if m.value == 0 => {}
            _ => return Some(format!("A multisig move ihatecrypto signs is a plain payment of a coin or a token. This one calls the contract {}.", m.to)),
        }
        call.payees(&m.to, m.value, m.chain_id).into_iter().find(|p| !approved.contains(&p.to_lowercase()))
            .map(|p| format!("Money from the multisig goes to an unapproved address {p}."))
    }
}

/// KyberSwap's MetaAggregationRouterV2 (`KyberSwap.swift`): the call read back. Routes and builds are the engine's.
pub mod kyber {
    use super::*;

    pub const ROUTER: &str = "0x6131b5fae19ea4f9d964eac0408e4408b66337b5";
    pub const API: &str = "https://aggregator-api.kyberswap.com";

    /// Its API's name for each network ihatecrypto swaps on there.
    pub fn chain(chain_id: u64) -> Option<&'static str> {
        Some(match chain_id { 1 => "ethereum", 10 => "optimism", 56 => "bsc", 137 => "polygon", 999 => "hyperevm", 8453 => "base", 42161 => "arbitrum", _ => return None })
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Swap {
        pub src_token: String,
        pub dst_token: String,
        pub dst_receiver: String,
        pub amount: U256,
        pub min_return: U256,
        pub fee_receivers: Vec<String>,
        pub permit: bool,
    }

    /// swap(SwapExecutionParams{callTarget, approveTarget, targetData, desc, clientData}) — e21fd0e9. None for anything
    /// else, or offsets that aren't 32-byte aligned or run past the data.
    pub fn decode(data: &[u8]) -> Option<Swap> {
        if !data.starts_with(&[0xe2, 0x1f, 0xd0, 0xe9]) { return None }
        let args = &data[4..];
        let word = |at: usize| -> Option<usize> {
            if at % 32 != 0 { return None }
            uint_in(args, at / 32).filter(|&n| n < args.len() as u64 && n % 32 == 0).map(|n| n as usize)
        };
        let address = |at: usize| if at % 32 == 0 { address_in(args, at / 32) } else { None };
        let params = word(0)?;
        let desc = params + word(params + 3 * 32)?;
        let src = address(desc)?;
        let dst = address(desc + 32)?;
        let receiver = address(desc + 6 * 32)?;
        let amount = u256_in(args, (desc + 7 * 32) / 32)?;
        let min_return = u256_in(args, (desc + 8 * 32) / 32)?;
        let fees = relay::addresses(&args[desc.min(args.len())..], 4 * 32)?;
        let permit_at = word(desc + 10 * 32)?;
        let permit_length = uint_in(args, (desc + permit_at) / 32)?;
        Some(Swap { src_token: src, dst_token: dst, dst_receiver: receiver, amount, min_return, fee_receivers: fees, permit: permit_length > 0 })
    }

    /// Why the signer won't sign this swap, or None.
    pub fn refusal(s: &Swap, sender: &str) -> Option<String> {
        if s.dst_receiver.to_lowercase() != sender.to_lowercase() { return Some(format!("The KyberSwap route would pay {}, not this wallet.", s.dst_receiver)) }
        if let Some(fee) = s.fee_receivers.first() { return Some(format!("The KyberSwap route pays a fee to {fee}. ihatecrypto's routes pay no one else.")) }
        if s.permit { return Some("The KyberSwap route carries a permit to spend your tokens. ihatecrypto approves the exact amount itself.".into()) }
        if s.min_return.is_zero() { return Some("The KyberSwap route sets no least amount to come out: any price would do. ihatecrypto always sets one.".into()) }
        None
    }
}

/// Uniswap v2 Router02 and v3 SwapRouter02 per network (`UniswapV2.swift`, `UniswapV3.swift`).
pub mod uniswap {
    use super::*;

    /// getPool(address,address,uint24)
    pub fn get_pool(a: &str, b: &str, fee: u64) -> Result<Vec<u8>> {
        Ok([&[0x16, 0x98, 0xee, 0x82][..], &abi::address(a)?, &abi::address(b)?, &abi::uint(fee)].concat())
    }

    /// QuoterV2.quoteExactInputSingle((tokenIn, tokenOut, amountIn, fee, sqrtPriceLimitX96)).
    pub fn quote_exact_input_single(token_in: &str, token_out: &str, amount_in: U256, fee: u64) -> Result<Vec<u8>> {
        Ok([&[0xc6, 0xa5, 0x02, 0x6a][..], &abi::address(token_in)?, &abi::address(token_out)?, &abi::word(amount_in), &abi::uint(fee), &abi::uint(0)].concat())
    }

    /// SwapRouter02.exactInputSingle((tokenIn, tokenOut, fee, recipient, amountIn, amountOutMinimum, sqrtPriceLimitX96)).
    pub fn exact_input_single(token_in: &str, token_out: &str, fee: u64, recipient: &str, amount_in: U256, amount_out_minimum: U256) -> Result<Vec<u8>> {
        Ok([&[0x04, 0xe4, 0x5a, 0xaf][..], &abi::address(token_in)?, &abi::address(token_out)?, &abi::uint(fee), &abi::address(recipient)?,
            &abi::word(amount_in), &abi::word(amount_out_minimum), &abi::uint(0)].concat())
    }

    /// A route through pools: token, fee (3 bytes), token, fee, token… One pool is a path of two tokens.
    pub fn path(tokens: &[&str], fees: &[u64]) -> Result<Vec<u8>> {
        if tokens.len() != fees.len() + 1 { return fail("A Uniswap path needs one fee between each two tokens.") }
        let mut out = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            let Some(bytes) = from_hex(token).filter(|b| b.len() == 20) else { return fail(format!("Invalid address: {token}")) };
            out.extend(bytes);
            if let Some(fee) = fees.get(i) { out.extend(&(*fee as u32).to_be_bytes()[1..]) }
        }
        Ok(out)
    }

    fn bytes(d: &[u8]) -> Vec<u8> { [abi::uint(d.len() as u64).to_vec(), padded(d)].concat() }

    /// QuoterV2.quoteExactInput(bytes path, uint256 amountIn).
    pub fn quote_exact_input(path: &[u8], amount_in: U256) -> Vec<u8> {
        [&[0xcd, 0xca, 0x17, 0x53][..], &abi::uint(0x40), &abi::word(amount_in), &bytes(path)].concat()
    }

    /// SwapRouter02.exactInput((bytes path, address recipient, uint256 amountIn, uint256 amountOutMinimum)).
    pub fn exact_input(path: &[u8], recipient: &str, amount_in: U256, amount_out_minimum: U256) -> Result<Vec<u8>> {
        Ok([&[0xb8, 0x58, 0x18, 0x3f][..], &abi::uint(0x20), &abi::uint(0x80), &abi::address(recipient)?, &abi::word(amount_in),
            &abi::word(amount_out_minimum), &bytes(path)].concat())
    }

    /// address[]: its length, then each address.
    fn addresses(path: &[&str]) -> Result<Vec<u8>> {
        let mut out = abi::uint(path.len() as u64).to_vec();
        for a in path { out.extend(abi::address(a)?) }
        Ok(out)
    }

    /// Router02.getAmountsOut(amountIn, path).
    pub fn get_amounts_out(amount_in: U256, path: &[&str]) -> Result<Vec<u8>> {
        Ok([&[0xd0, 0x6c, 0xa6, 0x1f][..], &abi::word(amount_in), &abi::uint(0x40), &addresses(path)?].concat())
    }

    /// The last of getAmountsOut's amounts (uint256[]: offset, length, amounts): what comes out, before any transfer tax.
    pub fn amount_out(answer: &[u8]) -> Option<U256> {
        let n = uint_in(answer, 1).filter(|n| (2..=4).contains(n))?;
        u256_in(answer, 1 + n as usize)
    }

    /// swapExactTokensForTokensSupportingFeeOnTransferTokens(amountIn, amountOutMin, path, to, deadline); `to_eth`:
    /// swapExactTokensForETHSupportingFeeOnTransferTokens, the path ending in WETH, which the router unwraps for `to`.
    pub fn swap_exact_tokens(amount_in: U256, amount_out_min: U256, path: &[&str], to: &str, deadline: u64, to_eth: bool) -> Result<Vec<u8>> {
        let selector: [u8; 4] = if to_eth { [0x79, 0x1a, 0xc9, 0x47] } else { [0x5c, 0x11, 0xd7, 0x95] };
        Ok([&selector[..], &abi::word(amount_in), &abi::word(amount_out_min), &abi::uint(0xa0), &abi::address(to)?, &abi::uint(deadline),
            &addresses(path)?].concat())
    }

    /// (router, WETH the router wraps the coin into).
    pub fn v2(chain_id: u64) -> Option<(&'static str, &'static str)> {
        Some(match chain_id {
            8453 => ("0x4752ba5dbc23f44d87826276bf6fd6b1c372ad24", "0x4200000000000000000000000000000000000006"),
            42161 => ("0x4752ba5dbc23f44d87826276bf6fd6b1c372ad24", "0x82af49447d8a07e3bd95bd0d56f35241523fbab1"),
            1 => ("0x7a250d5630b4cf539739df2c5dacb4c659f2488d", "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
            137 => ("0xedf6066a2b290c185783862c7f4776a2c8077ad1", "0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270"),
            4663 => ("0x89e5db8b5aa49aa85ac63f691524311aeb649eba", "0x0bd7d308f8e1639fab988df18a8011f41eacad73"),
            10 => ("0x4a7b5da61326a6379179b40d00f57e5bbdc962c2", "0x4200000000000000000000000000000000000006"),
            999 => ("0x1f7d7550b1b028f7571e69a784071f0205fd2efa", "0x5555555555555555555555555555555555555555"),
            56 => ("0x4752ba5dbc23f44d87826276bf6fd6b1c372ad24", "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"),
            _ => return None,
        })
    }

    pub const ROBINHOOD_ROUTER: &str = "0xcaf681a66d020601342297493863e78c959e5cb2";

    /// (QuoterV2, SwapRouter02, WETH).
    pub fn v3(chain_id: u64) -> Option<(&'static str, &'static str, &'static str)> {
        const Q: &str = "0x61ffe014ba17989e743c5f6cb21bf9697530b21e";
        const R: &str = "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45";
        Some(match chain_id {
            8453 => ("0x3d4e44eb1374240ce5f1b871ab261cd16335b76a", "0x2626664c2603336e57b271c5c0b26f421741e481", "0x4200000000000000000000000000000000000006"),
            42161 => (Q, R, "0x82af49447d8a07e3bd95bd0d56f35241523fbab1"),
            1 => (Q, R, "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
            137 => (Q, R, "0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270"),
            10 => (Q, R, "0x4200000000000000000000000000000000000006"),
            999 => ("0x7dfd4f31be6814d2906bde155c3e1b146eac1468", "0x7adf4701abcdbc5dcf5cb58b526f897e048f0d11", "0x5555555555555555555555555555555555555555"),
            4663 => ("0x33e885ed0ec9bf04ecfb19341582aadcb4c8a9e7", ROBINHOOD_ROUTER, "0x0bd7d308f8e1639fab988df18a8011f41eacad73"),
            56 => ("0x78d78e420da98ad378d7799be8f4af69033eb077", "0xb971ef87ede563556b2ed4b1c0b0019111dd85d2", "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"),
            _ => return None,
        })
    }
}

/// Uniswap v4 through the Universal Router 2.1.x (`UniswapV4.swift`).
pub mod v4 {
    use super::*;

    pub const PERMIT2: &str = "0x000000000022d473030f116ddee9f6b43ac78ba3";
    /// The currency v4 pools name the network's own coin by.
    pub const NATIVE: &str = ZERO;

    pub struct Deployment {
        pub pool_manager: &'static str,
        pub quoter: &'static str,
        pub router: &'static str,
        pub position_manager: &'static str,
        pub blockscout: &'static str,
    }

    pub fn on(chain_id: u64) -> Option<Deployment> {
        let d = |pool_manager, quoter, router, position_manager, blockscout| Deployment { pool_manager, quoter, router, position_manager, blockscout };
        Some(match chain_id {
            4663 => d("0x8366a39cc670b4001a1121b8f6a443a643e40951", "0x8dc178efb8111bb0973dd9d722ebeff267c98f94", "0x204faca1764b154221e35c0d20abb3c525710498",
                      "0x58daec3116aae6d93017baaea7749052e8a04fa7", "robinhoodchain.blockscout.com"),
            8453 => d("0x498581ff718922c3f8e6a244956af099b2652b2b", "0x0d5e0f971ed27fbff6c2837bf31316121532048d", "0xd6145b2d3f379919e8cdeda7b97e37c4b2ca9c40",
                      "0x7c5f5a4bbd8fd63184577525326123b519429bdc", "base.blockscout.com"),
            42161 => d("0x360e68faccca8ca495c1b759fd9eee466db9fb32", "0x3972c00f7ed4885e145823eb7c655375d275a1c5", "0x2d01411773c8c24805306e89a41f7855c3c4fe65",
                       "0xd88f38f930b7952f2db2432cb002e7abbf3dd869", "arbitrum.blockscout.com"),
            1 => d("0x000000000004444c5dc75cb358380d2e3de08a90", "0x52f0e24d1c21c8a0cb1e5a5dd6198556bd9e1203", "0x23617e59a5925b2a4bf75d73ff6711cd0b29de85",
                   "0xbd216513d74c8cf14cf4747e6aaa6420ff64ee9e", "eth.blockscout.com"),
            137 => d("0x67366782805870060151383f4bbff9dab53e5cd6", "0xb3d5c3dfc3a7aebff71895a7191796bffc2c81b9", "0xdc264714f68d84cf29bc605589405e78bdbe7c9f",
                     "0x1ec2ebf4f37e7363fdfe3551602425af0b3ceef9", "polygon.blockscout.com"),
            10 => d("0x9a13f98cb987694c9f086b1f5eb990eea8264ec3", "0x1f3131a13296fb91c90870043742c3cdbff1a8d7", "0xc09255d86db563cbc11c2fcf4a0c512e160111b4",
                    "0x3c3ea4b57a46241e54610e5f022e5c45859a1017", "explorer.optimism.io"),
            999 => d("0x12d4fd9c5dedd00ab8a0bce2cf0167bbf94b6b1f", "0x108bfe38532c98f8cd8d7ab49c9ddaa3675a1c5e", "0xed270a1bcdc63cf1356d695e7f40961d4819d6bf",
                     "0x0d7ab5b3db668128aff6f70c4ebc71d7d4da9bf9", ""),
            56 => d("0x28e2ea090877bf75740558f6bfb36a5ffee9e9df", "0x9f75dd27d6664c475b90e105573e550ff69437b0", "0xdc264714f68d84cf29bc605589405e78bdbe7c9f",
                    "0x7a4a5c919ae2541aed11041a1aeee68f1287f95b", ""),
            _ => return None,
        })
    }

    /// keccak256 of the PoolManager's Initialize event.
    pub const INITIALIZE_TOPIC: &str = "0xdd466e674ea557f56295e2d0218a125ea4b4f0f6f3307b95f85e6110838d6438";

    /// A pool's identity: its two currencies (the lower address first), fee, tick spacing and hooks contract.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PoolKey {
        pub currency0: String,
        pub currency1: String,
        pub fee: u32,
        pub tick_spacing: i32,
        pub hooks: String,
    }

    impl PoolKey {
        pub fn new(currency0: &str, currency1: &str, fee: u32, tick_spacing: i32, hooks: &str) -> Self {
            Self { currency0: currency0.to_lowercase(), currency1: currency1.to_lowercase(), fee, tick_spacing, hooks: hooks.to_lowercase() }
        }

        /// abi.encode(currency0, currency1, fee, tickSpacing, hooks): five words, tick spacing sign-extended.
        pub fn encoded(&self) -> Result<Vec<u8>> {
            let mut spacing = if self.tick_spacing >= 0 { [0u8; 32] } else { [0xff; 32] };
            spacing[24..].copy_from_slice(&(self.tick_spacing as i64).to_be_bytes());
            Ok([abi::address(&self.currency0)?, abi::address(&self.currency1)?, abi::uint(self.fee as u64), spacing, abi::address(&self.hooks)?].concat())
        }

        /// keccak256(abi.encode(key)): the id DexScreener and the PoolManager's events know the pool by.
        pub fn id(&self) -> Option<String> { self.encoded().ok().map(|e| format!("0x{}", to_hex(&keccak(&e)))) }

        pub fn has_hooks(&self) -> bool { self.hooks != ZERO }
    }

    /// One exact-input swap in one pool, as the app sends it and as the signer reads it back.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Swap {
        pub key: PoolKey,
        pub zero_for_one: bool,
        pub amount_in: U256,
        pub amount_out_minimum: U256,
        /// SETTLE_ALL: what is paid and at most how much; TAKE_ALL: what comes out and at least how much.
        pub settle_currency: String,
        pub settle_max: U256,
        pub take_currency: String,
        pub take_min: U256,
        pub deadline: u64,
    }

    impl Swap {
        pub fn currency_in(&self) -> &str { if self.zero_for_one { &self.key.currency0 } else { &self.key.currency1 } }
        pub fn currency_out(&self) -> &str { if self.zero_for_one { &self.key.currency1 } else { &self.key.currency0 } }
    }

    const EXECUTE: [u8; 4] = [0x35, 0x93, 0x56, 0x4c];
    const V4_SWAP_COMMAND: u8 = 0x10;
    /// SWAP_EXACT_IN_SINGLE, SETTLE_ALL, TAKE_ALL
    const ACTIONS: [u8; 3] = [0x06, 0x0c, 0x0f];

    fn bytes_value(d: &[u8]) -> Vec<u8> { [abi::uint(d.len() as u64).to_vec(), padded(d)].concat() }

    /// bytes[]: its length, an offset to each element (from after the length), the elements.
    fn dynamic_array(items: &[Vec<u8>]) -> Vec<u8> {
        let (mut head, mut tail) = (Vec::new(), Vec::new());
        for item in items {
            head.extend(abi::uint((items.len() * 32 + tail.len()) as u64));
            tail.extend(bytes_value(item));
        }
        [abi::uint(items.len() as u64).to_vec(), head, tail].concat()
    }

    /// V4Quoter.quoteExactInputSingle((key, zeroForOne, exactAmount, hookData)).
    pub fn quote_exact_input_single(key: &PoolKey, zero_for_one: bool, amount: U256) -> Result<Vec<u8>> {
        Ok([&[0xaa, 0x9d, 0x21, 0xcb][..], &abi::uint(0x20), &key.encoded()?, &abi::uint(zero_for_one as u64), &abi::word(amount),
            &abi::uint(0x100), &abi::uint(0)].concat())
    }

    /// UniversalRouter.execute(commands = [V4_SWAP], inputs = [abi.encode(actions, params)], deadline).
    pub fn execute(key: &PoolKey, zero_for_one: bool, amount_in: U256, amount_out_minimum: U256, deadline: u64) -> Result<Vec<u8>> {
        let (cin, cout) = if zero_for_one { (&key.currency0, &key.currency1) } else { (&key.currency1, &key.currency0) };
        let swap = [&abi::uint(0x20)[..], &key.encoded()?, &abi::uint(zero_for_one as u64), &abi::word(amount_in), &abi::word(amount_out_minimum),
                    &abi::uint(0), &abi::uint(0x140), &abi::uint(0)].concat();
        let settle = [abi::address(cin)?, abi::word(amount_in)].concat();
        let take = [abi::address(cout)?, abi::word(amount_out_minimum)].concat();
        let input = [abi::uint(0x40).to_vec(), abi::uint(0x40 + bytes_value(&ACTIONS).len() as u64).to_vec(), bytes_value(&ACTIONS),
                     dynamic_array(&[swap, settle, take])].concat();
        let commands = [V4_SWAP_COMMAND];
        Ok([&EXECUTE[..], &abi::uint(0x60), &abi::uint(0x60 + bytes_value(&commands).len() as u64), &abi::uint(deadline),
            &bytes_value(&commands), &dynamic_array(&[input])].concat())
    }

    /// Permit2.approve(token, spender, uint160 amount, uint48 expiration).
    pub fn permit2_approve(token: &str, spender: &str, amount: U256, expiration: u64) -> Result<Vec<u8>> {
        Ok([&[0x87, 0x51, 0x7c, 0x45][..], &abi::address(token)?, &abi::address(spender)?, &abi::word(amount), &abi::uint(expiration)].concat())
    }

    /// Permit2.allowance(owner, token, spender) → (amount, expiration, nonce).
    pub fn permit2_allowance(owner: &str, token: &str, spender: &str) -> Result<Vec<u8>> {
        Ok([&[0x92, 0x7d, 0xa1, 0x05][..], &abi::address(owner)?, &abi::address(token)?, &abi::address(spender)?].concat())
    }

    /// The `bytes` whose offset is head word `i` of `d`; each number is held to the data's size first.
    fn bytes_at(d: &[u8], i: usize) -> Option<Vec<u8>> {
        let at = uint_in(d, i).filter(|&a| a % 32 == 0 && a < d.len() as u64 && a as usize + 32 <= d.len())? as usize;
        let length = uint_in(&d[at..], 0).filter(|&l| l <= d.len() as u64 && at + 32 + l as usize <= d.len())? as usize;
        Some(d[at + 32..at + 32 + length].to_vec())
    }

    /// The `bytes[]` whose offset is head word `i` of `d`.
    fn array_at(d: &[u8], i: usize) -> Option<Vec<Vec<u8>>> {
        let at = uint_in(d, i).filter(|&a| a % 32 == 0 && a < d.len() as u64 && a as usize + 32 <= d.len())? as usize;
        let array = &d[at..];
        let count = uint_in(array, 0).filter(|&c| c <= 16)? as usize;
        (0..count).map(|k| bytes_at(&array[32..], k)).collect()
    }

    /// The swap in an execute call when it is exactly this app's shape — one V4_SWAP command whose actions are
    /// SWAP_EXACT_IN_SINGLE, SETTLE_ALL, TAKE_ALL, each well formed — else None.
    pub fn decode_execute(data: &[u8]) -> Option<Swap> {
        if !data.starts_with(&EXECUTE) { return None }
        let args = &data[4..];
        if bytes_at(args, 0)? != [V4_SWAP_COMMAND] { return None }
        let inputs = array_at(args, 1).filter(|i| i.len() == 1)?;
        let deadline = uint_in(args, 2)?;
        let input = &inputs[0];
        if bytes_at(input, 0)? != ACTIONS { return None }
        let params = array_at(input, 1).filter(|p| p.len() == 3)?;
        if uint_in(&params[0], 0) != Some(0x20) { return None }
        let s = &params[0][0x20..];
        if s.len() < 11 * 32 { return None }
        let c0 = address_in(s, 0)?;
        let c1 = address_in(s, 1)?;
        let fee = uint_in(s, 2).filter(|&f| f < 1 << 24)?;
        let hooks = address_in(s, 4)?;
        let dir = uint_in(s, 5).filter(|&d| d <= 1)?;
        let amount_in = u256_in(s, 6)?;
        let min_out = u256_in(s, 7)?;
        if uint_in(s, 9) != Some(0x140) || uint_in(s, 10) != Some(0) { return None }
        let spacing = i64::from_be_bytes(s[3 * 32 + 24..4 * 32].try_into().ok()?) as i32;
        if params[1].len() != 64 || params[2].len() != 64 { return None }
        Some(Swap {
            key: PoolKey::new(&c0, &c1, fee as u32, spacing, &hooks), zero_for_one: dir == 1, amount_in, amount_out_minimum: min_out,
            settle_currency: address_in(&params[1], 0)?, settle_max: u256_in(&params[1], 1)?,
            take_currency: address_in(&params[2], 0)?, take_min: u256_in(&params[2], 1)?, deadline,
        })
    }

    /// LPFeeLibrary.DYNAMIC_FEE_FLAG in a pool's fee: its hook sets the fee each swap.
    const DYNAMIC_FEE: u32 = 0x800000;

    /// BEFORE_SWAP_RETURNS_DELTA or AFTER_SWAP_RETURNS_DELTA in the hook address's low bits.
    pub(crate) fn takes_swap_delta(hooks: &str) -> bool {
        u16::from_str_radix(&hooks[hooks.len().saturating_sub(4)..], 16).map_or(true, |low| low & 0b1100 != 0)
    }

    /// Whether a swap may run through the pool's hooks contract: none, or a vanilla one Uniswap's registry vouches for
    /// on this network that takes no delta, in a pool whose fee isn't set by the hook.
    pub fn allows_hooks(key: &PoolKey, chain_id: u64) -> bool {
        !key.has_hooks() || (key.fee & DYNAMIC_FEE == 0 && !takes_swap_delta(&key.hooks) && crate::hooks::vanilla_hooks(chain_id).contains(&key.hooks.as_str()))
    }

    /// Why the signer must not sign this swap, or None.
    pub fn refusal(s: &Swap, chain_id: Option<u64>) -> Option<String> {
        if s.key.has_hooks() && !chain_id.is_some_and(|c| allows_hooks(&s.key, c)) {
            return Some(format!("The Uniswap v4 pool runs a hooks contract ({}) that isn't on Uniswap's list of hooks that swap like a pool without one: ihatecrypto doesn't swap there.", s.key.hooks));
        }
        if s.settle_currency.to_lowercase() != s.currency_in() { return Some(format!("The swap would pay in {}, not the pool's {}.", s.settle_currency, s.currency_in())) }
        if s.take_currency.to_lowercase() != s.currency_out() { return Some(format!("The swap would take {}, not the pool's {}.", s.take_currency, s.currency_out())) }
        if s.settle_max != s.amount_in { return Some(format!("The swap may take more than it swaps ({} vs {}).", s.settle_max, s.amount_in)) }
        if s.amount_out_minimum.is_zero() || s.take_min.is_zero() { return Some("The swap has no minimum out: it could pay nothing back.".into()) }
        None
    }
}

/// Aave v3's USDC markets (`AaveV3.swift`): the calls and where they go. Reading the pool and the rate is the engine's.
pub mod aave {
    use super::*;

    /// One network's USDC market. Arbitrum and Polygon share the pool's address but not the aToken.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Market {
        /// The network's key («base», «arbitrum», «polygon», «ethereum») and name.
        pub chain: &'static str,
        pub chain_name: &'static str,
        pub chain_id: u64,
        pub usdc: &'static str,
        pub provider: &'static str,
        pub pool: &'static str,
        pub a_usdc: &'static str,
    }

    impl Market {
        /// How the app names it: «Aave · Arbitrum».
        pub fn place(&self) -> String { format!("Aave · {}", self.chain_name) }
    }

    pub const MARKETS: [Market; 4] = [
        Market { chain: "base", chain_name: "Base", chain_id: 8453, usdc: crate::BASE_USDC, provider: "0xe20fcbdbffc4dd138ce8b2e6fbb6cb49777ad64d",
                 pool: "0xa238dd80c259a72e81d7e4664a9801593f98d1c5", a_usdc: "0x4e65fe4dba92790696d040ac24aa414708f5c0ab" },
        Market { chain: "arbitrum", chain_name: "Arbitrum", chain_id: 42161, usdc: crate::ARBITRUM_USDC, provider: "0xa97684ead0e402dc232d5a977953df7ecbab3cdb",
                 pool: "0x794a61358d6845594f94dc1db02a252b5b4814ad", a_usdc: "0x724dc807b04555b71ed48a6896b6f41593b8c637" },
        Market { chain: "polygon", chain_name: "Polygon", chain_id: 137, usdc: crate::POLYGON_USDC, provider: "0xa97684ead0e402dc232d5a977953df7ecbab3cdb",
                 pool: "0x794a61358d6845594f94dc1db02a252b5b4814ad", a_usdc: "0xa4d94019934d8333ef880abffbf2fdd611c762bd" },
        Market { chain: "ethereum", chain_name: "Ethereum", chain_id: 1, usdc: crate::ETHEREUM_USDC, provider: "0x2f39d218133afab8f2b819b1066c7e434ad94e9e",
                 pool: "0x87870bca3f3fd6335c3f4ce8392d69350b4fa4e2", a_usdc: "0x98c23e9d8f34fefb1b7bd6a91b7ff122f4e16f5c" },
    ];

    pub fn market(place: &str) -> Option<&'static Market> { MARKETS.iter().find(|m| m.place() == place) }

    /// supply(address asset, uint256 amount, address onBehalfOf, uint16 referralCode)
    pub fn supply(asset: &str, amount: u64, on_behalf_of: &str) -> Result<Vec<u8>> {
        Ok([&[0x61, 0x7b, 0xa0, 0x37][..], &abi::address(asset)?, &abi::uint(amount), &abi::address(on_behalf_of)?, &abi::uint(0)].concat())
    }

    /// withdraw(address asset, uint256 amount, address to); None = everything (type(uint256).max).
    pub fn withdraw(asset: &str, amount: Option<u64>, to: &str) -> Result<Vec<u8>> {
        Ok([&[0x69, 0x32, 0x8d, 0xec][..], &abi::address(asset)?, &amount.map_or([0xff; 32], abi::uint), &abi::address(to)?].concat())
    }

    /// borrow(address asset, uint256 amount, uint256 interestRateMode, uint16 referralCode, address onBehalfOf): variable
    /// rate (2), sent to the caller, the debt on `on_behalf_of`.
    pub fn borrow(asset: &str, amount: u64, on_behalf_of: &str) -> Result<Vec<u8>> {
        Ok([&[0xa4, 0x15, 0xbc, 0xad][..], &abi::address(asset)?, &abi::uint(amount), &abi::uint(2), &abi::uint(0), &abi::address(on_behalf_of)?].concat())
    }

    /// repay(address asset, uint256 amount, uint256 interestRateMode, address onBehalfOf); None = the whole debt.
    pub fn repay(asset: &str, amount: Option<u64>, on_behalf_of: &str) -> Result<Vec<u8>> {
        Ok([&[0x57, 0x3a, 0xde, 0x81][..], &abi::address(asset)?, &amount.map_or([0xff; 32], abi::uint), &abi::uint(2), &abi::address(on_behalf_of)?].concat())
    }

    /// getUserAccountData(address).
    pub fn account_data(user: &str) -> Result<Vec<u8>> { Ok([&[0xbf, 0x92, 0x85, 0x7c][..], &abi::address(user)?].concat()) }
    /// getPriceOracle() on the addresses provider.
    pub const GET_PRICE_ORACLE: [u8; 4] = [0xfc, 0xa5, 0x13, 0xa8];
    /// getAssetPrice(asset) on the oracle, USD with 8 decimals.
    pub fn asset_price(asset: &str) -> Result<Vec<u8>> { Ok([&[0xb3, 0x59, 0x6f, 0x07][..], &abi::address(asset)?].concat()) }
    /// getPool()
    pub const GET_POOL: [u8; 4] = [0x02, 0x6b, 0x1d, 0x5f];
    /// getReserveData(address): word 2 is the supply rate in ray (1e27), word 8 the aToken, word 10 the variable debt token.
    pub fn get_reserve_data(asset: &str) -> Result<Vec<u8>> { Ok([&[0x35, 0xea, 0x6a, 0x75][..], &abi::address(asset)?].concat()) }

    /// Supply APY in %: the ray rate compounded every second over a year.
    pub fn apy(rate_ray: Decimal) -> Decimal {
        let r = (rate_ray / Decimal::from_i128_with_scale(10i128.pow(27), 0)).to_f64().unwrap_or(0.0);
        Decimal::from_f64(r.exp_m1() * 100.0).unwrap_or_default()
    }
}

/// Morpho vaults, ERC-4626 (`MorphoVault.swift`).
pub mod morpho {
    use super::*;

    pub const GAUNTLET_USDC_PRIME: &str = "0xee8f4ec5672f09119b96ab6fb59c27e1b7e44b61";

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Vault {
        pub chain: &'static str,
        pub chain_name: &'static str,
        pub chain_id: u64,
        pub usdc: &'static str,
        pub address: &'static str,
        pub name: &'static str,
    }

    impl Vault {
        /// How the app names it: «Morpho · Ethereum».
        pub fn place(&self) -> String { format!("Morpho · {}", self.chain_name) }
    }

    pub const VAULTS: [Vault; 3] = [
        Vault { chain: "base", chain_name: "Base", chain_id: 8453, usdc: crate::BASE_USDC, address: GAUNTLET_USDC_PRIME, name: "Gauntlet USDC Prime" },
        Vault { chain: "ethereum", chain_name: "Ethereum", chain_id: 1, usdc: crate::ETHEREUM_USDC, address: "0xdd0f28e19c1780eb6396170735d45153d261490d",
                name: "Gauntlet USDC Prime" },
        Vault { chain: "arbitrum", chain_name: "Arbitrum", chain_id: 42161, usdc: crate::ARBITRUM_USDC, address: "0x7e97fa6893871a2751b5fe961978dccb2c201e65",
                name: "Gauntlet USDC Core" },
    ];

    pub fn vault(place: &str) -> Option<&'static Vault> { VAULTS.iter().find(|v| v.place() == place) }

    /// deposit(uint256 assets, address receiver)
    pub fn deposit(assets: u64, receiver: &str) -> Result<Vec<u8>> { Ok([&[0x6e, 0x55, 0x3f, 0x65][..], &abi::uint(assets), &abi::address(receiver)?].concat()) }

    /// withdraw(uint256 assets, address receiver, address owner): an exact amount of USDC out.
    pub fn withdraw(assets: u64, receiver: &str, owner: &str) -> Result<Vec<u8>> {
        Ok([&[0xb4, 0x60, 0xaf, 0x94][..], &abi::uint(assets), &abi::address(receiver)?, &abi::address(owner)?].concat())
    }

    /// redeem(uint256 shares, address receiver, address owner): every share out; `shares` is balanceOf's word.
    pub fn redeem(shares: &[u8; 32], receiver: &str, owner: &str) -> Result<Vec<u8>> {
        Ok([&[0xba, 0x08, 0x76, 0x52][..], shares, &abi::address(receiver)?, &abi::address(owner)?].concat())
    }

    /// maxWithdraw(address owner): the owner's USDC in the vault, interest included (6 decimals).
    pub fn max_withdraw(owner: &str) -> Result<Vec<u8>> { Ok([&[0xce, 0x96, 0xcb, 0x77][..], &abi::address(owner)?].concat()) }

    /// asset()
    pub const ASSET: [u8; 4] = [0x38, 0xd5, 0x2e, 0x0f];
}

/// The spenders ihatecrypto approves on each network: Aave's pools and Morpho's vaults there.
pub fn aave_pools(chain_id: u64) -> Vec<&'static str> { aave::MARKETS.iter().filter(|m| m.chain_id == chain_id).map(|m| m.pool).collect() }

pub fn morpho_vaults(chain_id: u64) -> Vec<&'static str> { morpho::VAULTS.iter().filter(|v| v.chain_id == chain_id).map(|v| v.address).collect() }

/// A dollar transfer on BNB Chain at gas price 0, which MegaFuel (BEP-414) pays the gas for (`MegaFuel.swift`): only a
/// dollar it knows, only `transfer`, the fee zero. Asking MegaFuel and sending are the engine's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SponsoredTransfer {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_limit: u64,
    pub token: String,
    pub to: String,
    /// In the token's own units (18 decimals on BNB Chain), as a decimal string.
    pub amount: String,
}

impl SponsoredTransfer {
    /// The dollars MegaFuel sponsors a transfer of, by chain and token, with their symbol and decimals.
    pub const TOKENS: [(u64, &'static str, &'static str, usize); 1] = [(56, crate::BNB_USDC, "USDC", 18)];

    pub fn supports(chain_id: u64, token: &str) -> bool { Self::TOKENS.iter().any(|t| t.0 == chain_id && t.1 == token.to_lowercase()) }

    fn known(&self) -> Option<(&'static str, usize)> {
        Self::TOKENS.iter().find(|t| t.0 == self.chain_id && t.1 == self.token.to_lowercase()).map(|t| (t.2, t.3))
    }

    /// A whole, positive number of units; None otherwise.
    fn units(&self) -> Option<U256> { self.amount.parse::<U256>().ok().filter(|u| !u.is_zero()) }

    /// Why the signer won't sign it, or None.
    pub fn refusal(&self) -> Option<String> {
        if self.known().is_none() { return Some(format!("{} on network {} isn't a dollar BNB Chain pays the gas for.", self.token, self.chain_id)) }
        if self.units().is_none() { return Some("The transfer's amount isn't a whole number of units.".into()) }
        if abi::address(&self.to).is_err() || self.to.to_lowercase() == ZERO { return Some("The transfer has no recipient.".into()) }
        if self.gas_limit == 0 || self.gas_limit > 100_000 { return Some("A token transfer takes under 100,000 gas.".into()) }
        None
    }

    pub fn calldata(&self) -> Result<Vec<u8>> {
        let Some(units) = self.amount.parse::<U256>().ok() else { return fail(format!("Invalid amount {}.", self.amount)) };
        crate::erc20::transfer(&self.to, units)
    }

    /// rlp([nonce, gasPrice 0, gasLimit, token, value 0, data]): the legacy fields.
    fn fields(&self) -> Result<Vec<Rlp>> {
        let Some(token) = from_hex(&self.token).filter(|t| t.len() == 20) else { return fail(format!("Invalid token address: {}", self.token)) };
        Ok(vec![Rlp::uint(self.nonce), Rlp::uint(0), Rlp::uint(self.gas_limit), Rlp::Bytes(token), Rlp::uint(0), Rlp::Bytes(self.calldata()?)])
    }

    /// EIP-155's signing payload: the fields, then chainId, 0, 0.
    pub fn signing_hash(&self) -> Result<[u8; 32]> {
        let mut f = self.fields()?;
        f.extend([Rlp::uint(self.chain_id), Rlp::uint(0), Rlp::uint(0)]);
        Ok(keccak(&Rlp::List(f).encoded()))
    }

    /// The raw legacy transaction, v = recovery id + chainId·2 + 35.
    pub fn signed(&self, private_key: &[u8]) -> Result<(Vec<u8>, String)> {
        let s = sign(&self.signing_hash()?, private_key)?;
        let mut f = self.fields()?;
        f.extend([Rlp::uint((s.v - 27) as u64 + self.chain_id * 2 + 35), Rlp::uint_be(&s.r), Rlp::uint_be(&s.s)]);
        let raw = Rlp::List(f).encoded();
        let hash = format!("0x{}", to_hex(&keccak(&raw)));
        Ok((raw, hash))
    }

    /// What the prompt says.
    pub fn summary(&self) -> String {
        let (symbol, decimals) = self.known().unwrap_or(("tokens", 0));
        format!("send {} {symbol} on {} to {} (BNB Chain pays the gas)",
                crate::units(&self.amount.parse::<U256>().unwrap_or_default(), decimals), network_name(self.chain_id), self.to)
    }
}

/// A dollar Relay takes by the owner's EIP-3009 signature (`Permit3009.swift`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permit3009 {
    pub chain_id: u64,
    pub token: String,
    pub from: String,
    pub to: String,
    /// In the token's units (6 decimals for every token below).
    pub value: u64,
    /// Unix seconds; validAfter is always 0.
    pub valid_before: u64,
    pub nonce: Vec<u8>,
}

/// Relay's approval proxies, at one address on every network: v3, and v3.2 from 2026-10-26.
pub const RELAY_PROXIES: [&str; 2] = ["0xccc88a9d1b4ed6b0eaba998850414b24f1c315be", "0x5567954cd026cebfe486062cb75848e0f1c12e58"];

impl Permit3009 {
    /// The dollars it's signed for: chain, token, their EIP-712 domain name and version (each checked against the
    /// contract's DOMAIN_SEPARATOR, 2026-10-23), and symbol. Not on BNB Chain or HyperEVM.
    pub const TOKENS: [(u64, &'static str, &'static str, &'static str, &'static str); 6] = [
        (8453, crate::BASE_USDC, "USD Coin", "2", "USDC"), (42161, crate::ARBITRUM_USDC, "USD Coin", "2", "USDC"),
        (10, crate::OPTIMISM_USDC, "USD Coin", "2", "USDC"), (137, crate::POLYGON_USDC, "USD Coin", "2", "USDC"),
        (1, crate::ETHEREUM_USDC, "USD Coin", "2", "USDC"), (4663, crate::ROBINHOOD_USDG, "Global Dollar", "1", "USDG"),
    ];

    /// Its domain name and version, and symbol.
    pub fn token_info(chain_id: u64, token: &str) -> Option<(&'static str, &'static str, &'static str)> {
        let token = token.to_lowercase();
        Self::TOKENS.iter().find(|t| t.0 == chain_id && t.1 == token).map(|t| (t.2, t.3, t.4))
    }

    pub fn supports(chain_id: u64, token: &str) -> bool { Self::token_info(chain_id, token).is_some() }

    fn known(&self) -> Option<(&'static str, &'static str, &'static str)> { Self::token_info(self.chain_id, &self.token) }

    /// Why the signer won't sign it, or None. `now`: Unix seconds.
    pub fn refusal(&self, owner: &str, now: u64) -> Option<String> {
        if self.known().is_none() { return Some(format!("{} on network {} isn't a dollar ihatecrypto pays by signature.", self.token, self.chain_id)) }
        if self.from.to_lowercase() != owner.to_lowercase() { return Some("The payment isn't from this wallet.".into()) }
        if !RELAY_PROXIES.contains(&self.to.to_lowercase().as_str()) { return Some(format!("Only Relay's approval proxy may take dollars by signature, not {}.", self.to)) }
        if self.value == 0 || self.nonce.len() != 32 { return Some("The payment by signature is incomplete.".into()) }
        if self.valid_before <= now || self.valid_before > now + 3600 { return Some("The signature would stay valid for more than an hour, or has run out.".into()) }
        None
    }

    pub fn typed(&self) -> Result<Typed> {
        let Some((name, version, _)) = self.known() else { return fail(format!("Unknown dollar token {} on network {}.", self.token, self.chain_id)) };
        Ok(Typed {
            domain: vec![Field::new("name", "string", Value::String(name.into())), Field::new("version", "string", Value::String(version.into())),
                         Field::new("chainId", "uint256", Value::Uint(self.chain_id)), Field::new("verifyingContract", "address", Value::Address(self.token.clone()))],
            ty: "ReceiveWithAuthorization".into(),
            message: vec![Field::new("from", "address", Value::Address(self.from.clone())), Field::new("to", "address", Value::Address(self.to.clone())),
                          Field::new("value", "uint256", Value::Uint(self.value)), Field::new("validAfter", "uint256", Value::Uint(0)),
                          Field::new("validBefore", "uint256", Value::Uint(self.valid_before)), Field::new("nonce", "bytes32", Value::Bytes32(self.nonce.clone()))],
        })
    }

    /// What the prompt says.
    pub fn summary(&self) -> String {
        let amount = crate::units(&U256::from(self.value), 6);
        format!("pay {amount} {} on {} to Relay by signature (Relay pays the network fee)",
                self.known().map_or("tokens", |k| k.2), network_name(self.chain_id))
    }
}

/// Relay's ERC-3009 payment on its v3.2 approval proxy, tied to the exact calls the proxy runs (`RelayMulticallPermit`).
pub mod relay {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Call {
        pub target: String,
        pub allow_failure: bool,
        pub value: u64,
        pub call_data: Vec<u8>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MulticallPermit {
        pub chain_id: u64,
        pub token: String,
        pub from: String,
        pub value: u64,
        pub valid_before: u64,
        /// Who sends the transaction (msg.sender, Relay's relayer) and gets what's left over.
        pub relayer: String,
        pub refund_to: String,
        pub nft_recipient: String,
        pub metadata: Vec<u8>,
        pub calls: Vec<Call>,
    }

    pub const PROXY: &str = "0x5567954cd026cebfe486062cb75848e0f1c12e58";
    /// Relay's ERC-20 routers (v3, v3.2).
    pub const ROUTERS: [&str; 2] = ["0xb92fe925dc43a0ecde6c8b1a2709c170ec4fff4f", "0x0729e2a4716d9f7dd17858d5d504426d228697f7"];
    pub const DEPOSITORY: &str = "0x4cd00e387622c35bddb9b4c962c136462338bc31";
    /// cleanupErc20sViaCall(address[],address[],bytes[],uint256[]) on the router; depositErc20(address,address,bytes32).
    const CLEANUP_VIA_CALL: [u8; 4] = [0x73, 0xb7, 0xbb, 0x2f];
    const DEPOSIT_ALL: [u8; 4] = [0x5a, 0x1e, 0xe3, 0xac];

    impl MulticallPermit {
        /// The ERC-3009 half, whose nonce is this authorization's digest.
        pub fn permit(&self) -> Result<Permit3009> {
            Ok(Permit3009 { chain_id: self.chain_id, token: self.token.clone(), from: self.from.clone(), to: PROXY.into(), value: self.value,
                            valid_before: self.valid_before, nonce: self.digest()?.to_vec() })
        }

        /// The Relay order id its deposit names: depositAll's last word, inside the router's one call.
        pub fn deposit_id(&self) -> Option<Vec<u8>> {
            let call = self.calls.first()?;
            let datas = bytes_array(call.call_data.get(4..)?, 64)?;
            let deposit = datas.first().filter(|d| d.len() == 4 + 96)?;
            Some(deposit[deposit.len() - 32..].to_vec())
        }

        /// Why the signer won't sign it, or None.
        pub fn refusal(&self, owner: &str, now: u64) -> Option<String> {
            if let Some(r) = self.permit().ok().and_then(|p| p.refusal(owner, now)) { return Some(r) }
            if ![owner.to_lowercase(), self.relayer.to_lowercase()].contains(&self.refund_to.to_lowercase()) {
                return Some(format!("Relay's leftovers would go to {}, neither this wallet nor its relayer.", self.refund_to));
            }
            let one = match self.calls.as_slice() {
                [c] if ROUTERS.contains(&c.target.to_lowercase().as_str()) && !c.allow_failure && c.value == 0 && c.call_data.starts_with(&CLEANUP_VIA_CALL) => c,
                _ => return Some("Relay's calls with this dollar aren't one deposit through its router.".into()),
            };
            let args = &one.call_data[4..];
            let read = (addresses(args, 0), addresses(args, 32), bytes_array(args, 64), numbers(args, 96));
            let ok = matches!(&read, (Some(t), Some(to), Some(d), Some(a))
                if t.len() == 1 && to.len() == 1 && d.len() == 1 && a.len() == 1 && t[0] == self.token.to_lowercase() && to[0] == DEPOSITORY
                   && (a[0] == 0 || a[0] == self.value));
            if !ok { return Some("Relay's router call doesn't deposit this dollar into Relay's depository.".into()) }
            let datas = read.2.expect("checked");
            let deposit = &datas[0][4.min(datas[0].len())..];
            let mine = datas[0].starts_with(&DEPOSIT_ALL) && deposit.len() == 96
                && address_in(deposit, 0).is_some_and(|a| a == owner.to_lowercase()) && address_in(deposit, 1).is_some_and(|a| a == self.token.to_lowercase());
            if !mine { return Some("Relay's deposit isn't for this wallet.".into()) }
            None
        }

        /// EIP-712 digest of MulticallAuthorization under the v3.2 proxy's domain.
        pub fn digest(&self) -> Result<[u8; 32]> {
            let k = |s: &str| keccak(s.as_bytes());
            let call3 = "Call3Value(address target,bool allowFailure,uint256 value,bytes callData)";
            let funding = "FundingAuthorization(address token,address from,uint256 value,uint256 validAfter,uint256 validBefore)";
            let mut calls = Vec::new();
            for c in &self.calls {
                calls.extend(keccak(&[&k(call3)[..], &abi::address(&c.target)?, &abi::uint(c.allow_failure as u64), &abi::uint(c.value), &keccak(&c.call_data)].concat()));
            }
            let calls_hash = keccak(&calls);
            let funding_hash = keccak(&keccak(&[&k(funding)[..], &abi::address(&self.token)?, &abi::address(&self.from)?, &abi::uint(self.value),
                                                &abi::uint(0), &abi::uint(self.valid_before)].concat()));
            let ty = format!("MulticallAuthorization(address from,address relayer,uint256 msgValue,address refundTo,address nftRecipient,bytes metadata,uint256 fundingIndex,Call3Value[] calls,FundingAuthorization[] funding){call3}{funding}");
            let message = keccak(&[&k(&ty)[..], &abi::address(&self.from)?, &abi::address(&self.relayer)?, &abi::uint(0), &abi::address(&self.refund_to)?,
                                   &abi::address(&self.nft_recipient)?, &keccak(&self.metadata), &abi::uint(0), &calls_hash, &funding_hash].concat());
            let domain = eip712::hash_struct("EIP712Domain", &[
                Field::new("name", "string", Value::String("RelayApprovalProxy".into())), Field::new("version", "string", Value::String("3.1".into())),
                Field::new("chainId", "uint256", Value::Uint(self.chain_id)), Field::new("verifyingContract", "address", Value::Address(PROXY.into())),
            ])?;
            Ok(keccak(&[&[0x19, 0x01][..], &domain, &message].concat()))
        }
    }

    fn words(data: &[u8], at: usize) -> Option<Vec<&[u8]>> {
        let offset = uint_in(data, at / 32).filter(|&o| o % 32 == 0 && o < data.len() as u64)? as usize;
        let count = uint_in(data, offset / 32).filter(|&c| c <= 64)? as usize;
        if offset + 32 + count * 32 > data.len() { return None }
        Some((0..count).map(|i| &data[offset + 32 + i * 32..offset + 64 + i * 32]).collect())
    }

    /// An `address[]` argument whose offset sits at byte `at`; None when it runs out of the data.
    pub fn addresses(data: &[u8], at: usize) -> Option<Vec<String>> {
        Some(words(data, at)?.into_iter().map(|w| format!("0x{}", to_hex(&w[12..]))).collect())
    }

    /// A `uint256[]` argument whose words all fit 64 bits.
    pub fn numbers(data: &[u8], at: usize) -> Option<Vec<u64>> {
        words(data, at)?.into_iter().map(|w| w[..24].iter().all(|&b| b == 0).then(|| u64::from_be_bytes(w[24..].try_into().expect("8")))).collect()
    }
}

/// EIP-7702 batches through eth-infinitism's Simple7702Account v0.8 (`Batch7702.swift`).
pub mod batch7702 {
    use super::*;

    pub const DELEGATE: &str = "0x4cd241e8d1510e30b2076397afc7508ae59c66c9";
    /// Networks past Prague with that bytecode at `DELEGATE`.
    pub const CHAINS: [u64; 6] = [1, 137, 42161, 8453, 4663, 10];
    /// ERC-4337's EntryPoints v0.6, v0.7 and v0.8.
    pub const ENTRY_POINTS: [&str; 3] = ["0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789", "0x0000000071727de22e5e9d8baf0edac6f37da032",
                                         "0x4337084d9e255ff0702461cf8895ce9e3b5ff108"];
    /// Gas a transaction's authorization adds on top of what its calls use (after Glamsterdam 43,006, with a margin).
    pub const AUTHORIZATION_GAS: u64 = 45_000;
    /// More calls than any of ihatecrypto's moves makes.
    pub const MAX_CALLS: usize = 4;
    const EXECUTE_BATCH: [u8; 4] = [0x34, 0xfc, 0xd5, 0xbe];

    /// The contract an address's code delegates it to by EIP-7702 when it isn't ours; None for no code, a contract,
    /// or our own `DELEGATE`.
    pub fn foreign_delegate(code: &str) -> Option<String> {
        let c = code.to_lowercase();
        if c.len() != 2 + 46 || !c.starts_with("0xef0100") { return None }
        let to = format!("0x{}", &c[8..]);
        (to != DELEGATE).then_some(to)
    }

    /// One call of the batch; no coins go with it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct BatchCall {
        pub to: String,
        pub data: Vec<u8>,
    }

    /// executeBatch((address target, uint256 value, bytes data)[]), every value zero.
    pub fn execute_batch(calls: &[BatchCall]) -> Result<Vec<u8>> {
        let tuples: Vec<Vec<u8>> = calls.iter().map(|c| {
            Ok([&abi::address(&c.to)?[..], &abi::uint(0), &abi::uint(96), &abi::uint(c.data.len() as u64), &padded(&c.data)].concat())
        }).collect::<Result<_>>()?;
        let (mut offsets, mut at) = (Vec::new(), 32 * calls.len() as u64);
        for t in &tuples {
            offsets.extend(abi::uint(at));
            at += t.len() as u64;
        }
        Ok([&EXECUTE_BATCH[..], &abi::uint(32), &abi::uint(calls.len() as u64), &offsets, &tuples.concat()].concat())
    }

    /// The calls of an executeBatch input, read back; None when it isn't one (or isn't well formed).
    pub fn calls(data: &[u8]) -> Option<Vec<BatchCall>> {
        if !data.starts_with(&EXECUTE_BATCH) { return None }
        let body = &data[4..];
        let word = |at: usize| -> Option<usize> {
            if at + 32 > body.len() || body[at..at + 24].iter().any(|&b| b != 0) { return None }
            usize::try_from(u64::from_be_bytes(body[at + 24..at + 32].try_into().ok()?)).ok()
        };
        let array = word(0)?;
        let count = word(array).filter(|&c| c <= 64)?;
        let mut out = Vec::new();
        for i in 0..count {
            let tuple = array.checked_add(32)?.checked_add(word(array + 32 + 32 * i)?)?;
            if tuple.checked_add(96)? > body.len() { return None }
            let at = word(tuple + 64)?;
            let length = word(tuple.checked_add(at)?)?;
            let start = tuple + at + 32;
            if start.checked_add(length)? > body.len() { return None }
            out.push(BatchCall { to: format!("0x{}", to_hex(&body[tuple + 12..tuple + 32])), data: body[start..start + length].to_vec() });
        }
        Some(out)
    }

    /// keccak256(0x05 ‖ rlp([chainId, delegate, nonce])): what the key signs to let its address run `DELEGATE`'s code.
    pub fn authorization_digest(chain_id: u64, nonce: u64) -> [u8; 32] {
        keccak(&[vec![0x05], Rlp::List(vec![Rlp::uint(chain_id), Rlp::Bytes(from_hex(DELEGATE).expect("hex")), Rlp::uint(nonce)]).encoded()].concat())
    }

    /// Why a batch isn't signed, or the empty list. Payees are checked by the caller, which knows the approved recipients.
    pub fn problems(calls: &[BatchCall], owner: &str, chain_id: u64) -> Vec<String> {
        let mut out = Vec::new();
        if !CHAINS.contains(&chain_id) { out.push(format!("ihatecrypto doesn't batch calls on network {chain_id}.")) }
        if calls.is_empty() || calls.len() > MAX_CALLS { out.push(format!("A batch holds 1 to {MAX_CALLS} calls, not {}.", calls.len())) }
        for c in calls {
            if from_hex(&c.to).map(|b| b.len()) != Some(20) { out.push(format!("Not an address: {}.", c.to)) }
            out.extend(self_call(&c.to, &c.data, owner));
            out.extend(Call::decode(&c.data).refusal(owner, Some(chain_id)));
            out.extend(Call::target_refusal(&c.to, &c.data, owner));
        }
        out
    }

    /// A call into the wallet's own address runs whatever code it is delegated to.
    pub fn self_call(to: &str, data: &[u8], owner: &str) -> Option<String> {
        (to.to_lowercase() == owner.to_lowercase() && !data.is_empty()).then(|| "A call into this wallet's own address: it would run calls nobody checked. Refused.".into())
    }

    /// The signed type-4 transaction: from and to the wallet, executeBatch(calls), with the authorization to run
    /// `DELEGATE` at nonce + 1.
    #[allow(clippy::too_many_arguments)]
    pub fn signed(chain_id: u64, nonce: u64, max_priority_fee: u64, max_fee: u64, gas_limit: u64, owner: &str, calls: &[BatchCall],
                  private_key: &[u8]) -> Result<(Vec<u8>, String)> {
        let auth = sign(&authorization_digest(chain_id, nonce + 1), private_key)?;
        let tx = Transaction {
            chain_id, nonce, max_priority_fee_per_gas: max_priority_fee, max_fee_per_gas: max_fee, gas_limit, to: owner.into(), value: 0,
            data: execute_batch(calls)?, authorizations: vec![Authorization { chain_id, address: DELEGATE.into(), nonce: nonce + 1, signature: auth }],
        };
        tx.signed(private_key)
    }
}
