use super::d;
use crate::moved_line::{Kind, MovedLine};

#[test]
fn history_lines_say_what_moved_and_where() {
    let sent = MovedLine::new("Sent 5 USDT to Automation on Solana. https://solscan.io/tx/4uH2y76CkZcaLF6x22gY2Kqc2iiwhsGcTyGnPNUqKu1rXm7DfWkss7yHhQJfNW4TQYCQZ7qiu73GTrAEHU5MxhFX").unwrap();
    assert!(sent.kind == Kind::Sent && sent.text == "Sent 5 USDT to Automation on Solana" && sent.amount == Some(d("5")) && sent.symbol.as_deref() == Some("USDT"));
    assert!(sent.to.as_deref() == Some("Automation") && sent.link.as_deref().is_some_and(|l| l.starts_with("https://solscan.io/")));
    let swap = MovedLine::new("Swapped 3 USDC → USDT +2.997906.").unwrap();
    assert!(swap.kind == Kind::Swapped && swap.text == "Swapped 3 USDC → USDT +2.997906" && swap.amount == Some(d("3")) && swap.symbol.as_deref() == Some("USDC"));
    let came = MovedLine::new("💸 Received +55.09 USDC on Solana (now 55.09).").unwrap();
    assert!(came.kind == Kind::Received && came.text == "Received +55.09 USDC on Solana" && came.amount == Some(d("55.09")));
    assert_eq!(MovedLine::new("💸 Пришло +1,250.5 USDC на Solana (сейчас 1,300).").unwrap().amount, Some(d("1250.5")));
    assert_eq!(MovedLine::new("Moved 1 USDC to Arbitrum: 1 USDC there now. Say “move USDC from Arbitrum to Solana” to bring it back.").unwrap().text, "Moved 1 USDC to Arbitrum");
    // Between the wallet's own networks it's a move, not money leaving.
    assert_eq!(MovedLine::new("Sent 5 USDT from Arbitrum to Optimism over USDT0: 5 USDT lands there in 2–4 min").unwrap().kind, Kind::Moved);
    assert_eq!(MovedLine::new("Sent 0.25 SOL to Bob on Solana.").unwrap().to.as_deref(), Some("Bob"));
    // Plans, answers and cancellations aren't moves.
    for line in ["USDC → USDT via Jupiter: 3 → ≈ 3.00362, at least 2.988602.", "Cancelled — nothing moved.", "Понял: deposit 1 USDC into Jupiter Lend."] {
        assert_eq!(MovedLine::new(line), None);
    }
    let day: Vec<MovedLine> = ["Sent 2 USDT to Automation on Solana.", "Sent 10 USDT to Automation on Solana.", "Sent 13 USDT to Automation on Solana.",
                               "Swapped 2 USDC → USDT +2.00242.", "💸 Received +1 USDC on Base (now 2.42)."].iter().filter_map(|l| MovedLine::new(l)).collect();
    assert_eq!(MovedLine::summary(&day), ["−25 USDT sent to Automation · 3 sends", "+1 USDC came in", "1 swap, the money stays in the wallet"]);
}
