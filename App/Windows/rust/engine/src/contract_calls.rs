//! A contract call the chat wrote out (ContractCalls.swift): «call arbitrum 0x… deposit(uint256,address) 10e6 me
//! approving 10 USDC», checked and simulated, waiting for Confirm.
//! Layer 2 adds here: review_contract_call (encoding, the explorer's check, the simulation), its card, run_contract_call,
//! look_up_contract.

use ihatecrypto_core::position_command::{ContractCall, WalletCommand};
use rust_decimal::Decimal;
use trade::EvmNetwork;
use trade::contract_calls::ContractTrust;

/// A contract call checked and simulated, waiting for Confirm. The model only writes the command: ihatecrypto encodes it
/// from the function's signature, checks the contract on the network's explorer, simulates the exact approve and the
/// call together and draws the card; anything it can't decode, verify or simulate isn't offered (it fails closed). The
/// signer reads the calldata again: no unlimited approvals, nothing paid to someone else, a new spender confirmed in person.
#[derive(Debug, Clone)]
pub struct CallReview {
    pub command: WalletCommand,
    pub call: ContractCall,
    pub net: EvmNetwork,
    pub data: Vec<u8>,
    /// Wei of the network's coin sent with the call.
    pub value: u64,
    pub approve: Option<CallApprove>,
    /// What it does, read from the calldata.
    pub lines: Vec<String>,
    /// The simulated change of each of this wallet's balances.
    pub changes: Vec<String>,
    pub trust: ContractTrust,
    pub warnings: Vec<String>,
    /// The simulated calls and what they did: run again just before signing (`run_contract_call`).
    pub calls: Vec<SimulatedCall>,
    pub simulation: evm::simulation::Simulation,
    /// «First time with this contract», «proxy: its owner can change the code»: on the card, above everything.
    pub badges: Vec<String>,
}

/// `CallReview.Approve`: the exact approve made first.
#[derive(Debug, Clone, PartialEq)]
pub struct CallApprove {
    pub token: String,
    pub symbol: String,
    pub amount: Decimal,
    pub units: Decimal,
}

/// One call of the simulation, as it's sent again.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulatedCall {
    pub to: String,
    pub value: u64,
    pub data: Vec<u8>,
}

impl CallReview {
    /// «FluidLending» or «0x1a99…6096».
    pub fn contract_name(&self) -> String {
        self.trust.implementation.clone().or_else(|| self.trust.name.clone()).unwrap_or_else(|| {
            let c: Vec<char> = self.call.contract.chars().collect();
            format!("{}…{}", c.iter().take(6).collect::<String>(), c[c.len().saturating_sub(4)..].iter().collect::<String>())
        })
    }
}
