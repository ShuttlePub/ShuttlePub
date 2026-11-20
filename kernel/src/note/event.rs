use std::time::SystemTime;

use bytes::Bytes;
use nitinol::contract::{Codec, Event};
use serde::{Deserialize, Serialize, de::Error as _};

use crate::id::{ActivityId, ActorId};

/// What happened to a Note. Stored as `shuttlepub.note.v1.NoteEvent`.
#[derive(Clone, Debug, PartialEq, Eq, Event)]
#[event(family = "shuttlepub.note.v1", codec = NoteCodec)]
pub enum NoteEvent {
    /// The Note `note`, written by `author` on the producer `producer`, arrived
    /// in the activity `activity` at `received_at`; `raw` is that activity's
    /// JSON as it arrived.
    Received {
        activity: ActivityId,
        note: ActivityId,
        author: ActorId,
        producer: String,
        received_at: SystemTime,
        raw: String,
    },
}

/// Writes a `NoteEvent` as JSON. The shape is `StoredNoteEvent`'s and is part
/// of what is stored: changing it makes stored events unreadable.
pub struct NoteCodec;

impl Codec<NoteEvent> for NoteCodec {
    type Error = serde_json::Error;

    fn encode(event: &NoteEvent) -> Result<Bytes, serde_json::Error> {
        serde_json::to_vec(&StoredNoteEvent::from(event)).map(Bytes::from)
    }

    fn decode(payload: &[u8]) -> Result<NoteEvent, serde_json::Error> {
        serde_json::from_slice::<StoredNoteEvent>(payload)?.try_into()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum StoredNoteEvent {
    Received {
        activity: String,
        note: String,
        author: String,
        producer: String,
        received_at: SystemTime,
        raw: String,
    },
}

impl From<&NoteEvent> for StoredNoteEvent {
    fn from(event: &NoteEvent) -> Self {
        match event {
            NoteEvent::Received {
                activity,
                note,
                author,
                producer,
                received_at,
                raw,
            } => Self::Received {
                activity: activity.as_str().to_owned(),
                note: note.as_str().to_owned(),
                author: author.as_str().to_owned(),
                producer: producer.clone(),
                received_at: *received_at,
                raw: raw.clone(),
            },
        }
    }
}

impl TryFrom<StoredNoteEvent> for NoteEvent {
    type Error = serde_json::Error;

    fn try_from(stored: StoredNoteEvent) -> Result<Self, serde_json::Error> {
        match stored {
            StoredNoteEvent::Received {
                activity,
                note,
                author,
                producer,
                received_at,
                raw,
            } => Ok(Self::Received {
                activity: stored_id("activity id", activity)?,
                note: stored_id("Note id", note)?,
                author: ActorId::parse(author).map_err(|report| {
                    serde_json::Error::custom(format!("stored author: {report}"))
                })?,
                producer,
                received_at,
                raw,
            }),
        }
    }
}

fn stored_id(what: &str, raw: String) -> Result<ActivityId, serde_json::Error> {
    ActivityId::parse(raw)
        .map_err(|report| serde_json::Error::custom(format!("stored {what}: {report}")))
}
