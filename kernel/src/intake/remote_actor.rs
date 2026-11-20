use error_stack::Report;

use crate::{KernelError, community::PublicKey, id::ActorId};

/// Actor fetched from another server whose document names the requested actor
/// and whose key is owned by that actor.
///
/// The inbox is kept as published; checking that it is an allowed delivery
/// destination belongs to the adapter that resolved the actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteActor {
    inbox: String,
    public_key: PublicKey,
}

impl RemoteActor {
    pub fn new(
        requested: &ActorId,
        id: &str,
        inbox: impl Into<String>,
        public_key: PublicKey,
    ) -> Result<Self, Report<KernelError>> {
        if id != requested.as_str() || public_key.owner() != id {
            return Err(Report::new(KernelError::UntrustedIdentity));
        }
        Ok(Self {
            inbox: inbox.into(),
            public_key,
        })
    }

    pub fn inbox(&self) -> &str {
        &self.inbox
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
}
