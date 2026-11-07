//! The NEAR hackathon's commands (main's PrivateSendTests, AgentWalletTests and the «via NEAR Intents» lines of
//! FundingPlanTests): private sends and swaps, the agent wallet, USDC moved by name through NEAR Intents.

use rust_decimal::Decimal;

use super::{p, some};
use crate::position_command::WalletCommand as W;
use crate::tests::d;

fn private_send(name: &str, token: &str, amount: &str, network: Option<&str>) -> W {
    W::SendPrivately { name: name.into(), token: token.into(), amount: d(amount), network: network.map(Into::into) }
}

fn private_swap(from: &str, on: &str, to: &str, at: &str, amount: &str) -> W {
    W::SwapPrivately { from: from.into(), on: on.into(), to: to.into(), at: at.into(), amount: d(amount) }
}

#[test]
fn private_send_reads_in_both_languages() {
    assert_eq!(p("send 20 USDC privately to Mom"), Some(private_send("Mom", "USDC", "20", None)));
    assert_eq!(p("send 5 USDC privately on base to Mom"), Some(private_send("Mom", "USDC", "5", Some("base"))));
    assert_eq!(p("send 0.001 BTC privately to Binance"), Some(private_send("Binance", "BTC", "0.001", None)));
    assert_eq!(p("send 20 USDC to Mom privately"), Some(private_send("Mom", "USDC", "20", None)));
    assert_eq!(p("переведи приватно 20 USDC на Маму"), Some(private_send("Маму", "USDC", "20", None)));
    assert_eq!(p("отправь анонимно 0,001 BTC на Binance"), Some(private_send("Binance", "BTC", "0.001", None)));
    assert_eq!(p("отправь 30 USDT в троне конфиденциально на Bybit"), Some(private_send("Bybit", "USDT", "30", Some("tron"))));
}

#[test]
fn private_send_writes_back_to_itself() {
    for c in [private_send("Mom", "USDC", "20", Some("base")), private_send("Binance", "BTC", "0.0005", None)] {
        assert_eq!(p(&c.summary()), Some(c));
    }
}

#[test]
fn a_name_with_a_privacy_stem_stays_a_name() {
    // «Приватный кошелёк», «Тайный» are names, not requests: the send stays the public one the user wrote.
    assert_eq!(p("send 5 USDC to Приватный кошелёк"), Some(W::SendTo { name: "Приватный кошелёк".into(), amount: d("5"), network: None }));
    assert_eq!(p("переведи 5 USDC на Тайный"), Some(W::SendTo { name: "Тайный".into(), amount: d("5"), network: None }));
    assert!(W::asks_privacy("скинь втихую 10 баксов маме"));
    assert!(!W::asks_privacy("send 5 USDC to Приватный кошелёк"));
}

#[test]
fn a_private_request_never_reads_as_a_public_send() {
    // No amount, or no send at all: nothing, so the chat asks — never the same move in the open.
    assert_eq!(p("send all USDC privately to Mom"), None);
    assert_eq!(p("переведи всё приватно на Маму"), None);
    assert_eq!(p("buy SOL privately for 5 USDC"), None);
    assert_eq!(p("send 200 USDC from the multisig to Alex privately"), None);
}

#[test]
fn a_public_send_in_a_chain_is_found_when_privacy_was_asked() {
    assert!(W::has_public_send("send 20 USDC to Mom then buy SOL for 5 USDC"));
    assert!(W::has_public_send("every friday: send 20 USDC to Mom"));
    assert!(W::has_public_send("when SOL is above 300: send 20 USDC to Mom"));
    assert!(!W::has_public_send("send 20 USDC privately to Mom then buy SOL for 5 USDC"));
    assert!(!W::has_public_send("send 20 USDC to Mom"));   // alone: made private where it's run
    assert!(!W::has_public_send("buy SOL for 5 USDC then deposit 5 USDC to kamino"));
}

#[test]
fn private_swap_reads_in_both_languages() {
    let buy = private_swap("USDC", "solana", "NEAR", "near", "5");
    assert_eq!(p("buy NEAR for 5 USDC privately"), Some(buy.clone()));
    assert_eq!(p("купи приватно NEAR на 5 USDC"), Some(buy.clone()));
    assert_eq!(p("swap 5 USDC on solana privately for NEAR on near"), Some(buy));
    assert_eq!(p("sell 0.5 NEAR privately"), Some(private_swap("NEAR", "near", "USDC", "solana", "0.5")));
    assert_eq!(p("swap 1 NEAR privately for AURORA on near"), Some(private_swap("NEAR", "near", "AURORA", "near", "1")));
    assert_eq!(p("обменяй анонимно 0,1 SOL с соланы на ETH в арбитруме"), Some(private_swap("SOL", "solana", "ETH", "arbitrum", "0.1")));
    assert_eq!(p("buy NEAR privately"), None);
    assert_eq!(p("sell all NEAR privately"), None);
}

#[test]
fn private_swap_writes_back_to_itself_and_to_its_public_twin() {
    for c in [
        private_swap("USDC", "solana", "NEAR", "near", "5"),
        private_swap("NEAR", "near", "AURORA", "near", "1"),
        private_swap("NEAR", "near", "USDC", "base", "2"),
        private_swap("SOL", "solana", "ETH", "arbitrum", "0.1"),
    ] {
        assert_eq!(p(&c.summary()), Some(c.clone()), "{}", c.summary());
        // The same swap in the open, and back: the switch over Confirm flips between them.
        assert_eq!(c.made_public().and_then(|w| w.made_private()), Some(c));
    }
    assert_eq!(private_swap("USDC", "solana", "NEAR", "near", "5").made_public(), Some(W::BuyNear { usdc: some("5") }));
    assert_eq!(private_swap("NEAR", "near", "USDC", "solana", "1").made_public(), Some(W::SellNear { amount: some("1"), to: None }));
}

#[test]
fn agent_wallet_commands_read_in_both_languages() {
    assert_eq!(p("create an agent wallet with $20 a day"), Some(W::AgentCreate { daily_usd: d("20") }));
    assert_eq!(p("создай кошелёк агента с лимитом 20$ в день"), Some(W::AgentCreate { daily_usd: d("20") }));
    assert_eq!(p("fund the agent wallet with 5 USDC"), Some(W::AgentFund { usdc: d("5") }));
    assert_eq!(p("пополни кошелёк агента на 5 USDC"), Some(W::AgentFund { usdc: d("5") }));
    assert_eq!(p("send 1 USDC from the agent wallet to Automation"), Some(W::AgentSend { usdc: d("1"), name: "Automation".into() }));
    assert_eq!(p("переведи 1,5 USDC с кошелька агента на Маму"), Some(W::AgentSend { usdc: d("1.5"), name: "Маму".into() }));
    assert_eq!(p("freeze the agent wallet"), Some(W::AgentFreeze(true)));
    assert_eq!(p("разморозь кошелёк агента"), Some(W::AgentFreeze(false)));
}

/// «send … from the agent wallet to Main» pays the wallet's own address; a saved name or anyone else is not the wallet.
#[test]
fn the_agent_wallet_pays_the_wallet_itself_by_its_own_words() {
    for own in ["me", "Main", "my wallet", "my main wallet", "myself", "себе", "мой кошелёк", "основной кошелек", "Trading"] {
        assert!(W::is_own_wallet(own, Some("Trading")), "{own}");
    }
    for other in ["Mom", "Automation", "Маме", "my mom", "Binance deposit", "maine"] {
        assert!(!W::is_own_wallet(other, Some("Trading")), "{other}");
    }
}

#[test]
fn agent_wallet_commands_write_back_and_need_their_numbers() {
    for c in [W::AgentCreate { daily_usd: d("20") }, W::AgentFund { usdc: d("2.5") }, W::AgentSend { usdc: Decimal::ONE, name: "Mom".into() },
              W::AgentFreeze(true), W::AgentFreeze(false)] {
        assert_eq!(p(&c.summary()), Some(c));
    }
    assert_eq!(p("create an agent wallet"), None);
    assert_eq!(p("fund the agent wallet"), None);
    // A payee's name is a name, and money taken out of the agent wallet is never read as money put in.
    assert_eq!(p("send 1 USDC from the agent wallet to Binance deposit"), Some(W::AgentSend { usdc: Decimal::ONE, name: "Binance deposit".into() }));
    assert_eq!(p("send 1 USDC from the agent wallet to Frozen Yogurt"), Some(W::AgentSend { usdc: Decimal::ONE, name: "Frozen Yogurt".into() }));
    assert_eq!(p("положи 1 USDC с кошелька агента маме"), None);
}

#[test]
fn a_move_via_near_intents_is_read_in_english_and_russian() {
    let intents = |from: &str, to: &str, x: &str| W::Intents { from: from.into(), to: to.into(), amount: some(x) };
    assert_eq!(p("move 2 USDC from Base to Solana via NEAR Intents"), Some(intents("base", "solana", "2")));
    assert_eq!(p("переведи 2 USDC с бейза на солану через NEAR Intents"), Some(intents("base", "solana", "2")));
    assert_eq!(p("перекинь 3 USDC из соланы в арбитрум через неар интентс"), Some(intents("solana", "arbitrum", "3")));
    assert_eq!(p("move 2 USDC from Base to Solana via NEAR Intents").map(|c| c.summary()), Some("move 2 USDC from Base to Solana via NEAR Intents".into()));
    // Without the name it's the usual move, whichever bridge lands more.
    assert_ne!(p("move 2 USDC from Base to Solana"), Some(intents("base", "solana", "2")));
    // Polygon isn't one 1Click pays USDC on: asked about, never quietly sent over Relay.
    assert_eq!(p("move 2 USDC from Polygon to Solana via NEAR Intents"), None);
    // A deposit into Aave there isn't a bare move.
    assert_ne!(p("переведи 5 USDC с бейза в aave на арбитрум через NEAR Intents"), Some(intents("base", "arbitrum", "5")));
}
