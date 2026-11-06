use crate::address_book_command::AddressBookCommand;

fn save(address: &str, name: &str, network: Option<&str>) -> Option<AddressBookCommand> {
    Some(AddressBookCommand::Save { address: address.into(), name: name.into(), network: network.map(Into::into) })
}

fn parse(text: &str) -> Option<AddressBookCommand> { AddressBookCommand::parse(text) }

#[test]
fn address_book_from_chat() {
    let (evm, sol) = ("0x742d35Cc6634C0532925a3b844Bc454e4438f44e", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    assert_eq!(parse(&format!("save {evm} as Bybit on Arbitrum")), save(evm, "Bybit", Some("arbitrum")));
    assert_eq!(parse(&format!("сохрани {sol} как Binance депозит")), save(sol, "Binance депозит", None));
    assert_eq!(parse(&format!("remember {evm} as “Mom” on base.")), save(evm, "Mom", Some("base")));
    assert_eq!(parse("add Bybit to favorites"), Some(AddressBookCommand::Favorite { name: "Bybit".into(), on: true }));
    assert_eq!(parse("добавь bybit в избранное"), Some(AddressBookCommand::Favorite { name: "bybit".into(), on: true }));
    assert_eq!(parse("remove Mom from my favourites"), Some(AddressBookCommand::Favorite { name: "Mom".into(), on: false }));
    // Without a name nothing is saved; an address alone or a send is not this command.
    assert_eq!(parse(&format!("save {evm}")), None);
    assert_eq!(parse(&format!("send 5 USDC to {evm}")), None);
    assert_eq!(parse("long ETH risk $5"), None);
}

/// A NEAR account in a save command.
// pending: ChatRouter.addressesAllowed (Normalize round).
#[test]
fn near_accounts_are_saved_on_near() {
    let implicit = "ab".repeat(32);
    assert_eq!(parse(&format!("save {implicit} as Bybit on near")), save(&implicit, "Bybit", Some("near")));
    assert_eq!(parse("save alice.near as Alice"), save("alice.near", "Alice", Some("near")));
}

#[test]
fn evm_networks_by_name() {
    let shop = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC";
    assert_eq!(parse(&format!("save {shop} as Shop on BNB")), save(shop, "Shop", Some("bnb")));
    assert_eq!(parse(&format!("save {shop} as Shop on HyperEVM")), save(shop, "Shop", Some("hyperevm")));
    assert_eq!(parse(&format!("save {shop} as Shop on Optimism")), save(shop, "Shop", Some("optimism")));
}

/// «save vitalik.eth as Vitalik» saves a name to resolve; a look-alike letter makes it no name, and a token or a buy
/// never reads a name as an address.
#[test]
fn address_book_takes_ens_and_sns_names_only_to_save() {
    assert_eq!(parse("save vitalik.eth as Vitalik"), save("vitalik.eth", "Vitalik", Some("ethereum")));
    assert_eq!(parse("сохрани Vitalik.eth как Виталик на base"), save("vitalik.eth", "Виталик", Some("base")));
    assert_eq!(parse("save toly.sol as Toly"), save("toly.sol", "Toly", Some("solana")));
    // A Cyrillic «і» in «vіtalik.eth»: not a name at all, so nothing to save.
    assert_eq!(parse("save vіtalik.eth as Vitalik"), None);
    assert_eq!(AddressBookCommand::domain("pay sub.name.eth now").as_deref(), Some("sub.name.eth"));
    assert_eq!(AddressBookCommand::address("add token uniswap.eth to favorites"), None);
}

/// Networks only NEAR Intents reaches: an exchange's Bitcoin, TRON or XRP address.
// pending: the sends in receivingFromNetworksWithoutAWalletIsParsed (WalletCommand.parse, PositionCommand round).
#[test]
fn addresses_on_networks_without_a_wallet() {
    assert_eq!(parse("save bc1quc8z9y2dmpe2qztdw4zh0ucr7g5p8gw6x3c8ph as Binance BTC"),
               save("bc1quc8z9y2dmpe2qztdw4zh0ucr7g5p8gw6x3c8ph", "Binance BTC", Some("bitcoin")));
    assert_eq!(parse("save TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t as Binance USDT on tron"),
               save("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t", "Binance USDT", Some("tron")));
    assert_eq!(parse("save rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh as Bitstamp XRP"), save("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh", "Bitstamp XRP", Some("xrp")));
    // A 0x address keeps its own network, a ticker in its name notwithstanding.
    assert_eq!(parse("save 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC as BTC vault on base"),
               save("0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC", "BTC vault", Some("base")));
}
