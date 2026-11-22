use std::{convert::Infallible, time::SystemTime};

use nitinol::{
    contract::{Aggregate, Decider, Decision, Never},
    persistence::AggregateId,
};

use super::NoteEvent;
use crate::id::{ActivityId, ActorId};

/// Prefix of every Note's aggregate id, which keeps the ids apart from the
/// other aggregates' and out of the namespace nitinol reserves.
const ID_PREFIX: &str = "note:";

/// One Note, identified by its object id. Its only fact is that it arrived;
/// whatever arrives later for the same Note adds nothing.
#[derive(Debug, Default)]
pub struct Note {
    received: bool,
}

impl Note {
    pub fn aggregate_id(note: &ActivityId) -> AggregateId {
        AggregateId::new(format!("{ID_PREFIX}{}", note.as_str()))
    }
}

/// Receives the Note `note`, written by `author` on the producer `producer`
/// (its authority), carried by the activity `activity`. `raw` is that
/// activity's JSON as it arrived; `received_at` is the caller's clock, as the
/// aggregate reads none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiveNote {
    pub activity: ActivityId,
    pub note: ActivityId,
    pub author: ActorId,
    pub producer: String,
    pub received_at: SystemTime,
    pub raw: String,
}

impl Aggregate for Note {
    type Event = NoteEvent;
    type Recover = Never;

    fn apply(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::Received { .. } => self.received = true,
        }
    }
}

impl Decider<ReceiveNote> for Note {
    type Output = ();
    type Rejection = Infallible;

    /// A Note that already arrived is not refused: it succeeds without an
    /// event, whichever activity carries it this time.
    fn decide(&self, command: ReceiveNote) -> Decision<NoteEvent, (), Infallible> {
        let events = if self.received {
            Vec::new()
        } else {
            vec![NoteEvent::Received {
                activity: command.activity,
                note: command.note,
                author: command.author,
                producer: command.producer,
                received_at: command.received_at,
                raw: command.raw,
            }]
        };
        Decision::persist(events).output(())
    }
}
