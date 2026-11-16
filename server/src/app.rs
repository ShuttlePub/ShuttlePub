use std::{ops::Deref, sync::Arc};

use application::{
    config::{AppConfig, DependOnAppConfig},
    identity::DependOnLocalActorService,
    relay::DependOnAcceptFollowService,
};
use driver::{
    config::Config,
    identity::{DevelopmentKey, LocalIdentity},
    relay::{FollowerJournal, RemoteClient, SignatureVerifier},
};
use error_stack::{Report, ResultExt};
use kernel::{
    identity::{DependOnLocalActorProvider, LocalActorProvider},
    relay::{
        DependOnActivityDelivery, DependOnFollowerRecorder, DependOnHttpSignatureVerifier,
        DependOnRemoteActorResolver,
    },
};

use crate::error::ServerError;

/// Composition root: owns the adapters and provides every dependency the
/// services and the HTTP layer ask for.
pub struct Handler {
    bind_address: String,
    local: LocalIdentity,
    remote: RemoteClient,
    verifier: SignatureVerifier,
    followers: FollowerJournal,
}

impl Handler {
    pub fn new(
        config: Config,
        key: DevelopmentKey,
        followers: FollowerJournal,
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
            followers,
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

impl DependOnFollowerRecorder for Handler {
    type FollowerRecorder = FollowerJournal;
    fn follower_recorder(&self) -> &Self::FollowerRecorder {
        &self.followers
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

impl DependOnAcceptFollowService for Handler {
    type AcceptFollowService = Self;
    fn accept_follow_service(&self) -> &Self::AcceptFollowService {
        self
    }
}

impl DependOnLocalActorService for Handler {
    type LocalActorService = Self;
    fn local_actor_service(&self) -> &Self::LocalActorService {
        self
    }
}

/// axum state: a cheap handle to the shared `Handler`.
#[derive(Clone)]
pub struct AppModule(Arc<Handler>);

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
