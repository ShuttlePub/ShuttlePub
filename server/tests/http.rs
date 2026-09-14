use application::NoteService;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn create_and_read_timeline_over_http() -> Result<(), Box<dyn std::error::Error>> {
    let notes = Arc::new(NoteService::in_memory().await);
    let key = Arc::new(driver::DevelopmentKey::generate()?);
    let federation = Arc::new(driver::Federation::new(
        "https://local.example/actor",
        vec![],
        key,
    )?);
    let app = server::router(notes.clone(), federation);
    let id = uuid::Uuid::new_v4();
    let payload = json!({"id": id, "author": uuid::Uuid::new_v4(), "content": "hello", "kind": {"type": "post"}});
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/notes")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        notes.wait_for(kernel::NoteId(id), 0),
    )
    .await?;
    let response = app
        .clone()
        .oneshot(Request::get("/api/timeline?limit=10").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(body[0]["note"]["created"]["content"], "hello");
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/notes")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let response = app
        .clone()
        .oneshot(
            Request::post("/inbox")
                .header("content-type", "application/activity+json")
                .body(Body::from("{}"))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .oneshot(Request::get("/openapi.json").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let spec: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert!(spec["paths"]["/api/timeline"]["get"].is_object());
    notes.stop().await?;
    Ok(())
}
