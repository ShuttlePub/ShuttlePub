//! `POST /inbox` through the `shuttlepub-server` binary down to the events in
//! PostgreSQL, and what survives a restart on the same database.
//!
//! The binary is started exactly as an operator starts it, so these tests
//! cover the PostgreSQL store, its migration and the Accept delivery's saved
//! position together. Events are counted in the journal table nitinol-sqlx's
//! migrations create.

mod support;

use std::error::Error;

use reqwest::StatusCode;
use serde_json::Value;
use support::{
    activity::{LOCAL_ACTOR, create, follow, note},
    binary::{ACTOR, BIND, DATABASE_URL, REMOTE_ORIGINS, ServerProcess},
    peer::{Peer, valid_peer},
    postgres::TestDatabase,
    signing::signed,
};

type TestResult = Result<(), Box<dyn Error>>;

const PRODUCER: &str = "shuttlepub.producer.v1";
const NOTE: &str = "shuttlepub.note.v1";

/// Sends `activity`, signed by `peer`, to the binary's inbox. The request
/// keeps the `Host` it was signed for rather than the loopback address.
async fn post_inbox(
    server: &ServerProcess,
    peer: &Peer,
    activity: &Value,
) -> Result<StatusCode, Box<dyn Error>> {
    let request = signed(peer, activity).await?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!("http://{}/inbox", server.address()))
        .headers(request.headers)
        .body(request.body)
        .send()
        .await?;
    Ok(response.status())
}

async fn start(database: &TestDatabase, peers: &[&Peer]) -> Result<ServerProcess, Box<dyn Error>> {
    let origins = peers
        .iter()
        .map(|peer| peer.origin.as_str())
        .collect::<Vec<_>>()
        .join(",");
    ServerProcess::start(&[
        (BIND, "127.0.0.1:0"),
        (ACTOR, LOCAL_ACTOR),
        (REMOTE_ORIGINS, origins.as_str()),
        (DATABASE_URL, database.url()),
    ])
    .await
}

#[tokio::test]
async fn r01_follow_reaches_postgres_as_federation_and_delivered_accept() -> TestResult {
    // Given the binary on an empty database and a remote service it allows.
    let database = TestDatabase::create().await?;
    let peer = valid_peer().await?;
    let server = start(&database, &[&peer]).await?;

    // When the service posts a correctly signed Follow.
    let status = post_inbox(&server, &peer, &follow(&peer, 1)).await?;

    // Then the Accept reaches it and PostgreSQL holds the federation and the
    // delivered Accept.
    assert_eq!(status, StatusCode::ACCEPTED);
    peer.wait_for_deliveries(1).await?;
    database.wait_for_recorded(PRODUCER, 2).await?;
    assert_eq!(peer.deliveries()?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn n01_public_note_reaches_postgres_once() -> TestResult {
    // Given the binary and a service whose federation it accepted.
    let database = TestDatabase::create().await?;
    let peer = valid_peer().await?;
    let server = start(&database, &[&peer]).await?;
    assert_eq!(
        post_inbox(&server, &peer, &follow(&peer, 1)).await?,
        StatusCode::ACCEPTED
    );

    // When the service posts a public Create.
    let status = post_inbox(&server, &peer, &create(&peer, 1, &note(&peer, 1))).await?;

    // Then PostgreSQL holds one "note received" event.
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(database.recorded(NOTE).await?, 1);
    Ok(())
}

#[tokio::test]
async fn r06_restart_on_the_same_database_keeps_federation_notes_and_sent_accepts() -> TestResult {
    // Given a service whose federation was accepted and answered, and one of
    // whose Notes was recorded, before the binary stopped.
    let database = TestDatabase::create().await?;
    let peer = valid_peer().await?;
    let other = valid_peer().await?;
    let before = start(&database, &[&peer, &other]).await?;
    assert_eq!(
        post_inbox(&before, &peer, &follow(&peer, 1)).await?,
        StatusCode::ACCEPTED
    );
    peer.wait_for_deliveries(1).await?;
    database.wait_for_recorded(PRODUCER, 2).await?;
    let first_note = create(&peer, 1, &note(&peer, 1));
    assert_eq!(
        post_inbox(&before, &peer, &first_note).await?,
        StatusCode::ACCEPTED
    );
    assert_eq!(database.recorded(NOTE).await?, 1);
    database.wait_for_offset_past(PRODUCER).await?;
    before.stop().await?;

    // When the binary starts again on the same database.
    let after = start(&database, &[&peer, &other]).await?;

    // Then the service is still accepted: a new public Note is recorded (N1).
    assert_eq!(
        post_inbox(&after, &peer, &create(&peer, 2, &note(&peer, 2))).await?,
        StatusCode::ACCEPTED
    );
    assert_eq!(database.recorded(NOTE).await?, 2);

    // And the Note recorded before the restart is still known (N2).
    assert_eq!(
        post_inbox(&after, &peer, &first_note).await?,
        StatusCode::ACCEPTED
    );
    assert_eq!(database.recorded(NOTE).await?, 2);

    // And restarting sent no Accept again: once the delivery has answered a
    // Follow recorded after the restart, the first service still holds one.
    assert_eq!(
        post_inbox(&after, &other, &follow(&other, 1)).await?,
        StatusCode::ACCEPTED
    );
    other.wait_for_deliveries(1).await?;
    database.wait_for_recorded(PRODUCER, 4).await?;
    assert_eq!(peer.deliveries()?.len(), 1);
    Ok(())
}
