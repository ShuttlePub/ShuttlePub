use std::convert::Infallible;

use nitinol::{
    contract::{Aggregate, Decider, Decision, Never},
    persistence::AggregateId,
};

use super::FollowerEvent;
use crate::identity::{ActivityId, ActorId};

/// Prefix of every follower's aggregate id, so that a follower's stream cannot
/// share a key with another aggregate's.
const AGGREGATE_ID_PREFIX: &str = "shuttlepub.relay.follower:";

/// One remote actor's relation to the local actor: whether it follows it, and
/// if so through which inbox and which accepted Follow.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Follower {
    #[default]
    NotFollowing,
    Following {
        actor: String,
        inbox: String,
        follow: String,
    },
}

impl Follower {
    /// The aggregate of `actor`: the same actor always maps to the same id.
    pub fn aggregate_id(actor: &ActorId) -> AggregateId {
        AggregateId::new(format!("{AGGREGATE_ID_PREFIX}{}", actor.as_str()))
    }
}

impl Aggregate for Follower {
    type Event = FollowerEvent;
    type Recover = Never;

    fn apply(&mut self, event: FollowerEvent) {
        match event {
            FollowerEvent::Followed {
                actor,
                inbox,
                follow,
            } => {
                *self = Self::Following {
                    actor,
                    inbox,
                    follow,
                }
            }
        }
    }
}

/// A Follow of the local actor that passed every check and is being accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptFollow {
    actor: ActorId,
    inbox: String,
    follow: ActivityId,
}

impl AcceptFollow {
    pub fn new(actor: ActorId, inbox: String, follow: ActivityId) -> Self {
        Self {
            actor,
            inbox,
            follow,
        }
    }
}

impl Decider<AcceptFollow> for Follower {
    type Output = ();
    type Rejection = Infallible;

    /// A Follow from an actor that already follows is a resend and is
    /// accepted without an event, even when it names another inbox or has
    /// another id: the follower keeps the inbox and Follow it was first
    /// recorded with.
    fn decide(&self, cmd: AcceptFollow) -> Decision<FollowerEvent, (), Infallible> {
        match self {
            Self::NotFollowing => Decision::persist(vec![FollowerEvent::Followed {
                actor: cmd.actor.as_str().to_owned(),
                inbox: cmd.inbox,
                follow: cmd.follow.as_str().to_owned(),
            }])
            .output(()),
            Self::Following { .. } => Decision::persist(Vec::new()).output(()),
        }
    }
}
