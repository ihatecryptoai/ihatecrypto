//! What the assistant may use (`Modules.swift`): networks, kinds of moves and protocols, each switched in Settings ›
//! Modules (networks also from the chat). A protocol works only while it, its kind and every network it needs are on.
//! Off means the assistant never plans a move through it; money already there still shows.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModuleNetwork { Solana, Hyperliquid, Base, Near, Arbitrum, Robinhood, Polygon, Ethereum, Tempo, Hyperevm, Optimism, Bnb }

impl ModuleNetwork {
    pub const ALL: [Self; 12] = [
        Self::Solana, Self::Hyperliquid, Self::Base, Self::Near, Self::Arbitrum, Self::Robinhood, Self::Polygon, Self::Ethereum, Self::Tempo,
        Self::Hyperevm, Self::Optimism, Self::Bnb,
    ];

    pub fn raw(self) -> &'static str {
        match self {
            Self::Solana => "solana", Self::Hyperliquid => "hyperliquid", Self::Base => "base", Self::Near => "near", Self::Arbitrum => "arbitrum",
            Self::Robinhood => "robinhood", Self::Polygon => "polygon", Self::Ethereum => "ethereum", Self::Tempo => "tempo",
            Self::Hyperevm => "hyperevm", Self::Optimism => "optimism", Self::Bnb => "bnb",
        }
    }

    /// `init(rawValue:)`.
    pub fn from_raw(raw: &str) -> Option<Self> { Self::ALL.into_iter().find(|n| n.raw() == raw) }

    pub fn name(self) -> &'static str {
        match self {
            Self::Solana => "Solana", Self::Hyperliquid => "Hyperliquid", Self::Base => "Base", Self::Near => "NEAR", Self::Arbitrum => "Arbitrum",
            Self::Robinhood => "Robinhood", Self::Polygon => "Polygon", Self::Ethereum => "Ethereum", Self::Tempo => "Tempo",
            Self::Hyperevm => "HyperEVM", Self::Optimism => "Optimism", Self::Bnb => "BNB Chain",
        }
    }

    /// A place as commands and the address book name it: «base», «arbitrum», «robinhood», «solana», «hyperliquid»,
    /// «near», «tempo».
    pub fn from_place(place: &str) -> Option<Self> {
        let p = place.to_lowercase();
        Self::ALL.into_iter().find(|n| p.starts_with(n.raw())).or_else(|| p.starts_with("hl").then_some(Self::Hyperliquid))
    }

    /// The line under the network in the chat's menu: what the assistant does there.
    pub fn uses(self) -> &'static str {
        match self {
            Self::Solana => "Jupiter swaps · Jupiter Lend · Kamino · JitoSOL",
            Self::Hyperliquid => "Perps · oil and stocks (xyz) · HLP vault",
            Self::Base => "Aave · Morpho · USDC",
            Self::Near => "NEAR Intents swaps · 1Click",
            Self::Arbitrum => "Aave · Morpho · USDC in and out",
            Self::Robinhood => "Stock tokens · USDG · Uniswap",
            Self::Polygon => "Aave · USDC",
            Self::Ethereum => "Aave · Morpho · mainnet gas varies",
            Self::Tempo => "USDC transfers · pay for services (MPP) · fees in USDC",
            Self::Hyperevm => "USDC in and out · HYPE for gas",
            Self::Optimism => "USDC in and out · Circle's CCTP",
            Self::Bnb => "USDC in and out · BNB for gas · swaps",
        }
    }

    /// The logo's name for TokenLogo.
    pub fn logo(self) -> &'static str {
        match self {
            Self::Solana => "sol", Self::Hyperliquid => "hype", Self::Base => "base", Self::Near => "near", Self::Arbitrum => "arb",
            Self::Robinhood => "robinhood", Self::Polygon => "pol", Self::Ethereum => "eth", Self::Tempo => "tempo", Self::Hyperevm => "hyperevm",
            Self::Optimism => "op", Self::Bnb => "bnb",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleKind { Swaps, Earn, Perps, Stocks, Bridges, Payments }

impl ModuleKind {
    pub const ALL: [Self; 6] = [Self::Swaps, Self::Earn, Self::Perps, Self::Stocks, Self::Bridges, Self::Payments];

    pub fn raw(self) -> &'static str {
        match self {
            Self::Swaps => "swaps", Self::Earn => "earn", Self::Perps => "perps", Self::Stocks => "stocks", Self::Bridges => "bridges",
            Self::Payments => "payments",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> { Self::ALL.into_iter().find(|k| k.raw() == raw) }

    pub fn name(self) -> &'static str {
        match self {
            Self::Swaps => "Swaps", Self::Earn => "Earn · DeFi", Self::Perps => "Perps", Self::Stocks => "Stocks & oil", Self::Bridges => "Bridges",
            Self::Payments => "Payments",
        }
    }

    pub fn line(self) -> &'static str {
        match self {
            Self::Swaps => "Trade one token for another",
            Self::Earn => "Lend and stake idle money",
            Self::Perps => "Long or short with leverage",
            Self::Stocks => "Commodities and tokenized shares",
            Self::Bridges => "Move money between networks",
            Self::Payments => "Pay for services per request",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleProtocol {
    Jupiter, JupiterDca, Uniswap, Kyberswap, JupiterLend, Kamino, JitoSol, Aave, Morpho, Hlp, HlPerps, Xyz, RobinhoodStocks, Relay, OneClick, Mpp,
}

impl ModuleProtocol {
    pub const ALL: [Self; 16] = [
        Self::Jupiter, Self::JupiterDca, Self::Uniswap, Self::Kyberswap, Self::JupiterLend, Self::Kamino, Self::JitoSol, Self::Aave, Self::Morpho,
        Self::Hlp, Self::HlPerps, Self::Xyz, Self::RobinhoodStocks, Self::Relay, Self::OneClick, Self::Mpp,
    ];

    pub fn raw(self) -> &'static str {
        match self {
            Self::Jupiter => "jupiter", Self::JupiterDca => "jupiterDCA", Self::Uniswap => "uniswap", Self::Kyberswap => "kyberswap",
            Self::JupiterLend => "jupiterLend", Self::Kamino => "kamino", Self::JitoSol => "jitoSOL", Self::Aave => "aave", Self::Morpho => "morpho",
            Self::Hlp => "hlp", Self::HlPerps => "hlPerps", Self::Xyz => "xyz", Self::RobinhoodStocks => "robinhoodStocks", Self::Relay => "relay",
            Self::OneClick => "oneClick", Self::Mpp => "mpp",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> { Self::ALL.into_iter().find(|p| p.raw() == raw) }

    pub fn kind(self) -> ModuleKind {
        match self {
            Self::Jupiter | Self::JupiterDca | Self::Uniswap | Self::Kyberswap => ModuleKind::Swaps,
            Self::JupiterLend | Self::Kamino | Self::JitoSol | Self::Aave | Self::Morpho | Self::Hlp => ModuleKind::Earn,
            Self::HlPerps => ModuleKind::Perps,
            Self::Xyz | Self::RobinhoodStocks => ModuleKind::Stocks,
            Self::Relay | Self::OneClick => ModuleKind::Bridges,
            Self::Mpp => ModuleKind::Payments,
        }
    }

    /// The networks it can't work without. Relay's other end is checked per move (`Modules::refusal`'s `via`).
    pub fn networks(self) -> &'static [ModuleNetwork] {
        match self {
            Self::Jupiter | Self::JupiterDca | Self::JupiterLend | Self::Kamino | Self::JitoSol | Self::Relay => &[ModuleNetwork::Solana],
            // Uniswap, KyberSwap, Aave and Morpho work on several networks: the one a move uses is checked per move.
            Self::Uniswap | Self::Kyberswap | Self::Aave | Self::Morpho => &[],
            Self::RobinhoodStocks => &[ModuleNetwork::Robinhood],
            Self::Hlp | Self::HlPerps | Self::Xyz => &[ModuleNetwork::Hyperliquid],
            Self::OneClick => &[ModuleNetwork::Near],
            Self::Mpp => &[ModuleNetwork::Tempo],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Jupiter => "Jupiter", Self::JupiterDca => "Jupiter DCA", Self::Uniswap => "Uniswap", Self::Kyberswap => "KyberSwap",
            Self::JupiterLend => "Jupiter Lend", Self::Kamino => "Kamino", Self::JitoSol => "JitoSOL staking", Self::Aave => "Aave v3",
            Self::Morpho => "Morpho", Self::Hlp => "HLP vault", Self::HlPerps => "Hyperliquid", Self::Xyz => "Hyperliquid xyz",
            Self::RobinhoodStocks => "Robinhood Stock Tokens", Self::Relay => "Relay", Self::OneClick => "NEAR Intents · 1Click", Self::Mpp => "MPP · x402",
        }
    }

    /// The line under the name in Settings.
    pub fn place(self) -> String {
        match self {
            Self::HlPerps => "BTC, ETH, SOL and more",
            Self::JupiterDca => "Solana · on-chain buys over time, from 100 USDC",
            Self::Xyz => "Oil, metals, stocks",
            Self::Relay => "Solana ⇄ Base · Arbitrum · Polygon · Ethereum · Robinhood · Tempo · HyperEVM · Optimism · BNB Chain",
            Self::Uniswap => "Base · Arbitrum · Polygon · Ethereum · Optimism · BNB Chain · Robinhood",
            Self::Kyberswap => "Base · Arbitrum · Polygon · Ethereum · Optimism · BNB Chain · HyperEVM",
            Self::Aave => "Base · Arbitrum · Polygon · Ethereum",
            Self::Morpho => "Base · Arbitrum · Ethereum",
            Self::OneClick => "NEAR ⇄ Solana",
            Self::Mpp => "APIs, email, calls, letters · USDC on Tempo",
            _ => return self.networks().iter().map(|n| n.name()).collect::<Vec<_>>().join(" · "),
        }
        .into()
    }

    /// The logo in Logos/.
    pub fn logo(self) -> &'static str {
        match self {
            Self::Jupiter | Self::JupiterDca | Self::JupiterLend => "jup",
            Self::Uniswap => "uni",
            Self::Kyberswap => "kyber",
            Self::Kamino => "kamino",
            Self::JitoSol => "jitosol",
            Self::Aave => "aave",
            Self::Morpho => "morpho",
            Self::Hlp | Self::HlPerps | Self::Xyz => "hype",
            Self::RobinhoodStocks => "robinhood",
            Self::Relay => "relay",
            Self::OneClick => "near",
            Self::Mpp => "tempo",
        }
    }
}

/// A switch in Settings › Modules: a network, a kind or a protocol, by its stored id («network.base»).
pub trait Switch: Copy {
    fn id(self) -> String;
}

impl Switch for ModuleNetwork {
    fn id(self) -> String { format!("network.{}", self.raw()) }
}

impl Switch for ModuleKind {
    fn id(self) -> String { format!("kind.{}", self.raw()) }
}

impl Switch for ModuleProtocol {
    fn id(self) -> String { format!("protocol.{}", self.raw()) }
}

/// The switches as the user left them; anything never touched keeps its default.
// Not here: `Modules.current` and `storedKnown` read the Mac's UserDefaults; the engine keeps the stored form
// (`Modules::KEY`, `Modules::KNOWN_KEY`) and hands it in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Modules {
    /// id → on, for the switches the user touched. Ids: "network.base", "kind.perps", "protocol.kamino".
    pub choices: HashMap<String, bool>,
    /// The modules this computer has met (`Modules::KNOWN_KEY`): one outside it came with a later version and starts
    /// switched off until the user says. None: nothing recorded yet, every module as it was.
    pub known: Option<HashSet<String>>,
}

impl Modules {
    pub const KNOWN_KEY: &str = "modulesKnown";
    pub const KEY: &str = "modules";

    pub fn new(choices: HashMap<String, bool>, known: Option<HashSet<String>>) -> Self { Self { choices, known } }

    /// Every module's id, as the switches store them.
    pub fn all_ids() -> Vec<String> {
        ModuleNetwork::ALL.into_iter().map(Switch::id)
            .chain(ModuleKind::ALL.into_iter().map(Switch::id))
            .chain(ModuleProtocol::ALL.into_iter().map(Switch::id))
            .collect()
    }

    /// Came with this version and not yet answered: off until the user turns it on.
    pub fn is_new(&self, id: &str) -> bool { !self.choices.contains_key(id) && self.known.as_ref().is_some_and(|k| !k.contains(id)) }

    /// The new modules waiting for the user's word.
    pub fn new_ids(&self) -> Vec<String> { Self::all_ids().into_iter().filter(|id| self.is_new(id)).collect() }

    /// «protocol.kamino=0,network.arbitrum=1»: the form kept under `Modules::KEY`; what it can't read is skipped.
    pub fn from_stored(stored: &str, known: Option<HashSet<String>>) -> Self {
        let mut choices = HashMap::new();
        for pair in stored.split(',').filter(|s| !s.is_empty()) {
            let kv: Vec<&str> = pair.split('=').filter(|s| !s.is_empty()).collect();
            if kv.len() == 2 { choices.insert(kv[0].to_string(), kv[1] == "1"); }
        }
        Self { choices, known }
    }

    pub fn stored(&self) -> String {
        let mut pairs: Vec<(&String, &bool)> = self.choices.iter().collect();
        pairs.sort();
        pairs.iter().map(|(k, v)| format!("{k}={}", u8::from(**v))).collect::<Vec<_>>().join(",")
    }

    /// The switch's own state, whatever a protocol's kind and networks say.
    pub fn is_on(&self, s: impl Switch) -> bool {
        let id = s.id();
        self.choices.get(&id).copied().unwrap_or_else(|| !self.is_new(&id))
    }

    pub fn set(&mut self, s: impl Switch, on: bool) { self.choices.insert(s.id(), on); }

    /// A module's name for the question, by its id.
    pub fn name(id: &str) -> String {
        // `split(separator: ".", maxSplits: 1)`: empty pieces skipped.
        let Some((head, rest)) = id.trim_start_matches('.').split_once('.').filter(|(_, rest)| !rest.is_empty()) else { return id.into() };
        match head {
            "network" => ModuleNetwork::from_raw(rest).map(|n| n.name()),
            "kind" => ModuleKind::from_raw(rest).map(|k| k.name()),
            _ => ModuleProtocol::from_raw(rest).map(|p| p.name()),
        }
        .unwrap_or(rest)
        .into()
    }

    pub fn allows(&self, p: ModuleProtocol) -> bool { self.refusal(p, &[], false).is_none() }

    /// Why a move through `p` (and across `via`, e.g. the other end of a bridge) can't be planned; None when it can.
    pub fn refusal(&self, p: ModuleProtocol, via: &[ModuleNetwork], ru: bool) -> Option<String> {
        if !self.is_on(p) {
            return Some(if ru { format!("{} выключен в Settings › Modules.", p.name()) } else { format!("{} is off in Settings › Modules.", p.name()) });
        }
        if !self.is_on(p.kind()) {
            return Some(if ru {
                format!("{} выключены в Settings › Modules, а с ними и {}.", p.kind().name(), p.name())
            } else {
                format!("{} is off in Settings › Modules, so {} is too.", p.kind().name(), p.name())
            });
        }
        p.networks().iter().chain(via).find(|&&n| !self.is_on(n)).and_then(|&n| self.refusal_network(n, ru))
    }

    /// A move on a network with no protocol of its own (a plain transfer).
    pub fn refusal_network(&self, n: ModuleNetwork, ru: bool) -> Option<String> {
        if self.is_on(n) { return None }
        Some(if ru { format!("{} выключена в Settings › Modules.", n.name()) } else { format!("{} is off in Settings › Modules.", n.name()) })
    }

    // pending: `refusal(_ command: WalletCommand, ru:)` (round 2): it matches every WalletCommand case, and round 1's
    // WalletCommand has two of them.

    /// A vault by the name the yield planners give it («Kamino Lend», «Aave · Arbitrum»), with the bridge it needs.
    /// A name it doesn't know is refused: nothing goes to a place no switch covers.
    pub fn refusal_vault(&self, name: &str) -> Option<String> {
        match name {
            "Jupiter Lend" => self.refusal(ModuleProtocol::JupiterLend, &[], false),
            "Kamino Lend" => self.refusal(ModuleProtocol::Kamino, &[], false),
            "JitoSOL staking" => self.refusal(ModuleProtocol::JitoSol, &[], false),
            "HLP (Hyperliquid)" => {
                self.refusal(ModuleProtocol::Hlp, &[], false).or_else(|| self.refusal(ModuleProtocol::Relay, &[ModuleNetwork::Hyperliquid], false))
            }
            _ => {
                // «Aave · Polygon», «Morpho · Ethereum»: the protocol, then its network and the bridge there.
                let parts: Vec<&str> = name.split(" · ").collect();
                let protocol = match parts.first() { Some(&"Aave") => Some(ModuleProtocol::Aave), Some(&"Morpho") => Some(ModuleProtocol::Morpho), _ => None };
                let network = parts.get(1).and_then(|n| ModuleNetwork::ALL.into_iter().find(|m| m.name() == *n));
                match (parts.len(), protocol, network) {
                    (2, Some(p), Some(n)) => self.refusal(p, &[], false).or_else(|| self.refusal(ModuleProtocol::Relay, &[n], false)),
                    _ => Some(format!("{name} isn't a place ihatecrypto knows.")),
                }
            }
        }
    }

    /// A Hyperliquid trade: perps, or an xyz market (oil, stocks); `bridges` when USDC comes over from Solana first.
    pub fn refusal_trade(&self, xyz: bool, bridges: bool, ru: bool) -> Option<String> {
        self.refusal(if xyz { ModuleProtocol::Xyz } else { ModuleProtocol::HlPerps }, &[], ru)
            .or_else(|| if bridges { self.refusal(ModuleProtocol::Relay, &[ModuleNetwork::Hyperliquid], ru) } else { None })
    }

    /// For the model's brief: what it must not offer. Empty when everything is on.
    pub fn off_summary(&self) -> String {
        let off: Vec<&str> = ModuleNetwork::ALL.into_iter().filter(|&n| !self.is_on(n)).map(|n| n.name())
            .chain(ModuleKind::ALL.into_iter().filter(|&k| !self.is_on(k)).map(|k| k.name()))
            .chain(ModuleProtocol::ALL.into_iter().filter(|&p| !self.is_on(p)).map(|p| p.name()))
            .collect();
        off.join(", ")
    }
}
