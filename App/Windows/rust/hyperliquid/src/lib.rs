//! Hyperliquid actions and their signatures (`Protocols/Hyperliquid/HyperliquidSigning.swift`): msgpack exactly as
//! the Python SDK packs it, orders on the wire, L1 actions signed over a phantom agent, user-signed actions over their
//! own fields, ihatecrypto's builder fee and referral code.

use std::str::FromStr;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use wallet::evm::eip712::{Field, Parts, Typed, Value};
use wallet::evm::{EvmSignature, keccak, sign};
use wallet::{Result, WalletError, from_hex, to_hex};

fn fail<T>(m: impl Into<String>) -> Result<T> { Err(WalletError(m.into())) }

/// MessagePack exactly as Python's msgpack.packb writes it: smallest int form, str8 for 32–255 byte strings, maps in
/// insertion order. The action hash depends on every byte, so key order is part of the format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MsgPack {
    Null,
    Bool(bool),
    Uint(u64),
    String(String),
    Array(Vec<MsgPack>),
    Map(Vec<(String, MsgPack)>),
}

impl MsgPack {
    pub fn str(s: &str) -> Self { Self::String(s.into()) }

    pub fn map(pairs: Vec<(&str, MsgPack)>) -> Self { Self::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()) }

    pub fn packed(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Null => out.push(0xc0),
            Self::Bool(b) => out.push(if *b { 0xc3 } else { 0xc2 }),
            Self::Uint(n) => match *n {
                0..0x80 => out.push(*n as u8),
                0x80..=0xff => out.extend([0xcc, *n as u8]),
                0x100..=0xffff => { out.push(0xcd); out.extend((*n as u16).to_be_bytes()) }
                0x1_0000..=0xffff_ffff => { out.push(0xce); out.extend((*n as u32).to_be_bytes()) }
                _ => { out.push(0xcf); out.extend(n.to_be_bytes()) }
            },
            Self::String(s) => {
                let n = s.len();
                match n {
                    0..32 => out.push(0xa0 | n as u8),
                    32..=0xff => out.extend([0xd9, n as u8]),
                    0x100..=0xffff => { out.push(0xda); out.extend((n as u16).to_be_bytes()) }
                    _ => { out.push(0xdb); out.extend((n as u32).to_be_bytes()) }
                }
                out.extend(s.as_bytes());
            }
            Self::Array(items) => {
                if items.len() < 16 { out.push(0x90 | items.len() as u8) } else { out.push(0xdc); out.extend((items.len() as u16).to_be_bytes()) }
                for item in items { item.write(out) }
            }
            Self::Map(pairs) => {
                if pairs.len() < 16 { out.push(0x80 | pairs.len() as u8) } else { out.push(0xde); out.extend((pairs.len() as u16).to_be_bytes()) }
                for (key, value) in pairs {
                    Self::String(key.clone()).write(out);
                    value.write(out);
                }
            }
        }
    }

    /// The value under `key` in a map; the first one when a key repeats, as Swift's `uniquingKeysWith: first` keeps.
    pub fn get(&self, key: &str) -> Option<&MsgPack> {
        let Self::Map(pairs) = self else { return None };
        pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// A string field of a map.
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key) { Some(Self::String(s)) => Some(s), _ => None }
    }

    /// The same action as JSON for the HTTP body; the server re-encodes it to msgpack to check the hash.
    pub fn to_json(&self) -> Json {
        match self {
            Self::Null => Json::Null,
            Self::Bool(b) => json!(b),
            Self::Uint(n) => json!(n),
            Self::String(s) => json!(s),
            Self::Array(items) => Json::Array(items.iter().map(Self::to_json).collect()),
            Self::Map(pairs) => {
                let mut m = serde_json::Map::new();
                for (k, v) in pairs { m.entry(k.clone()).or_insert_with(|| v.to_json()); }
                Json::Object(m)
            }
        }
    }
}

/// Across a pipe a map is ordered [key, value] pairs, so an action keeps its key order: the Swift `Codable` form,
/// {"kind": "map", "keys": […], "values": […]}.
#[derive(Serialize, Deserialize)]
struct Wire {
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bool: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uint: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    array: Option<Vec<MsgPack>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    keys: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    values: Option<Vec<MsgPack>>,
}

impl Serialize for MsgPack {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut w = Wire { kind: String::new(), bool: None, uint: None, string: None, array: None, keys: None, values: None };
        w.kind = match self {
            Self::Null => "null",
            Self::Bool(b) => { w.bool = Some(*b); "bool" }
            Self::Uint(n) => { w.uint = Some(*n); "uint" }
            Self::String(x) => { w.string = Some(x.clone()); "string" }
            Self::Array(a) => { w.array = Some(a.clone()); "array" }
            Self::Map(p) => {
                w.keys = Some(p.iter().map(|(k, _)| k.clone()).collect());
                w.values = Some(p.iter().map(|(_, v)| v.clone()).collect());
                "map"
            }
        }.into();
        w.serialize(s)
    }
}

impl<'de> Deserialize<'de> for MsgPack {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        use serde::de::Error;
        let w = Wire::deserialize(d)?;
        let missing = |k: &str| D::Error::missing_field(Box::leak(k.to_string().into_boxed_str()));
        Ok(match w.kind.as_str() {
            "null" => Self::Null,
            "bool" => Self::Bool(w.bool.ok_or_else(|| missing("bool"))?),
            "uint" => Self::Uint(w.uint.ok_or_else(|| missing("uint"))?),
            "string" => Self::String(w.string.ok_or_else(|| missing("string"))?),
            "array" => Self::Array(w.array.ok_or_else(|| missing("array"))?),
            // Swift zips the two lists, so the shorter one decides.
            "map" => Self::Map(w.keys.ok_or_else(|| missing("keys"))?.into_iter().zip(w.values.ok_or_else(|| missing("values"))?).collect()),
            other => return Err(D::Error::unknown_variant(other, &["null", "bool", "uint", "string", "array", "map"])),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderKind {
    /// tif: "Gtc", "Ioc" or "Alo"
    Limit { tif: String },
    /// tpsl: "tp" or "sl"
    Trigger { price: Decimal, is_market: bool, tpsl: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub asset: u32,
    pub is_buy: bool,
    pub price: Decimal,
    pub size: Decimal,
    pub reduce_only: bool,
    pub kind: OrderKind,
    /// 16 bytes as "0x" + 32 hex chars. Derived from the plan hash so a resend is recognisable.
    pub cloid: Option<String>,
}

/// ihatecrypto's fee on Hyperliquid trades (a builder code) and its referral code. Both are public by design: every
/// order that pays the fee names this address on the exchange, and the code is a public link.
pub mod builder {
    use super::MsgPack;
    use rust_decimal::Decimal;

    /// Where the fee goes, lowercase as an order's builder field must be.
    pub const ADDRESS: &str = "0x3828b920d705587b16be05d2d3764333a58c87ec";
    /// Tenths of a basis point: 50 = 0.05% of the trade's value.
    pub const FEE: u64 = 50;
    /// The approval's maxFeeRate: exactly the fee, so the fee can't grow without a new approval.
    pub const RATE: &str = "0.05%";
    /// The approval taken back.
    pub const REVOKE: &str = "0%";
    pub const REFERRAL_CODE: &str = "IHATECRYPTO";

    /// The fee as a share of the trade: 0.0005.
    pub fn share() -> Decimal { Decimal::from(FEE) / Decimal::from(100_000) }

    /// approveBuilderFee, signed by the main key: `max_fee_rate` is RATE to approve, REVOKE to take it back.
    pub fn approve_action(max_fee_rate: &str, nonce: u64, mainnet: bool) -> MsgPack {
        MsgPack::map(vec![
            ("type", MsgPack::str("approveBuilderFee")), ("maxFeeRate", MsgPack::str(max_fee_rate)), ("builder", MsgPack::str(ADDRESS)),
            ("nonce", MsgPack::Uint(nonce)), ("signatureChainId", MsgPack::str("0x66eee")), ("hyperliquidChain", MsgPack::str(super::chain(mainnet))),
        ])
    }

    /// setReferrer, an L1 action: the account gets 4% off Hyperliquid's fees, ihatecrypto a share of them.
    pub fn referrer_action() -> MsgPack { MsgPack::map(vec![("type", MsgPack::str("setReferrer")), ("code", MsgPack::str(REFERRAL_CODE))]) }
}

fn chain(mainnet: bool) -> &'static str { if mainnet { "Mainnet" } else { "Testnet" } }

pub mod action {
    use super::*;

    /// {"type": "order", "orders": […], "grouping": …}. grouping: "na", "normalTpsl" or "positionTpsl".
    pub fn order(orders: &[Order], grouping: &str) -> Result<MsgPack> {
        Ok(MsgPack::map(vec![
            ("type", MsgPack::str("order")),
            ("orders", MsgPack::Array(orders.iter().map(wire).collect::<Result<_>>()?)),
            ("grouping", MsgPack::str(grouping)),
        ]))
    }

    /// The action's "type" field.
    pub fn kind(action: &MsgPack) -> Option<&str> { action.text("type") }

    /// A cancel, or orders that are all reduce-only (a stop, a take, a close): nothing that opens or adds to a position.
    pub fn only_reduces(action: &MsgPack) -> bool {
        match kind(action) {
            Some("cancel" | "cancelByCloid") => matches!(action, MsgPack::Map(_)),
            Some("order") => match action.get("orders") {
                Some(MsgPack::Array(orders)) if !orders.is_empty() => orders.iter().all(|o| o.get("r") == Some(&MsgPack::Bool(true))),
                _ => false,
            },
            _ => false,
        }
    }

    /// The same order action paying ihatecrypto's fee: {"b": address, "f": fee} as its last key, after grouping, where
    /// the SDK puts it (the hash covers the order of keys). Anything else comes back as it was.
    pub fn with_builder(action: &MsgPack) -> MsgPack {
        match action {
            MsgPack::Map(pairs) if kind(action) == Some("order") && !pairs.iter().any(|(k, _)| k == "builder") => {
                let mut pairs = pairs.clone();
                pairs.push(("builder".into(), MsgPack::map(vec![("b", MsgPack::str(builder::ADDRESS)), ("f", MsgPack::Uint(builder::FEE))])));
                MsgPack::Map(pairs)
            }
            _ => action.clone(),
        }
    }

    /// {"type": "cancel", "cancels": [{"a": asset, "o": oid}, …]}
    pub fn cancel(orders: &[(u32, u64)]) -> MsgPack {
        MsgPack::map(vec![
            ("type", MsgPack::str("cancel")),
            ("cancels", MsgPack::Array(orders.iter().map(|(a, o)| MsgPack::map(vec![("a", MsgPack::Uint(*a as u64)), ("o", MsgPack::Uint(*o))])).collect())),
        ])
    }

    fn wire(o: &Order) -> Result<MsgPack> {
        let ty = match &o.kind {
            OrderKind::Limit { tif } => MsgPack::map(vec![("limit", MsgPack::map(vec![("tif", MsgPack::str(tif))]))]),
            OrderKind::Trigger { price, is_market, tpsl } => MsgPack::map(vec![("trigger", MsgPack::map(vec![
                ("isMarket", MsgPack::Bool(*is_market)), ("triggerPx", MsgPack::String(number(*price)?)), ("tpsl", MsgPack::str(tpsl)),
            ]))]),
        };
        let mut fields = vec![
            ("a", MsgPack::Uint(o.asset as u64)), ("b", MsgPack::Bool(o.is_buy)),
            ("p", MsgPack::String(number(o.price)?)), ("s", MsgPack::String(number(o.size)?)),
            ("r", MsgPack::Bool(o.reduce_only)), ("t", ty),
        ];
        if let Some(cloid) = &o.cloid {
            if !(cloid.starts_with("0x") && cloid.chars().count() == 34 && from_hex(cloid).is_some()) {
                return fail("cloid must be 0x plus 32 hex characters.");
            }
            fields.push(("c", MsgPack::str(cloid)));
        }
        Ok(MsgPack::map(fields))
    }

    /// `NSDecimalRound`: to `scale` decimals, a negative scale rounding to tens, hundreds…
    fn round(d: Decimal, scale: i32, strategy: RoundingStrategy) -> Decimal {
        if scale >= 0 { return d.round_dp_with_strategy(scale as u32, strategy) }
        let p = Decimal::from(10i64.pow((-scale) as u32));
        (d / p).round_dp_with_strategy(0, strategy) * p
    }

    /// Like the SDK's _slippage_price: 5 significant figures, then at most 6 − szDecimals decimals (8 for spot).
    pub fn price(px: Decimal, sz_decimals: i32, spot: bool) -> Decimal {
        let magnitude = px.to_f64().filter(|f| *f > 0.0).map_or(0, |f| f.log10().floor() as i32);
        let significant = round(px, (4 - magnitude).max(0), RoundingStrategy::MidpointAwayFromZero);
        round(significant, if spot { 8 } else { 6 } - sz_decimals, RoundingStrategy::MidpointAwayFromZero)
    }

    /// Size in whole steps of the market's szDecimals, rounded down so it never exceeds what was asked.
    pub fn size(sz: Decimal, sz_decimals: i32) -> Decimal { round(sz, sz_decimals, RoundingStrategy::ToZero) }

    /// Like the SDK's float_to_wire: at most 8 decimals, no trailing zeros, no exponent ("100", "0.0147").
    pub fn number(d: Decimal) -> Result<String> {
        let rounded = round(d, 8, RoundingStrategy::MidpointAwayFromZero);
        if rounded != d { return fail(format!("{d}: more than 8 decimal places.")) }
        Ok(if rounded.is_zero() { "0".into() } else { rounded.normalize().to_string() })
    }
}

pub mod signer {
    use super::*;

    const ZERO: &str = "0x0000000000000000000000000000000000000000";

    /// Orders, cancels, leverage and the referral code: what a trading (agent) key may sign. Nothing here moves money
    /// out of the account (the guard holds an order's fee and the code to ihatecrypto's own).
    pub fn is_trading(action: &MsgPack) -> bool {
        action::kind(action).is_some_and(|k| ["order", "cancel", "updateLeverage", "batchModify", "modify", "setReferrer"].contains(&k))
    }

    /// keccak256(msgpack(action) ‖ nonce ‖ vault flag [‖ vault] [‖ 0x00 ‖ expiresAfter]).
    pub fn action_hash(action: &MsgPack, vault: Option<&str>, nonce: u64, expires_after: Option<u64>) -> Result<[u8; 32]> {
        let mut data = action.packed();
        data.extend(nonce.to_be_bytes());
        match vault {
            Some(v) => match from_hex(v) {
                Some(bytes) if bytes.len() == 20 => { data.push(1); data.extend(bytes) }
                _ => return fail("Invalid vault address."),
            },
            None => data.push(0),
        }
        if let Some(e) = expires_after {
            data.push(0);
            data.extend(e.to_be_bytes());
        }
        Ok(keccak(&data))
    }

    /// Orders, cancels, leverage: EIP-712 over a "phantom agent" that carries the action hash.
    pub fn sign_l1(action: &MsgPack, private_key: &[u8], vault: Option<&str>, nonce: u64, expires_after: Option<u64>, mainnet: bool) -> Result<EvmSignature> {
        sign(&l1_typed(action, vault, nonce, expires_after, mainnet)?.parts()?.digest(), private_key)
    }

    /// The typed data an L1 action is signed as, in the two hashes a Ledger signs.
    pub fn l1_parts(action: &MsgPack, vault: Option<&str>, nonce: u64, expires_after: Option<u64>, mainnet: bool) -> Result<Parts> {
        l1_typed(action, vault, nonce, expires_after, mainnet)?.parts()
    }

    pub fn l1_typed(action: &MsgPack, vault: Option<&str>, nonce: u64, expires_after: Option<u64>, mainnet: bool) -> Result<Typed> {
        let hash = action_hash(action, vault, nonce, expires_after)?;
        Ok(Typed {
            domain: vec![
                Field::new("name", "string", Value::String("Exchange".into())),
                Field::new("version", "string", Value::String("1".into())),
                Field::new("chainId", "uint256", Value::Uint(1337)),
                Field::new("verifyingContract", "address", Value::Address(ZERO.into())),
            ],
            ty: "Agent".into(),
            message: vec![
                Field::new("source", "string", Value::String(if mainnet { "a" } else { "b" }.into())),
                Field::new("connectionId", "bytes32", Value::Bytes32(hash.to_vec())),
            ],
        })
    }

    /// Transfers, withdrawals, approving an agent: signed by the main wallet with the action fields themselves.
    /// `fields` start after hyperliquidChain, which is added here.
    pub fn sign_user_action(ty: &str, fields: Vec<Field>, private_key: &[u8], mainnet: bool) -> Result<EvmSignature> {
        sign(&user_typed(ty, fields, mainnet).parts()?.digest(), private_key)
    }

    pub fn user_parts(ty: &str, fields: Vec<Field>, mainnet: bool) -> Result<Parts> { user_typed(ty, fields, mainnet).parts() }

    pub fn user_typed(ty: &str, fields: Vec<Field>, mainnet: bool) -> Typed {
        let mut message = vec![Field::new("hyperliquidChain", "string", Value::String(chain(mainnet).into()))];
        message.extend(fields);
        Typed {
            domain: vec![
                Field::new("name", "string", Value::String("HyperliquidSignTransaction".into())),
                Field::new("version", "string", Value::String("1".into())),
                Field::new("chainId", "uint256", Value::Uint(0x66eee)),
                Field::new("verifyingContract", "address", Value::Address(ZERO.into())),
            ],
            ty: format!("HyperliquidTransaction:{ty}"),
            message,
        }
    }

    /// Hyperliquid user-signed actions a signer knows, with their EIP-712 fields after hyperliquidChain.
    fn user_schema(kind: &str) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
        Some(match kind {
            "usdClassTransfer" => ("UsdClassTransfer", &[("amount", "string"), ("toPerp", "bool"), ("nonce", "uint64")]),
            "spotSend" => ("SpotSend", &[("destination", "string"), ("token", "string"), ("amount", "string"), ("time", "uint64")]),
            "sendAsset" => ("SendAsset", &[("destination", "string"), ("sourceDex", "string"), ("destinationDex", "string"),
                                           ("token", "string"), ("amount", "string"), ("fromSubAccount", "string"), ("nonce", "uint64")]),
            "withdraw3" => ("Withdraw", &[("destination", "string"), ("amount", "string"), ("time", "uint64")]),
            "usdSend" => ("UsdSend", &[("destination", "string"), ("amount", "string"), ("time", "uint64")]),
            "approveBuilderFee" => ("ApproveBuilderFee", &[("maxFeeRate", "string"), ("builder", "address"), ("nonce", "uint64")]),
            _ => return None,
        })
    }

    /// The EIP-712 type and fields of a user-signed action, read from the action itself: what is signed is what the
    /// guard checked.
    pub fn user_action(action: &MsgPack) -> Result<(String, Vec<Field>)> {
        if !matches!(action, MsgPack::Map(_)) { return fail("Unrecognized action.") }
        let Some((ty, schema)) = action.text("type").and_then(user_schema) else {
            return fail("Signing this Hyperliquid action isn't supported.");
        };
        let fields = schema.iter().map(|&(name, ty)| {
            let value = match (ty, action.get(name)) {
                ("string", Some(MsgPack::String(s))) => Value::String(s.clone()),
                ("address", Some(MsgPack::String(s))) => Value::Address(s.clone()),
                ("bool", Some(MsgPack::Bool(b))) => Value::Bool(*b),
                ("uint64", Some(MsgPack::Uint(n))) => Value::Uint(*n),
                _ => return fail(format!("The action has no {name} field of type {ty}.")),
            };
            Ok(Field::new(name, ty, value))
        }).collect::<Result<_>>()?;
        Ok((ty.into(), fields))
    }

    /// Moves USDC between the HyperCore spot and perp balances. Returns the action to send with the signature.
    pub fn usd_class_transfer(amount: Decimal, to_perp: bool, nonce: u64, private_key: &[u8], mainnet: bool) -> Result<(MsgPack, EvmSignature)> {
        let amount = action::number(amount)?;
        let signature = sign_user_action("UsdClassTransfer", vec![
            Field::new("amount", "string", Value::String(amount.clone())),
            Field::new("toPerp", "bool", Value::Bool(to_perp)),
            Field::new("nonce", "uint64", Value::Uint(nonce)),
        ], private_key, mainnet)?;
        let action = MsgPack::map(vec![
            ("type", MsgPack::str("usdClassTransfer")), ("amount", MsgPack::String(amount)), ("toPerp", MsgPack::Bool(to_perp)),
            ("nonce", MsgPack::Uint(nonce)), ("signatureChainId", MsgPack::str("0x66eee")), ("hyperliquidChain", MsgPack::str(chain(mainnet))),
        ]);
        Ok((action, signature))
    }

    /// Sends a spot token on HyperCore to another address (a 1Click deposit address for the way back).
    /// `token` is "NAME:tokenId", e.g. HyperCore USDC "USDC:0x6d1e7cde53ba9467b783cb7c530ce054".
    pub fn spot_send(destination: &str, token: &str, amount: Decimal, time: u64, private_key: &[u8], mainnet: bool) -> Result<(MsgPack, EvmSignature)> {
        let amount = action::number(amount)?;
        let signature = sign_user_action("SpotSend", vec![
            Field::new("destination", "string", Value::String(destination.into())),
            Field::new("token", "string", Value::String(token.into())),
            Field::new("amount", "string", Value::String(amount.clone())),
            Field::new("time", "uint64", Value::Uint(time)),
        ], private_key, mainnet)?;
        let action = MsgPack::map(vec![
            ("type", MsgPack::str("spotSend")), ("destination", MsgPack::str(destination)), ("token", MsgPack::str(token)),
            ("amount", MsgPack::String(amount)), ("time", MsgPack::Uint(time)),
            ("signatureChainId", MsgPack::str("0x66eee")), ("hyperliquidChain", MsgPack::str(chain(mainnet))),
        ]);
        Ok((action, signature))
    }

    /// Relay's binding of a Hyperliquid deposit to an order: EIP-712 «RelayNonceMapping», signed by the depositor.
    pub fn relay_nonce_mapping_typed(wallet: &str, depositor: &str, id: &[u8], nonce: u64) -> Result<Typed> {
        if id.len() != 32 { return fail("Invalid Relay mapping id.") }
        Ok(Typed {
            domain: vec![
                Field::new("name", "string", Value::String("RelayNonceMapping".into())), Field::new("version", "string", Value::String("2".into())),
                Field::new("chainId", "uint256", Value::Uint(1)), Field::new("verifyingContract", "address", Value::Address(ZERO.into())),
            ],
            ty: "NonceMapping".into(),
            message: vec![
                Field::new("chainId", "string", Value::String("hyperliquid".into())), Field::new("wallet", "address", Value::Address(wallet.into())),
                Field::new("depositor", "address", Value::Address(depositor.into())), Field::new("id", "bytes32", Value::Bytes32(id.to_vec())),
                Field::new("nonce", "uint256", Value::Uint(nonce)),
            ],
        })
    }

    pub fn relay_nonce_mapping_digest(wallet: &str, depositor: &str, id: &[u8], nonce: u64) -> Result<[u8; 32]> {
        Ok(relay_nonce_mapping_typed(wallet, depositor, id, nonce)?.parts()?.digest())
    }

    /// usdSend: USDC from the perp balance to another Hyperliquid address; `time` (ms) is the nonce. For a signer in
    /// another process, which builds the EIP-712 «UsdSend» from these very fields.
    pub fn usd_send_action(destination: &str, amount: &str, time: u64, mainnet: bool) -> MsgPack {
        MsgPack::map(vec![
            ("type", MsgPack::str("usdSend")), ("signatureChainId", MsgPack::str("0x66eee")), ("hyperliquidChain", MsgPack::str(chain(mainnet))),
            ("destination", MsgPack::str(destination)), ("amount", MsgPack::str(amount)), ("time", MsgPack::Uint(time)),
        ])
    }

    /// The sendAsset action alone, for a signer in another process.
    pub fn send_asset_action(destination: &str, source_dex: &str, destination_dex: &str, token: &str, amount: &str, nonce: u64, mainnet: bool) -> MsgPack {
        MsgPack::map(vec![
            ("type", MsgPack::str("sendAsset")), ("destination", MsgPack::str(destination)), ("sourceDex", MsgPack::str(source_dex)),
            ("destinationDex", MsgPack::str(destination_dex)), ("token", MsgPack::str(token)), ("amount", MsgPack::str(amount)),
            ("fromSubAccount", MsgPack::str("")), ("nonce", MsgPack::Uint(nonce)),
            ("signatureChainId", MsgPack::str("0x66eee")), ("hyperliquidChain", MsgPack::str(chain(mainnet))),
        ])
    }

    /// Moves a token between balances or to another address (Relay's depository for the way out).
    /// source_dex "" = the perp balance, "spot" = spot.
    pub fn send_asset(destination: &str, source_dex: &str, destination_dex: &str, token: &str, amount: &str, nonce: u64,
                      private_key: &[u8], mainnet: bool) -> Result<(MsgPack, EvmSignature)> {
        let action = send_asset_action(destination, source_dex, destination_dex, token, amount, nonce, mainnet);
        let (ty, fields) = user_action(&action)?;
        Ok((action, sign_user_action(&ty, fields, private_key, mainnet)?))
    }

    /// The one approval that lets a separate agent key trade (but not withdraw) for this account. Signed by the main
    /// key; afterwards orders are signed by the agent key.
    pub fn approve_agent(agent: &str, name: &str, nonce: u64, private_key: &[u8], mainnet: bool) -> Result<(MsgPack, EvmSignature)> {
        let (action, typed) = approve_agent_typed(agent, name, nonce, mainnet);
        Ok((action, sign(&typed.parts()?.digest(), private_key)?))
    }

    /// The approval as typed data, for a key held elsewhere (a Ledger signs it and shows the agent's address).
    pub fn approve_agent_typed(agent: &str, name: &str, nonce: u64, mainnet: bool) -> (MsgPack, Typed) {
        let typed = user_typed("ApproveAgent", vec![
            Field::new("agentAddress", "address", Value::Address(agent.into())),
            Field::new("agentName", "string", Value::String(name.into())),
            Field::new("nonce", "uint64", Value::Uint(nonce)),
        ], mainnet);
        let action = MsgPack::map(vec![
            ("type", MsgPack::str("approveAgent")), ("agentAddress", MsgPack::str(agent)), ("agentName", MsgPack::str(name)),
            ("nonce", MsgPack::Uint(nonce)), ("signatureChainId", MsgPack::str("0x66eee")), ("hyperliquidChain", MsgPack::str(chain(mainnet))),
        ]);
        (action, typed)
    }
}

/// What /exchange takes: the action, the nonce, the signature with 0x hex r and s, the vault or null.
pub fn exchange_body(action: &MsgPack, nonce: u64, signature: &EvmSignature, vault: Option<&str>) -> Json {
    json!({
        "action": action.to_json(),
        "nonce": nonce,
        "signature": {"r": format!("0x{}", to_hex(&signature.r)), "s": format!("0x{}", to_hex(&signature.s)), "v": signature.v},
        "vaultAddress": vault,
    })
}

pub fn exchange_url(mainnet: bool) -> &'static str {
    if mainnet { "https://api.hyperliquid.xyz/exchange" } else { "https://api.hyperliquid-testnet.xyz/exchange" }
}

/// A decimal from text, for tests and fixed values.
pub fn dec(s: &str) -> Decimal { Decimal::from_str(s).expect("a decimal") }

#[cfg(test)]
mod tests;
