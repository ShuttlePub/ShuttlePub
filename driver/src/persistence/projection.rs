use async_trait::async_trait;
use error_stack::{Report, ResultExt};
use nitinol::{
    eventsource::TxProvider,
    runtime::process::AnyProxy,
    sqlx::sqlx::{self, PgPool, PgTransaction},
};

use crate::DriverError;

/// A running projection. nitinol has no call that stops a whole system, so
/// each projection is stopped on its own once the server stopped serving.
pub struct Projection(AnyProxy);

impl Projection {
    pub(crate) fn new(process: AnyProxy) -> Self {
        Self(process)
    }

    /// Asks the projection's process to stop; fails if it already stopped.
    pub async fn stop(self) -> Result<(), Report<DriverError>> {
        self.0.stop().await.change_context(DriverError::Projection)
    }
}

/// Opens the transaction a projection batch updates its read model and saves
/// its offset in.
pub(crate) struct PoolTransactions {
    pool: PgPool,
}

impl PoolTransactions {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TxProvider for PoolTransactions {
    type Tx = PgTransaction<'static>;
    type Error = sqlx::Error;

    async fn begin(&self) -> Result<Self::Tx, Self::Error> {
        self.pool.begin().await
    }

    async fn commit(&self, tx: Self::Tx) -> Result<(), Self::Error> {
        tx.commit().await
    }

    /// A rollback that cannot be sent means the connection is going away, and
    /// the transaction goes with it, so there is nothing left to undo.
    async fn rollback(&self, tx: Self::Tx) {
        let _ = tx.rollback().await;
    }
}
