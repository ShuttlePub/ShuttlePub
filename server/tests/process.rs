//! The `shuttlepub-server` binary: what it serves once started, and when it
//! refuses to start. Tests that start it successfully need PostgreSQL (see
//! `support::postgres`).

mod support;

use std::{error::Error, time::Duration};

use support::{
    binary::{
        ACTOR, BIND, DATABASE_URL, REMOTE_ORIGINS, ServerProcess, listening_address, run_until_exit,
    },
    postgres::TestDatabase,
};

type TestResult = Result<(), Box<dyn Error>>;

fn client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
}

async fn actor_status(server: &ServerProcess) -> Result<reqwest::StatusCode, Box<dyn Error>> {
    Ok(client()?
        .get(format!("http://{}/actor", server.address()))
        .send()
        .await?
        .status())
}

#[tokio::test]
async fn binary_serves_default_actor_and_rejects_unsigned_inbox() -> TestResult {
    let database = TestDatabase::create().await?;
    let server =
        ServerProcess::start(&[(BIND, "127.0.0.1:0"), (DATABASE_URL, database.url())]).await?;
    let client = client()?;
    let response = client
        .get(format!("http://{}/actor", server.address()))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.headers()[reqwest::header::CONTENT_TYPE],
        "application/activity+json"
    );
    let actor = response.json::<serde_json::Value>().await?;
    assert_eq!(actor["id"], "http://127.0.0.1:3000/actor");
    assert_eq!(actor["inbox"], "http://127.0.0.1:3000/inbox");
    let response = client
        .post(format!("http://{}/inbox", server.address()))
        .body("{}")
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn binary_refuses_to_start_with_an_invalid_remote_origin() -> TestResult {
    let (status, _) =
        run_until_exit(&[(BIND, "127.0.0.1:0"), (REMOTE_ORIGINS, "not a url")]).await?;
    assert!(!status.success(), "exit status {status}");
    Ok(())
}

// ------------------------------------------------------------------------ R7 ----

#[tokio::test]
async fn r07_binary_starts_on_an_empty_database_and_again_on_the_same_one() -> TestResult {
    // Given a database nothing has been migrated into.
    let database = TestDatabase::create().await?;
    let vars = [
        (BIND, "127.0.0.1:0"),
        (ACTOR, "https://local.example/actor"),
        (DATABASE_URL, database.url()),
    ];

    // When the binary is started, stopped and started again on it.
    let first = ServerProcess::start(&vars).await?;
    let first_status = actor_status(&first).await?;
    first.stop().await?;
    let second = ServerProcess::start(&vars).await?;

    // Then both starts serve requests.
    assert_eq!(first_status, reqwest::StatusCode::OK);
    assert_eq!(actor_status(&second).await?, reqwest::StatusCode::OK);
    Ok(())
}

// ------------------------------------------------------------------------ R8 ----

/// R8: the binary exits with an error and never reports a listening address.
fn assert_refused_to_start(status: std::process::ExitStatus, log: &[String]) {
    assert!(!status.success(), "exit status {status}");
    let listening: Vec<&String> = log
        .iter()
        .filter(|line| listening_address(line).is_some())
        .collect();
    assert!(listening.is_empty(), "the binary listened: {listening:?}");
}

#[tokio::test]
async fn r08_binary_refuses_to_start_without_a_database_url() -> TestResult {
    // Given every setting but SHUTTLEPUB_DATABASE_URL.
    let vars = [
        (BIND, "127.0.0.1:0"),
        (ACTOR, "https://local.example/actor"),
    ];

    // When the binary is started.
    let (status, log) = run_until_exit(&vars).await?;

    // Then it stops with an error before accepting HTTP.
    assert_refused_to_start(status, &log);
    Ok(())
}

#[tokio::test]
async fn r08_binary_refuses_to_start_when_the_database_cannot_be_reached() -> TestResult {
    // Given a database URL on a port nothing listens on.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = closed.local_addr()?.port();
    drop(closed);
    let url = format!("postgres://shuttlepub:shuttlepub@127.0.0.1:{port}/shuttlepub");
    let vars = [(BIND, "127.0.0.1:0"), (DATABASE_URL, url.as_str())];

    // When the binary is started.
    let (status, log) = run_until_exit(&vars).await?;

    // Then it stops with an error before accepting HTTP.
    assert_refused_to_start(status, &log);
    Ok(())
}
