//! Databases for tests. Compiled for this crate's tests and, through the
//! `test-support` feature, for other crates' tests; never part of a server
//! build.

use std::{
    env::{self, VarError},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use error_stack::{Report, ResultExt};
pub use nitinol::persistence::store::InMemoryStore;
use nitinol::sqlx::sqlx::{AssertSqlSafe, Connection, PgConnection};
use testcontainers::{ContainerAsync, Healthcheck, ImageExt, core::WaitFor, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use url::Url;

use crate::DriverError;

/// A server URL the tests may create databases on. Unset, a container is
/// started instead.
const TEST_DATABASE_URL: &str = "SHUTTLEPUB_TEST_DATABASE_URL";

/// The image a container is started from, pinned by digest so that a moved
/// tag does not change what the tests run against.
const POSTGRES_IMAGE_TAG: &str =
    "17-alpine@sha256:18cfe3ef5e6815560c98237d6216d1e5119702fb0f3894c8785dd58b8bbe5d73";

/// The official image first runs a temporary server that only listens on a
/// unix socket, so the container is ready once the real one answers on TCP.
const READY_PROBE: [&str; 7] = [
    "pg_isready",
    "-h",
    "localhost",
    "-p",
    "5432",
    "-U",
    "postgres",
];

static CREATED: AtomicU64 = AtomicU64::new(0);

/// An empty database of its own for one test.
pub struct TestDatabase {
    url: String,
    _container: Option<ContainerAsync<Postgres>>,
}

impl TestDatabase {
    /// Creates a new database on `SHUTTLEPUB_TEST_DATABASE_URL`'s server, or
    /// starts a PostgreSQL container when it is unset. Fails, rather than
    /// letting a test pass without a database, when neither works.
    pub async fn start() -> Result<Self, Report<DriverError>> {
        match env::var(TEST_DATABASE_URL) {
            Ok(server) => Self::create_on(&server).await,
            Err(VarError::NotPresent) => Self::start_container().await,
            Err(error) => Err(error)
                .change_context(DriverError::InvalidSetting)
                .attach(format!("{TEST_DATABASE_URL} is not valid unicode")),
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// The databases created here are left behind on the server.
    async fn create_on(server: &str) -> Result<Self, Report<DriverError>> {
        let name = unique_database_name()?;
        let mut connection = PgConnection::connect(server)
            .await
            .change_context(DriverError::Database)
            .attach(format!("connecting to {TEST_DATABASE_URL}"))?;
        nitinol::sqlx::sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&mut connection)
            .await
            .change_context(DriverError::Database)
            .attach(format!("creating {name}"))?;
        connection
            .close()
            .await
            .change_context(DriverError::Database)?;
        let mut url = Url::parse(server)
            .change_context(DriverError::InvalidSetting)
            .attach(format!("{TEST_DATABASE_URL} is not a URL"))?;
        url.set_path(&name);
        Ok(Self {
            url: url.into(),
            _container: None,
        })
    }

    async fn start_container() -> Result<Self, Report<DriverError>> {
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
            .await
            .change_context(DriverError::Database)
            .attach(format!(
                "{TEST_DATABASE_URL} is unset and no PostgreSQL container could be started"
            ))?;
        let host = container
            .get_host()
            .await
            .change_context(DriverError::Database)?;
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .change_context(DriverError::Database)?;
        Ok(Self {
            url: format!("postgres://postgres:postgres@{host}:{port}/postgres"),
            _container: Some(container),
        })
    }
}

/// Unique across the test processes of one run and across runs, so that a
/// server shared by several runs never hands one test another's database.
fn unique_database_name() -> Result<String, Report<DriverError>> {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .change_context(DriverError::Database)?
        .as_nanos();
    let count = CREATED.fetch_add(1, Ordering::Relaxed);
    Ok(format!(
        "shuttlepub_test_{}_{started}_{count}",
        std::process::id()
    ))
}
