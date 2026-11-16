use error_stack::Report;

use super::Accept;
use crate::KernelError;

/// Delivers an Accept, signed by the local actor, to a remote inbox.
pub trait ActivityDelivery: Send + Sync + 'static {
    fn deliver(
        &self,
        inbox: &str,
        accept: &Accept,
    ) -> impl Future<Output = Result<(), Report<KernelError>>> + Send;
}

pub trait DependOnActivityDelivery: Send + Sync + 'static {
    type ActivityDelivery: ActivityDelivery;
    fn activity_delivery(&self) -> &Self::ActivityDelivery;
}
