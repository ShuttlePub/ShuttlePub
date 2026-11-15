use error_stack::Report;
use kernel::{
    entities::{Accept, ActivityId, ActorId, Follow},
    interface::{
        ActivityDelivery, DependOnActivityDelivery, DependOnLocalActorProvider,
        DependOnRemoteActorResolver, LocalActorProvider, RemoteActorResolver,
    },
};

use crate::{
    ApplicationError,
    transfer::{IdentifiedFollow, InboundFollow},
};

/// Answers a Follow of the local actor with an Accept.
///
/// The work is split in two so that the caller can verify the request's HTTP
/// signature with the key `identify` resolved before `accept` delivers anything.
pub trait AcceptFollowService:
    DependOnLocalActorProvider + DependOnRemoteActorResolver + DependOnActivityDelivery
{
    /// Checks, in this order: the Follow id is a URL, the Follow targets the
    /// local actor, the request was addressed to the local actor's host and
    /// port, the follower is a URL; then resolves the follower and checks that
    /// `key_id` is its public key. The first failing check decides the error.
    fn identify(
        &self,
        inbound: InboundFollow,
        key_id: &str,
    ) -> impl Future<Output = Result<IdentifiedFollow, Report<ApplicationError>>> + Send {
        async move {
            let local = self.local_actor_provider().local_actor();
            let id = ActivityId::parse(inbound.id).map_err(ApplicationError::from_kernel)?;
            if inbound.object != local.id().as_str() {
                return Err(Report::new(ApplicationError::UntrustedIdentity));
            }
            if inbound.host.as_deref() != Some(local.authority()) {
                return Err(Report::new(ApplicationError::UntrustedIdentity));
            }
            let follower = ActorId::parse(inbound.actor).map_err(ApplicationError::from_kernel)?;
            let actor = self
                .remote_actor_resolver()
                .resolve(&follower)
                .await
                .map_err(ApplicationError::from_kernel)?;
            if key_id != actor.public_key().id() {
                return Err(Report::new(ApplicationError::UntrustedIdentity));
            }
            Ok(IdentifiedFollow::new(
                Follow::new(id, follower, local.id().clone()),
                actor,
            ))
        }
    }

    /// Delivers the Accept to the follower's inbox; succeeds only once the
    /// inbox took it.
    fn accept(
        &self,
        follow: IdentifiedFollow,
    ) -> impl Future<Output = Result<(), Report<ApplicationError>>> + Send {
        async move {
            let (follow, actor) = follow.into_parts();
            let accept = Accept::new(self.local_actor_provider().local_actor(), follow)
                .map_err(ApplicationError::from_kernel)?;
            self.activity_delivery()
                .deliver(actor.inbox(), &accept)
                .await
                .map_err(ApplicationError::from_kernel)
        }
    }
}

impl<T> AcceptFollowService for T where
    T: DependOnLocalActorProvider + DependOnRemoteActorResolver + DependOnActivityDelivery
{
}

pub trait DependOnAcceptFollowService: Send + Sync + 'static {
    type AcceptFollowService: AcceptFollowService;
    fn accept_follow_service(&self) -> &Self::AcceptFollowService;
}
