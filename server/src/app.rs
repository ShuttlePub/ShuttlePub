use std::{ops::Deref, sync::Arc};

use application::{
    community::DependOnLocalActorService,
    config::{AppConfig, DependOnAppConfig},
    eventsource::DependOnEventSourceSystem,
    intake::DependOnIntakeService,
    note::DependOnNoteService,
    producer::DependOnProducerService,
};
use driver::{
    community::LocalIdentity, config::Config, intake::SignatureVerifier, keys::DevelopmentKey,
    remote::RemoteClient,
};
use error_stack::{Report, ResultExt};
use kernel::{
    community::{DependOnLocalActorProvider, LocalActorProvider},
    intake::DependOnHttpSignatureVerifier,
};
use nitinol::{eventsource::system::EventSourceSystem, sqlx::PostgresStore};
use port::{delivery::DependOnActivityDelivery, intake::DependOnRemoteActorResolver};

use crate::error::ServerError;

/// Composition root: owns the adapters and the event-sourcing system, and
/// provides every dependency the services, the reactions and the HTTP layer
/// ask for. The events are kept in PostgreSQL.
pub struct Handler {
    bind_address: String,
    local: LocalIdentity,
    remote: RemoteClient,
    verifier: SignatureVerifier,
    system: EventSourceSystem<PostgresStore>,
}

impl Handler {
    pub fn new(
        config: Config,
        key: DevelopmentKey,
        system: EventSourceSystem<PostgresStore>,
    ) -> Result<Self, Report<ServerError>> {
        let local = LocalIdentity::new(config.actor().clone(), &key)
            .change_context(ServerError::Assembly)?;
        let key_id = local.local_actor().public_key().id().to_owned();
        let remote = RemoteClient::new(config.remote_origins().clone(), key, key_id)
            .change_context(ServerError::Assembly)?;
        Ok(Self {
            bind_address: config.bind().to_owned(),
            local,
            remote,
            verifier: SignatureVerifier,
            system,
        })
    }
}

impl AppConfig for Handler {
    fn bind_address(&self) -> &str {
        &self.bind_address
    }
}

impl DependOnAppConfig for Handler {
    type AppConfig = Self;
    fn app_config(&self) -> &Self::AppConfig {
        self
    }
}

impl DependOnEventSourceSystem for Handler {
    type Store = PostgresStore;
    fn event_source_system(&self) -> &EventSourceSystem<PostgresStore> {
        &self.system
    }
}

impl DependOnLocalActorProvider for Handler {
    type LocalActorProvider = LocalIdentity;
    fn local_actor_provider(&self) -> &Self::LocalActorProvider {
        &self.local
    }
}

impl DependOnRemoteActorResolver for Handler {
    type RemoteActorResolver = RemoteClient;
    fn remote_actor_resolver(&self) -> &Self::RemoteActorResolver {
        &self.remote
    }
}

impl DependOnActivityDelivery for Handler {
    type ActivityDelivery = RemoteClient;
    fn activity_delivery(&self) -> &Self::ActivityDelivery {
        &self.remote
    }
}

impl DependOnHttpSignatureVerifier for Handler {
    type HttpSignatureVerifier = SignatureVerifier;
    fn http_signature_verifier(&self) -> &Self::HttpSignatureVerifier {
        &self.verifier
    }
}

impl DependOnLocalActorService for Handler {
    type LocalActorService = Self;
    fn local_actor_service(&self) -> &Self::LocalActorService {
        self
    }
}

impl DependOnProducerService for Handler {
    type ProducerService = Self;
    fn producer_service(&self) -> &Self::ProducerService {
        self
    }
}

impl DependOnNoteService for Handler {
    type NoteService = Self;
    fn note_service(&self) -> &Self::NoteService {
        self
    }
}

impl DependOnIntakeService for Handler {
    type IntakeService = Self;
    fn intake_service(&self) -> &Self::IntakeService {
        self
    }
}

/// axum state: a cheap handle to the shared `Handler`.
#[derive(Clone)]
pub struct AppModule(pub(crate) Arc<Handler>);

impl AppModule {
    pub fn new(handler: Handler) -> Self {
        Self(Arc::new(handler))
    }
}

impl Deref for AppModule {
    type Target = Handler;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
