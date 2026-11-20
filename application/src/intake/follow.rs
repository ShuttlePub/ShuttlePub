use kernel::{intake::RemoteActor, producer::Follow};

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
/// The request signature still has to be verified with the sender's key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentifiedFollow {
    follow: Follow,
    sender: RemoteActor,
}

impl IdentifiedFollow {
    pub(super) fn new(follow: Follow, sender: RemoteActor) -> Self {
        Self { follow, sender }
    }

    pub fn follow(&self) -> &Follow {
        &self.follow
    }

    pub fn sender(&self) -> &RemoteActor {
        &self.sender
    }
}
