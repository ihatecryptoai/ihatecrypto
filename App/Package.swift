// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "ihatecrypto",
    // en.lproj/InfoPlist.strings (the app's display name) makes SwiftPM ask for a default language.
    defaultLocalization: "en",
    platforms: [.macOS(.v26)],
    products: [
        // For the Xcode project (project.yml) that builds ihatecrypto.app and its signing service.
        .library(name: "IhatecryptoKit", targets: [
            "IhatecryptoCore", "LLMCore", "LLMCatalog", "WalletCore", "TxGuard", "GuardJev", "SignerAPI", "HyperliquidSigning", "SolanaChain",
            "TradeExecution", "ExecutionCore", "DeFiJupiterLend", "JupiterSwap", "EVMChain", "DeFiKamino", "DeFiYields", "Ledger",
        ]),
        .library(name: "IhatecryptoSignerKit", targets: ["SignerAPI", "WalletCore", "TxGuard", "HyperliquidSigning", "SolanaChain", "EVMChain"]),
    ],
    dependencies: [
        // libsecp256k1 from Bitcoin Core: secp256k1 keys and recoverable ECDSA for EVM and Hyperliquid signatures.
        .package(url: "https://github.com/21-DOT-DEV/swift-secp256k1", exact: "0.23.2"),
        // Updates for the copy from the site (Updater.swift). Declared here too, so `swift build` and the Xcode
        // project resolve the same packages and stop rewriting Package.resolved over each other.
        .package(url: "https://github.com/sparkle-project/Sparkle", exact: "2.10.0"),
    ],
    targets: [
        // Model access: one protocol in Core, one folder per way to connect, Catalog lists them all.
        .target(name: "LLMCore", path: "Sources/LLM/Core"),
        .target(name: "LLMAPIKey", dependencies: ["LLMCore"], path: "Sources/LLM/APIKey"),
        .target(name: "LLMOpenRouter", dependencies: ["LLMCore", "LLMAPIKey"], path: "Sources/LLM/OpenRouter"),
        .target(name: "LLMChatGPT", dependencies: ["LLMCore"], path: "Sources/LLM/ChatGPT"),
        .target(name: "LLMClaudeCode", dependencies: ["LLMCore"], path: "Sources/LLM/ClaudeCode"),
        .target(name: "LLMApple", dependencies: ["LLMCore"], path: "Sources/LLM/Apple"),
        .target(
            name: "LLMCatalog",
            dependencies: ["LLMCore", "LLMAPIKey", "LLMOpenRouter", "LLMChatGPT", "LLMClaudeCode", "LLMApple"],
            path: "Sources/LLM/Catalog"
        ),
        .executableTarget(name: "LLMCheck", dependencies: ["LLMCatalog", "LLMCore", "IhatecryptoCore", "TradeExecution"], path: "Sources/LLM/Check"),

        // Keys and signatures: BIP-39, derivation, Keccak, EIP-712. Knows nothing about protocols or UI.
        .target(name: "WalletCore", dependencies: [.product(name: "P256K", package: "swift-secp256k1")], path: "Sources/Wallet/Core"),
        // Hyperliquid wire format and signing, checked against the official Python SDK vectors.
        .target(name: "HyperliquidSigning", dependencies: ["WalletCore"], path: "Sources/Protocols/Hyperliquid"),
        .target(name: "SolanaChain", dependencies: ["WalletCore"], path: "Sources/Chains/Solana"),
        .target(name: "EVMChain", dependencies: ["WalletCore", .product(name: "P256K", package: "swift-secp256k1")], path: "Sources/Chains/EVM"),
        // Write-before-send journal; later the approval envelope and step states.
        .target(name: "ExecutionCore", path: "Sources/Execution", exclude: ["Trade"]),
        .target(name: "JupiterSwap", dependencies: ["WalletCore"], path: "Sources/Protocols/Jupiter"),
        .target(name: "OneClickKit", dependencies: ["WalletCore"], path: "Sources/Protocols/OneClick"),
        .target(name: "RelayBridge", dependencies: ["SolanaChain", "WalletCore"], path: "Sources/Protocols/Relay"),
        // DeFi: one folder per protocol. Jupiter Lend Earn first.
        .target(name: "DeFiJupiterLend", dependencies: ["WalletCore"], path: "Sources/DeFi/JupiterLend"),
        .target(name: "DeFiKamino", path: "Sources/DeFi/Kamino"),
        // Where money earns now across the DeFi modules: lending, liquid staking, Hyperliquid's HLP.
        .target(name: "DeFiYields", dependencies: ["DeFiJupiterLend", "DeFiKamino"], path: "Sources/DeFi/Yields"),
        // Runs an approved plan: bridge, leverage, entry, stop and take, through whichever signer holds the keys.
        .target(name: "TradeExecution", dependencies: ["IhatecryptoCore", "ExecutionCore", "HyperliquidSigning", "SolanaChain", "EVMChain", "RelayBridge", "TxGuard", "WalletCore",
                                                       "DeFiJupiterLend", "JupiterSwap", "DeFiKamino", "DeFiYields", "OneClickKit"],
                path: "Sources/Execution/Trade"),
        // Last check before signing: allowed programs, recipients bound to an approved quote, simulated balances.
        .target(name: "TxGuard", dependencies: ["SolanaChain", "HyperliquidSigning"], path: "Sources/Guard/Core"),
        // Signing process: the XPC protocol shared by the app and Signer, and the service itself
        // (built as an .xpc by project.yml; the SwiftPM target only compile-checks it).
        .target(name: "SignerAPI", dependencies: ["WalletCore", "HyperliquidSigning"], path: "Sources/Signer/API"),
        .executableTarget(name: "IhatecryptoSigner", dependencies: ["SignerAPI", "WalletCore", "TxGuard", "HyperliquidSigning", "SolanaChain", "EVMChain"],
                          path: "Sources/Signer/Service"),
        // A Ledger over USB (or its emulator over TCP): the Ethereum and Solana apps sign what the guard let through.
        .target(name: "Ledger", dependencies: ["WalletCore", "EVMChain", "SolanaChain", "HyperliquidSigning", "TxGuard", "TradeExecution"],
                path: "Sources/Wallet/Ledger"),
        // Jev (TypeSafe via OpenRouter) as an extra classifier for untrusted text; it can only add problems.
        .target(name: "GuardJev", dependencies: ["TxGuard"], path: "Sources/Guard/Jev"),
        .executableTarget(
            name: "WalletCheck",
            dependencies: ["WalletCore", "HyperliquidSigning", "SolanaChain", "JupiterSwap", "OneClickKit", "IhatecryptoCore", "ExecutionCore", "TxGuard", "GuardJev", "DeFiJupiterLend", "RelayBridge", "EVMChain", "TradeExecution", "DeFiKamino", "DeFiYields", "Ledger"],
            path: "Sources/Wallet/Check"
        ),

        // Trading plan: knows only the LLMClient protocol, not the providers.
        .target(name: "IhatecryptoCore", dependencies: ["LLMCore", "WalletCore", "OneClickKit", "RelayBridge", "HyperliquidSigning"]),
        .executableTarget(
            name: "IhatecryptoApp",
            dependencies: ["IhatecryptoCore", "LLMCore", "LLMCatalog", "WalletCore", "TxGuard", "GuardJev", "SignerAPI", "HyperliquidSigning", "SolanaChain",
                           "TradeExecution", "ExecutionCore", "DeFiJupiterLend", "JupiterSwap", "EVMChain", "DeFiKamino", "DeFiYields", "Ledger",
                           .product(name: "Sparkle", package: "Sparkle")],
            // Logos, sounds and the stories are bundled by the Xcode project; `swift run` shows lettered circles instead.
            exclude: ["Logos", "Sounds", "WhatsNew"],
            // The local debug API (DebugAPI.swift) is in `swift run` and the copies build-app.sh makes, never in a release.
            swiftSettings: [.defaultIsolation(MainActor.self), .define("DEBUG_API")]
        ),

        .testTarget(name: "IhatecryptoCoreTests", dependencies: ["IhatecryptoCore", "LLMCore"]),
        // The window's own logic that needs no window: the profile menu's accounts, Settings › Modules' rows.
        .testTarget(name: "IhatecryptoAppTests", dependencies: ["IhatecryptoApp", "IhatecryptoCore", "LLMCore", "LLMCatalog"],
                    swiftSettings: [.defaultIsolation(MainActor.self)]),
        .testTarget(name: "DeFiTests", dependencies: ["DeFiYields"]),
        .testTarget(name: "WalletTests", dependencies: ["WalletCore", "HyperliquidSigning", "SolanaChain", "ExecutionCore", "TxGuard", "EVMChain", "TradeExecution", "SignerAPI", "Ledger", "OneClickKit", "RelayBridge", "IhatecryptoCore", "GuardJev", "JupiterSwap", "DeFiKamino"]),
        .testTarget(
            name: "LLMTests",
            dependencies: ["LLMCore", "LLMAPIKey", "LLMOpenRouter", "LLMChatGPT", "LLMClaudeCode", "LLMApple", "LLMCatalog"]
        ),
    ]
)
