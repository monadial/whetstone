//! Whetstone's AI port: what Practice asks of Jev, the evaluator, and of
//! the generator, behind Whetstone types only, so the rest of the app never
//! sees how a request is made. A scripted implementation stands in for the
//! real one in tests, with the `scripted` feature.

mod port;
#[cfg(any(test, feature = "scripted"))]
mod scripted;

pub use crate::port::{Ai, CandidateCount, Pending, Reply, RequestFailure};
#[cfg(any(test, feature = "scripted"))]
pub use crate::scripted::{Asked, ScriptedAi};
