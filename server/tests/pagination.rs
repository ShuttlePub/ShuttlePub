use std::{sync::Arc, time::Duration};

use application::NoteService;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use kernel::{ActorId, CreateNote, NoteCommand, NoteId, NoteKind};
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn timeline_pages_by_creation_sequence_when_older_note_receives_reaction()
-> Result<(), Box<dyn std::error::Error>> {
    // Given two projected notes and a later reaction on the older note.
    let notes = Arc::new(NoteService::in_memory().await);
    let older = NoteId(uuid::Uuid::new_v4());
    let newer = NoteId(uuid::Uuid::new_v4());
    let author = ActorId(uuid::Uuid::new_v4());
    for id in [older, newer] {
        notes
            .execute(
                id,
                NoteCommand::Create(CreateNote {
                    id,
                    author,
                    content: "hello".into(),
                    kind: NoteKind::Post,
                }),
            )
            .await?;
    }
    let federation = Arc::new(driver::Federation::new(
        "https://local.example/actor",
        vec![],
        Arc::new(driver::DevelopmentKey::generate()?),
    )?);
    let app = server::router(notes.clone(), federation);
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/notes/{}/reactions", older.0))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"actor": author, "reaction": "star"}).to_string(),
                ))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    tokio::time::timeout(Duration::from_secs(5), notes.wait_for(older, 1)).await?;

    // When reading the first page and then its exclusive cursor.
    let response = app
        .clone()
        .oneshot(Request::get("/api/timeline?limit=1").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let first: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(first.as_array().ok_or("expected timeline array")?.len(), 1);
    assert_eq!(first[0]["note"]["created"]["id"], newer.0.to_string());
    let cursor = first[0]["sequence"].as_u64().ok_or("missing cursor")?;
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/timeline?limit=1&before={cursor}")).body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let second: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;

    // Then pages are distinct, newest-first, and carry projected reaction state.
    assert_eq!(second.as_array().ok_or("expected timeline array")?.len(), 1);
    assert_eq!(second[0]["note"]["created"]["id"], older.0.to_string());
    assert_eq!(second[0]["note"]["reactions"][author.0.to_string()], "star");
    for limit in [0, 101] {
        let response = app
            .clone()
            .oneshot(Request::get(format!("/api/timeline?limit={limit}")).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    notes.stop().await?;
    Ok(())
}
