use super::d;
use crate::top_up::{Coin, Network, TopUpOrder};

/// USDC topped up on Solana from a browser wallet brings `TopUpOrder.solanaFeeTopUp` SOL along.
// pending: TopUpFeesTests' message itself (TopUpPage.usdcWithFees, the window's top-up page).
#[test]
fn top_up_links_carry_the_amount_coin_and_network() {
    let (sol, evm) = ("6nod592sTfEWD3VSVPdQndLMVBCNmMc6ngt7MyGBK21j", "0x976EA74026E726554dB657fA54763abd0C3a0aa9");
    let base = Network::new("base", "Base", Some(8453), "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", Some("ETH"), "https://basescan.org", "https://mainnet.base.org");
    let tempo = Network::new("tempo", "Tempo", Some(4217), "0x20c000000000000000000000b9537d11c60e8b50", None, "https://explore.tempo.xyz", "https://rpc.mainnet.tempo.xyz");
    let usdc = TopUpOrder::new(Network::solana(), Coin::Usdc, Some(d("20.5")), sol);
    assert_eq!(usdc.units().as_deref(), Some("20500000"));
    assert_eq!(usdc.pay_link().unwrap(), format!("solana:{sol}?amount=20.5&spl-token=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v&label=ihatecrypto&message=Top%20up"));
    let lamports = TopUpOrder::new(Network::solana(), Coin::Native, Some(d("0.02")), sol);
    assert_eq!(lamports.units().as_deref(), Some("20000000"));
    assert_eq!(lamports.pay_link().unwrap(), format!("solana:{sol}?amount=0.02&label=ihatecrypto&message=Top%20up"));
    let base_usdc = TopUpOrder::new(base.clone(), Coin::Usdc, Some(d("100")), evm);
    assert_eq!(base_usdc.pay_link().unwrap(), format!("ethereum:0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913@8453/transfer?address={evm}&uint256=100000000"));
    // Wei past UInt64 still prints whole, not as 1e+19.
    let eth = TopUpOrder::new(base.clone(), Coin::Native, Some(d("20")), evm);
    assert_eq!(eth.units().as_deref(), Some("20000000000000000000"));
    assert_eq!(eth.pay_link().unwrap(), format!("ethereum:{evm}@8453?value=20000000000000000000"));
    // Any amount: the link names the coin and the network, the wallet asks how much.
    assert_eq!(TopUpOrder::new(Network::solana(), Coin::Usdc, None, sol).pay_link().unwrap(),
               format!("solana:{sol}?spl-token=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v&label=ihatecrypto&message=Top%20up"));
    assert_eq!(TopUpOrder::new(base.clone(), Coin::Usdc, None, evm).pay_link().unwrap(),
               format!("ethereum:0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913@8453/transfer?address={evm}"));
    // Any coin: only the address and the network, whatever the amount says.
    let any = TopUpOrder::new(base.clone(), Coin::Any, Some(d("20")), evm);
    assert!(any.units().is_none() && any.symbol() == "any coin" && any.pay_link().unwrap() == format!("ethereum:{evm}@8453"));
    assert_eq!(TopUpOrder::new(Network::solana(), Coin::Any, None, sol).pay_link().unwrap(), format!("solana:{sol}?label=ihatecrypto&message=Top%20up"));
    // Tempo has no coin of its own: its dollar it is.
    assert_eq!(TopUpOrder::new(tempo, Coin::Native, Some(d("5")), evm).symbol(), "USDC");
    // More digits than USDC has, nothing, or too much: no amount to ask for, and no link that would ask for another one.
    assert!(TopUpOrder::new(Network::solana(), Coin::Usdc, Some(d("1.0000001")), sol).units().is_none());
    assert!(TopUpOrder::new(Network::solana(), Coin::Usdc, Some(d("0")), sol).pay_link().is_none());
    assert!(TopUpOrder::new(base, Coin::Usdc, Some(d("2000000")), evm).units().is_none());
}
