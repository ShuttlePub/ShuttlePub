use axum::{
    Json, Router,
    body::Bytes,
    routing::{get, post},
};
use driver::{DevelopmentKey, Federation, Follow, RemoteActor};
use http_body_util::{BodyExt, Full};
use std::sync::{Arc, LazyLock};
use tokio::sync::mpsc;

static KEY: LazyLock<Result<DevelopmentKey, driver::FederationError>> =
    LazyLock::new(DevelopmentKey::generate);

#[tokio::test]
async fn valid_follow_resolves_actor_and_delivers_signed_accept()
-> Result<(), Box<dyn std::error::Error>> {
    let key = KEY.as_ref().map_err(ToString::to_string)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let actor_id = format!("{origin}/actor");
    let actor = RemoteActor::new(&actor_id, &format!("{origin}/inbox"), key.public_pem()?);
    let (tx, mut rx) = mpsc::channel(1);
    let app = Router::new()
        .route(
            "/actor",
            get(move || {
                let actor = actor.clone();
                async { Json(actor) }
            }),
        )
        .route(
            "/inbox",
            post(move |headers: http::HeaderMap, body: Bytes| {
                let tx = tx.clone();
                async move {
                    tx.send((headers, body)).await.ok();
                }
            }),
        );
    let task = tokio::spawn(async move { axum::serve(listener, app).await });
    let local = "https://local.example/actor";
    let federation = Federation::new(local, vec![origin], Arc::new(key.clone()))?;
    let follow = Follow::new("https://remote.example/follows/1", &actor_id, local);
    let request = driver::sign_request(
        key,
        &format!("{actor_id}#main-key"),
        "https://local.example/inbox",
        serde_json::to_vec(&follow)?,
    )
    .await?;
    let (parts, body) = request.into_parts();
    let bytes = body.collect().await?.to_bytes();
    federation
        .accept_follow(http::Request::from_parts(parts, Full::new(bytes)))
        .await?;
    let (headers, body) = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await?
        .ok_or("no Accept")?;
    let accept: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(accept["type"], "Accept");
    assert_eq!(accept["actor"], local);
    assert_eq!(accept["object"]["id"], follow.id);
    assert!(headers.contains_key("signature"));
    let mut request =
        http::Request::post(format!("{}/inbox", actor_id.trim_end_matches("/actor")))
            .body(Full::new(body))?;
    *request.headers_mut() = headers;
    driver::verify_request(request, &federation.local_actor()?).await?;
    task.abort();
    Ok(())
}

#[tokio::test]
async fn signatures_reject_tampering_and_malformed_headers()
-> Result<(), Box<dyn std::error::Error>> {
    let key = KEY.as_ref().map_err(ToString::to_string)?;
    let actor = RemoteActor::new(
        "https://peer.example/actor",
        "https://peer.example/inbox",
        key.public_pem()?,
    );
    let key_id = "https://peer.example/actor#main-key";
    let request = driver::sign_request(
        key,
        key_id,
        "https://local.example/inbox",
        b"original".to_vec(),
    )
    .await?;
    let (parts, body) = request.into_parts();
    let bytes = body.collect().await?.to_bytes();
    assert!(
        driver::verify_request(
            http::Request::from_parts(parts.clone(), Full::new(bytes.clone())),
            &actor
        )
        .await
        .is_ok()
    );
    let mut wrong_path = parts.clone();
    wrong_path.uri = "https://local.example/different-inbox".parse()?;
    assert!(
        driver::verify_request(
            http::Request::from_parts(wrong_path, Full::new(bytes.clone())),
            &actor
        )
        .await
        .is_err()
    );
    let mut wrong_owner = actor.clone();
    wrong_owner.public_key.owner = "https://attacker.example/actor".into();
    assert!(
        driver::verify_request(
            http::Request::from_parts(parts.clone(), Full::new(bytes)),
            &wrong_owner
        )
        .await
        .is_err()
    );
    assert!(
        driver::verify_request(
            http::Request::from_parts(parts.clone(), Full::new(Bytes::from_static(b"changed"))),
            &actor
        )
        .await
        .is_err()
    );
    for value in [
        "garbage",
        "keyId=\"a\",algorithm=\"rsa-sha256\",headers=\"date\",signature=\"%%%\"",
        "keyId=\"a\",algorithm=\"rsa-sha256\",headers=\"[\",signature=\"AA==\"",
    ] {
        let mut parts = parts.clone();
        parts.headers.insert("signature", value.parse()?);
        assert!(
            driver::verify_request(
                http::Request::from_parts(parts, Full::new(Bytes::new())),
                &actor
            )
            .await
            .is_err()
        );
    }
    Ok(())
}

#[tokio::test]
async fn actor_resolution_rejects_unlisted_origins() -> Result<(), Box<dyn std::error::Error>> {
    let key = KEY.as_ref().map_err(ToString::to_string)?;
    let federation = Federation::new("https://local.example/actor", vec![], Arc::new(key.clone()))?;
    assert!(federation.resolve("http://127.0.0.1/actor").await.is_err());
    Ok(())
}
