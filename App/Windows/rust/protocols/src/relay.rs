//! Relay (relay.link): an intent bridge where a solver fills on the destination within seconds and the person's money
//! goes to Relay's depository on the origin chain. Quotes need no API key (`Relay.swift`).

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Map, Value, json};
use solana::Instruction;
use wallet::{Result, WalletError, from_hex};

use crate::{dec_value, fail, number, post, s, text};

pub const BASE: &str = "https://api.relay.link";

/// Relay's chain ids.
pub mod chain {
    pub const SOLANA: u64 = 792_703_809;
    pub const HYPERLIQUID: u64 = 1337;
    pub const ARBITRUM: u64 = 42161;
    pub const BASE: u64 = 8453;
    pub const ROBINHOOD: u64 = 4663;
    pub const ETHEREUM: u64 = 1;
    pub const POLYGON: u64 = 137;
    pub const TEMPO: u64 = 4217;
    pub const HYPEREVM: u64 = 999;
    pub const OPTIMISM: u64 = 10;
    pub const BNB: u64 = 56;
}

/// Relay's depository on Hyperliquid (protocol.v2.depository in /chains, checked 2026-09-25). Callers compare it with
/// what /chains publishes at run time; both must agree.
pub const HYPERLIQUID_DEPOSITORY: &str = "0x66cf0aace1b4e562593bec10ec7868fba9932224";

pub mod currency {
    pub const SOLANA_USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    pub const SOLANA_SOL: &str = "11111111111111111111111111111111";
    /// USDC on the Hyperliquid perp balance, 8 decimals.
    pub const HYPERLIQUID_USDC: &str = "0x00000000000000000000000000000000";
}

/// `RelayError`: what Relay said, in its own words.
pub fn error(body: &[u8]) -> WalletError { WalletError(format!("Relay: {}", net::said(body, 0))) }

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quote {
    pub request_id: String,
    pub sender: String,
    pub recipient: String,
    pub amount_in: String,
    pub amount_in_formatted: String,
    pub amount_out_formatted: String,
    pub fees_usd: String,
    pub seconds: i64,
    /// Native coin for gas that arrives with the fill («0.000037» ETH), empty without a top-up.
    pub gas_topup: String,
    /// Solana steps: instructions to compile and sign.
    pub solana_instructions: Vec<Instruction>,
    /// Hyperliquid origin: an EIP-712 "RelayNonceMapping" to sign and post, then a sendAsset to the depository.
    pub hyperliquid: Option<HyperliquidSteps>,
    /// EVM origin: transactions in order («approve», «deposit»), for the caller to check and sign.
    pub evm: Vec<EvmStep>,
    /// The relayer's own gas in USD (delivering to a new Hyperliquid account makes it Hyperliquid's opening fee).
    pub relayer_gas_usd: String,
    /// What arrives in the destination's smallest units, and what goes in and comes out in USD.
    pub amount_out: String,
    pub amount_in_usd: String,
    pub amount_out_usd: String,
    /// With `use_permit`: the EIP-3009 item Relay asks for instead of transactions, when that is the step's only item.
    pub permit: Option<Permit>,
    /// On Relay's v3.2 proxy: the MulticallAuthorization to sign first, with the EIP-3009 item it binds.
    pub multicall: Option<MulticallPermit>,
    /// The order a deposit names (protocol.v2.orderData, as JSON): the signer checks it before it signs the deposit.
    pub order: Option<Vec<u8>>,
    /// With `use_deposit_address`: where a plain transfer of the origin token starts the order.
    pub deposit_address: Option<String>,
}

/// One payment by signature as Relay's quote gives it, and where its signature goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permit {
    pub chain_id: u64,
    pub token: String,
    pub domain_name: String,
    pub domain_version: String,
    pub from: String,
    pub to: String,
    pub value: String,
    pub valid_after: u64,
    pub valid_before: u64,
    pub nonce: Vec<u8>,
    pub endpoint: String,
    /// The post body as Relay returned it ({kind: "eip3009", requestId, api}).
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MulticallCall {
    pub target: String,
    pub allow_failure: bool,
    pub value: String,
    pub call_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Funding {
    pub token: String,
    pub from: String,
    pub value: String,
    pub valid_after: u64,
    pub valid_before: u64,
}

/// Relay's v3.2 MulticallAuthorization item as its quote gives it, and the EIP-3009 item after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MulticallPermit {
    pub chain_id: u64,
    pub domain_name: String,
    pub domain_version: String,
    pub verifying_contract: String,
    pub from: String,
    pub relayer: String,
    pub msg_value: String,
    pub refund_to: String,
    pub nft_recipient: String,
    pub metadata: Vec<u8>,
    pub funding_index: u64,
    pub calls: Vec<MulticallCall>,
    pub funding: Vec<Funding>,
    pub endpoint: String,
    pub body: Vec<u8>,
    pub permit: Permit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmStep {
    pub id: String,
    pub to: String,
    pub data: Vec<u8>,
    pub value: String,
    pub chain_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperliquidSteps {
    pub mapping_id: String,
    pub mapping_nonce: u64,
    pub wallet: String,
    pub depositor: String,
    /// The /authorize body as Relay returned it, without the signature.
    pub authorize_body: Vec<u8>,
    pub destination: String,
    pub token: String,
    pub amount: String,
    pub nonce: u64,
    pub source_dex: String,
    pub destination_dex: String,
}

/// What /quote/v2 is asked: `exact_output` (amount is what must arrive), `topup_gas_usd` (also that much of the
/// destination's coin for gas), `units` (the amount as a decimal string past 64 bits), `use_permit` (an origin USDC goes
/// by a signature), `use_deposit_address` and `refund_to`.
#[derive(Debug, Clone, Default)]
pub struct Ask {
    pub user: String,
    pub recipient: String,
    pub origin_chain: u64,
    pub destination_chain: u64,
    pub origin_currency: String,
    pub destination_currency: String,
    pub amount: u64,
    pub exact_output: bool,
    pub topup_gas_usd: Option<Decimal>,
    pub units: Option<String>,
    pub use_permit: bool,
    pub use_deposit_address: bool,
    pub refund_to: Option<String>,
}

impl Ask {
    pub fn body(&self) -> Value {
        let mut b = json!({
            "user": self.user, "recipient": self.recipient, "originChainId": self.origin_chain, "destinationChainId": self.destination_chain,
            "originCurrency": self.origin_currency, "destinationCurrency": self.destination_currency,
            "amount": self.units.clone().unwrap_or_else(|| self.amount.to_string()),
            "tradeType": if self.exact_output { "EXACT_OUTPUT" } else { "EXACT_INPUT" },
        });
        // useMulticallAuthorization: Relay's v3.2 proxy binds the dollar to its calls with a second signature.
        if self.use_permit { b["usePermit"] = json!(true); b["useMulticallAuthorization"] = json!(true) }
        if self.use_deposit_address { b["useDepositAddress"] = json!(true) }
        if let Some(r) = &self.refund_to { b["refundTo"] = json!(r) }
        if let Some(usd) = self.topup_gas_usd {
            b["topupGas"] = json!(true);
            // Int((usd.doubleValue * 1_000_000).rounded(.up)): micro-dollars, up.
            b["topupGasAmount"] = json!(((usd.to_f64().unwrap_or(0.0) * 1_000_000.0).ceil() as i64).to_string());
        }
        b
    }
}

/// Asks Relay for a quote.
pub async fn quote(ask: &Ask) -> Result<Quote> { quote_at(BASE, ask).await }

pub async fn quote_at(base: &str, ask: &Ask) -> Result<Quote> {
    let (data, _) = net::post_json(&format!("{base}/quote/v2"), &ask.body()).await.map_err(|e| WalletError(e.to_string()))?;
    parse_quote(&data, ask.use_permit, ask.use_deposit_address)
}

fn items(step: &Value) -> impl Iterator<Item = &Value> { step.get("items").and_then(Value::as_array).into_iter().flatten() }

/// /quote/v2's answer read: its steps (Solana instructions, EVM transactions, Hyperliquid's mapping and deposit, the
/// payment by signature), amounts, fees and the order.
pub fn parse_quote(data: &[u8], use_permit: bool, use_deposit_address: bool) -> Result<Quote> {
    let refused = || Err(error(data));
    let Ok(j) = serde_json::from_slice::<Value>(data) else { return refused() };
    let (Some(steps), Some(details)) = (j.get("steps").and_then(Value::as_array), j.get("details").filter(|d| d.is_object())) else { return refused() };
    let Some(request_id) = s(&j, "requestId").or_else(|| steps.iter().find_map(|st| s(st, "requestId"))).map(str::to_string) else { return refused() };
    let amounts = |key: &str| details.get(key).cloned().unwrap_or(Value::Object(Map::new()));
    let field = |v: &Value, k: &str| s(v, k).unwrap_or("").to_string();

    let (mut mapping, mut authorize_body, mut deposit, mut deposit_nonce) = (None, None, None, None);
    for item in steps.iter().flat_map(items) {
        let d = item.get("data").cloned().unwrap_or(Value::Null);
        if let (Some(sign), Some(post)) = (d.get("sign").filter(|v| v.is_object()), d.get("post").filter(|v| v.is_object())) {
            mapping = sign.get("value").filter(|v| v.is_object()).cloned();
            authorize_body = post.get("body").filter(|v| v.is_object()).map(|b| b.to_string().into_bytes());
        }
        if let Some(action) = d.get("action").filter(|a| s(a, "type") == Some("sendAsset")) {
            deposit = action.get("parameters").filter(|v| v.is_object()).cloned();
            deposit_nonce = d.get("nonce").and_then(Value::as_u64);
        }
    }
    let hyperliquid = match (mapping, authorize_body, deposit, deposit_nonce) {
        (Some(m), Some(body), Some(p), Some(nonce)) => Some(HyperliquidSteps {
            mapping_id: field(&m, "id"), mapping_nonce: m.get("nonce").and_then(Value::as_u64).unwrap_or(0),
            wallet: field(&m, "wallet"), depositor: field(&m, "depositor"), authorize_body: body,
            destination: field(&p, "destination"), token: field(&p, "token"), amount: field(&p, "amount"), nonce,
            source_dex: field(&p, "sourceDex"), destination_dex: field(&p, "destinationDex"),
        }),
        _ => None,
    };

    let mut solana_instructions = Vec::new();
    for item in steps.iter().flat_map(items) {
        for ix in item.get("data").and_then(|d| d.get("instructions")).and_then(Value::as_array).into_iter().flatten() {
            let (Some(program), Some(bytes)) = (s(ix, "programId"), s(ix, "data").and_then(from_hex)) else { return refused() };
            let accounts = ix.get("keys").and_then(Value::as_array).into_iter().flatten().map(|k| (
                field(k, "pubkey"), k.get("isSigner").and_then(Value::as_bool).unwrap_or(false), k.get("isWritable").and_then(Value::as_bool).unwrap_or(false),
            )).collect();
            solana_instructions.push(Instruction { program: program.into(), accounts, data: bytes });
        }
    }

    let mut evm = Vec::new();
    for step in steps.iter().filter(|st| s(st, "kind") == Some("transaction")) {
        for item in items(step) {
            let Some(d) = item.get("data").filter(|d| d.is_object()) else { continue };
            let (Some(to), Some(bytes)) = (s(d, "to"), s(d, "data").and_then(from_hex)) else { continue };
            evm.push(EvmStep { id: field(step, "id"), to: to.into(), data: bytes, value: text(d.get("value"), "0"),
                               chain_id: d.get("chainId").and_then(Value::as_u64).unwrap_or(0) });
        }
    }

    let fees = j.get("fees").cloned().unwrap_or(Value::Null);
    let (cin, cout) = (amounts("currencyIn"), amounts("currencyOut"));
    Ok(Quote {
        request_id,
        sender: field(details, "sender"),
        recipient: field(details, "recipient"),
        amount_in: field(&cin, "amount"),
        amount_in_formatted: field(&cin, "amountFormatted"),
        amount_out_formatted: field(&cout, "amountFormatted"),
        fees_usd: fees.get("relayer").map(|r| field(r, "amountUsd")).unwrap_or_default(),
        seconds: details.get("timeEstimate").and_then(Value::as_f64).map_or(0, |t| t as i64),
        gas_topup: details.get("currencyGasTopup").map(|g| field(g, "amountFormatted")).unwrap_or_default(),
        solana_instructions,
        hyperliquid,
        evm,
        relayer_gas_usd: fees.get("relayerGas").map(|r| field(r, "amountUsd")).unwrap_or_default(),
        amount_out: field(&cout, "amount"),
        amount_in_usd: field(&cin, "amountUsd"),
        amount_out_usd: field(&cout, "amountUsd"),
        permit: if use_permit { permit_in(steps) } else { None },
        multicall: if use_permit { multicall_in(steps) } else { None },
        order: j.get("protocol").and_then(|p| p.get("v2")).and_then(|v| v.get("orderData")).map(|o| o.to_string().into_bytes()),
        deposit_address: if use_deposit_address { steps.iter().find_map(|st| s(st, "depositAddress")).map(str::to_string) } else { None },
    })
}

/// The step's one ReceiveWithAuthorization item, or None when the steps ask for anything else as well.
pub fn permit_in(steps: &[Value]) -> Option<Permit> {
    let all: Vec<&Value> = steps.iter().flat_map(items).collect();
    if steps.len() != 1 || all.len() != 1 { return None }
    permit_item(all[0])
}

/// The step's two items on the v3.2 proxy: the MulticallAuthorization, then the ReceiveWithAuthorization it binds.
pub fn multicall_in(steps: &[Value]) -> Option<MulticallPermit> {
    let all: Vec<&Value> = steps.iter().flat_map(items).collect();
    if steps.len() != 1 || all.len() != 2 { return None }
    let permit = permit_item(all[1])?;
    let d = all[0].get("data")?;
    let sign = d.get("sign").filter(|v| v.is_object())?;
    if s(sign, "primaryType") != Some("MulticallAuthorization") { return None }
    let domain = sign.get("domain").filter(|v| v.is_object())?;
    let v = sign.get("value").filter(|v| v.is_object())?;
    let post = d.get("post").filter(|v| v.is_object())?;
    let endpoint = s(post, "endpoint")?;
    let body = post.get("body").filter(|b| s(b, "kind") == Some("multicall-authorization"))?;
    let chain_id = number(domain.get("chainId"))?;
    let metadata = s(v, "metadata").and_then(from_hex)?;
    let funding_index = number(v.get("fundingIndex"))?;
    let calls = v.get("calls")?.as_array()?.iter().map(|c| Some(MulticallCall {
        target: s(c, "target")?.into(), allow_failure: c.get("allowFailure").and_then(Value::as_bool).unwrap_or(true),
        value: text(c.get("value"), ""), call_data: s(c, "callData").and_then(from_hex)?,
    })).collect::<Option<Vec<_>>>()?;
    let funding = v.get("funding")?.as_array()?.iter().map(|f| Some(Funding {
        token: s(f, "token").unwrap_or("").into(), from: s(f, "from").unwrap_or("").into(), value: text(f.get("value"), ""),
        valid_after: number(f.get("validAfter"))?, valid_before: number(f.get("validBefore"))?,
    })).collect::<Option<Vec<_>>>()?;
    let field = |k: &str| s(v, k).unwrap_or("").to_string();
    Some(MulticallPermit {
        chain_id, domain_name: s(domain, "name").unwrap_or("").into(), domain_version: s(domain, "version").unwrap_or("").into(),
        verifying_contract: s(domain, "verifyingContract").unwrap_or("").into(), from: field("from"), relayer: field("relayer"),
        msg_value: text(v.get("msgValue"), ""), refund_to: field("refundTo"), nft_recipient: field("nftRecipient"), metadata, funding_index,
        calls, funding, endpoint: endpoint.into(), body: body.to_string().into_bytes(), permit,
    })
}

pub fn permit_item(item: &Value) -> Option<Permit> {
    let d = item.get("data")?;
    let sign = d.get("sign").filter(|v| v.is_object())?;
    if s(sign, "primaryType") != Some("ReceiveWithAuthorization") { return None }
    let domain = sign.get("domain").filter(|v| v.is_object())?;
    let v = sign.get("value").filter(|v| v.is_object())?;
    let post = d.get("post").filter(|v| v.is_object())?;
    let endpoint = s(post, "endpoint")?;
    let body = post.get("body").filter(|b| s(b, "kind") == Some("eip3009"))?;
    let nonce = s(v, "nonce").and_then(from_hex)?;
    Some(Permit {
        chain_id: number(domain.get("chainId"))?, token: s(domain, "verifyingContract").unwrap_or("").into(),
        domain_name: s(domain, "name").unwrap_or("").into(), domain_version: s(domain, "version").unwrap_or("").into(),
        from: s(v, "from").unwrap_or("").into(), to: s(v, "to").unwrap_or("").into(), value: text(v.get("value"), ""),
        valid_after: number(v.get("validAfter"))?, valid_before: number(v.get("validBefore"))?, nonce, endpoint: endpoint.into(),
        body: body.to_string().into_bytes(),
    })
}

async fn get_json(url: &str) -> Result<Vec<u8>> {
    net::get(url, &[]).await.map(|(b, _)| b).map_err(|e| WalletError(e.to_string()))
}

/// /chains' entry for `chain_id`.
fn chain_entry(data: &[u8], chain_id: u64) -> Option<Value> {
    let j: Value = serde_json::from_slice(data).ok()?;
    j.get("chains")?.as_array()?.iter().find(|c| c.get("id").and_then(Value::as_u64) == Some(chain_id)).cloned()
}

/// Relay's own depository address on a chain, from /chains (protocol.v2.depository).
pub fn depository_in(data: &[u8], chain_id: u64) -> Option<String> {
    chain_entry(data, chain_id)?.get("protocol")?.get("v2")?.get("depository")?.as_str().map(str::to_string)
}

/// The approval proxy Relay publishes for a chain (contracts.approvalProxy in /chains).
pub fn approval_proxy_in(data: &[u8], chain_id: u64) -> Option<String> {
    chain_entry(data, chain_id)?.get("contracts")?.get("approvalProxy")?.as_str().map(str::to_string)
}

pub async fn depository(chain_id: u64) -> Result<Option<String>> { Ok(depository_in(&get_json(&format!("{BASE}/chains")).await?, chain_id)) }

pub async fn approval_proxy(chain_id: u64) -> Result<Option<String>> {
    Ok(approval_proxy_in(&get_json(&format!("{BASE}/chains")).await?, chain_id))
}

/// v2's `{"price":"2415.813385"}`: a decimal string (v1's JSON number still reads). None on `{"error":"Could not get price"}`.
pub fn price(data: &[u8]) -> Result<Option<Decimal>> {
    let Ok(j) = serde_json::from_slice::<Value>(data) else { return fail("Relay's price isn't JSON.") };
    Ok(dec_value(j.get("price")))
}

/// The chain's own coin (ETH, POL) in USD, as Relay prices it: what a gas top-up of some wei costs.
pub async fn native_price(chain_id: u64) -> Result<Option<Decimal>> {
    price(&get_json(&format!("{BASE}/currencies/token/price/v2?address=0x0000000000000000000000000000000000000000&chainId={chain_id}")).await?)
}

/// Posts one signed item of a step: its body as Relay returned it, the signature as a query parameter (as Relay's SDK).
pub async fn post_signature(endpoint: &str, body: &[u8], signature: &str) -> Result<()> { post_signature_at(BASE, endpoint, body, signature).await }

pub async fn post_signature_at(base: &str, endpoint: &str, body: &[u8], signature: &str) -> Result<()> {
    let (data, status) = post(&format!("{base}{endpoint}?signature={signature}"), body.to_vec(), &[], None).await.map_err(|e| WalletError(e.to_string()))?;
    if (200..300).contains(&status) { Ok(()) } else { Err(error(&data)) }
}

/// Posts the signed nonce mapping that ties a Hyperliquid deposit to the order.
pub async fn authorize(body: &[u8], signature: &str) -> Result<()> { post_signature("/authorize", body, signature).await }

/// Posts a signed permit (`signature`: 0x r‖s‖v); Relay's solver then sends it and fills the order.
pub async fn post_permit(permit: &Permit, signature: &str) -> Result<()> { post_signature(&permit.endpoint, &permit.body, signature).await }

/// "waiting", "pending", "submitted", "success", "delayed", "refund", "failure", and the destination's transactions.
/// Asked while money is on its way: a throttle, a server error or a dropped connection is asked again.
pub async fn status(request_id: &str) -> Result<(String, Vec<String>)> { status_at(BASE, request_id, &net::DELAYS).await }

pub async fn status_at(base: &str, request_id: &str, delays: &[std::time::Duration]) -> Result<(String, Vec<String>)> {
    let url = format!("{base}/intents/status/v3?requestId={request_id}");
    net::run(delays, |_| net::get(&url, &[]), |data, code| {
        let j: Option<Value> = serde_json::from_slice(data).ok();
        let Some(status) = j.as_ref().and_then(|j| s(j, "status")) else { return Err(net::Again::new(code == 429 || code >= 500, error(data).0)) };
        let txs = j.as_ref().and_then(|j| j.get("txHashes")).and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default();
        Ok((status.to_string(), txs))
    }).await
}
