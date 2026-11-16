use nitinol::{
    eventsource::system::EventSourceSystem,
    persistence::provider::{PersistenceProvider, Store},
    runtime::ProcessSystem,
};

/// The nitinol event sourcing system over `St`: `PostgresStore` when serving,
/// `InMemoryStore` in tests that need no database.
pub struct EventSourcing<St> {
    system: EventSourceSystem<St>,
}

impl<St: Store> EventSourcing<St> {
    pub async fn start(store: St) -> Self {
        let processes = ProcessSystem::new().await;
        let system = EventSourceSystem::builder(processes)
            .with_persistence(PersistenceProvider::new(store))
            .build();
        Self { system }
    }

    pub(crate) fn system(&self) -> &EventSourceSystem<St> {
        &self.system
    }
}
