//! The chat's own commands (ChatCommands.swift): wallets, folders, notes, tokens by their symbol. Its WalletScope (whose
//! per-wallet records are in memory) is `State::wallet_scope_costs` / `wallet_scope_tasks`; `Engine.scoped` is
//! `store::scoped`.
//! Layer 2 adds here: the wallet commands (use_wallet, load_wallet_state, describe, wallet_removed), folder and note
//! commands, token commands and the rest of the file's `extension Engine`.
