use async_trait::async_trait;
use error_stack::{Report, ResultExt};
use kernel::{
    KernelError,
    identity::ActorId,
    relay::{FollowerEntry, FollowerEvent, FollowerReader},
};
use nitinol::{
    eventsource::{ProjectionContext, ProjectionId, Projector},
    sqlx::{
        PostgresStore,
        sqlx::{self, PgPool, PgTransaction},
    },
};

use crate::persistence::{Database, EventSourcing, PoolTransactions, Projection};

/// The projection's name, and the key its offset is saved under.
const PROJECTION_ID: &str = "shuttlepub.relay.followers";

/// The current followers as the projection last wrote them to
/// `relay_followers`.
pub struct FollowerReadModel {
    pool: PgPool,
}

impl FollowerReadModel {
    pub fn new(database: &Database) -> Self {
        Self {
            pool: database.pool().clone(),
        }
    }
}

impl FollowerReader for FollowerReadModel {
    async fn list(&self) -> Result<Vec<FollowerEntry>, Report<KernelError>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT actor_id, inbox FROM relay_followers ORDER BY actor_id",
        )
        .fetch_all(&self.pool)
        .await
        .change_context(KernelError::Internal)?;
        rows.into_iter()
            .map(|(actor, inbox)| {
                let actor = ActorId::parse(actor).change_context(KernelError::Internal)?;
                Ok(FollowerEntry::new(actor, inbox))
            })
            .collect()
    }
}

/// Starts projecting `FollowerEvent`s, in the journal's global order, into
/// `relay_followers`. Each batch updates the table and saves the offset in one
/// transaction.
///
/// # Panics
///
/// nitinol panics if it cannot load the projection's saved offset.
pub async fn start_follower_projection(
    events: &EventSourcing<PostgresStore>,
    database: &Database,
) -> Projection {
    let projector = events
        .system()
        .projector_props(ProjectionId::new(PROJECTION_ID), || FollowerProjector)
        .with_tx_provider(PoolTransactions::new(database.pool().clone()))
        .with_event::<FollowerEvent>()
        .catchup_from_global()
        .spawn()
        .await;
    Projection::new(projector.into())
}

struct FollowerProjector;

#[async_trait]
impl Projector<FollowerEvent, PgTransaction<'static>> for FollowerProjector {
    type Error = ProjectionError;

    /// An upsert, so that an event delivered again leaves the row as it was.
    async fn project(
        &mut self,
        event: FollowerEvent,
        ctx: &mut ProjectionContext<'_, PgTransaction<'static>>,
    ) -> Result<(), ProjectionError> {
        let FollowerEvent::Followed { actor, inbox, .. } = event;
        let tx = ctx.tx().ok_or(ProjectionError::NoTransaction)?;
        sqlx::query(
            "INSERT INTO relay_followers (actor_id, inbox) VALUES ($1, $2) \
             ON CONFLICT (actor_id) DO UPDATE SET inbox = EXCLUDED.inbox",
        )
        .bind(actor)
        .bind(inbox)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum ProjectionError {
    #[error("the projection was handed no transaction")]
    NoTransaction,
    #[error("the read model could not be written")]
    Database(#[from] sqlx::Error),
}
