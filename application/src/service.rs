use std::{convert::Infallible, sync::Arc};

use kernel::{GetNote, Note, NoteCommand, NoteError, NoteEvent, NoteId};
use nitinol::{
    eventsource::{
        ProjectorProps, SnapshotPersistor,
        error::{AskError, ExecError},
        system::{EventSourceSystem, StoreSet},
    },
    persistence::{
        AggregateId, ProjectionId,
        store::{
            EventStore, InMemoryCheckpointStore, InMemoryEventStore, InMemorySnapshotStore,
            SnapshotStore,
        },
    },
    runtime::{ProcessSystem, ident::Pid},
};

use crate::{
    codec::JsonCodec,
    timeline::{Timeline, TimelineEntry},
};

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Command(#[from] AskError<NoteError>),
    #[error(transparent)]
    Query(#[from] ExecError<Infallible>),
    #[error("stream identity differs from note identity")]
    IdentityMismatch,
    #[error(transparent)]
    Stop(#[from] nitinol::runtime::error::SendError),
}

pub struct NoteService {
    system: EventSourceSystem<JsonCodec, StoreSet>,
    snapshots: nitinol::eventsource::SnapshotPersistorProxy,
    timeline: Timeline,
    projector: Pid,
}

impl NoteService {
    pub async fn in_memory() -> Self {
        Self::start(
            Arc::new(InMemoryEventStore::default()),
            Arc::new(InMemorySnapshotStore::default()),
        )
        .await
    }

    pub async fn start(events: Arc<dyn EventStore>, snapshots: Arc<dyn SnapshotStore>) -> Self {
        let system = EventSourceSystem::builder(ProcessSystem::new().await)
            .with_codec::<JsonCodec>()
            .with_event_store(events.clone())
            .build();
        let timeline = Timeline::default();
        let projection_model = timeline.clone();
        let projector = ProjectorProps::new(
            ProjectionId::new("timeline"),
            events,
            Arc::new(InMemoryCheckpointStore::default()),
            move || projection_model.clone(),
        )
        .with_event::<NoteEvent>(system.codec::<NoteEvent>())
        .catchup_from_global()
        .spawn(system.process_system())
        .await;
        let snapshots = SnapshotPersistor::spawn(system.process_system(), snapshots).await;
        Self {
            system,
            snapshots,
            timeline,
            projector: projector.pid(),
        }
    }

    async fn aggregate(&self, id: NoteId) -> nitinol::eventsource::AggregateProxy<Note> {
        self.system
            .aggregate_props::<Note>(AggregateId::new(id.0.to_string()))
            .with_snapshot_persistor(self.snapshots.clone(), Arc::new(JsonCodec))
            .spawn(self.system.process_system())
            .await
    }

    pub async fn execute(&self, id: NoteId, command: NoteCommand) -> Result<(), ServiceError> {
        if let NoteCommand::Create(created) = &command
            && created.id != id
        {
            return Err(ServiceError::IdentityMismatch);
        }
        self.aggregate(id).await.ask(command).await?;
        Ok(())
    }

    pub async fn get(&self, id: NoteId) -> Result<Note, ServiceError> {
        Ok(self.aggregate(id).await.exec(GetNote).await?)
    }

    pub async fn timeline(&self) -> Vec<TimelineEntry> {
        let mut entries: Vec<_> = self
            .timeline
            .entries
            .read()
            .await
            .values()
            .cloned()
            .collect();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.sequence));
        entries
    }

    pub async fn wait_for(&self, id: NoteId, reaction_count: usize) {
        loop {
            let notified = self.timeline.updated.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self
                .timeline
                .entries
                .read()
                .await
                .get(&id)
                .is_some_and(|entry| entry.note.reactions.len() >= reaction_count)
            {
                return;
            }
            notified.await;
        }
    }

    pub async fn stop(&self) -> Result<(), ServiceError> {
        if let Some(projector) = self.system.process_system().lookup(self.projector).await {
            projector.stop().await?;
        }
        Ok(())
    }
}
