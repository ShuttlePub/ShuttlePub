use error_stack::{Report, ResultExt};
use kernel::{
    id::ActorId,
    producer::{AcceptFederation, IsAccepted, Producer, RecordAcceptDelivered, RecordAcceptFailed},
};
use nitinol::eventsource::{AggregateProxy, RetryPolicy};

use crate::{ApplicationError, eventsource::DependOnEventSourceSystem};

/// The producer aggregates: one per service that asked to federate, found by
/// the authority of any of its actors.
pub trait ProducerService: DependOnEventSourceSystem {
    /// Records that the federation `command.actor` asked for is accepted.
    fn accept_federation(
        &self,
        command: AcceptFederation,
    ) -> impl Future<Output = Result<(), Report<ApplicationError>>> + Send {
        async move {
            producer(self, &command.actor)
                .ask_with_retry(command, &RetryPolicy::default())
                .await
                .change_context(ApplicationError::Internal)
        }
    }

    /// Whether the service `actor` belongs to has had its federation accepted.
    fn is_accepted(
        &self,
        actor: &ActorId,
    ) -> impl Future<Output = Result<bool, Report<ApplicationError>>> + Send {
        async move {
            producer(self, actor)
                .exec(IsAccepted)
                .await
                .change_context(ApplicationError::Internal)
        }
    }

    /// Records that the Accept for `actor`'s service reached its inbox.
    fn record_accept_delivered(
        &self,
        actor: &ActorId,
        command: RecordAcceptDelivered,
    ) -> impl Future<Output = Result<(), Report<ApplicationError>>> + Send {
        async move {
            producer(self, actor)
                .ask_with_retry(command, &RetryPolicy::default())
                .await
                .change_context(ApplicationError::Internal)
        }
    }

    /// Records that the Accept for `actor`'s service did not reach its inbox.
    fn record_accept_failed(
        &self,
        actor: &ActorId,
        command: RecordAcceptFailed,
    ) -> impl Future<Output = Result<(), Report<ApplicationError>>> + Send {
        async move {
            producer(self, actor)
                .ask_with_retry(command, &RetryPolicy::default())
                .await
                .change_context(ApplicationError::Internal)
        }
    }
}

impl<T: DependOnEventSourceSystem> ProducerService for T {}

pub trait DependOnProducerService: Send + Sync + 'static {
    type ProducerService: ProducerService;
    fn producer_service(&self) -> &Self::ProducerService;
}

fn producer<T: DependOnEventSourceSystem + ?Sized>(
    module: &T,
    actor: &ActorId,
) -> AggregateProxy<Producer> {
    module
        .event_source_system()
        .aggregate_proxy::<Producer>(Producer::aggregate_id(actor))
}
