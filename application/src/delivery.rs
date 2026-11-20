//! Answering an accepted federation with an Accept, as a reaction to the
//! recorded event rather than while the Follow is answered.

use std::sync::Arc;

use async_trait::async_trait;
use error_stack::Report;
use kernel::{
    community::{DependOnLocalActorProvider, LocalActorProvider},
    producer::{Accept, Follow, ProducerEvent, RecordAcceptDelivered, RecordAcceptFailed},
};
use nitinol::eventsource::{ProjectionContext, ProjectionId, Projector};
use port::delivery::{ActivityDelivery, DependOnActivityDelivery};

use crate::{
    ApplicationError,
    producer::{DependOnProducerService, ProducerService},
};

/// The id under which the Accept delivery's position in the journal is kept.
pub fn accept_delivery_projection() -> ProjectionId {
    ProjectionId::new("shuttlepub.delivery.accept")
}

/// Delivers the Accept of every recorded federation to the producer's inbox
/// and records whether it arrived.
///
/// Events are taken one at a time in journal order, and the position moves on
/// only once an event's outcome is recorded: an event may be taken again after
/// a restart, and its Accept delivered twice. A failed delivery is an outcome
/// like a successful one, so one unresponsive inbox holds back the events
/// behind it for no longer than a delivery may take; it is not retried, and
/// the producer gets another Accept by sending its Follow again.
pub struct AcceptDelivery<M>(Arc<M>);

impl<M> AcceptDelivery<M> {
    pub fn new(module: Arc<M>) -> Self {
        Self(module)
    }
}

// `Projector` is declared with `#[async_trait]` by nitinol, so its
// implementation has to be written the same way.
#[async_trait]
impl<M, Tx> Projector<ProducerEvent, Tx> for AcceptDelivery<M>
where
    M: DependOnLocalActorProvider + DependOnActivityDelivery + DependOnProducerService,
    Tx: Send + 'static,
{
    type Error = OutcomeNotRecorded;

    async fn project(
        &mut self,
        event: ProducerEvent,
        _: &mut ProjectionContext<'_, Tx>,
    ) -> Result<(), OutcomeNotRecorded> {
        let ProducerEvent::FederationAccepted {
            actor,
            inbox,
            follow,
        } = event
        else {
            return Ok(());
        };
        let module = &*self.0;
        let local = module.local_actor_provider().local_actor();
        let delivered = match Accept::new(
            local,
            Follow::new(follow.clone(), actor.clone(), local.id().clone()),
        ) {
            Ok(accept) => module.activity_delivery().deliver(&inbox, &accept).await,
            Err(report) => Err(report),
        };
        let producers = module.producer_service();
        match delivered {
            Ok(()) => {
                producers
                    .record_accept_delivered(&actor, RecordAcceptDelivered { follow })
                    .await
            }
            Err(report) => {
                producers
                    .record_accept_failed(
                        &actor,
                        RecordAcceptFailed {
                            follow,
                            reason: format!("{report:#}"),
                        },
                    )
                    .await
            }
        }
        .map_err(OutcomeNotRecorded)
    }
}

/// The outcome of an Accept delivery could not be recorded. The event is
/// taken again, so its Accept is delivered again.
#[derive(Debug, thiserror::Error)]
#[error("the outcome of an Accept delivery could not be recorded: {0:#}")]
pub struct OutcomeNotRecorded(Report<ApplicationError>);
