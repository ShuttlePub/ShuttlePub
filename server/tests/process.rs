//! The built `shuttlepub-server` binary, started as a child process. It needs
//! PostgreSQL: every test that lets it start gets its own empty database from
//! `TestDatabase` (`SHUTTLEPUB_TEST_DATABASE_URL`, or a container), and fails
//! rather than passing when neither is available. Test names carry the issue's
//! check numbers (`r08_`, `r09_`) where they cover one.

use std::{
    error::Error,
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

use driver::persistence::test_support::TestDatabase;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const DATABASE_URL_ENV: &str = "SHUTTLEPUB_DATABASE_URL";
const LISTENING_MARK: &str = "address=";
const START_TIMEOUT: Duration = Duration::from_secs(60);

struct ServerProcess(Child);

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn test_database() -> TestResult<TestDatabase> {
    TestDatabase::start()
        .await
        .map_err(|error| format!("no PostgreSQL for the test: {error:?}").into())
}

/// The binary with the default actor, an ephemeral port and `database_url`
/// (`None` leaves `SHUTTLEPUB_DATABASE_URL` unset).
fn server_command(database_url: Option<&str>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shuttlepub-server"));
    command
        .env("SHUTTLEPUB_BIND", "127.0.0.1:0")
        .env_remove("SHUTTLEPUB_ACTOR")
        .env_remove("SHUTTLEPUB_REMOTE_ORIGINS")
        .env("RUST_LOG", "info");
    match database_url {
        Some(url) => command.env(DATABASE_URL_ENV, url),
        None => command.env_remove(DATABASE_URL_ENV),
    };
    command
}

/// Starts the binary and waits until it logs the address it listens on.
async fn start_listening(mut command: Command) -> TestResult<(ServerProcess, String)> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let process = ServerProcess(child);
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line?;
            if let Some(address) = line
                .split(LISTENING_MARK)
                .nth(1)
                .and_then(|value| value.split_whitespace().next())
            {
                let _ = tx.send(address.to_owned());
                break;
            }
        }
        Ok::<_, std::io::Error>(())
    });
    let address = tokio::time::timeout(START_TIMEOUT, rx)
        .await
        .map_err(|_| "the server did not start listening in time")?
        .map_err(|_| "the server exited without listening")?;
    Ok((process, address))
}

/// Waits until the process exits on its own, failing after `START_TIMEOUT`.
async fn exit_status(process: &mut ServerProcess) -> TestResult<ExitStatus> {
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    let mut poll = tokio::time::interval(Duration::from_millis(50));
    loop {
        poll.tick().await;
        if let Some(status) = process.0.try_wait()? {
            return Ok(status);
        }
        if tokio::time::Instant::now() > deadline {
            return Err("the server kept running".into());
        }
    }
}

/// Runs the binary until it exits on its own and returns its status and the
/// lines it wrote to stdout.
async fn run_to_exit(mut command: Command) -> TestResult<(ExitStatus, Vec<String>)> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let mut process = ServerProcess(child);
    let lines = tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        std::io::BufReader::new(stdout)
            .lines()
            .collect::<Result<Vec<_>, _>>()
    });
    let status = exit_status(&mut process).await?;
    Ok((status, lines.await??))
}

fn http_client() -> TestResult<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?)
}

async fn assert_serves_actor(address: &str) -> TestResult {
    let response = http_client()?
        .get(format!("http://{address}/actor"))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    Ok(())
}

fn assert_never_listened(lines: &[String]) {
    for line in lines {
        assert!(
            !line.contains(LISTENING_MARK),
            "the server announced a listener: {line}"
        );
    }
}

#[tokio::test]
async fn binary_serves_default_actor_and_rejects_unsigned_inbox() -> TestResult {
    let database = test_database().await?;
    let (_process, address) = start_listening(server_command(Some(database.url()))).await?;
    let client = http_client()?;
    let response = client.get(format!("http://{address}/actor")).send().await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.headers()[reqwest::header::CONTENT_TYPE],
        "application/activity+json"
    );
    let actor = response.json::<serde_json::Value>().await?;
    assert_eq!(actor["id"], "http://127.0.0.1:3000/actor");
    assert_eq!(actor["inbox"], "http://127.0.0.1:3000/inbox");
    let response = client
        .post(format!("http://{address}/inbox"))
        .body("{}")
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn binary_refuses_to_start_with_an_invalid_remote_origin() -> TestResult {
    // A reachable database, so that the remote origin is what stops the start.
    let database = test_database().await?;
    let mut command = server_command(Some(database.url()));
    command
        .env("SHUTTLEPUB_REMOTE_ORIGINS", "not a url")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut process = ServerProcess(command.spawn()?);
    let status = exit_status(&mut process).await?;
    assert!(!status.success(), "exit status {status}");
    Ok(())
}

// ------------------------------------------------------------------------ R8 ----

#[tokio::test]
async fn r08_binary_starts_on_an_empty_database_and_again_after_stopping() -> TestResult {
    // Given an empty database.
    let database = test_database().await?;

    // When the server is started on it, stopped, and started again.
    let (first, address) = start_listening(server_command(Some(database.url()))).await?;
    assert_serves_actor(&address).await?;
    drop(first);
    let (_second, address) = start_listening(server_command(Some(database.url()))).await?;

    // Then the second start, which finds the migrations already applied, also
    // reaches listening and serves.
    assert_serves_actor(&address).await?;
    Ok(())
}

// ------------------------------------------------------------------------ R9 ----

#[tokio::test]
async fn r09_binary_without_a_database_url_stops_before_listening() -> TestResult {
    // When the server is started with SHUTTLEPUB_DATABASE_URL unset.
    let (status, lines) = run_to_exit(server_command(None)).await?;

    // Then it exits with an error and never listened.
    assert!(!status.success(), "exit status {status}");
    assert_never_listened(&lines);
    Ok(())
}

#[tokio::test]
async fn r09_binary_with_an_unreachable_database_stops_before_listening() -> TestResult {
    // Given a database URL whose port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let url = format!("postgres://shuttlepub:shuttlepub@127.0.0.1:{port}/shuttlepub");

    // When the server is started with it.
    let (status, lines) = run_to_exit(server_command(Some(&url))).await?;

    // Then it exits with an error and never listened.
    assert!(!status.success(), "exit status {status}");
    assert_never_listened(&lines);
    Ok(())
}
