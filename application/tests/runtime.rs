use application::NoteService;
use kernel::{ActorId, CreateNote, NoteCommand, NoteId, NoteKind};
use nitinol::persistence::store::{InMemoryEventStore, InMemorySnapshotStore};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn all_event_kinds_survive_reactivation_and_project_live()
-> Result<(), Box<dyn std::error::Error>> {
    let events = Arc::new(InMemoryEventStore::default());
    let snapshots = Arc::new(InMemorySnapshotStore::default());
    let service = NoteService::start(events.clone(), snapshots.clone()).await;
    let target = NoteId(uuid::Uuid::new_v4());
    let author = ActorId(uuid::Uuid::new_v4());
    for kind in [
        NoteKind::Post,
        NoteKind::Reply { target },
        NoteKind::Turbo { target },
        NoteKind::TurboQuote { target },
    ] {
        let id = NoteId(uuid::Uuid::new_v4());
        let content = if matches!(kind, NoteKind::Turbo { .. }) {
            ""
        } else {
            "hello"
        }
        .into();
        service
            .execute(
                id,
                NoteCommand::Create(CreateNote {
                    id,
                    author,
                    content,
                    kind,
                }),
            )
            .await?;
        service
            .execute(
                id,
                NoteCommand::React {
                    actor: author,
                    reaction: "star".into(),
                },
            )
            .await?;
        let expected = service.get(id).await?;
        let restored = NoteService::start(events.clone(), snapshots.clone()).await;
        assert_eq!(restored.get(id).await?, expected);
        tokio::time::timeout(Duration::from_secs(5), restored.wait_for(id, 1)).await?;
        assert_eq!(
            restored
                .timeline()
                .await
                .iter()
                .find(|entry| entry.note.created.as_ref().map(|n| n.id) == Some(id))
                .map(|entry| &entry.note),
            Some(&expected)
        );
        restored.stop().await?;
    }
    assert_eq!(service.timeline().await.len(), 4);
    service.stop().await?;
    Ok(())
}

#[tokio::test]
async fn creation_stream_identity_must_match_payload() {
    let service = NoteService::in_memory().await;
    let id = NoteId(uuid::Uuid::new_v4());
    let command = CreateNote {
        id: NoteId(uuid::Uuid::new_v4()),
        author: ActorId(uuid::Uuid::new_v4()),
        content: "hello".into(),
        kind: NoteKind::Post,
    };
    assert!(
        service
            .execute(id, NoteCommand::Create(command))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn snapshot_is_restored_before_event_delta() -> Result<(), Box<dyn std::error::Error>> {
    use nitinol::persistence::{AggregateId, PersistedSnapshot, store::SnapshotStore};
    let events = Arc::new(InMemoryEventStore::default());
    let snapshots = Arc::new(InMemorySnapshotStore::default());
    let service = NoteService::start(events.clone(), snapshots.clone()).await;
    let id = NoteId(uuid::Uuid::new_v4());
    let author = ActorId(uuid::Uuid::new_v4());
    service
        .execute(
            id,
            NoteCommand::Create(CreateNote {
                id,
                author,
                content: "snapshot".into(),
                kind: NoteKind::Post,
            }),
        )
        .await?;
    let captured = service.get(id).await?;
    snapshots
        .save(PersistedSnapshot {
            aggregate_id: AggregateId::new(id.0.to_string()),
            sequence: 1,
            payload: serde_json::to_vec(&captured)?.into(),
            created_at: jiff::Timestamp::now(),
        })
        .await?;
    service
        .execute(
            id,
            NoteCommand::React {
                actor: author,
                reaction: "star".into(),
            },
        )
        .await?;
    let restored = NoteService::start(events, snapshots).await;
    let note = restored.get(id).await?;
    assert_eq!(note.created, captured.created);
    assert_eq!(
        note.reactions.get(&author).map(String::as_str),
        Some("star")
    );
    restored.stop().await?;
    service.stop().await?;
    Ok(())
}
