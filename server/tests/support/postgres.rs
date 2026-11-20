//! The PostgreSQL a test points the application at.
//!
//! `SHUTTLEPUB_TEST_DATABASE_URL` names a server to use. Without it, one
//! PostgreSQL container is started by the first test of this binary that needs
//! it and shared by all the others, whether or not they overlap in time. Rust
//! never drops a static and testcontainers has no reaper, so a helper process
//! removes the container once this binary has exited. When neither is
//! available the test fails: a test that needs a database never passes
//! without one.
//!
//! Each test gets a database of its own on that server, so tests that run in
//! parallel never read each other's events.

use std::{
    error::Error,
    process::{Command, Stdio},
    time::Duration,
};

use nitinol::{
    eventsource::system::EventSourceSystem,
    sqlx::{
        PostgresStore,
        sqlx::{self, AssertSqlSafe, Executor as _, PgPool, postgres::PgPoolOptions},
    },
};
use testcontainers::{ContainerAsync, Healthcheck, ImageExt, core::WaitFor, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

use super::wait::{REACTION_DEADLINE, eventually};

pub const TEST_DATABASE_URL: &str = "SHUTTLEPUB_TEST_DATABASE_URL";

/// Pinned by digest, so a moving tag cannot change what a green run means.
const POSTGRES_IMAGE_TAG: &str =
    "17-alpine@sha256:18cfe3ef5e6815560c98237d6216d1e5119702fb0f3894c8785dd58b8bbe5d73";

/// The official image first runs a temporary server on a unix socket only;
/// TCP readiness is what tells the real server apart.
const READY_PROBE: [&str; 7] = [
    "pg_isready",
    "-h",
    "localhost",
    "-p",
    "5432",
    "-U",
    "postgres",
];

/// Waits for the process that started the container to be gone, then removes
/// it. Arguments: that process's id, the container id.
const REAPER: &str = r#"while kill -0 "$1" 2>/dev/null; do sleep 1; done; docker rm --force --volumes "$2" >/dev/null 2>&1"#;

/// The container of this binary, kept for as long as the process lives, and the
/// URL of its server. The URL is read once: the Docker client belongs to the
/// runtime of the test that started the container, which may be gone by the
/// time another test needs the address.
static SHARED_POSTGRES: tokio::sync::OnceCell<SharedPostgres> = tokio::sync::OnceCell::const_new();

struct SharedPostgres {
    _container: ContainerAsync<Postgres>,
    url: String,
}

/// A database of its own for one test, on the server this run reaches.
pub struct TestDatabase {
    url: String,
    pool: PgPool,
}

impl TestDatabase {
    /// A new, empty database: nothing has been migrated into it.
    pub async fn create() -> Result<Self, Box<dyn Error>> {
        let server_url = match std::env::var(TEST_DATABASE_URL) {
            Ok(url) => url,
            Err(_) => SHARED_POSTGRES
                .get_or_try_init(start_postgres)
                .await
                .map_err(|error| {
                    format!(
                        "{TEST_DATABASE_URL} is not set and no PostgreSQL container could be \
                         started: {error}"
                    )
                })?
                .url
                .clone(),
        };
        let name = format!("shuttlepub_test_{}", uuid::Uuid::new_v4().simple());
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&server_url)
            .await?;
        // A database name cannot be a bind parameter; this one is made of a
        // fixed prefix and a UUID, so nothing in it was shaped by input.
        admin
            .execute(AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .await?;
        admin.close().await;
        let mut url = reqwest::Url::parse(&server_url)?;
        url.set_path(&format!("/{name}"));
        let url = url.to_string();
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await?;
        Ok(Self { url, pool })
    }

    /// The URL to hand the application as `SHUTTLEPUB_DATABASE_URL`.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Connects the application to this database the way `main` does, which
    /// also migrates it.
    pub async fn connect(&self) -> Result<PgPool, Box<dyn Error>> {
        Ok(server::database::connect(&self.url)
            .await
            .map_err(|error| format!("{error:?}"))?)
    }

    /// The event-sourcing system over this database, built the way `main`
    /// builds it, for a test that starts no Accept delivery.
    pub async fn event_source_system(
        &self,
    ) -> Result<EventSourceSystem<PostgresStore>, Box<dyn Error>> {
        let pool = self.connect().await?;
        Ok(server::database::event_source_system(pool)
            .await
            .map_err(|error| format!("{error:?}"))?)
    }

    /// How many events of `family` the journal holds. The journal table is
    /// the one nitinol-sqlx's migrations create.
    pub async fn recorded(&self, family: &str) -> Result<i64, Box<dyn Error>> {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM nitinol_journal WHERE event_type LIKE $1")
                .bind(format!("{family}.%"))
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }

    pub async fn wait_for_recorded(&self, family: &str, count: i64) -> Result<(), Box<dyn Error>> {
        eventually(
            &format!("{count} {family} events in PostgreSQL"),
            REACTION_DEADLINE,
            async || Ok(self.recorded(family).await? == count),
        )
        .await
    }

    /// Waits until a consumer's offset has been carried past the last event
    /// of `family`: from then on, a restart resumes after every one of them.
    pub async fn wait_for_offset_past(&self, family: &str) -> Result<(), Box<dyn Error>> {
        eventually(
            &format!("an offset past the last {family} event"),
            REACTION_DEADLINE,
            async || {
                let caught_up: Option<bool> = sqlx::query_scalar(
                    "SELECT (SELECT max(sequence) FROM nitinol_offset) >= \
                     (SELECT max(global_sequence) FROM nitinol_journal WHERE event_type LIKE $1)",
                )
                .bind(format!("{family}.%"))
                .fetch_one(&self.pool)
                .await?;
                Ok(caught_up == Some(true))
            },
        )
        .await
    }
}

async fn start_postgres() -> Result<SharedPostgres, Box<dyn Error>> {
    let container = Postgres::default()
        .with_tag(POSTGRES_IMAGE_TAG)
        .with_health_check(
            Healthcheck::cmd(READY_PROBE)
                .with_interval(Duration::from_millis(250))
                .with_timeout(Duration::from_secs(5))
                .with_retries(240),
        )
        .with_ready_conditions(vec![WaitFor::healthcheck()])
        .start()
        .await?;
    Command::new("sh")
        .args(["-c", REAPER, "reaper"])
        .arg(std::process::id().to_string())
        .arg(container.id())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let url = connection_url(&container).await?;
    Ok(SharedPostgres {
        _container: container,
        url,
    })
}

async fn connection_url(container: &ContainerAsync<Postgres>) -> Result<String, Box<dyn Error>> {
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    Ok(format!(
        "postgres://postgres:postgres@{host}:{port}/postgres"
    ))
}
