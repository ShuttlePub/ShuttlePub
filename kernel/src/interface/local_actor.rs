use crate::entities::LocalActor;

/// Provides the actor this server speaks as: its id, inbox and public key.
pub trait LocalActorProvider: Send + Sync + 'static {
    fn local_actor(&self) -> &LocalActor;
}

pub trait DependOnLocalActorProvider: Send + Sync + 'static {
    type LocalActorProvider: LocalActorProvider;
    fn local_actor_provider(&self) -> &Self::LocalActorProvider;
}
