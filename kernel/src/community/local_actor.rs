use error_stack::{Report, ResultExt};
use url::Url;

use super::PublicKey;
use crate::{KernelError, id::ActorId};

/// The actor this server speaks as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalActor {
    id: ActorId,
    inbox: Url,
    public_key: PublicKey,
}

impl LocalActor {
    /// Fails for actor URLs that cannot be a base (e.g. `mailto:`), because the
    /// inbox and the Accept ids are derived from the actor URL's path.
    pub fn new(id: Url, public_key_pem: impl Into<String>) -> Result<Self, Report<KernelError>> {
        let inbox = id.join("inbox").change_context(KernelError::Internal)?;
        let public_key = PublicKey::new(format!("{id}#main-key"), id.as_str(), public_key_pem);
        Ok(Self {
            id: ActorId::from(id),
            inbox,
            public_key,
        })
    }

    pub fn id(&self) -> &ActorId {
        &self.id
    }

    pub fn inbox(&self) -> &Url {
        &self.inbox
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// `host[:port]` of the actor URL, the value a request addressed to this
    /// server carries in its `Host` header.
    pub fn authority(&self) -> &str {
        self.id.authority()
    }
}
