//! Settings › General › Your own nodes and the NEAR Intents key, as on the Mac (Nodes.swift). A node's address carries
//! the provider's key and the 1Click key is a partner token, so both are kept encrypted to the Windows user (DPAPI, as
//! the model keys, nodes.bin), put to use at launch, and only a node's host ever goes to the window.

use std::collections::HashMap;

use net::evm_rpc::{self, EvmRpc};
use net::solana_rpc::{self, SolanaRpc};
use protocols::one_click;
use serde::Serialize;
use wallet::{Result, WalletError};
use zeroize::Zeroizing;

use crate::engine::Engine;
use crate::store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeNetwork {
    Solana,
    Base,
    Arbitrum,
    Ethereum,
    Polygon,
    Robinhood,
}

impl NodeNetwork {
    pub const ALL: [NodeNetwork; 6] = [Self::Solana, Self::Base, Self::Arbitrum, Self::Ethereum, Self::Polygon, Self::Robinhood];

    pub fn raw(self) -> &'static str {
        match self {
            Self::Solana => "solana", Self::Base => "base", Self::Arbitrum => "arbitrum", Self::Ethereum => "ethereum", Self::Polygon => "polygon",
            Self::Robinhood => "robinhood",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> { Self::ALL.into_iter().find(|n| n.raw() == raw) }

    pub fn title(self) -> &'static str {
        match self {
            Self::Solana => "Solana", Self::Base => "Base", Self::Arbitrum => "Arbitrum", Self::Ethereum => "Ethereum", Self::Polygon => "Polygon",
            Self::Robinhood => "Robinhood",
        }
    }

    pub fn logo(self) -> &'static str {
        match self { Self::Solana => "sol", Self::Arbitrum => "arb", Self::Ethereum => "eth", Self::Polygon => "pol", n => n.raw() }
    }

    pub fn chain_id(self) -> Option<u64> {
        match self {
            Self::Base => Some(8453), Self::Arbitrum => Some(42161), Self::Ethereum => Some(1), Self::Polygon => Some(137), Self::Robinhood => Some(4663),
            Self::Solana => None,
        }
    }

    /// The person's own node in use now.
    pub fn custom(self) -> Option<String> { self.chain_id().map_or_else(solana_rpc::custom, evm_rpc::custom) }

    pub fn use_node(self, url: Option<String>) {
        match self.chain_id() { Some(id) => evm_rpc::set_custom(url, id), None => solana_rpc::set_custom(url) }
    }

    pub async fn problem(self, url: &str) -> Option<String> {
        match self.chain_id() { Some(id) => EvmRpc::problem(url, id).await, None => SolanaRpc::problem(url).await }
    }

    /// Who gives one for free, and what its address looks like: (name, site, example).
    pub fn provider(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Solana => ("Helius", "https://www.helius.dev", "https://mainnet.helius-rpc.com/?api-key=…"),
            Self::Base => ("Alchemy", "https://www.alchemy.com", "https://base-mainnet.g.alchemy.com/v2/…"),
            Self::Arbitrum => ("Alchemy", "https://www.alchemy.com", "https://arb-mainnet.g.alchemy.com/v2/…"),
            Self::Ethereum => ("Alchemy", "https://www.alchemy.com", "https://eth-mainnet.g.alchemy.com/v2/…"),
            Self::Polygon => ("Alchemy", "https://www.alchemy.com", "https://polygon-mainnet.g.alchemy.com/v2/…"),
            Self::Robinhood => ("Alchemy", "https://www.alchemy.com", "https://…"),
        }
    }
}

/// What Settings draws: each network's own node by its host, and whether a 1Click key is kept.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodesView {
    pub nodes: Vec<NodeView>,
    pub one_click_key: bool,
}

/// `NodesView.Node`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NodeView {
    pub id: String,
    pub title: String,
    pub logo: String,
    pub provider: String,
    pub site: String,
    pub example: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// An address's host, as `URL(string:)?.host` reads it.
pub fn host(url: &str) -> Option<String> { reqwest::Url::parse(url).ok()?.host_str().map(str::to_string) }

/// The secrets, one DPAPI blob: «solana», «base»… → a node's address; «1click» → the partner key.
pub fn node_secrets() -> HashMap<String, String> {
    let Some(plain) = std::fs::read(store::url("nodes.bin")).ok().and_then(|blob| store::unprotect(&blob)) else { return HashMap::new() };
    serde_json::from_slice(&plain).unwrap_or_default()
}

pub fn set_node_secret(value: Option<String>, name: &str) {
    let mut all = node_secrets();
    match value { Some(v) => { all.insert(name.into(), v); } None => { all.remove(name); } }
    let Ok(plain) = serde_json::to_vec(&all).map(Zeroizing::new) else { return };
    if let Some(blob) = store::protect(&plain) { let _ = store::write("nodes.bin", &blob); }
}

/// At launch: each saved node is the one its network asks first, and 1Click's partner key goes on its requests.
pub fn apply_nodes() {
    let all = node_secrets();
    for net in NodeNetwork::ALL { net.use_node(all.get(net.raw()).filter(|u| host(u).is_some()).cloned()) }
    one_click::set_key(all.get("1click").cloned());
}

impl Engine {
    pub fn nodes_view(&self) -> NodesView {
        NodesView {
            nodes: NodeNetwork::ALL.into_iter().map(|n| {
                let (provider, site, example) = n.provider();
                NodeView { id: n.raw().into(), title: n.title().into(), logo: n.logo().into(), provider: provider.into(), site: site.into(),
                           example: example.into(), host: n.custom().and_then(|u| host(&u)) }
            }).collect(),
            one_click_key: one_click::key().is_some(),
        }
    }

    /// A node of the person's own, checked first (https, the right network, the calls balances and moves need); an empty
    /// address takes it away, back to the built-in ones.
    pub async fn set_node(&self, network: Option<&str>, address: Option<&str>) -> Result<()> {
        let Some(net) = network.and_then(NodeNetwork::from_raw) else {
            return Err(WalletError(format!("Unknown network «{}».", network.unwrap_or(""))));
        };
        let text = address.unwrap_or("").trim().to_string();
        if text.is_empty() {
            set_node_secret(None, net.raw());
            net.use_node(None);
            return Ok(());
        }
        if host(&text).is_none() { return Err(WalletError(self.say("That isn't an address.", "Это не адрес."))) }
        if let Some(problem) = net.problem(&text).await { return Err(WalletError(problem)) }
        set_node_secret(Some(text.clone()), net.raw());
        net.use_node(Some(text));
        Ok(())
    }

    /// The NEAR Intents partner key, checked against 1Click first; empty removes it.
    pub async fn set_one_click_key(&self, key: Option<&str>) -> Result<()> {
        let token = key.unwrap_or("").trim().to_string();
        if token.is_empty() {
            set_node_secret(None, "1click");
            one_click::set_key(None);
            return Ok(());
        }
        if let Some(problem) = one_click::problem_with_key(&token).await { return Err(WalletError(problem)) }
        set_node_secret(Some(token.clone()), "1click");
        one_click::set_key(Some(token));
        Ok(())
    }
}
