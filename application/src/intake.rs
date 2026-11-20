//! Checking what arrives at the inbox and turning it into events: a Follow
//! into an accepted federation, a Create of a public Note from an accepted
//! producer into a received Note.
//!
//! Each activity is handled in two steps so that the caller can verify the
//! request's HTTP signature with the key the first step resolved before the
//! second step records anything.
mod create;
mod follow;

use std::time::SystemTime;

use error_stack::Report;
use kernel::{
    community::{DependOnLocalActorProvider, LocalActor, LocalActorProvider},
    id::{ActivityId, ActorId},
    intake::RemoteActor,
    note::ReceiveNote,
    producer::{AcceptFederation, Follow},
};
use port::intake::{DependOnRemoteActorResolver, RemoteActorResolver};

pub use self::{
    create::{IdentifiedCreate, InboundCreate},
    follow::{IdentifiedFollow, InboundFollow},
};
use crate::{
    ApplicationError,
    note::{DependOnNoteService, NoteService},
    producer::{DependOnProducerService, ProducerService},
};

pub trait IntakeService:
    DependOnLocalActorProvider
    + DependOnRemoteActorResolver
    + DependOnProducerService
    + DependOnNoteService
{
    /// Checks, in this order: the Follow id is a URL, the Follow targets the
    /// local actor, then the sender (see `identify_sender`). The first failing
    /// check decides the error.
    fn identify_follow(
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
            let (actor, sender) =
                identify_sender(self, local, inbound.host.as_deref(), inbound.actor, key_id)
                    .await?;
            Ok(IdentifiedFollow::new(
                Follow::new(id, actor, local.id().clone()),
                sender,
            ))
        }
    }

    /// Checks, in this order: the Create id, its actor and the Note id are
    /// URLs, then the sender (see `identify_sender`). The first failing check
    /// decides the error.
    fn identify_create(
        &self,
        inbound: InboundCreate,
        key_id: &str,
    ) -> impl Future<Output = Result<IdentifiedCreate, Report<ApplicationError>>> + Send {
        async move {
            let local = self.local_actor_provider().local_actor();
            let activity = ActivityId::parse(inbound.id).map_err(ApplicationError::from_kernel)?;
            // A Create's shape is checked before its Host, but the sender
            // checks parse the actor after the Host, as a Follow's order
            // requires; so the actor is parsed here only to be checked.
            ActorId::parse(inbound.actor.as_str()).map_err(ApplicationError::from_kernel)?;
            let note = ActivityId::parse(inbound.note).map_err(ApplicationError::from_kernel)?;
            let (actor, sender) =
                identify_sender(self, local, inbound.host.as_deref(), inbound.actor, key_id)
                    .await?;
            Ok(IdentifiedCreate {
                activity,
                actor,
                note,
                attributed_to: inbound.attributed_to,
                audience: inbound.audience,
                raw: inbound.raw,
                sender,
            })
        }
    }

    /// Records the federation of the Follow's sender, so that the Accept is
    /// delivered by the reaction to it rather than before answering.
    fn accept_follow(
        &self,
        follow: IdentifiedFollow,
    ) -> impl Future<Output = Result<(), Report<ApplicationError>>> + Send {
        async move {
            let command = AcceptFederation {
                actor: follow.follow().actor().clone(),
                inbox: follow.sender().inbox().to_owned(),
                follow: follow.follow().id().clone(),
            };
            self.producer_service().accept_federation(command).await
        }
    }

    /// Checks, in this order: the Note is the Create actor's own (untrusted
    /// identity otherwise), the actor's service is an accepted producer (not
    /// accepted otherwise); then records the Note if it is addressed to the
    /// public and succeeds without recording it if not.
    fn accept_create(
        &self,
        create: IdentifiedCreate,
        received_at: SystemTime,
    ) -> impl Future<Output = Result<(), Report<ApplicationError>>> + Send {
        async move {
            if !create.is_by_its_actor() {
                return Err(Report::new(ApplicationError::UntrustedIdentity));
            }
            if !self.producer_service().is_accepted(&create.actor).await? {
                return Err(Report::new(ApplicationError::ProducerNotAccepted));
            }
            if !create.is_addressed_to_the_public() {
                return Ok(());
            }
            let producer = create.actor.authority().to_owned();
            self.note_service()
                .receive(ReceiveNote {
                    activity: create.activity,
                    note: create.note,
                    author: create.actor,
                    producer,
                    received_at,
                    raw: create.raw,
                })
                .await
        }
    }
}

impl<T> IntakeService for T where
    T: DependOnLocalActorProvider
        + DependOnRemoteActorResolver
        + DependOnProducerService
        + DependOnNoteService
{
}

pub trait DependOnIntakeService: Send + Sync + 'static {
    type IntakeService: IntakeService;
    fn intake_service(&self) -> &Self::IntakeService;
}

/// The checks every activity's sender goes through, in this order: the
/// request was addressed to the local actor's host and port, the actor is a
/// URL; then the actor is resolved and `key_id` must be its public key.
async fn identify_sender<T: DependOnRemoteActorResolver + ?Sized>(
    module: &T,
    local: &LocalActor,
    host: Option<&str>,
    actor: String,
    key_id: &str,
) -> Result<(ActorId, RemoteActor), Report<ApplicationError>> {
    if host != Some(local.authority()) {
        return Err(Report::new(ApplicationError::UntrustedIdentity));
    }
    let actor = ActorId::parse(actor).map_err(ApplicationError::from_kernel)?;
    let sender = module
        .remote_actor_resolver()
        .resolve(&actor)
        .await
        .map_err(ApplicationError::from_kernel)?;
    if key_id != sender.public_key().id() {
        return Err(Report::new(ApplicationError::UntrustedIdentity));
    }
    Ok((actor, sender))
}
