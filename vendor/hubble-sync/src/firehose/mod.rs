mod event;
mod event_validation;
mod subscribe_repos;
mod tolerant;

pub use event::{FirehoseAck, FirehoseEvent, FirehosePayload};
pub use event_validation::{FirehoseCommit, FirehoseSync, FirehoseValidationError};
pub use subscribe_repos::{FirehoseConfig, FirehoseError, FirehoseSubscriber};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seq(pub(crate) u64);

impl Seq {
    /// return a sequence just-before the current one
    fn prev(&self) -> Seq {
        Seq(self.0.saturating_sub(1))
    }
    fn is_non_zero(&self) -> bool {
        self.0 > 0
    }
    pub(crate) fn as_u64(&self) -> u64 {
        self.0
    }
}
