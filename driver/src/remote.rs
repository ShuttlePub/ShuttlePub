use std::time::Duration;

use error_stack::{Report, ResultExt};
use http::header;
use kernel::{
    KernelError,
    entities::{Accept, ActorId, PublicKey, RemoteActor},
    interface::{ActivityDelivery, RemoteActorResolver},
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{DriverError, config::RemoteOrigins, keys::DevelopmentKey, signature::RequestSigner};

const ACTIVITY_JSON: &str = "application/activity+json";
const ACTIVITY_STREAMS: &str = "https://www.w3.org/ns/activitystreams";
const ACTOR_DOCUMENT_LIMIT: usize = 262_144;

/// HTTP client for other servers: resolves actors and delivers Accepts, only
/// to allowed origins, without redirects or ambient proxies.
pub struct RemoteClient {
    http: reqwest::Client,
    origins: RemoteOrigins,
    signer: RequestSigner,
}

impl RemoteClient {
    /// `key_id` is the id under which the local actor publishes `key`.
    pub fn new(
        origins: RemoteOrigins,
        key: DevelopmentKey,
        key_id: String,
    ) -> Result<Self, Report<DriverError>> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .change_context(DriverError::HttpClient)?;
        Ok(Self {
            http,
            origins,
            signer: RequestSigner::new(key, key_id),
        })
    }

    fn destination(&self, url: Url) -> Result<Url, Report<KernelError>> {
        if !self.origins.permits(&url) {
            return Err(Report::new(KernelError::OriginNotAllowed)
                .attach(format!("origin {}", url.origin().ascii_serialization())));
        }
        Ok(url)
    }
}

impl RemoteActorResolver for RemoteClient {
    async fn resolve(&self, id: &ActorId) -> Result<RemoteActor, Report<KernelError>> {
        let url = self.destination(id.as_url().clone())?;
        let mut response = self
            .http
            .get(url)
            .header(header::ACCEPT, ACTIVITY_JSON)
            .send()
            .await
            .change_context(KernelError::RemoteFailure)?
            .error_for_status()
            .change_context(KernelError::RemoteFailure)?;
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .change_context(KernelError::RemoteFailure)?
        {
            if body.len().saturating_add(chunk.len()) > ACTOR_DOCUMENT_LIMIT {
                return Err(Report::new(KernelError::RemoteFailure).attach(format!(
                    "actor document exceeds {ACTOR_DOCUMENT_LIMIT} bytes"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        let document: ActorDocument =
            serde_json::from_slice(&body).change_context(KernelError::Malformed)?;
        let actor = RemoteActor::new(
            id,
            &document.id,
            document.inbox,
            PublicKey::new(
                document.public_key.id,
                document.public_key.owner,
                document.public_key.public_key_pem,
            ),
        )?;
        self.destination(Url::parse(actor.inbox()).change_context(KernelError::Malformed)?)?;
        Ok(actor)
    }
}

impl ActivityDelivery for RemoteClient {
    async fn deliver(&self, inbox: &str, accept: &Accept) -> Result<(), Report<KernelError>> {
        let body = serde_json::to_vec(&AcceptDocument::new(accept))
            .change_context(KernelError::Internal)?;
        let request = self.signer.sign_post(inbox, body).await?;
        let request = reqwest::Request::try_from(request.map(reqwest::Body::wrap))
            .change_context(KernelError::RemoteFailure)?;
        self.http
            .execute(request)
            .await
            .change_context(KernelError::RemoteFailure)?
            .error_for_status()
            .change_context(KernelError::RemoteFailure)?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActorDocument {
    id: String,
    inbox: String,
    public_key: PublicKeyDocument,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicKeyDocument {
    id: String,
    owner: String,
    public_key_pem: String,
}

#[derive(Serialize)]
struct AcceptDocument<'a> {
    #[serde(rename = "@context")]
    context: &'static str,
    id: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    actor: &'a str,
    object: FollowDocument<'a>,
}

impl<'a> AcceptDocument<'a> {
    fn new(accept: &'a Accept) -> Self {
        let follow = accept.object();
        Self {
            context: ACTIVITY_STREAMS,
            id: accept.id().as_str(),
            kind: "Accept",
            actor: accept.actor().as_str(),
            object: FollowDocument {
                id: follow.id().as_str(),
                kind: "Follow",
                actor: follow.actor().as_str(),
                object: follow.object().as_str(),
            },
        }
    }
}

#[derive(Serialize)]
struct FollowDocument<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    kind: &'static str,
    actor: &'a str,
    object: &'a str,
}
