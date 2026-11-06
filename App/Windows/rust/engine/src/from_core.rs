//! IhatecryptoCore's types the engine keeps or draws, under the names layer 1 gave them before core's round 4 landed:
//! Normalize.swift's `ChatRouter.Option`, MPP.swift's `Answer` and `Challenge`, ModelExam.swift's `Result`. Now core's
//! own types; the importers can name them directly, and this file go, when both sides of layer 2 are in.

pub use ihatecrypto_core::model_exam::Score as ExamResult;
pub use ihatecrypto_core::mpp::{Answer as MppAnswer, Challenge as MppChallenge, Fact as MppFact, Item as MppItem};
pub use ihatecrypto_core::normalize::Choice as RouterOption;
