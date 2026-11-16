use kernel::entities::{Follow, RemoteActor};

/// A Follow as received, before any check: the four Follow fields as given and
/// the `Host` the request was addressed to (`None` if absent or not text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboundFollow {
    pub id: String,
    pub actor: String,
    pub object: String,
    pub host: Option<String>,
}

/// A Follow whose sender was resolved and whose claimed key belongs to it.
/// The request signature still has to be verified with `actor`'s key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentifiedFollow {
    follow: Follow,
    actor: RemoteActor,
}

impl IdentifiedFollow {
    pub fn follow(&self) -> &Follow {
        &self.follow
    }

    pub fn actor(&self) -> &RemoteActor {
        &self.actor
    }

    pub(crate) fn new(follow: Follow, actor: RemoteActor) -> Self {
        Self { follow, actor }
    }

    pub(crate) fn into_parts(self) -> (Follow, RemoteActor) {
        (self.follow, self.actor)
    }
}
