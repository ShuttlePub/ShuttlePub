use std::borrow::Cow;

use error_stack::{Report, ResultExt};
use nitinol::sqlx::{
    PostgresEventStore, PostgresStore,
    sqlx::{
        PgPool, SqlSafeStr,
        migrate::{Migration, MigrationType, Migrator},
    },
};

use super::DatabaseSettings;
use crate::DriverError;

/// The table the application's migrations are recorded in. nitinol records
/// its own in sqlx's default `_sqlx_migrations` and cannot be told otherwise,
/// and a migrator that finds a version it does not carry in its table fails,
/// so the two must not share one.
const MIGRATIONS_TABLE: &str = "shuttlepub_migrations";

/// The PostgreSQL database the journal, the projection offsets and the read
/// models live in.
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Fails if the database cannot be reached; sqlx keeps trying to connect
    /// for its acquire timeout (30 s) before giving up.
    pub async fn connect(settings: &DatabaseSettings) -> Result<Self, Report<DriverError>> {
        let pool = PgPool::connect(settings.url())
            .await
            .change_context(DriverError::Database)?;
        Ok(Self { pool })
    }

    /// Brings the schema up to date: nitinol's tables, then the application's.
    /// Running it again on an up-to-date database changes nothing.
    pub async fn migrate(&self) -> Result<(), Report<DriverError>> {
        PostgresEventStore::migrate(&self.pool)
            .await
            .change_context(DriverError::Database)
            .attach("nitinol migrations")?;
        application_migrator()
            .run(&self.pool)
            .await
            .change_context(DriverError::Database)
            .attach("application migrations")
    }

    /// nitinol's store over this database.
    pub fn event_store(&self) -> Result<PostgresStore, Report<DriverError>> {
        PostgresStore::new(self.pool.clone()).change_context(DriverError::Database)
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}

fn application_migrator() -> Migrator {
    let mut migrator = Migrator::with_migrations(vec![Migration::new(
        1,
        Cow::Borrowed("create relay followers"),
        MigrationType::Simple,
        include_str!("../../migrations/0001_create_relay_followers.sql").into_sql_str(),
        false,
    )]);
    migrator.dangerous_set_table_name(MIGRATIONS_TABLE);
    migrator
}
