use nitinol::{eventsource::system::EventSourceSystem, persistence::provider::Store};

/// Hands out the event-sourcing system the aggregates run on and the
/// reactions read from. The store is a type parameter, so production
/// (PostgreSQL) and tests (in memory) run the same services.
pub trait DependOnEventSourceSystem: Send + Sync + 'static {
    type Store: Store;
    fn event_source_system(&self) -> &EventSourceSystem<Self::Store>;
}
