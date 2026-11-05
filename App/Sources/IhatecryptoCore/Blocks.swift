import Foundation

/// The blocks the agent builds a request from: each is one of ChatRouter.forms, drawn as the route card it becomes.
/// «Add block» on the canvas lists them; a pick puts its example into the chat to finish and send. The `automatic` ones
/// the agent adds to every trade by itself (the rules, Jev, the result): they're shown, not picked.
public enum BlockCatalog {
    public struct Block: Sendable, Identifiable, Equatable {
        public var id: String { label }
        public let group: String
        /// The route card's label; its first word picks the drawing (StepKind).
        public let label: String
        public let title: String
        /// The router's form, as the card says it.
        public let form: String
        /// What a pick puts into the chat. Every example parses (BlocksTests).
        public let example: String
        /// A RouteFill's raw value: the card's colour.
        public let fill: String
        public let logo: String?
        public var automatic = false
    }

    public static let groups = ["Trade", "Triggers", "Move money", "Buy and sell", "Earn", "Tempo", "HyperEVM", "Optimism", "BNB Chain"]

    public static let all: [Block] = [
        Block(group: "Trade", label: "OPEN LONG · HYPERLIQUID", title: "Long or short", form: "long|short ASSET · risk $N · stop N% · take N% · leverage Nx",
              example: "long SOL by the book", fill: "hyperliquid", logo: "hype"),
        Block(group: "Trade", label: "PROTECT", title: "Stop and take", form: "take NR · take in parts · trailing N% · move stop to breakeven",
              example: "take in parts", fill: "hyperliquid", logo: "hype"),
        Block(group: "Trade", label: "CHECK RULES", title: "Risk rules", form: "risk ≤ 2% of capital · reward/risk ≥ 1.5 · leverage ≤ 5×",
              example: "", fill: "check", logo: nil, automatic: true),
        Block(group: "Trade", label: "SECOND CHECK", title: "Second check", form: "the plan against your words, then every signature",
              example: "", fill: "check", logo: nil, automatic: true),
        Block(group: "Trade", label: "RESULT · ESTIMATE", title: "Loss or profit", form: "at the stop and at the take, in $ and % of capital",
              example: "", fill: "check", logo: nil, automatic: true),

        Block(group: "Triggers", label: "WHEN · PRICE FALLS TO", title: "Price alert", form: "when ASSET is above|below PRICE: a trade or a move",
              example: "alert me when SOL is below 115", fill: "check", logo: "sol"),
        Block(group: "Triggers", label: "LATER · SCHEDULE", title: "Later or on repeat", form: "in N days: … · at HH:MM: … · every week: … · every month on the 1st: …",
              example: "in 1 week: rebalance USDC", fill: "check", logo: nil),

        Block(group: "Move money", label: "BRIDGE · RELAY", title: "Between networks", form: "move N USDC between Solana, Base, Arbitrum, Polygon, Ethereum, Robinhood, Tempo, HyperEVM, Optimism, BNB Chain — any two, straight",
              example: "move 5 USDC from Solana to Base", fill: "relay", logo: "relay"),
        Block(group: "Move money", label: "GAS · TOP UP", title: "Gas on any network", form: "add gas on Base|Arbitrum|Polygon|Ethereum|Robinhood for N USDC · moves top it up by themselves",
              example: "add gas on robinhood for 1 USDC", fill: "robinhood", logo: "eth"),
        Block(group: "Move money", label: "FROM · HYPERLIQUID", title: "Back to Solana", form: "withdraw N USDC to Solana",
              example: "withdraw 1 USDC to Solana", fill: "hyperliquid", logo: "hype"),
        Block(group: "Move money", label: "TO · ADDRESS BOOK", title: "Send", form: "send N USDC|SOL|NEAR to NAME from the address book",
              example: "send 1 USDC to Bybit", fill: "solana", logo: "usdc"),
        Block(group: "Move money", label: "ANY TOKEN · AS IT IS", title: "Send a token", form: "send N|all TICKER|0xCONTRACT|MINT on NETWORK to NAME from the address book",
              example: "send all RAY on solana to Bybit", fill: "solana", logo: "sol"),
        Block(group: "Move money", label: "PRIVATE · NEAR INTENTS", title: "Send privately",
              form: "send N USDC privately to NAME from the address book · on chain the deposit and the payout can't be matched",
              example: "send 2 USDC privately to Bybit", fill: "check", logo: "near"),

        Block(group: "Buy and sell", label: "SWAP · JUPITER", title: "Tokens on Solana", form: "buy|sell SOL, BTC, ETH, JUP, BONK… for N USDC",
              example: "buy SOL for 2 USDC", fill: "jupiter", logo: "jup"),
        Block(group: "Buy and sell", label: "EVERY DAY · JUPITER DCA", title: "DCA on Jupiter",
              form: "Jupiter DCA N USDC into SOL, BTC, JUP, BONK… in K orders every day|week · on-chain, from 100 USDC, 50 an order",
              example: "Jupiter DCA 100 USDC into SOL in 2 orders every day", fill: "jupiter", logo: "jup"),
        Block(group: "Buy and sell", label: "BUY · ON BASE", title: "Base and Arbitrum", form: "buy TICKER on base|arbitrum for N USDC · Relay or Uniswap, the better",
              example: "buy AERO on base for 2 USDC", fill: "base", logo: "base"),
        Block(group: "Buy and sell", label: "SWEEP · DUST", title: "Dust into USDC", form: "sweep dust [under $N] · leftovers on Base, Arbitrum, Polygon",
              example: "sweep dust under $5", fill: "base", logo: "usdc"),
        Block(group: "Buy and sell", label: "SWAP · UNISWAP", title: "Token for token", form: "swap N ETH|TICKER for TICKER on base|arbitrum|ethereum|polygon · straight, no USDC in the middle",
              example: "swap 0.001 ETH for AERO on base", fill: "base", logo: "uni"),
        Block(group: "Buy and sell", label: "SWAP · STABLECOINS", title: "Dollar for dollar",
              form: "USDT, USDC, USDS, USDe, DAI on every network they're on · Uniswap, Jupiter, Relay or Hyperliquid, whichever gives more",
              example: "buy USDe on base for 2 USDC from base", fill: "base", logo: "usdc"),
        Block(group: "Buy and sell", label: "BUY · ROBINHOOD CHAIN", title: "Stock tokens", form: "buy TSLA, NVDA, SpaceX… stock for N USDC",
              example: "buy TSLA stock for 2 USDC", fill: "robinhood", logo: "robinhood"),
        Block(group: "Buy and sell", label: "SWAP · NEAR INTENTS", title: "NEAR", form: "buy NEAR for N USDC · sell NEAR",
              example: "buy NEAR for 2 USDC", fill: "near", logo: "near"),

        Block(group: "Earn", label: "INTO · JUPITER LEND", title: "Lend on Solana", form: "lend N USDC|SOL · withdraw N USDC from the vault",
              example: "lend 1 USDC", fill: "jupiter", logo: "jup"),
        Block(group: "Earn", label: "INTO · KAMINO", title: "Kamino", form: "deposit N USDC|SOL to kamino · withdraw [N] USDC from kamino",
              example: "deposit 1 USDC to kamino", fill: "kamino", logo: "kamino"),
        Block(group: "Earn", label: "INTO · AAVE", title: "Aave", form: "lend N USDC on Aave on Base|Arbitrum|Polygon|Ethereum",
              example: "lend 2 USDC on Aave", fill: "aave", logo: "aave"),
        Block(group: "Earn", label: "INTO · MORPHO", title: "Morpho", form: "deposit N USDC into Morpho on Base|Arbitrum|Ethereum",
              example: "deposit 2 USDC into Morpho", fill: "morpho", logo: "morpho"),
        Block(group: "Earn", label: "INTO · HLP", title: "Hyperliquid's vault", form: "deposit N USDC to HLP · from $5, locked 4 days",
              example: "deposit 5 USDC to HLP", fill: "hlp", logo: "hype"),
        Block(group: "Earn", label: "STAKE · JITO", title: "Stake SOL", form: "stake N SOL · unstake N JitoSOL",
              example: "stake 0.01 SOL", fill: "jito", logo: "jitosol"),
        Block(group: "Earn", label: "SELL · REWARDS", title: "Sell rewards", form: "sweep rewards [over $N] [into TOKEN] · every hour: sweep rewards over $10 into USDC",
              example: "every day: sweep rewards over $10 into USDC", fill: "morpho", logo: "usdc"),
        Block(group: "Earn", label: "BORROW · AAVE", title: "Borrow USDC", form: "supply N ETH as collateral on Aave · borrow N USDC on Aave · repay all USDC on Aave — health factor kept at 1.5+",
              example: "borrow 5 USDC on Aave on base", fill: "aave", logo: "aave"),
        Block(group: "Earn", label: "INTO · BEST RATES", title: "The best rates", form: "spread N USDC across the best rates · rebalance USDC",
              example: "spread USDC across the best rates", fill: "jupiter", logo: "usdc"),

        // HyperEVM: Hyperliquid's EVM, its money in and out over Relay, HYPE for its gas (2026-10-06: Relay lands 1.98 of 2 USDC
        // back on Solana, and brings ≈ 0.0016 HYPE of gas along the way there).
        Block(group: "HyperEVM", label: "BRIDGE · TO HYPEREVM", title: "USDC to HyperEVM", form: "move N USDC from Solana to HyperEVM · HYPE for gas comes along",
              example: "move 2 USDC from Solana to HyperEVM", fill: "hyperevm", logo: "hyperevm"),
        Block(group: "HyperEVM", label: "BACK · HYPEREVM → SOLANA", title: "USDC back to Solana", form: "move N USDC from HyperEVM to Solana · no N = all of it",
              example: "move all USDC from HyperEVM to Solana", fill: "hyperevm", logo: "hyperevm"),
        Block(group: "HyperEVM", label: "GAS · HYPE", title: "HYPE for gas", form: "add gas on HyperEVM [for $N] · ≈ $0.001 a transaction",
              example: "add gas on hyperevm", fill: "hyperevm", logo: "hyperevm"),
        Block(group: "HyperEVM", label: "SEND · ON HYPEREVM", title: "Send on HyperEVM", form: "send N USDC to NAME on HyperEVM · a saved 0x address",
              example: "send 5 USDC to Mom on hyperevm", fill: "hyperevm", logo: "hyperevm"),

        // Optimism: an OP Stack network beside Base; its USDC in and out over Relay or Circle's CCTP, ETH for its gas.
        Block(group: "Optimism", label: "BRIDGE · TO OPTIMISM", title: "USDC to Optimism", form: "move N USDC from Solana to Optimism · ETH for gas comes along",
              example: "move 2 USDC from Solana to Optimism", fill: "relay", logo: "op"),
        Block(group: "Optimism", label: "BACK · OPTIMISM → SOLANA", title: "USDC back to Solana", form: "move N USDC from Optimism to Solana · no N = all of it",
              example: "move all USDC from Optimism to Solana", fill: "relay", logo: "op"),
        Block(group: "Optimism", label: "CCTP · BASE", title: "To Base by Circle", form: "move N USDC from Optimism to Base via CCTP · Circle burns and mints",
              example: "move 100 USDC from Optimism to Base via CCTP", fill: "relay", logo: "op"),
        Block(group: "Optimism", label: "SEND · ON OPTIMISM", title: "Send on Optimism", form: "send N USDC to NAME on Optimism · a saved 0x address",
              example: "send 5 USDC to Mom on optimism", fill: "relay", logo: "op"),

        // BNB Chain (BSC): its Binance-Peg USDC in and out over Relay, BNB for its gas, tokens on Uniswap there or Relay's
        // swap, whichever gives more (2026-10-08).
        Block(group: "BNB Chain", label: "BRIDGE · TO BNB CHAIN", title: "USDC to BNB Chain", form: "move N USDC from Solana to BNB Chain · BNB for gas comes along",
              example: "move 2 USDC from Solana to BNB", fill: "relay", logo: "bnb"),
        Block(group: "BNB Chain", label: "BACK · BNB CHAIN → SOLANA", title: "USDC back to Solana", form: "move N USDC from BNB Chain to Solana · no N = all of it",
              example: "move all USDC from BNB to Solana", fill: "relay", logo: "bnb"),
        Block(group: "BNB Chain", label: "BUY · ON BNB CHAIN", title: "Buy a token there", form: "buy TICKER on BNB Chain for N USDC · CAKE, BTCB, ETH, LINK…",
              example: "buy CAKE on bnb for 2 USDC", fill: "relay", logo: "bnb"),
        Block(group: "BNB Chain", label: "SWAP · ON BNB CHAIN", title: "One token for another", form: "swap N BNB|TICKER for TICKER on BNB Chain · straight, no USDC between",
              example: "swap 0.001 BNB for CAKE on bnb", fill: "relay", logo: "bnb"),
        Block(group: "BNB Chain", label: "GAS · BNB", title: "BNB for gas", form: "add gas on BNB Chain [for $N] · ≈ a cent a transaction",
              example: "add gas on bnb", fill: "relay", logo: "bnb"),
        Block(group: "BNB Chain", label: "SEND · ON BNB CHAIN", title: "Send on BNB Chain", form: "send N USDC to NAME on BNB Chain · a saved 0x address",
              example: "send 5 USDC to Mom on bnb", fill: "relay", logo: "bnb"),

        // Tempo: its money moves, then services paid per request in USDC there (MPP). Each example is a real endpoint
        // from mpp.dev's directory with the body its docs give, priced live by its 402 (2026-09-30); the card shows the price
        // before paying. Tempo's own proxies (*.mpp.tempo.xyz: fal, OpenAI, Anthropic) take only sponsored transactions
        // the service sends, which the signer doesn't make yet, and StableStudio's pictures come later behind a sign-in.
        Block(group: "Tempo", label: "BRIDGE · TO TEMPO", title: "USDC to Tempo", form: "move N USDC from Solana to Tempo · and back to Solana",
              example: "move 2 USDC from Solana to Tempo", fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "SEND · ON TEMPO", title: "Send on Tempo", form: "send N USDC to NAME saved on Tempo · fee ≈ $0.001 in USDC",
              example: "send 5 USDC to Mom", fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "SWAP · ON TEMPO", title: "Swap dollars", form: "swap N USDC for pathUSD or OUSD on tempo · Tempo's own exchange, ≈ 1–2 bp",
              example: "swap 1 USDC for pathUSD on tempo", fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "EVERY MONTH · 1ST", title: "Monthly transfer", form: "every month on the 1st…28th [at HH:MM]: send N USDC to NAME",
              example: "every month on the 1st at 10:00: send 20 USDC to Mom", fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "PAY · WEB SEARCH", title: "Search the web", form: "Brave Search · $0.035 a search · web, news, images, video",
              example: #"pay POST https://brave.mpp.paywithlocus.com/brave/web-search {"q":"Tempo MPP payments"}"#, fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "PAY · CRYPTO PRICE", title: "Crypto prices", form: "CoinGecko · $0.06 a request · prices, markets, coin data",
              example: #"pay POST https://coingecko.mpp.paywithlocus.com/coingecko/simple-price {"ids":"bitcoin","vs_currencies":"usd"}"#, fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "PAY · WEATHER", title: "Weather", form: "OpenWeather · $0.006 · now, 5 days, air quality by lat/lon",
              example: #"pay POST https://openweather.mpp.paywithlocus.com/openweather/current-weather {"lat":55.7558,"lon":37.6173}"#, fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "PAY · TRANSLATE", title: "Translate", form: "DeepL · priced per text · 30+ languages",
              example: #"pay POST https://deepl.mpp.paywithlocus.com/deepl/translate {"text":["Hello from ihatecrypto"],"target_lang":"RU"}"#, fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "PAY · EMAIL", title: "Send an email", form: "StableEmail · $0.02 a letter · to, subject, text",
              example: #"pay POST https://stableemail.dev/api/send {"to":["you@example.com"],"subject":"Hi","text":"Sent from ihatecrypto"}"#, fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "PAY · PHONE CALL", title: "AI phone call", form: "StablePhone · $0.54 a call · a number and what to say",
              example: #"pay POST https://stablephone.dev/api/call {"phone_number":"+14155551234","task":"Ask if the table for two at 8 pm is still free."}"#,
              fill: "tempo", logo: "tempo"),
        Block(group: "Tempo", label: "PAY · FLIGHTS", title: "Cheapest flights", form: "StableTravel · $0.02 · Google Flights by airports and date",
              example: "pay GET https://stabletravel.dev/api/google-flights/search?departure_id=SVO&arrival_id=IST&outbound_date=2026-10-15&type=2",
              fill: "tempo", logo: "tempo"),
    ]
}
