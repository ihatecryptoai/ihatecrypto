//! The offline cases of App/Tests that exercise the executor (ExecutorTests, ExecutorCloseTests, LimitFillTests,
//! PartialTakeTests, TrailingStopTests, TwapExecutionTests, HedgeRiskTests, and the executor's parts of EVMTests,
//! UniswapRoutesTests and Batch7702Tests), one file per module; FakeHL is the stand-in exchange they share.

mod aave;
mod aave_borrow;
mod agent_reads;
mod approvals;
mod cctp_bridge;
mod contract_calls;
mod cross_swap;
mod evm_places;
mod exchange;
mod executor;
mod fake_hl;
mod g2_fake;
mod g4_fake;
mod hl_spot;
mod kamino_borrow;
mod limit_order;
mod multisig;
mod one_click_bridge;
mod send;
mod stocks;
mod swap;
mod transit;
mod wallet;

pub(crate) fn d(s: &str) -> rust_decimal::Decimal { s.parse().unwrap() }
