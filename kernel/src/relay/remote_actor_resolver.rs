use error_stack::Report;

use super::RemoteActor;
use crate::{KernelError, identity::ActorId};

/// Fetches an actor from another server. Implementations only contact allowed
/// origins, bound the response size and time out.
pub trait RemoteActorResolver: Send + Sync + 'static {
    fn resolve(
        &self,
        id: &ActorId,
    ) -> impl Future<Output = Result<RemoteActor, Report<KernelError>>> + Send;
}

pub trait DependOnRemoteActorResolver: Send + Sync + 'static {
    type RemoteActorResolver: RemoteActorResolver;
    fn remote_actor_resolver(&self) -> &Self::RemoteActorResolver;
}
