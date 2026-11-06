use crate::stablecoins::*;

/// The big five stablecoins (USDT, USDC, USDS, USDe, DAI) on every network they're on: the ticker finds the coin.
// pending: stablecoinSwapsParse and the sends in stablecoinSendsAndMovesParse (WalletCommand.parse, ChatRouter.forms);
// USDT0Tests' match of USDT0_NETWORKS with evm's deployments needs EVMNetwork (Network round).
#[test]
fn stablecoins_are_found_by_ticker() {
    assert_eq!(find("usdt", "arbitrum").unwrap().address, "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9");
    assert_eq!(find("USD₮0", "arbitrum").unwrap().symbol, "USDT");
    assert_eq!(find("USDT", "hyperliquid").unwrap().symbol, "USDT0");
    assert_eq!(find("USDe", "hyperliquid").unwrap().symbol, "USDE");
    assert_eq!(find("$USDS", "solana").unwrap().address, "USDSwr9ApdHk5bvJKMjzff41FfuX8bSxdKcR81vTwcA");
    assert_ne!(find("USDT0", "optimism").map(|c| c.address), find("USDT", "optimism").map(|c| c.address));
    assert!(find("DAI", "solana").is_none() && find("USDS", "polygon").is_none());
    // Every network has USDC, and every coin is one of the five or USDT0, on a network ihatecrypto has.
    for (network, coins) in BY_NETWORK {
        assert!(coins.iter().any(|c| c.symbol == "USDC"), "{network}");
        assert!(coins.iter().all(|c| ["USDT", "USDT0", "USDC", "USDS", "USDE", "DAI"].contains(&c.symbol.to_uppercase().as_str())), "{network}");
    }
    assert!(["USDS", "USDE", "DAI", "USDT0", "USD₮0", "USDG"].iter().all(|t| is_dollar(t)) && !is_dollar("ETH"));
}

#[test]
fn genuine_contracts_and_the_brief() {
    assert!(is_genuine("0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9", "arbitrum") && !is_genuine("0xdead", "arbitrum"));
    // ChatRouter.forms carries this line (stablecoinSwapsParse).
    assert!(brief().contains("Arbitrum: USDC, USDT, USDS, USDe, DAI"));
    assert!(brief().starts_with("Solana: USDC, USDT, USDS, USDe · Ethereum: ") && brief().contains("Hyperliquid spot: USDC, USDT0, USDE · Tempo: "));
    assert_eq!(networks("USDS"), ["arbitrum", "base", "ethereum", "solana"]);
}
