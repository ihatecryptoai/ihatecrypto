//! `Blocks.swift` (`BlockCatalog`): the blocks the agent builds a request from: each is one of ChatRouter.forms, drawn as
//! the route card it becomes. «Add block» on the canvas lists them; a pick puts its example into the chat to finish and
//! send. The `automatic` ones the agent adds to every trade by itself (the rules, Jev, the result): they're shown, not
//! picked.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub group: &'static str,
    /// The route card's label; its first word picks the drawing (StepKind).
    pub label: &'static str,
    pub title: &'static str,
    /// The router's form, as the card says it.
    pub form: &'static str,
    /// What a pick puts into the chat. Every example parses (tests/blocks.rs).
    pub example: &'static str,
    /// A RouteFill's raw value: the card's colour.
    pub fill: &'static str,
    pub logo: Option<&'static str>,
    pub automatic: bool,
}

impl Block {
    /// `Identifiable`: the label.
    pub fn id(&self) -> &'static str { self.label }
}

#[allow(clippy::too_many_arguments)]
const fn block(group: &'static str, label: &'static str, title: &'static str, form: &'static str, example: &'static str, fill: &'static str,
               logo: Option<&'static str>) -> Block {
    Block { group, label, title, form, example, fill, logo, automatic: false }
}

/// One the agent adds by itself: no example, the check's colour, no logo.
const fn automatic(group: &'static str, label: &'static str, title: &'static str, form: &'static str) -> Block {
    Block { group, label, title, form, example: "", fill: "check", logo: None, automatic: true }
}

pub const GROUPS: [&str; 9] = ["Trade", "Triggers", "Move money", "Buy and sell", "Earn", "Tempo", "HyperEVM", "Optimism", "BNB Chain"];

pub static ALL: &[Block] = &[
    block("Trade", "OPEN LONG · HYPERLIQUID", "Long or short", "long|short ASSET · risk $N · stop N% · take N% · leverage Nx",
          "long SOL by the book", "hyperliquid", Some("hype")),
    block("Trade", "OPEN LONG · TWAP", "Buy over time", "long|short ASSET N USDC over N min|h · randomize · from $100",
          "long ETH 200 USDC over 2 h leverage 1x", "hyperliquid", Some("hype")),
    block("Trade", "PROTECT", "Stop and take", "take NR · take in parts · trailing N% · move stop to breakeven",
          "take in parts", "hyperliquid", Some("hype")),
    automatic("Trade", "CHECK RULES", "Risk rules", "risk ≤ 2% of capital · reward/risk ≥ 1.5 · leverage ≤ 5×"),
    automatic("Trade", "SECOND CHECK", "Second check", "the plan against your words, then every signature"),
    automatic("Trade", "RESULT · ESTIMATE", "Loss or profit", "at the stop and at the take, in $ and % of capital"),

    block("Triggers", "WHEN · PRICE FALLS TO", "Price alert", "when ASSET is above|below PRICE: a trade or a move",
          "alert me when SOL is below 115", "check", Some("sol")),
    block("Triggers", "LATER · SCHEDULE", "Later or on repeat", "in N days: … · at HH:MM: … · every week: … · every month on the 1st: …",
          "in 1 week: rebalance USDC", "check", None),

    block("Move money", "BRIDGE · RELAY", "Between networks",
          "move N USDC between Solana, Base, Arbitrum, Polygon, Ethereum, Robinhood, Tempo, HyperEVM, Optimism, BNB Chain — any two, straight",
          "move 5 USDC from Solana to Base", "relay", Some("relay")),
    block("Move money", "GAS · TOP UP", "Gas on any network", "add gas on Base|Arbitrum|Polygon|Ethereum|Robinhood for N USDC · moves top it up by themselves",
          "add gas on robinhood for 1 USDC", "robinhood", Some("eth")),
    block("Move money", "FROM · HYPERLIQUID", "Back to Solana", "withdraw N USDC to Solana",
          "withdraw 1 USDC to Solana", "hyperliquid", Some("hype")),
    block("Move money", "TO · ADDRESS BOOK", "Send", "send N USDC|SOL|NEAR to NAME from the address book",
          "send 1 USDC to Bybit", "solana", Some("usdc")),
    block("Move money", "ANY TOKEN · AS IT IS", "Send a token", "send N|all TICKER|0xCONTRACT|MINT on NETWORK to NAME from the address book",
          "send all RAY on solana to Bybit", "solana", Some("sol")),

    block("Buy and sell", "SWAP · JUPITER", "Tokens on Solana", "buy|sell SOL, BTC, ETH, JUP, BONK… for N USDC",
          "buy SOL for 2 USDC", "jupiter", Some("jup")),
    block("Buy and sell", "EVERY DAY · JUPITER DCA", "DCA on Jupiter",
          "Jupiter DCA N USDC into SOL, BTC, JUP, BONK… in K orders every day|week · on-chain, from 100 USDC, 50 an order",
          "Jupiter DCA 100 USDC into SOL in 2 orders every day", "jupiter", Some("jup")),
    block("Buy and sell", "BUY · ON BASE", "Base and Arbitrum", "buy TICKER on base|arbitrum for N USDC · Relay or Uniswap, the better",
          "buy AERO on base for 2 USDC", "base", Some("base")),
    block("Buy and sell", "SWEEP · DUST", "Dust into USDC", "sweep dust [under $N] · leftovers on Base, Arbitrum, Polygon",
          "sweep dust under $5", "base", Some("usdc")),
    block("Buy and sell", "SWAP · UNISWAP", "Token for token", "swap N ETH|TICKER for TICKER on base|arbitrum|ethereum|polygon · straight, no USDC in the middle",
          "swap 0.001 ETH for AERO on base", "base", Some("uni")),
    block("Buy and sell", "SWAP · STABLECOINS", "Dollar for dollar",
          "USDT, USDC, USDS, USDe, DAI on every network they're on · Uniswap, Jupiter, Relay or Hyperliquid, whichever gives more",
          "buy USDe on base for 2 USDC from base", "base", Some("usdc")),
    block("Buy and sell", "BUY · ROBINHOOD CHAIN", "Stock tokens", "buy TSLA, NVDA, SpaceX… stock for N USDC",
          "buy TSLA stock for 2 USDC", "robinhood", Some("robinhood")),
    block("Buy and sell", "SWAP · NEAR INTENTS", "NEAR", "buy NEAR for N USDC · sell NEAR",
          "buy NEAR for 2 USDC", "near", Some("near")),

    block("Earn", "INTO · JUPITER LEND", "Lend on Solana", "lend N USDC|SOL · withdraw N USDC from the vault",
          "lend 1 USDC", "jupiter", Some("jup")),
    block("Earn", "INTO · KAMINO", "Kamino", "deposit N USDC|SOL to kamino · withdraw [N] USDC from kamino",
          "deposit 1 USDC to kamino", "kamino", Some("kamino")),
    block("Earn", "INTO · AAVE", "Aave", "lend N USDC on Aave on Base|Arbitrum|Polygon|Ethereum",
          "lend 2 USDC on Aave", "aave", Some("aave")),
    block("Earn", "INTO · MORPHO", "Morpho", "deposit N USDC into Morpho on Base|Arbitrum|Ethereum",
          "deposit 2 USDC into Morpho", "morpho", Some("morpho")),
    block("Earn", "INTO · HLP", "Hyperliquid's vault", "deposit N USDC to HLP · from $5, locked 4 days",
          "deposit 5 USDC to HLP", "hlp", Some("hype")),
    block("Earn", "STAKE · JITO", "Stake SOL", "stake N SOL · unstake N JitoSOL",
          "stake 0.01 SOL", "jito", Some("jitosol")),
    block("Earn", "SELL · REWARDS", "Sell rewards", "sweep rewards [over $N] [into TOKEN] · every hour: sweep rewards over $10 into USDC",
          "every day: sweep rewards over $10 into USDC", "morpho", Some("usdc")),
    block("Earn", "BORROW · AAVE", "Borrow USDC",
          "supply N ETH as collateral on Aave · borrow N USDC on Aave · repay all USDC on Aave — health factor kept at 1.5+",
          "borrow 5 USDC on Aave on base", "aave", Some("aave")),
    block("Earn", "INTO · BEST RATES", "The best rates", "spread N USDC across the best rates · rebalance USDC",
          "spread USDC across the best rates", "jupiter", Some("usdc")),

    // HyperEVM: Hyperliquid's EVM, its money in and out over Relay, HYPE for its gas (2026-10-06: Relay lands 1.98 of 2
    // USDC back on Solana, and brings ≈ 0.0016 HYPE of gas along the way there).
    block("HyperEVM", "BRIDGE · TO HYPEREVM", "USDC to HyperEVM", "move N USDC from Solana to HyperEVM · HYPE for gas comes along",
          "move 2 USDC from Solana to HyperEVM", "hyperevm", Some("hyperevm")),
    block("HyperEVM", "BACK · HYPEREVM → SOLANA", "USDC back to Solana", "move N USDC from HyperEVM to Solana · no N = all of it",
          "move all USDC from HyperEVM to Solana", "hyperevm", Some("hyperevm")),
    block("HyperEVM", "GAS · HYPE", "HYPE for gas", "add gas on HyperEVM [for $N] · ≈ $0.001 a transaction",
          "add gas on hyperevm", "hyperevm", Some("hyperevm")),
    block("HyperEVM", "SEND · ON HYPEREVM", "Send on HyperEVM", "send N USDC to NAME on HyperEVM · a saved 0x address",
          "send 5 USDC to Mom on hyperevm", "hyperevm", Some("hyperevm")),

    // Optimism: an OP Stack network beside Base; its USDC in and out over Relay or Circle's CCTP, ETH for its gas.
    block("Optimism", "BRIDGE · TO OPTIMISM", "USDC to Optimism", "move N USDC from Solana to Optimism · ETH for gas comes along",
          "move 2 USDC from Solana to Optimism", "relay", Some("op")),
    block("Optimism", "BACK · OPTIMISM → SOLANA", "USDC back to Solana", "move N USDC from Optimism to Solana · no N = all of it",
          "move all USDC from Optimism to Solana", "relay", Some("op")),
    block("Optimism", "CCTP · BASE", "To Base by Circle", "move N USDC from Optimism to Base via CCTP · Circle burns and mints",
          "move 100 USDC from Optimism to Base via CCTP", "relay", Some("op")),
    block("Optimism", "SEND · ON OPTIMISM", "Send on Optimism", "send N USDC to NAME on Optimism · a saved 0x address",
          "send 5 USDC to Mom on optimism", "relay", Some("op")),

    // BNB Chain (BSC): its Binance-Peg USDC in and out over Relay, BNB for its gas, tokens on Uniswap there or Relay's
    // swap, whichever gives more (2026-10-08).
    block("BNB Chain", "BRIDGE · TO BNB CHAIN", "USDC to BNB Chain", "move N USDC from Solana to BNB Chain · BNB for gas comes along",
          "move 2 USDC from Solana to BNB", "relay", Some("bnb")),
    block("BNB Chain", "BACK · BNB CHAIN → SOLANA", "USDC back to Solana", "move N USDC from BNB Chain to Solana · no N = all of it",
          "move all USDC from BNB to Solana", "relay", Some("bnb")),
    block("BNB Chain", "BUY · ON BNB CHAIN", "Buy a token there", "buy TICKER on BNB Chain for N USDC · CAKE, BTCB, ETH, LINK…",
          "buy CAKE on bnb for 2 USDC", "relay", Some("bnb")),
    block("BNB Chain", "SWAP · ON BNB CHAIN", "One token for another", "swap N BNB|TICKER for TICKER on BNB Chain · straight, no USDC between",
          "swap 0.001 BNB for CAKE on bnb", "relay", Some("bnb")),
    block("BNB Chain", "GAS · BNB", "BNB for gas", "add gas on BNB Chain [for $N] · ≈ a cent a transaction",
          "add gas on bnb", "relay", Some("bnb")),
    block("BNB Chain", "SEND · ON BNB CHAIN", "Send on BNB Chain", "send N USDC to NAME on BNB Chain · a saved 0x address",
          "send 5 USDC to Mom on bnb", "relay", Some("bnb")),

    // Tempo: its money moves, then services paid per request in USDC there (MPP). Each example is a real endpoint from
    // mpp.dev's directory with the body its docs give, priced live by its 402 (2026-09-30); the card shows the price
    // before paying. Tempo's own proxies (*.mpp.tempo.xyz: fal, OpenAI, Anthropic) take only sponsored transactions the
    // service sends, which the signer doesn't make yet, and StableStudio's pictures come later behind a sign-in.
    block("Tempo", "BRIDGE · TO TEMPO", "USDC to Tempo", "move N USDC from Solana to Tempo · and back to Solana",
          "move 2 USDC from Solana to Tempo", "tempo", Some("tempo")),
    block("Tempo", "SEND · ON TEMPO", "Send on Tempo", "send N USDC to NAME saved on Tempo · fee ≈ $0.001 in USDC",
          "send 5 USDC to Mom", "tempo", Some("tempo")),
    block("Tempo", "SWAP · ON TEMPO", "Swap dollars", "swap N USDC for pathUSD or OUSD on tempo · Tempo's own exchange, ≈ 1–2 bp",
          "swap 1 USDC for pathUSD on tempo", "tempo", Some("tempo")),
    block("Tempo", "EVERY MONTH · 1ST", "Monthly transfer", "every month on the 1st…28th [at HH:MM]: send N USDC to NAME",
          "every month on the 1st at 10:00: send 20 USDC to Mom", "tempo", Some("tempo")),
    block("Tempo", "PAY · WEB SEARCH", "Search the web", "Brave Search · $0.035 a search · web, news, images, video",
          r#"pay POST https://brave.mpp.paywithlocus.com/brave/web-search {"q":"Tempo MPP payments"}"#, "tempo", Some("tempo")),
    block("Tempo", "PAY · CRYPTO PRICE", "Crypto prices", "CoinGecko · $0.06 a request · prices, markets, coin data",
          r#"pay POST https://coingecko.mpp.paywithlocus.com/coingecko/simple-price {"ids":"bitcoin","vs_currencies":"usd"}"#, "tempo", Some("tempo")),
    block("Tempo", "PAY · WEATHER", "Weather", "OpenWeather · $0.006 · now, 5 days, air quality by lat/lon",
          r#"pay POST https://openweather.mpp.paywithlocus.com/openweather/current-weather {"lat":55.7558,"lon":37.6173}"#, "tempo", Some("tempo")),
    block("Tempo", "PAY · TRANSLATE", "Translate", "DeepL · priced per text · 30+ languages",
          r#"pay POST https://deepl.mpp.paywithlocus.com/deepl/translate {"text":["Hello from ihatecrypto"],"target_lang":"RU"}"#, "tempo", Some("tempo")),
    block("Tempo", "PAY · EMAIL", "Send an email", "StableEmail · $0.02 a letter · to, subject, text",
          r#"pay POST https://stableemail.dev/api/send {"to":["you@example.com"],"subject":"Hi","text":"Sent from ihatecrypto"}"#, "tempo", Some("tempo")),
    block("Tempo", "PAY · PHONE CALL", "AI phone call", "StablePhone · $0.54 a call · a number and what to say",
          r#"pay POST https://stablephone.dev/api/call {"phone_number":"+14155551234","task":"Ask if the table for two at 8 pm is still free."}"#,
          "tempo", Some("tempo")),
    block("Tempo", "PAY · FLIGHTS", "Cheapest flights", "StableTravel · $0.02 · Google Flights by airports and date",
          "pay GET https://stabletravel.dev/api/google-flights/search?departure_id=SVO&arrival_id=IST&outbound_date=2026-10-15&type=2",
          "tempo", Some("tempo")),
];
