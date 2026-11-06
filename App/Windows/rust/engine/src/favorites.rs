//! Favorite tokens (Favorites.swift): «show token 0x…», «add 0x… to favorites», «remove token AERO from favorites» —
//! the shared TokenCommand and DexScreener's TokenLookup, as on the Mac; the favorites' prices refresh with the balances.
//! A saved address starred in Favorites › Wallets, and the board's one «+» telling a token from a wallet.

use std::collections::{HashMap, HashSet};

use ihatecrypto_core::address_book_command::AddressBookCommand;
use ihatecrypto_core::foundation::{self, Rounding, number, rounded};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::tokens::{LookupInfo, TokenCommand, TokenLookup};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use signer_api::SignerAddress;
use trade::EvmNetwork;
use wallet::address_check::{self, Network};

use crate::canvas::Board;
use crate::engine::Engine;

/// One favorite (favorites.json).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteToken {
    pub chain: String,
    pub address: String,
    pub name: String,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub price: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change24h: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub liquidity: Option<Decimal>,
}

impl FavoriteToken {
    pub fn id(&self) -> String { format!("{}:{}", self.chain, self.address.to_lowercase()) }
}

/// The token a lookup found for `chain` (nil: any), else the deepest one.
fn found(infos: Option<Vec<LookupInfo>>, chain: Option<&str>) -> Option<LookupInfo> {
    let infos = infos?;
    infos.iter().find(|i| chain.is_none_or(|c| i.chain == c)).or(infos.first()).cloned()
}

impl Engine {
    pub fn star_key(e: &SignerAddress) -> String { format!("{}:{}", e.network, e.address.to_lowercase()) }

    /// A saved address starred in Favorites › Wallets (kept here: the address book itself is the signer's). The star on a
    /// row toggles in place, as the Mac's (AddressBook.swift:399-406); the chat's «favorite Mom» brings the block up.
    pub fn star(&self, e: &SignerAddress, on: bool, bring_block: bool) {
        let key = Self::star_key(e);
        self.set_starred(|s| if on { s.insert(key); } else { s.remove(&key); });
        if bring_block { self.pin(Board::new("favorites", "favorites")); } else { self.changed() }
    }

    pub async fn token_command(&self, command: TokenCommand, note: Option<&str>) {
        let note = note.map(str::to_string);
        match command {
            TokenCommand::Show { address, chain } => {
                let Some(info) = found(TokenLookup::lookup(&address).await.ok(), chain.as_deref()) else {
                    let (on, na) = chain.as_ref().map_or_else(Default::default, |c| (format!(" on {c}"), format!(" на {c}")));
                    self.reply(&self.say(&format!("DexScreener doesn’t know this token{on}: no pool trades it, so there’s no price."),
                                         &format!("DexScreener не знает этот токен{na}: ни один пул им не торгует, цены нет.")), note);
                    return;
                };
                self.open_token(&info.address, &info.chain).await;
                let line = self.describe(&info, None) + &self.say(" It’s a contract, not an address to send money to.", " Это контракт, а не адрес для перевода денег.");
                self.reply(&line, Some(note.unwrap_or_else(|| "DexScreener".into())));
            }
            TokenCommand::Unfavorite { symbol } => {
                let before = self.st().favorites.len();
                self.set_favorites(|f| f.retain(|t| t.symbol.to_uppercase() != symbol));
                let fewer = self.st().favorites.len() < before;
                self.reply(&if fewer { self.say(&format!("{symbol} is no longer in your favorites."), &format!("{symbol} убран из избранного.")) }
                           else { self.say(&format!("There’s no {symbol} among your favorites."), &format!("{symbol} нет в избранном.")) }, note);
            }
            TokenCommand::Favorite { address, chain } => {
                let Some(info) = found(TokenLookup::lookup(&address).await.ok(), chain.as_deref()) else {
                    // No pool and not a token the wallet holds: most likely a wallet, so its save goes into the field to be named.
                    let held = self.st().portfolio.networks.iter()
                        .any(|n| n.rows.iter().chain(&n.hidden).any(|r| r.contract.as_ref().is_some_and(|c| c.to_lowercase() == address.to_lowercase())));
                    if !self.is_own_wallet(&address) && !held {
                        // The Mac's line: «… as Name», on Base for an EVM address; a Solana address needs no network.
                        let evm = address.starts_with("0x");
                        let suggest = HashMap::from([
                            ("text", self.say(&format!("save {address} as Name{}", if evm { " on Base" } else { "" }), &format!("сохрани {address} как Имя{}", if evm { " на Base" } else { "" }))),
                            ("select", self.say("Name", "Имя")),
                        ]);
                        self.out.event("suggest", &suggest);
                        self.reply(&self.say(&format!("No pool trades {address}, so it isn’t a token to watch. If it’s a wallet, give it a name — the line is in the field."),
                                             &format!("Ни один пул не торгует {address}, так что это не токен. Если это кошелёк, дай ему имя — строка уже в поле.")), note);
                        return;
                    }
                    let (on, na) = chain.as_ref().map_or_else(Default::default, |c| (format!(" on {c}"), format!(" на {c}")));
                    self.reply(&self.say(&format!("DexScreener doesn’t know this contract{on}: no pool trades it, so there’s no price to watch."),
                                         &format!("DexScreener не знает этот контракт{na}: ни один пул им не торгует, следить не за чем.")), note);
                    return;
                };
                let mut token = FavoriteToken {
                    chain: info.chain.clone(), address: info.address.clone(), name: info.name.clone(), symbol: info.symbol.clone(), image: info.image_url.clone(),
                    price: info.price_usd, change24h: info.change24h, liquidity: info.liquidity_usd,
                };
                // What the contract says about itself wins over any listing (the Mac's TokensUI): a pool can carry a borrowed name.
                if let Some((symbol, name)) = Self::contract_names(&info.address, &info.chain).await { (token.symbol, token.name) = (symbol, name) }
                let line = "⭐ ".to_string() + &self.describe(&info, Some((&token.name, &token.symbol))) + &self.say(" It’s in your favorites now.", " Теперь в избранном.");
                self.set_favorites(|f| {
                    f.retain(|t| t.id() != token.id());
                    f.push(token);
                });
                self.pin(Board::new("favorites", "favorites"));
                self.reply(&line, Some(note.unwrap_or_else(|| "DexScreener".into())));
            }
        }
    }

    /// An EVM token's own `symbol()` and `name()`, read from its contract; None off EVM or when either doesn't answer.
    pub async fn contract_names(address: &str, chain: &str) -> Option<(String, String)> {
        if !address.starts_with("0x") { return None }
        let rpc = &EvmNetwork::named(chain)?.rpc;
        let s = evm::abi::string(&rpc.read(address, &[0x95, 0xd8, 0x9b, 0x41]).await.unwrap_or_default()).filter(|s| !s.is_empty())?;
        let n = evm::abi::string(&rpc.read(address, &[0x06, 0xfd, 0xde, 0x03]).await.unwrap_or_default()).filter(|n| !n.is_empty())?;
        Some((s, n))
    }

    fn describe(&self, info: &LookupInfo, named: Option<(&str, &str)>) -> String {
        let (name, symbol) = named.unwrap_or((&info.name, &info.symbol));
        let mut line = format!("{name} ({symbol}) {} {}: {}", self.say("on", "на"), info.chain, info.price_usd.map_or_else(|| self.say("no price", "нет цены"), Fmt::price));
        if let Some(c) = info.change24h { line += &format!(", {} {}", self.signed(c), self.say("today", "за сутки")) }
        if let Some(l) = info.liquidity_usd {
            line += &format!(", {} {}M", self.say("liquidity", "ликвидность"), Fmt::usd(rounded(l / Decimal::from(1_000_000), 2, Rounding::Plain)));
        }
        if TokenLookup::suspicious(&info.symbol, &info.address, &HashSet::new()) {
            line += &self.say(". ⚠ Its name looks like a well-known coin's but the contract isn't that coin's.", ". ⚠ Имя похоже на известную монету, но контракт не её.");
        }
        line + "."
    }

    /// The board's one «+» (TokensUI.AddFavoriteButton): what a pasted text is — a token DexScreener prices, a wallet
    /// address (with the network it's saved on by default: Base for a 0x one), or not known while DexScreener is silent.
    pub async fn look_token(&self, text: &str) -> HashMap<String, String> {
        let pairs = |p: &[(&str, &str)]| p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>();
        let Some(a) = AddressBookCommand::address(text) else { return pairs(&[("kind", "invalid")]) };
        let evm = a.len() == 42 && a.starts_with("0x") && a[2..].bytes().all(|b| b.is_ascii_hexdigit());
        let network = if evm { "base".to_string() } else {
            Network::ALL.into_iter().find(|&n| address_check::problem(&a, n).is_none())
                .and_then(|n| serde_json::to_value(n).ok()?.as_str().map(str::to_string)).unwrap_or_else(|| "solana".into())
        };
        // Bitcoin, TRON, NEAR… are never a DexScreener token.
        if network != "base" && network != "solana" { return pairs(&[("kind", "address"), ("address", &a), ("network", &network)]) }
        match TokenLookup::lookup(&a).await {
            Ok(infos) => {
                let Some(info) = infos.into_iter().next() else { return pairs(&[("kind", "address"), ("address", &a), ("network", &network)]) };
                let mut out = pairs(&[("kind", "token"), ("address", &info.address), ("chain", &info.chain), ("name", &info.name), ("symbol", &info.symbol)]);
                let extra = [("image", info.image_url), ("price", info.price_usd.map(foundation::text)), ("change24h", info.change24h.map(foundation::text)), ("liquidity", info.liquidity_usd.map(foundation::text))];
                out.extend(extra.into_iter().filter_map(|(k, v)| Some((k.to_string(), v?))));
                out
            }
            Err(_) => pairs(&[("kind", "unknown"), ("address", &a), ("network", &network)]),
        }
    }

    /// The favorites' prices, with the balances (DexScreener, one request each).
    pub async fn refresh_favorites(&self) {
        let mut next = self.st().favorites.clone();
        if next.is_empty() { return }
        for f in next.iter_mut() {
            let info = TokenLookup::lookup(&f.address).await.ok().and_then(|l| l.into_iter().find(|i| i.chain == f.chain));
            if let Some(info) = info {
                f.price = info.price_usd;
                f.change24h = info.change24h;
                f.liquidity = info.liquidity_usd;
            }
        }
        let changed = next != self.st().favorites;
        if changed { self.set_favorites(|f| *f = next) }
    }
}

#[cfg(test)]
#[path = "favorites/tests.rs"]
mod tests;
