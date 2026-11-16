//! The followers of the local actor: the journal the follower aggregates are
//! recorded in, and (in `read_model`) the list projected from it.
mod read_model;

use std::convert::Infallible;

use error_stack::{Report, ResultExt};
use kernel::{
    KernelError,
    identity::{ActivityId, ActorId},
    relay::{AcceptFollow, Follower, FollowerRecorder},
};
use nitinol::{
    eventsource::{AskError, RetryPolicy, system::ContingentSystem},
    persistence::provider::Store,
};

pub use read_model::{FollowerReadModel, start_follower_projection};

use crate::persistence::EventSourcing;

/// Records followers as `Follower` aggregates in the event sourcing system's
/// journal, whichever store that system runs on.
pub struct FollowerJournal {
    system: ContingentSystem,
}

impl FollowerJournal {
    pub fn new<St: Store>(events: &EventSourcing<St>) -> Self {
        Self {
            system: events.system().to_contingent(),
        }
    }
}

impl FollowerRecorder for FollowerJournal {
    /// Transient failures (a lost optimistic-lock race, a backend that failed
    /// for a moment) are retried by nitinol under its default policy.
    async fn record(
        &self,
        actor: &ActorId,
        inbox: &str,
        follow: &ActivityId,
    ) -> Result<(), Report<KernelError>> {
        let follower = self
            .system
            .aggregate_proxy::<Follower>(Follower::aggregate_id(actor));
        let command = AcceptFollow::new(actor.clone(), inbox.to_owned(), follow.clone());
        record_outcome(
            follower
                .ask_with_retry(command, &RetryPolicy::default())
                .await,
        )
    }
}

/// `AlreadyCreated` means the follower's first event met one already in the
/// journal, i.e. the actor already follows: a resend, so a success. What else
/// is left after the retries is an internal failure.
fn record_outcome(outcome: Result<(), AskError<Infallible>>) -> Result<(), Report<KernelError>> {
    match outcome {
        Ok(()) | Err(AskError::AlreadyCreated) => Ok(()),
        Err(error) => Err(error).change_context(KernelError::Internal),
    }
}

#[cfg(test)]
mod tests;
