use kernel::community::{DependOnLocalActorProvider, LocalActor, LocalActorProvider};

/// Publishes the local actor.
pub trait LocalActorService: DependOnLocalActorProvider {
    fn get_local_actor(&self) -> &LocalActor {
        self.local_actor_provider().local_actor()
    }
}

impl<T: DependOnLocalActorProvider> LocalActorService for T {}

pub trait DependOnLocalActorService: Send + Sync + 'static {
    type LocalActorService: LocalActorService;
    fn local_actor_service(&self) -> &Self::LocalActorService;
}
