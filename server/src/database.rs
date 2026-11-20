//! The PostgreSQL the events are kept in: connecting to it, bringing its
//! schema up to the one nitinol-sqlx expects, the event-sourcing system over
//! it, and the transactions the Accept delivery saves its position in.
//!
//! The connection is made without TLS, because nitinol-sqlx enables none of
//! sqlx's TLS features.

use async_trait::async_trait;
use error_stack::{Report, ResultExt};
use nitinol::{
    eventsource::{TxProvider, system::EventSourceSystem},
    persistence::provider::PersistenceProvider,
    runtime::ProcessSystem,
    sqlx::{
        PostgresEventStore, PostgresStore,
        sqlx::{self, PgPool, PgTransaction, postgres::PgPoolOptions},
    },
};

use crate::error::ServerError;

/// Connects to the database `url` names and applies nitinol-sqlx's
/// migrations, which it records in `nitinol_migrations`.
pub async fn connect(url: &str) -> Result<PgPool, Report<ServerError>> {
    let pool = PgPoolOptions::new()
        .connect(url)
        .await
        .change_context(ServerError::Database)?;
    PostgresEventStore::migrate(&pool)
        .await
        .change_context(ServerError::Database)?;
    Ok(pool)
}

/// The event-sourcing system whose every store is in the database `pool`
/// reaches.
pub async fn event_source_system(
    pool: PgPool,
) -> Result<EventSourceSystem<PostgresStore>, Report<ServerError>> {
    let store = PostgresStore::new(pool).change_context(ServerError::Database)?;
    Ok(EventSourceSystem::builder(ProcessSystem::new().await)
        .with_persistence(PersistenceProvider::new(store))
        .build())
}

/// Transactions on the pool. The Accept delivery saves its position in one,
/// because nitinol-sqlx's offset store writes positions through a transaction.
pub struct PoolTransactions(PgPool);

impl PoolTransactions {
    pub fn new(pool: PgPool) -> Self {
        Self(pool)
    }
}

// `TxProvider` is declared with `#[async_trait]` by nitinol, so its
// implementation has to be written the same way.
#[async_trait]
impl TxProvider for PoolTransactions {
    type Tx = PgTransaction<'static>;
    type Error = sqlx::Error;

    async fn begin(&self) -> Result<Self::Tx, sqlx::Error> {
        self.0.begin().await
    }

    async fn commit(&self, tx: Self::Tx) -> Result<(), sqlx::Error> {
        tx.commit().await
    }

    async fn rollback(&self, tx: Self::Tx) {
        // A rollback that cannot be sent means the connection is going away,
        // and the transaction it held goes with it.
        if let Err(error) = tx.rollback().await {
            tracing::warn!(%error, "rolling back the Accept delivery's transaction failed");
        }
    }
}
