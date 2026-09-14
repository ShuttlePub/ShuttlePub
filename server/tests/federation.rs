use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    body::Bytes,
    routing::{get, post},
};
use driver::{DevelopmentKey, Federation, Follow, RemoteActor};
use http_body_util::{BodyExt, Full};
use tokio::sync::mpsc;

#[tokio::test]
async fn inbox_delivers_verified_accept_when_follow_is_signed()
-> Result<(), Box<dyn std::error::Error>> {
    // Given two real HTTP endpoints and independent local/remote signing keys.
    let remote_key = DevelopmentKey::generate()?;
    let local_key = Arc::new(DevelopmentKey::generate()?);
    let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let local_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let remote_origin = format!("http://{}", remote_listener.local_addr()?);
    let local_origin = format!("http://{}", local_listener.local_addr()?);
    let actor_id = format!("{remote_origin}/actor");
    let remote_actor = RemoteActor::new(
        &actor_id,
        &format!("{remote_origin}/inbox"),
        remote_key.public_pem()?,
    );
    let (tx, mut rx) = mpsc::channel(2);
    let peer = Router::new()
        .route(
            "/actor",
            get(move || {
                let actor = remote_actor.clone();
                async move { Json(actor) }
            }),
        )
        .route(
            "/inbox",
            post(move |headers: axum::http::HeaderMap, body: Bytes| {
                let tx = tx.clone();
                async move {
                    tx.send((headers, body))
                        .await
                        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                }
            }),
        );
    let federation = Arc::new(Federation::new(
        &format!("{local_origin}/actor"),
        vec![remote_origin.clone()],
        local_key,
    )?);
    let local_actor = federation.local_actor()?;
    let notes = Arc::new(application::NoteService::in_memory().await);
    let app = server::router(notes.clone(), federation);
    let mut servers = tokio::task::JoinSet::new();
    servers.spawn(async move { axum::serve(remote_listener, peer).await });
    servers.spawn(async move { axum::serve(local_listener, app).await });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let follow = Follow::new(
        &format!("{remote_origin}/follows/1"),
        &actor_id,
        &local_actor.id,
    );
    let signed = driver::sign_request(
        &remote_key,
        &format!("{actor_id}#main-key"),
        &local_actor.inbox,
        serde_json::to_vec(&follow)?,
    )
    .await?;
    let (parts, body) = signed.into_parts();
    let bytes = body.collect().await?.to_bytes();

    // When a signed Follow crosses the public inbox route over HTTP.
    let response = client
        .post(&local_actor.inbox)
        .headers(parts.headers.clone())
        .body(bytes.to_vec())
        .send()
        .await?;

    // Then 202 means a matching, independently verifiable Accept was delivered.
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
    let (headers, body) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await?
        .ok_or("Accept was not delivered")?;
    let accept: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(accept["type"], "Accept");
    assert_eq!(accept["actor"], local_actor.id);
    assert_eq!(accept["object"], serde_json::to_value(&follow)?);
    let mut request =
        axum::http::Request::post(format!("{remote_origin}/inbox")).body(Full::new(body))?;
    *request.headers_mut() = headers;
    driver::verify_request(request, &local_actor).await?;

    // Given the original signature but a different, still valid Follow body.
    let tampered = Follow::new(
        &format!("{remote_origin}/follows/2"),
        &actor_id,
        &local_actor.id,
    );
    // When the changed bytes reach the same inbox.
    let response = client
        .post(&local_actor.inbox)
        .headers(parts.headers)
        .body(serde_json::to_vec(&tampered)?)
        .send()
        .await?;
    // Then the route rejects it without an additional Accept delivery.
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    notes.stop().await?;
    servers.shutdown().await;
    Ok(())
}
