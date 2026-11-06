//! What the chat may do with the address book (`AddressBookCommand.swift`). The address is always taken as written in
//! the user's own message — never from a model, a site or a pasted instruction — and saving waits for a confirmation.

use crate::foundation::{all, any_char, first, group, has, is_number, is_space, is_upper, split_chars, trim_set, trim_spaces, whole};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressBookCommand {
    /// Save the address written in the message under a name. `network` nil: Solana for a base58 address; a 0x address
    /// asks.
    Save { address: String, name: String, network: Option<String> },
    Favorite { name: String, on: bool },
}

const EVM: &str = "0x[0-9a-fA-F]{40}";
const SOLANA: &str = "[1-9A-HJ-NP-Za-km-z]{32,44}";

impl AddressBookCommand {
    pub fn parse(text: &str) -> Option<Self> {
        let q = text.to_lowercase();
        // An ENS or SNS name is saved as the address it resolves to (AddressBook's save): the name is what the message holds.
        if let Some(address) = Self::address(text).or_else(|| Self::domain(text))
            && has(&q, r"\b(?:save|add|remember|store|keep)\b|сохран|запомн|добав|запиши")
        {
            // «… as Bybit on Arbitrum», «… как Bybit в сети base»
            let named = first(text, r#"(?i)(?:\bas\b|\bcalled\b|\bnamed\b|\bкак\b|под\s+именем)\s+[«"“]?(.+?)[»"”]?(?=\s+(?:on|in|в|на)\s+(?:the\s+)?(?:сети\s+)?(?:solana|солан\w*|base|бейс\w*|arbitrum|арбитрум\w*|ethereum|эфир\w*|hyperliquid|хайперликвид\w*|near|неар\w*|polygon|полигон\w*|robinhood|робин[гх]уд\w*|tempo|темпо\w*|hyperevm|optimism|bnb|bsc|бнб|bitcoin|биткоин\w*|zcash|tron|трон\w*|xrp|ripple|dogecoin|doge|sui)\b|[.,!]?\s*$)"#);
            let name = named.map(|m| trim_spaces(group(&m, 1)).to_string()).filter(|n| !n.is_empty() && !n.contains(&address))?;
            return Some(Self::Save { address, name, network: Self::network(&q) });
        }
        if let Some(m) = first(text, r"(?i)(?:add|put|star|mark|добавь|отметь)\s+(.+?)\s+(?:to|in|into|as|в)\s+(?:my\s+|the\s+)?(?:favou?rites?|избранн\w*)") {
            return Some(Self::Favorite { name: trim_spaces(group(&m, 1)).into(), on: true });
        }
        if let Some(m) = first(text, r"(?i)(?:remove|unstar|take|убери|удали)\s+(.+?)\s+(?:from|out\s+of|из)\s+(?:my\s+|the\s+)?(?:favou?rites?|избранн\w*)") {
            return Some(Self::Favorite { name: trim_spaces(group(&m, 1)).into(), on: false });
        }
        None
    }

    /// The address as the user wrote it: a 0x one, or a base58 one that mixes cases and digits like real Solana keys.
    pub fn address(text: &str) -> Option<String> {
        let mixed = |s: &str| any_char(s, is_number) && any_char(s, is_upper) && any_char(s, |g| g.chars().next().is_some_and(char::is_lowercase));
        // Sui's 0x and 64 hex before an EVM address's 40; a Bitcoin bc1…; an XRP X-address (tag inside).
        for pattern in [r"\b0x[0-9a-fA-F]{64}\b", EVM, r"(?i)\bbc1[02-9ac-hj-np-z]{8,87}\b", r"\bX[1-9A-HJ-NP-Za-km-z]{46}\b"] {
            if let Some(m) = first(text, pattern) { return Some(group(&m, 0).into()) }
        }
        // Bitcoin's, Dogecoin's, TRON's, Zcash's and XRP's base58 ones are shorter than Solana's.
        if let Some(s) = all(text, r"\b[1-9A-HJ-NP-Za-km-z]{25,35}\b").iter().map(|m| group(m, 0))
            .find(|s| mixed(s) && s.chars().next().is_some_and(|c| "13DT9Atr".contains(c)))
        {
            return Some(s.into());
        }
        // NEAR: a 64-hex implicit account, or a named one ending in .near.
        for pattern in [r"\b[0-9a-f]{64}\b", r"\b[a-z0-9_-]+(?:\.[a-z0-9_-]+)*\.near\b"] {
            if let Some(m) = first(text, pattern) { return Some(group(&m, 0).into()) }
        }
        all(text, SOLANA).iter().map(|m| group(m, 0)).find(|s| mixed(s)).map(Into::into)
    }

    /// An ENS (vitalik.eth) or SNS (toly.sol) name standing alone in the text, lowercased: the whole word must be plain
    /// ASCII letters, digits, hyphens and dots — a look-alike letter («vіtalik.eth» with a Cyrillic і) makes it no name.
    pub fn domain(text: &str) -> Option<String> {
        split_chars(text, |g| is_space(g) || ["«", "»", "\"", "“", "”", ",", ";", "!", "?", "(", ")"].contains(&g))
            .into_iter()
            .map(|w| trim_set(w, ".:").to_lowercase())
            .find(|w| {
                whole(w, r"[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)*\.eth").is_some()
                    || whole(w, r"[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.sol").is_some()
            })
    }

    pub fn network(q: &str) -> Option<String> {
        let on = |p: &'static str| has(q, p);
        // A NEAR account can only be on NEAR; «on near» names it too.
        let named = if on(r"\b[0-9a-f]{64}\b|\.near\b|\b(?:on|in|в|на)\s+(?:the\s+)?(?:сети\s+)?(?:near|неар\w*)\b") {
            Some("near")
        } else if !on(r"\b0x[0-9a-f]{40}\b") && let Some(n) = [
            // The networks only NEAR Intents reaches, named or by the address's shape; never for a 0x one of 40 hex.
            (r"\bbc1[02-9ac-hj-np-z]{8,}|\bbitcoin\b|биткоин|\bbtc\b", "bitcoin"), (r"\bzcash\b|\bzec\b|зкеш|зкэш", "zcash"),
            (r"\btron\b|трон|\btrx\b|trc-?20", "tron"), (r"\bxrp\b|ripple|рипл", "xrp"), (r"\bdoge\b|dogecoin|додж", "dogecoin"),
            (r"\bsui\b|\b0x[0-9a-f]{64}\b", "sui"),
        ].into_iter().find(|(p, _)| on(p)) {
            Some(n.1)
        } else {
            [
                (r"\bbase\b|бейс", "base"), ("arbitrum|арбитрум", "arbitrum"), (r"hyperevm|hyper\s+evm|хайперевм|гиперевм", "hyperevm"),
                ("optimism|оптимизм", "optimism"), (r"\bbnb\b|\bbsc\b|бнб|бсц|binance\s+smart\s+chain", "bnb"),
                ("hyperliquid|хайперликвид", "hyperliquid"), ("polygon|полигон", "polygon"), ("robinhood|робин[гх]уд", "robinhood"),
                (r"\btempo\b|темпо", "tempo"), (r"ethereum|эфир|\beth\s+mainnet", "ethereum"), ("solana|солан", "solana"),
            ].into_iter().find(|(p, _)| on(p)).map(|n| n.1)
        };
        // A name with no network named: an ENS name is an Ethereum address, a .sol name a Solana one.
        named.map(Into::into).or_else(|| Self::domain(q).map(|name| if name.ends_with(".sol") { "solana" } else { "ethereum" }.into()))
    }
}
