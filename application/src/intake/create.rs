use kernel::{
    id::{ActivityId, ActorId},
    intake::RemoteActor,
};

/// The spellings of the ActivityStreams public collection a receiver must
/// understand in an audience.
const PUBLIC: [&str; 3] = [
    "https://www.w3.org/ns/activitystreams#Public",
    "as:Public",
    "Public",
];

/// A Create of an embedded Note as received, before any check: the fields as
/// given, every entry of its `to` and `cc`, the activity's JSON as it arrived,
/// and the `Host` the request was addressed to (`None` if absent or not text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboundCreate {
    pub id: String,
    pub actor: String,
    pub note: String,
    pub attributed_to: String,
    pub audience: Vec<String>,
    pub raw: String,
    pub host: Option<String>,
}

/// A Create whose ids are URLs, whose sender was resolved and whose claimed
/// key belongs to it. The request signature still has to be verified with the
/// sender's key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentifiedCreate {
    pub(super) activity: ActivityId,
    pub(super) actor: ActorId,
    pub(super) note: ActivityId,
    pub(super) attributed_to: String,
    pub(super) audience: Vec<String>,
    pub(super) raw: String,
    pub(super) sender: RemoteActor,
}

impl IdentifiedCreate {
    pub fn sender(&self) -> &RemoteActor {
        &self.sender
    }

    /// Whether the Note is written by the Create's actor on the actor's own
    /// origin, i.e. the sender speaks for the Note it carries.
    pub(super) fn is_by_its_actor(&self) -> bool {
        self.attributed_to == self.actor.as_str()
            && self.note.as_url().origin() == self.actor.as_url().origin()
    }

    pub(super) fn is_addressed_to_the_public(&self) -> bool {
        self.audience
            .iter()
            .any(|entry| PUBLIC.contains(&entry.as_str()))
    }
}
