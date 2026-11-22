use std::convert::Infallible;

use nitinol::{
    contract::{Aggregate, Decider, Decision, Never, Query},
    persistence::AggregateId,
};

use super::{AcceptFederation, ProducerEvent, RecordAcceptDelivered, RecordAcceptFailed};
use crate::id::{ActivityId, ActorId};

/// Prefix of every producer's aggregate id, which keeps the ids apart from
/// the other aggregates' and out of the namespace nitinol reserves.
const ID_PREFIX: &str = "producer:";

/// One service that asked to federate. Not federated until a federation is
/// accepted; from then on it is accepted, whether or not its Accept arrived.
///
/// No command is refused: a command that changes nothing succeeds without an
/// event.
#[derive(Debug, Default)]
pub struct Producer(Option<Federation>);

/// The accepted state: who asked, where its Accept goes, which Follow it
/// answers and whether that Accept arrived.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "`actor` and `inbox` are part of the accepted state, but only the Follow \
              and whether its Accept arrived decide commands"
)]
struct Federation {
    actor: ActorId,
    inbox: String,
    follow: ActivityId,
    accept_delivered: bool,
}

impl Producer {
    /// The producer `actor` belongs to. A service is told apart by the
    /// authority of its actors' URLs, because a Note is signed by its author,
    /// not by the actor that sent the Follow.
    pub fn aggregate_id(actor: &ActorId) -> AggregateId {
        AggregateId::new(format!("{ID_PREFIX}{}", actor.authority()))
    }

    /// Whether the Accept of `follow`, the Follow last accepted, has not been
    /// reported delivered yet.
    fn awaits_accept_of(&self, follow: &ActivityId) -> bool {
        self.0
            .as_ref()
            .is_some_and(|federation| federation.follow == *follow && !federation.accept_delivered)
    }
}

impl Aggregate for Producer {
    type Event = ProducerEvent;
    type Recover = Never;

    fn apply(&mut self, event: ProducerEvent) {
        match event {
            ProducerEvent::FederationAccepted {
                actor,
                inbox,
                follow,
            } => {
                self.0 = Some(Federation {
                    actor,
                    inbox,
                    follow,
                    accept_delivered: false,
                });
            }
            ProducerEvent::AcceptDelivered { follow } => {
                if let Some(federation) = self.0.as_mut().filter(|f| f.follow == follow) {
                    federation.accept_delivered = true;
                }
            }
            ProducerEvent::AcceptFailed { .. } => {}
        }
    }
}

impl Decider<AcceptFederation> for Producer {
    type Output = ();
    type Rejection = Infallible;

    /// Records the federation unless this very Follow was already answered by
    /// a delivered Accept. A Follow sent again after its Accept failed, or a
    /// Follow with another id, is recorded again so that it gets its Accept.
    fn decide(&self, command: AcceptFederation) -> Decision<ProducerEvent, (), Infallible> {
        let answered = self.0.as_ref().is_some_and(|federation| {
            federation.follow == command.follow && federation.accept_delivered
        });
        let events = if answered {
            Vec::new()
        } else {
            vec![ProducerEvent::FederationAccepted {
                actor: command.actor,
                inbox: command.inbox,
                follow: command.follow,
            }]
        };
        Decision::persist(events).output(())
    }
}

impl Decider<RecordAcceptDelivered> for Producer {
    type Output = ();
    type Rejection = Infallible;

    /// Only the Accept of the Follow last accepted counts, and only once.
    fn decide(&self, command: RecordAcceptDelivered) -> Decision<ProducerEvent, (), Infallible> {
        let events = if self.awaits_accept_of(&command.follow) {
            vec![ProducerEvent::AcceptDelivered {
                follow: command.follow,
            }]
        } else {
            Vec::new()
        };
        Decision::persist(events).output(())
    }
}

impl Decider<RecordAcceptFailed> for Producer {
    type Output = ();
    type Rejection = Infallible;

    /// Only an Accept still awaited for the Follow last accepted can fail.
    fn decide(&self, command: RecordAcceptFailed) -> Decision<ProducerEvent, (), Infallible> {
        let events = if self.awaits_accept_of(&command.follow) {
            vec![ProducerEvent::AcceptFailed {
                follow: command.follow,
                reason: command.reason,
            }]
        } else {
            Vec::new()
        };
        Decision::persist(events).output(())
    }
}

/// Whether the producer's federation was accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsAccepted;

impl Query<IsAccepted> for Producer {
    type Response = bool;
    type Error = Infallible;

    fn query(&self, _: IsAccepted) -> Result<bool, Infallible> {
        Ok(self.0.is_some())
    }
}
