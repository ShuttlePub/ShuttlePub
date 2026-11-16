use bytes::Bytes;
use nitinol::contract::{Codec, Event};
use serde::{Deserialize, Serialize};

/// What happened to a follower. Stored in the journal under the type
/// `shuttlepub.relay.follower.FollowerEvent` as JSON tagged by `type`; events
/// already stored are read back with the current definition, so the family,
/// the type name and the JSON shape must keep reading them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Event)]
#[event(family = "shuttlepub.relay.follower", codec = FollowerEventCodec)]
#[serde(tag = "type")]
pub enum FollowerEvent {
    /// The actor's Follow was accepted: it follows the local actor through
    /// `inbox`.
    Followed {
        actor: String,
        inbox: String,
        follow: String,
    },
}

/// The JSON form of `FollowerEvent` in the journal.
pub struct FollowerEventCodec;

impl Codec<FollowerEvent> for FollowerEventCodec {
    type Error = serde_json::Error;

    fn encode(event: &FollowerEvent) -> Result<Bytes, Self::Error> {
        serde_json::to_vec(event).map(Bytes::from)
    }

    fn decode(payload: &[u8]) -> Result<FollowerEvent, Self::Error> {
        serde_json::from_slice(payload)
    }
}
