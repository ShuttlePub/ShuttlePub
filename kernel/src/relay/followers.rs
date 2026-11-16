use error_stack::Report;

use crate::{
    KernelError,
    identity::{ActivityId, ActorId},
};

/// Records that a remote actor follows the local actor.
pub trait FollowerRecorder: Send + Sync + 'static {
    /// Succeeds both for a new follower and for an actor that already follows
    /// (a resent Follow); the latter keeps what was first recorded.
    fn record(
        &self,
        actor: &ActorId,
        inbox: &str,
        follow: &ActivityId,
    ) -> impl Future<Output = Result<(), Report<KernelError>>> + Send;
}

pub trait DependOnFollowerRecorder: Send + Sync + 'static {
    type FollowerRecorder: FollowerRecorder;
    fn follower_recorder(&self) -> &Self::FollowerRecorder;
}

/// Lists the current followers of the local actor. The list follows the
/// recorded followers with a delay, so a follower recorded a moment ago may
/// not be listed yet.
pub trait FollowerReader: Send + Sync + 'static {
    fn list(&self) -> impl Future<Output = Result<Vec<FollowerEntry>, Report<KernelError>>> + Send;
}

pub trait DependOnFollowerReader: Send + Sync + 'static {
    type FollowerReader: FollowerReader;
    fn follower_reader(&self) -> &Self::FollowerReader;
}

/// A follower as listed: the actor and the inbox it is reached through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowerEntry {
    actor: ActorId,
    inbox: String,
}

impl FollowerEntry {
    pub fn new(actor: ActorId, inbox: String) -> Self {
        Self { actor, inbox }
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn inbox(&self) -> &str {
        &self.inbox
    }
}
