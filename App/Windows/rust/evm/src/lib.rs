//! EVM read and built offline (`Chains/EVM`): RLP and EIP-1559/7702 transactions, ABI words, the networks the signer
//! signs for, calldata read back (`call`), the protocols whose calls the signer reads (`protocols`) and eth_simulateV1
//! answers (`simulation`). The RPC client belongs to the engine, so the signer links no network code.

pub mod call;
mod hooks;
pub mod protocols;
pub mod relay_order;
pub mod simulation;

pub use ruint::aliases::U256;
use wallet::evm::{EvmSignature, keccak, sign};
use wallet::{Result, WalletError, from_hex, to_hex};

pub(crate) fn fail<T>(m: impl Into<String>) -> Result<T> { Err(WalletError(m.into())) }

/// Recursive Length Prefix, Ethereum's serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rlp {
    Bytes(Vec<u8>),
    List(Vec<Rlp>),
}

impl Rlp {
    /// Integers are big-endian with no leading zeros; zero is the empty string.
    pub fn uint(n: u64) -> Self { Self::uint_be(&n.to_be_bytes()) }

    pub fn uint_be(bytes: &[u8]) -> Self { Self::Bytes(bytes.iter().skip_while(|&&b| b == 0).copied().collect()) }

    pub fn encoded(&self) -> Vec<u8> {
        match self {
            Self::Bytes(d) if d.len() == 1 && d[0] < 0x80 => d.clone(),
            Self::Bytes(d) => [header(d.len(), 0x80, 0xb7), d.clone()].concat(),
            Self::List(items) => {
                let body: Vec<u8> = items.iter().flat_map(Rlp::encoded).collect();
                [header(body.len(), 0xc0, 0xf7), body].concat()
            }
        }
    }
}

fn header(length: usize, short: u8, long: u8) -> Vec<u8> {
    if length < 56 { return vec![short + length as u8] }
    let be: Vec<u8> = (length as u64).to_be_bytes().into_iter().skip_while(|&b| b == 0).collect();
    [vec![long + be.len() as u8], be].concat()
}

fn address20(a: &str) -> Option<Vec<u8>> { from_hex(a).filter(|b| b.len() == 20) }

/// EIP-7702: this address may run `address`'s code. The key signed keccak256(0x05 ‖ rlp([chainId, address, nonce])).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    pub chain_id: u64,
    pub address: String,
    pub nonce: u64,
    pub signature: EvmSignature,
}

/// EIP-1559 (type 2) transaction; with `authorizations`, EIP-7702's type 4.
/// ponytail: value in wei as u64 caps a single native transfer at ~18 ETH, as in Swift; widen when that matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: u64,
    pub max_fee_per_gas: u64,
    pub gas_limit: u64,
    pub to: String,
    pub value: u64,
    pub data: Vec<u8>,
    pub authorizations: Vec<Authorization>,
}

/// The most gas × max fee per gas may come to, per network, in its smallest unit: far above a real fee, so only a
/// broken or hostile request reaches it (the Swift table's reasons: Ethereum 0.05 ETH, the L2s 0.01 ETH, Polygon
/// 10 POL, Tempo $10, HyperEVM 0.02 HYPE, BNB Chain 0.01 BNB).
pub fn fee_cap(chain_id: u64) -> Option<u64> {
    Some(match chain_id {
        1 => 50_000_000_000_000_000,
        42161 | 8453 | 4663 | 10 | 56 => 10_000_000_000_000_000,
        137 | 4217 => 10_000_000_000_000_000_000,
        999 => 20_000_000_000_000_000,
        _ => return None,
    })
}

impl Transaction {
    /// 2, or 4 when it carries authorizations.
    pub fn kind(&self) -> u8 { if self.authorizations.is_empty() { 2 } else { 4 } }

    fn fields(&self) -> Result<Vec<Rlp>> {
        let Some(to) = address20(&self.to) else { return fail(format!("Invalid recipient address: {}", self.to)) };
        let mut base = vec![Rlp::uint(self.chain_id), Rlp::uint(self.nonce), Rlp::uint(self.max_priority_fee_per_gas), Rlp::uint(self.max_fee_per_gas),
                            Rlp::uint(self.gas_limit), Rlp::Bytes(to), Rlp::uint(self.value), Rlp::Bytes(self.data.clone()), Rlp::List(vec![])];
        if self.authorizations.is_empty() { return Ok(base) }
        let list = self.authorizations.iter().map(|a| {
            let Some(address) = address20(&a.address) else { return fail(format!("Invalid authorization address: {}", a.address)) };
            Ok(Rlp::List(vec![Rlp::uint(a.chain_id), Rlp::Bytes(address), Rlp::uint(a.nonce), Rlp::uint((a.signature.v - 27) as u64),
                              Rlp::uint_be(&a.signature.r), Rlp::uint_be(&a.signature.s)]))
        }).collect::<Result<_>>()?;
        base.push(Rlp::List(list));
        Ok(base)
    }

    /// type ‖ rlp(fields): the unsigned transaction, as a hardware wallet reads it.
    pub fn unsigned_payload(&self) -> Result<Vec<u8>> { Ok([vec![self.kind()], Rlp::List(self.fields()?).encoded()].concat()) }

    /// keccak256(type ‖ rlp(fields)) — what the key signs.
    pub fn signing_hash(&self) -> Result<[u8; 32]> { Ok(keccak(&self.unsigned_payload()?)) }

    /// Raw signed transaction and its hash (the transaction id).
    pub fn signed(&self, private_key: &[u8]) -> Result<(Vec<u8>, String)> { self.signed_with(&sign(&self.signing_hash()?, private_key)?) }

    /// The transaction with a signature made elsewhere (a Ledger), v = 27 or 28.
    pub fn signed_with(&self, signature: &EvmSignature) -> Result<(Vec<u8>, String)> {
        let mut fields = self.fields()?;
        fields.extend([Rlp::uint((signature.v - 27) as u64), Rlp::uint_be(&signature.r), Rlp::uint_be(&signature.s)]);
        let raw = [vec![self.kind()], Rlp::List(fields).encoded()].concat();
        let hash = format!("0x{}", to_hex(&keccak(&raw)));
        Ok((raw, hash))
    }

    /// Why the signer won't sign this fee, or None: past the network's cap, or past 64 bits.
    pub fn fee_problem(&self) -> Option<String> {
        match (self.gas_limit.checked_mul(self.max_fee_per_gas), fee_cap(self.chain_id)) {
            (Some(fee), Some(cap)) if fee <= cap => None,
            _ => Some(format!("A network fee of {} is more than ihatecrypto signs on {}.", self.fee_text(), network_name(self.chain_id))),
        }
    }

    /// «up to 0.000042 ETH»: the fee ceiling, for the signing prompt.
    pub fn fee_text(&self) -> String {
        match self.gas_limit.checked_mul(self.max_fee_per_gas) {
            Some(fee) => format!("up to {} {}", units(&U256::from(fee), 18), coin(self.chain_id)),
            None => format!("over 18 {}", coin(self.chain_id)),
        }
    }
}

/// A whole number of smallest units as a plain decimal: no exponent, no trailing zeros («0.000042», «2»).
pub fn units(n: &U256, decimals: usize) -> String {
    let digits = n.to_string();
    if decimals == 0 { return digits }
    let padded = format!("{digits:0>width$}", width = decimals + 1);
    let (whole, fraction) = padded.split_at(padded.len() - decimals);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() { whole.to_string() } else { format!("{whole}.{fraction}") }
}

/// A 32-byte word (or any big-endian bytes up to 32) as a number.
pub fn word_value(word: &[u8]) -> U256 { U256::try_from_be_slice(&word[word.len().saturating_sub(32)..]).unwrap_or(U256::MAX) }

/// 32-byte ABI words.
pub mod abi {
    use super::*;

    pub fn address(a: &str) -> Result<[u8; 32]> {
        let Some(bytes) = address20(a) else { return fail(format!("Invalid address: {a}")) };
        let mut w = [0u8; 32];
        w[12..].copy_from_slice(&bytes);
        Ok(w)
    }

    pub fn uint(n: u64) -> [u8; 32] {
        let mut w = [0u8; 32];
        w[24..].copy_from_slice(&n.to_be_bytes());
        w
    }

    pub fn word(n: U256) -> [u8; 32] { n.to_be_bytes() }

    /// A whole, non-negative decimal as a 256-bit word.
    pub fn word_decimal(n: rust_decimal::Decimal) -> Result<[u8; 32]> {
        if n.is_sign_negative() || n != n.trunc() { return fail(format!("Not a whole token amount: {n}")) }
        match n.trunc().to_string().parse::<U256>() {
            Ok(v) => Ok(v.to_be_bytes()),
            Err(_) => fail(format!("Amount too large for a word: {n}")),
        }
    }

    /// ERC-20 `string` return value (symbol(), name()).
    pub fn string(data: &[u8]) -> Option<String> {
        let length = uint_in(data, 1)? as usize;
        if data.len() < 64 || length > data.len() || data.len() < 64 + length { return None }
        String::from_utf8(data[64..64 + length].to_vec()).ok()
    }

    /// Word `i` of a return value as an address.
    pub fn address_in(data: &[u8], i: usize) -> Option<String> {
        (i < data.len() / 32).then(|| format!("0x{}", to_hex(&data[i * 32 + 12..(i + 1) * 32])))
    }

    /// Word `i` as a number, None when it doesn't fit 64 bits. `i` is often read from someone's data (an offset / 32):
    /// it's held to the words there before any arithmetic.
    pub fn uint_in(data: &[u8], i: usize) -> Option<u64> {
        if i >= data.len() / 32 || data[i * 32..i * 32 + 24].iter().any(|&b| b != 0) { return None }
        Some(u64::from_be_bytes(data[i * 32 + 24..(i + 1) * 32].try_into().ok()?))
    }

    /// Word `i` as a 256-bit number (Swift reads it as a Decimal).
    pub fn u256_in(data: &[u8], i: usize) -> Option<U256> {
        (i < data.len() / 32).then(|| U256::from_be_slice(&data[i * 32..(i + 1) * 32]))
    }

    /// decimals()'s answer as a token can have it; None when it didn't answer or answered past 36. Never guessed.
    pub fn decimals(answer: Option<&[u8]>) -> Option<u32> { answer.and_then(|a| uint_in(a, 0)).filter(|&d| d <= 36).map(|d| d as u32) }

    /// A `bytes[]` argument whose offset sits at byte `at` of the arguments; None when any part runs out of the data.
    pub fn bytes_array(data: &[u8], at: usize) -> Option<Vec<Vec<u8>>> {
        // Offsets inside a bytes[] are 32-byte aligned; a misaligned or oversized one is someone's crafted data.
        let number = |offset: usize| -> Option<usize> {
            if offset % 32 != 0 { return None }
            uint_in(data, offset / 32).filter(|&n| n < data.len() as u64).map(|n| n as usize)
        };
        let base = number(at)?;
        let count = number(base).filter(|&c| c <= 64)?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let relative = number(base + 32 + i * 32)?;
            let length = number(base + 32 + relative)?;
            let start = base + 64 + relative;
            if start + length > data.len() { return None }
            out.push(data[start..start + length].to_vec());
        }
        Some(out)
    }

    /// `n` bytes padded with zeros to a whole number of words.
    pub fn padded(d: &[u8]) -> Vec<u8> { [d, &vec![0; (32 - d.len() % 32) % 32]].concat() }
}

pub mod erc20 {
    use super::*;

    /// transfer(address,uint256)
    pub fn transfer(to: &str, amount: U256) -> Result<Vec<u8>> {
        Ok([&[0xa9, 0x05, 0x9c, 0xbb][..], &abi::address(to)?, &abi::word(amount)].concat())
    }

    /// balanceOf(address)
    pub fn balance_of(owner: &str) -> Result<Vec<u8>> { Ok([&[0x70, 0xa0, 0x82, 0x31][..], &abi::address(owner)?].concat()) }

    /// approve(address,uint256) — always an exact amount; the guard refuses unlimited ones anyway.
    pub fn approve(spender: &str, amount: U256) -> Result<Vec<u8>> {
        Ok([&[0x09, 0x5e, 0xa7, 0xb3][..], &abi::address(spender)?, &abi::word(amount)].concat())
    }

    /// allowance(address owner, address spender)
    pub fn allowance(owner: &str, spender: &str) -> Result<Vec<u8>> {
        Ok([&[0xdd, 0x62, 0xed, 0x3e][..], &abi::address(owner)?, &abi::address(spender)?].concat())
    }

    /// transferWithMemo(address,uint256,bytes32): Tempo's TIP-20 transfer carrying a reference the payee asked for.
    pub fn transfer_with_memo(to: &str, amount: u64, memo: &[u8]) -> Result<Vec<u8>> {
        if memo.len() != 32 { return fail("A memo is 32 bytes.") }
        Ok([&[0x95, 0x77, 0x7d, 0x59][..], &abi::address(to)?, &abi::uint(amount), memo].concat())
    }
}

/// Tempo's fee rules and its stablecoin exchange (see the Swift enum `Tempo`).
pub mod tempo {
    use super::*;

    /// The FeeManager precompile.
    pub const FEE_MANAGER: &str = "0xfeec000000000000000000000000000000000000";
    /// Tempo's own stablecoin exchange (a precompile): a swap pays the caller.
    pub const EXCHANGE: &str = "0xdec0000000000000000000000000000000000000";
    /// The dollars it trades that ihatecrypto names, 6 decimals each.
    pub const STABLES: [(&str, &str); 4] = [("USDC", "0x20c000000000000000000000b9537d11c60e8b50"), ("PATHUSD", "0x20c0000000000000000000000000000000000000"),
                                            ("OUSD", "0x20c0000000000000000000006a37da5c996874be"), ("USDT0", "0x20c00000000000000000000014f22ca97301eb73")];

    /// setUserToken(address)
    pub fn set_user_token(token: &str) -> Result<Vec<u8>> { Ok([&[0xe7, 0x89, 0x74, 0x44][..], &abi::address(token)?].concat()) }
    /// userTokens(address): the fee token `owner` chose; the zero address when none.
    pub fn user_token(owner: &str) -> Result<Vec<u8>> { Ok([&[0xed, 0x49, 0x8f, 0xa8][..], &abi::address(owner)?].concat()) }
    /// A fee in a 6-decimal stablecoin's units from gas × price in attodollars: ceil(x / 10¹²).
    pub fn fee_units(attodollars: u64) -> u64 { attodollars.div_ceil(1_000_000_000_000) }
    /// quoteSwapExactAmountIn(tokenIn, tokenOut, amountIn) → amountOut.
    pub fn quote(token_in: &str, token_out: &str, units: u64) -> Result<Vec<u8>> {
        Ok([&[0xe7, 0xc9, 0x8f, 0x1a][..], &abi::address(token_in)?, &abi::address(token_out)?, &abi::uint(units)].concat())
    }
    /// swapExactAmountIn(tokenIn, tokenOut, amountIn, minAmountOut): what comes out goes to the caller.
    pub fn swap(token_in: &str, token_out: &str, units: u64, min_out: u64) -> Result<Vec<u8>> {
        Ok([&[0xf8, 0x85, 0x6c, 0x0f][..], &abi::address(token_in)?, &abi::address(token_out)?, &abi::uint(units), &abi::uint(min_out)].concat())
    }
}

pub const ARBITRUM_USDC: &str = "0xaf88d065e77c8cc2239327c5edb3a432268e5831";
pub const ROBINHOOD_USDG: &str = "0x5fc5360d0400a0fd4f2af552add042d716f1d168";
pub const BASE_USDC: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";
pub const POLYGON_USDC: &str = "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359";
pub const ETHEREUM_USDC: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
pub const TEMPO_USDC: &str = "0x20c000000000000000000000b9537d11c60e8b50";
pub const HYPEREVM_USDC: &str = "0xb88339cb7199b77e23db6e890353e22632ba630f";
pub const OPTIMISM_USDC: &str = "0x0b2c639c533813f4aa9d7837caf62653d097ff85";
/// Binance-Peg USDC on BNB Chain — 18 decimals, not 6.
pub const BNB_USDC: &str = "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d";

/// The only networks the signer signs for.
pub const SIGNABLE: [u64; 9] = [1, 137, 42161, 8453, 4663, 4217, 999, 10, 56];

/// The networks by name, for what the signer says it signs.
pub fn name(chain_id: u64) -> Option<&'static str> {
    Some(match chain_id {
        1 => "Ethereum", 137 => "Polygon", 42161 => "Arbitrum", 8453 => "Base", 4663 => "Robinhood Chain", 4217 => "Tempo",
        999 => "HyperEVM", 10 => "Optimism", 56 => "BNB Chain", _ => return None,
    })
}

/// «Base», or «network 12345».
pub fn network_name(chain_id: u64) -> String { name(chain_id).map_or_else(|| format!("network {chain_id}"), str::to_string) }

/// What a network's fees and `value` are counted in: Polygon's POL, HyperEVM's HYPE, BNB Chain's BNB, Tempo's dollars
/// (it has no coin), else ETH.
pub fn coin(chain_id: u64) -> &'static str {
    match chain_id { 137 => "POL", 999 => "HYPE", 56 => "BNB", 4217 => "USD", _ => "ETH" }
}

/// The dollars ihatecrypto moves, by contract (lowercase): their symbol for the signing prompt.
pub fn dollar(token: &str) -> Option<&'static str> {
    match token.to_lowercase().as_str() {
        BASE_USDC | ARBITRUM_USDC | POLYGON_USDC | ETHEREUM_USDC | TEMPO_USDC | HYPEREVM_USDC | OPTIMISM_USDC | BNB_USDC => Some("USDC"),
        ROBINHOOD_USDG => Some("USDG"),
        _ => None,
    }
}

/// Their decimals: 6, but BNB Chain's Binance-Peg USDC has 18.
pub fn dollar_decimals(token: &str) -> usize { if token.to_lowercase() == BNB_USDC { 18 } else { 6 } }

/// eth_getBalance's answer as wei: "0x" and hex digits, up to 128 bits; None for anything else (past 64 bits is
/// common: 20 ETH is 2·10¹⁹ wei).
pub fn wei(answer: &serde_json::Value) -> Option<u128> {
    let hex = answer.as_str()?.strip_prefix("0x")?;
    if hex.is_empty() { return None }
    u128::from_str_radix(hex, 16).ok()
}

/// A node that can't answer this call at all — no such method, a key it turns away — while another node may. A revert
/// is never that.
pub fn not_this_node(message: &str) -> bool {
    let m = message.to_lowercase();
    !m.contains("revert") && ["method not found", "not supported", "unsupported method", "method not allowed", "not whitelisted", "unauthorized",
                              "invalid api key", "api key", "forbidden", "authenticat"].iter().any(|w| m.contains(w))
}

/// The node's «insufficient funds for gas * price + value: address 0x… have 120 want 340» (geth, Arbitrum) or «EVM
/// error: OutOfFunds» (reth) in words, with what to do; None for anything else (a token's own revert included).
pub fn short_of_gas(message: &str, chain_id: u64) -> Option<String> {
    let m = message.to_lowercase();
    if !(m.contains("insufficient funds") || m.contains("outoffunds")) || m.contains("revert") { return None }
    let coin = coin(chain_id);
    // Wei, so 18 decimals; Tempo's fee is in a USD token, said without amounts.
    let amount = |word: &str| -> Option<String> {
        if coin == "USD" { return None }
        let at = m.find(&format!("{word} "))? + word.len() + 1;
        let digits: String = m[at..].chars().take_while(char::is_ascii_digit).collect();
        let wei = digits.parse::<U256>().ok().filter(|_| !digits.is_empty())?;
        Some(format!("{} {coin}", units(&wei, 18)))
    };
    let said: Vec<String> = [amount("have").map(|a| format!("has {a}")), amount("want").map(|a| format!("needs {a} with the fee"))]
        .into_iter().flatten().collect();
    Some(format!("Not enough {coin} for gas: the wallet {}. Send it a little {coin}, then try again.",
                 if said.is_empty() { "can't pay this network's fee".to_string() } else { said.join(" and ") }))
}

#[cfg(test)]
mod tests;
