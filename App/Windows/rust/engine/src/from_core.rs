//! IhatecryptoCore's types under the names the engine's modules use for them: Normalize.swift's `ChatRouter.Option`
//! (core's `Choice`), MPP.swift's `Answer` and `Challenge`, ModelExam.swift's `Result` (core's `Score`). Names only:
//! the types are core's.

pub use ihatecrypto_core::model_exam::Score as ExamResult;
pub use ihatecrypto_core::mpp::{Answer as MppAnswer, Challenge as MppChallenge, Fact as MppFact, Item as MppItem};
pub use ihatecrypto_core::normalize::Choice as RouterOption;
