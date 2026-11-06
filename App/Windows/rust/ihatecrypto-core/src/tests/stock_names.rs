use crate::stock_names::{Ondo, symbol};

/// No Swift test calls StockNames directly (WalletCommand's buys do); these are its own cases.
#[test]
fn company_names_and_tickers_become_stock_symbols() {
    assert_eq!(symbol("купи акции SpaceX на 5$").as_deref(), Some("SPCX"));
    assert_eq!(symbol("buy тесла for 2 USDC").as_deref(), Some("TSLA"));
    assert_eq!(symbol("buy $NVDA stock").as_deref(), Some("NVDA"));
    assert_eq!(symbol("buy USDC now"), None);
    assert_eq!(symbol("buy SOL for 5 USDT"), None);
}

#[test]
fn ondos_own_mints() {
    assert!(Ondo::issued("gEGtLTPNQ7jcg25zTetkbmF7teoDLcrfTnQfmn2ondo") && Ondo::issued(Ondo::USDY) && !Ondo::issued(crate::network::Solana::USDC_MINT));
    assert!(Ondo::terms(true).starts_with("Ondo не продаёт") && Ondo::terms(false).ends_with("needs KYC."));
}
