use error_stack::Report;
use kernel::{
    identity::{ActivityId, ActorId, DependOnLocalActorProvider, LocalActorProvider},
    relay::{
        Accept, ActivityDelivery, DependOnActivityDelivery, DependOnFollowerRecorder,
        DependOnRemoteActorResolver, Follow, FollowerRecorder, RemoteActorResolver,
    },
};

use super::{IdentifiedFollow, InboundFollow};
use crate::ApplicationError;

/// Answers a Follow of the local actor with an Accept.
///
/// The work is split in two so that the caller can verify the request's HTTP
/// signature with the key `identify` resolved before `accept` delivers anything.
pub trait AcceptFollowService:
    DependOnLocalActorProvider
    + DependOnRemoteActorResolver
    + DependOnFollowerRecorder
    + DependOnActivityDelivery
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

    /// Records the follower, then delivers the Accept to its inbox; succeeds
    /// only once the inbox took it.
    ///
    /// Recording comes first so that an Accept never reaches an actor this
    /// server did not record: if recording fails nothing is delivered, and if
    /// the delivery fails the record stays and the follower's resent Follow is
    /// recorded (as a resend) and answered again.
    fn accept(
        &self,
        follow: IdentifiedFollow,
    ) -> impl Future<Output = Result<(), Report<ApplicationError>>> + Send {
        async move {
            let (follow, actor) = follow.into_parts();
            let accept = Accept::new(self.local_actor_provider().local_actor(), follow)
                .map_err(ApplicationError::from_kernel)?;
            let follow = accept.object();
            self.follower_recorder()
                .record(follow.actor(), actor.inbox(), follow.id())
                .await
                .map_err(ApplicationError::from_kernel)?;
            self.activity_delivery()
                .deliver(actor.inbox(), &accept)
                .await
                .map_err(ApplicationError::from_kernel)
        }
    }
}

impl<T> AcceptFollowService for T where
    T: DependOnLocalActorProvider
        + DependOnRemoteActorResolver
        + DependOnFollowerRecorder
        + DependOnActivityDelivery
{
}

pub trait DependOnAcceptFollowService: Send + Sync + 'static {
    type AcceptFollowService: AcceptFollowService;
    fn accept_follow_service(&self) -> &Self::AcceptFollowService;
}
