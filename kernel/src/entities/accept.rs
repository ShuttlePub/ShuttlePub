use error_stack::Report;
use url::Url;

use super::{ActorId, Follow, LocalActor};
use crate::KernelError;

/// The local actor's answer to a Follow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accept {
    id: Url,
    actor: ActorId,
    object: Follow,
}

impl Accept {
    /// The id is `{local actor}/accepts/{Follow id}` with the Follow id as one
    /// percent-encoded path segment, so every Follow gets its own Accept id.
    pub fn new(local: &LocalActor, follow: Follow) -> Result<Self, Report<KernelError>> {
        let mut id = local.id().as_url().clone();
        id.path_segments_mut()
            .map_err(|()| Report::new(KernelError::Internal))?
            .extend(["accepts", follow.id().as_str()]);
        Ok(Self {
            id,
            actor: local.id().clone(),
            object: follow,
        })
    }

    pub fn id(&self) -> &Url {
        &self.id
    }

    pub fn actor(&self) -> &ActorId {
        &self.actor
    }

    pub fn object(&self) -> &Follow {
        &self.object
    }
}
