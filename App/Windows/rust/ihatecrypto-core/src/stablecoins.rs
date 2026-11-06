//! The five biggest dollar stablecoins (`Stablecoins.swift`) — USDT ($184B), USDC ($74B), USDS ($7.1B), USDe ($4.9B),
//! DAI ($4.8B) by DefiLlama on 2026-10-08 — where each really is on a network ihatecrypto has a wallet on. Addresses,
//! symbols, decimals and supply read on-chain 2026-10-08; a deployment with under ≈ $1M out (USDS and USDe on
//! Optimism, DAI on Solana) is left out.

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::position_command::WalletCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coin {
    /// The ticker people use: USDT for USD₮0 too.
    pub symbol: &'static str,
    /// The contract, the Solana mint, the NEAR token account, or Hyperliquid's spot name.
    pub address: &'static str,
    pub decimals: u32,
    /// The other tickers it answers to («USDT0», «USD₮0»).
    pub also: &'static [&'static str],
}

const fn coin(symbol: &'static str, address: &'static str, decimals: u32) -> Coin { Coin { symbol, address, decimals, also: &[] } }

const fn also(symbol: &'static str, address: &'static str, decimals: u32, also: &'static [&'static str]) -> Coin {
    Coin { symbol, address, decimals, also }
}

pub const TOP: [&str; 5] = ["USDT", "USDC", "USDS", "USDe", "DAI"];

/// The same address on every EVM network Ethena is on.
const USDE: &str = "0x5d3a1ff2b6bab83b63cd9ad0787074081a52ef34";
const USDT0: &[&str] = &["USDT0", "USD₮0"];

/// Each network's coins, by its key.
pub const BY_NETWORK: [(&str, &[Coin]); 11] = [
    ("solana", &[coin("USDC", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", 6), coin("USDT", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", 6),
                 coin("USDS", "USDSwr9ApdHk5bvJKMjzff41FfuX8bSxdKcR81vTwcA", 6), coin("USDe", "DEkqHyPN7GMRJ5cArtQFAWefqbZb33Hyf6s5iCwjEonT", 9)]),
    ("ethereum", &[coin("USDC", "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", 6), coin("USDT", "0xdac17f958d2ee523a2206206994597c13d831ec7", 6),
                   coin("USDS", "0xdc035d45d973e3ec169d2276ddab16f1e407384f", 18), coin("USDe", "0x4c9edd5852cd905f086c759e8383e09bff1e68b3", 18),
                   coin("DAI", "0x6b175474e89094c44da98b954eedeac495271d0f", 18)]),
    ("base", &[coin("USDC", "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", 6), coin("USDT", "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2", 6),
               coin("USDS", "0x820c137fa70c8691f0e44dc420a5e53c168921dc", 18), coin("USDe", USDE, 18),
               coin("DAI", "0x50c5725949a6f0c72e6c4a641f24049a917db0cb", 18)]),
    ("arbitrum", &[coin("USDC", "0xaf88d065e77c8cc2239327c5edb3a432268e5831", 6), also("USDT", "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9", 6, USDT0),
                   coin("USDS", "0x6491c05a82219b8d1479057361ff1654749b876b", 18), coin("USDe", USDE, 18),
                   coin("DAI", "0xda10009cbd5d07dd0cecc66161fc93d7c9000da1", 18)]),
    ("optimism", &[coin("USDC", "0x0b2c639c533813f4aa9d7837caf62653d097ff85", 6), coin("USDT", "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58", 6),
                   also("USDT0", "0x01bff41798a0bcf287b996046ca68b395dbc1071", 6, &["USD₮0"]), coin("DAI", "0xda10009cbd5d07dd0cecc66161fc93d7c9000da1", 18)]),
    ("polygon", &[coin("USDC", "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359", 6), also("USDT", "0xc2132d05d31c914a87c6611c10748aeb04b58e8f", 6, USDT0),
                  coin("DAI", "0x8f3cf7ad23cd3cadbd9735aff958023239c6a063", 18)]),
    // Binance-Peg: 18 decimals, USDC too.
    ("bnb", &[coin("USDC", "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d", 18), coin("USDT", "0x55d398326f99059ff775485246999027b3197955", 18),
              coin("USDe", USDE, 18), coin("DAI", "0x1af3f329e8be154074d8769d1ffa4ee058b1dbc3", 18)]),
    ("hyperevm", &[coin("USDC", "0xb88339cb7199b77e23db6e890353e22632ba630f", 6),
                   also("USDT0", "0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb", 6, &["USDT", "USD₮0"]), coin("USDe", USDE, 18)]),
    ("tempo", &[coin("USDC", "0x20c000000000000000000000b9537d11c60e8b50", 6),
                also("USDT0", "0x20c00000000000000000000014f22ca97301eb73", 6, &["USDT", "USD₮0"])]),
    ("hyperliquid", &[coin("USDC", "USDC", 8), also("USDT0", "USDT0", 8, &["USDT", "USD₮0"]), also("USDE", "USDE", 8, &["USDe"])]),
    ("near", &[coin("USDC", "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1", 6), coin("USDT", "usdt.tether-token.near", 6)]),
];

/// The networks Tether's own bridge (USDT0 on LayerZero) moves USDT between: EVMChain.USDT0.deployments by name.
pub const USDT0_NETWORKS: [&str; 5] = ["ethereum", "arbitrum", "optimism", "polygon", "hyperevm"];

/// The coins on a network ihatecrypto knows.
pub fn on(network: &str) -> Option<&'static [Coin]> { BY_NETWORK.iter().find(|(k, _)| *k == network).map(|(_, c)| *c) }

/// The coin a ticker names on a network: «usdt» is USD₮0 on Arbitrum and Polygon, USDT0 on HyperEVM and Hyperliquid.
pub fn find(ticker: &str, network: &str) -> Option<&'static Coin> {
    let want = ticker.trim_matches(['$', ' ']).to_uppercase();
    on(network)?.iter().find(|c| c.symbol.to_uppercase() == want || c.also.iter().any(|a| a.to_uppercase() == want))
}

/// Every dollar counted at $1: the five, their other tickers, and the networks' own (USDG, pathUSD, Tempo's OUSD).
pub static DOLLARS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    BY_NETWORK.iter().flat_map(|(_, coins)| coins.iter()).flat_map(|c| std::iter::once(&c.symbol).chain(c.also))
        .map(|s| s.to_uppercase())
        .chain(["USDG", "PATHUSD", "OUSD", "USDC.E", "USDBC", "PYUSD", "USD1"].map(String::from))
        .collect()
});

/// Whether a held token is one of the five on its own contract (or Hyperliquid spot name) there — not a lookalike.
pub fn is_genuine(contract: &str, network: &str) -> bool {
    on(network).is_some_and(|coins| coins.iter().any(|c| c.address.to_lowercase() == contract.to_lowercase()))
}

/// Whether a ticker is a dollar stablecoin ihatecrypto counts at $1.
pub fn is_dollar(ticker: &str) -> bool { DOLLARS.contains(&ticker.to_uppercase()) }

/// Where each is, for the model's brief and the texts: «Solana: USDC, USDT, USDS, USDe · Ethereum: …».
pub fn brief() -> String {
    ["solana", "ethereum", "base", "arbitrum", "optimism", "polygon", "bnb", "hyperevm", "hyperliquid", "tempo", "near"].iter()
        .filter_map(|&k| {
            let title = match k { "hyperliquid" => "Hyperliquid spot".into(), "solana" => "Solana".into(), _ => WalletCommand::place_name(k) };
            on(k).map(|coins| format!("{title}: {}", coins.iter().map(|c| c.symbol).collect::<Vec<_>>().join(", ")))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The networks a stablecoin is on, as ihatecrypto knows them: for the texts.
pub fn networks(symbol: &str) -> Vec<&'static str> {
    let mut keys: Vec<&str> = BY_NETWORK.iter().map(|(k, _)| *k).collect();
    keys.sort();
    keys.into_iter().filter(|k| find(symbol, k).is_some()).collect()
}
