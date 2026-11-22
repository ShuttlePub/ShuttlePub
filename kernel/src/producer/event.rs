use bytes::Bytes;
use nitinol::contract::{Codec, Event};
use serde::{Deserialize, Serialize, de::Error as _};

use crate::id::{ActivityId, ActorId};

/// What happened to a producer. Stored as `shuttlepub.producer.v1.ProducerEvent`.
#[derive(Clone, Debug, PartialEq, Eq, Event)]
#[event(family = "shuttlepub.producer.v1", codec = ProducerCodec)]
pub enum ProducerEvent {
    /// The federation `actor` asked for with the Follow `follow` was accepted;
    /// its Accept is to be delivered to `inbox`.
    FederationAccepted {
        actor: ActorId,
        inbox: String,
        follow: ActivityId,
    },
    /// The Accept of the Follow `follow` reached the producer's inbox.
    AcceptDelivered { follow: ActivityId },
    /// The Accept of the Follow `follow` did not reach the producer's inbox.
    AcceptFailed { follow: ActivityId, reason: String },
}

/// Writes a `ProducerEvent` as JSON. The shape is `StoredProducerEvent`'s and
/// is part of what is stored: changing it makes stored events unreadable.
pub struct ProducerCodec;

impl Codec<ProducerEvent> for ProducerCodec {
    type Error = serde_json::Error;

    fn encode(event: &ProducerEvent) -> Result<Bytes, serde_json::Error> {
        serde_json::to_vec(&StoredProducerEvent::from(event)).map(Bytes::from)
    }

    fn decode(payload: &[u8]) -> Result<ProducerEvent, serde_json::Error> {
        serde_json::from_slice::<StoredProducerEvent>(payload)?.try_into()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum StoredProducerEvent {
    FederationAccepted {
        actor: String,
        inbox: String,
        follow: String,
    },
    AcceptDelivered {
        follow: String,
    },
    AcceptFailed {
        follow: String,
        reason: String,
    },
}

impl From<&ProducerEvent> for StoredProducerEvent {
    fn from(event: &ProducerEvent) -> Self {
        match event {
            ProducerEvent::FederationAccepted {
                actor,
                inbox,
                follow,
            } => Self::FederationAccepted {
                actor: actor.as_str().to_owned(),
                inbox: inbox.clone(),
                follow: follow.as_str().to_owned(),
            },
            ProducerEvent::AcceptDelivered { follow } => Self::AcceptDelivered {
                follow: follow.as_str().to_owned(),
            },
            ProducerEvent::AcceptFailed { follow, reason } => Self::AcceptFailed {
                follow: follow.as_str().to_owned(),
                reason: reason.clone(),
            },
        }
    }
}

impl TryFrom<StoredProducerEvent> for ProducerEvent {
    type Error = serde_json::Error;

    fn try_from(stored: StoredProducerEvent) -> Result<Self, serde_json::Error> {
        Ok(match stored {
            StoredProducerEvent::FederationAccepted {
                actor,
                inbox,
                follow,
            } => Self::FederationAccepted {
                actor: ActorId::parse(actor).map_err(|report| {
                    serde_json::Error::custom(format!("stored actor: {report}"))
                })?,
                inbox,
                follow: stored_follow(follow)?,
            },
            StoredProducerEvent::AcceptDelivered { follow } => Self::AcceptDelivered {
                follow: stored_follow(follow)?,
            },
            StoredProducerEvent::AcceptFailed { follow, reason } => Self::AcceptFailed {
                follow: stored_follow(follow)?,
                reason,
            },
        })
    }
}

fn stored_follow(raw: String) -> Result<ActivityId, serde_json::Error> {
    ActivityId::parse(raw)
        .map_err(|report| serde_json::Error::custom(format!("stored Follow id: {report}")))
}
