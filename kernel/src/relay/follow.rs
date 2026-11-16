use crate::identity::{ActivityId, ActorId};

/// A Follow addressed to the local actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Follow {
    id: ActivityId,
    actor: ActorId,
    object: ActorId,
}

impl Follow {
    pub fn new(id: ActivityId, actor: ActorId, object: ActorId) -> Self {
        Self { id, actor, object }
    }

    pub fn id(&self) -> &ActivityId {
        &self.id
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn object(&self) -> &ActorId {
        &self.object
    }
}
