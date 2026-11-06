//! A request of several steps the model carries on with (Agent.swift).
//! Layer 2 adds here: continue_agent and the rest of the file's `extension Engine`.

use ihatecrypto_core::foundation::Date;

/// A request the model carries on with: after each command (or its Confirm, when it moves money) it hears how that went
/// and takes the next step, at most `AGENT_ROUNDS` in all — the Mac's continueAgent.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentGoal {
    pub request: String,
    pub command: String,
    /// Messages before the step: what was said since is what the model reads.
    pub mark: i64,
    pub round: i64,
    pub since: Date,
    /// What the model said it will do after this step: on the card waiting for Confirm («NEXT · AFTER THIS»).
    pub next: Option<String>,
}

impl AgentGoal {
    pub fn new(request: &str, command: &str, mark: i64, round: i64) -> Self {
        AgentGoal { request: request.into(), command: command.into(), mark, round, since: chrono::Utc::now(), next: None }
    }
}

pub const AGENT_ROUNDS: i64 = 5;
