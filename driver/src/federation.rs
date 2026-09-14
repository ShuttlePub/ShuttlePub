use std::{collections::BTreeSet, sync::Arc, time::Duration};

use bytes::Bytes;
use http::Request;
use http_body_util::{BodyExt, Full};
use url::Url;

use crate::{
    DevelopmentKey, FederationError, Follow, RemoteActor,
    signature::{sign_request, signature_input, verify_request},
    wire::Accept,
};

pub struct Federation {
    local: Url,
    origins: BTreeSet<String>,
    key: Arc<DevelopmentKey>,
    client: reqwest::Client,
}

impl Federation {
    pub fn new(
        local: &str,
        origins: Vec<String>,
        key: Arc<DevelopmentKey>,
    ) -> Result<Self, FederationError> {
        let local = Url::parse(local)?;
        let origins = origins
            .into_iter()
            .map(|origin| Url::parse(&origin).map(|url| url.origin().ascii_serialization()))
            .collect::<Result<_, _>>()?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()?;
        Ok(Self {
            local,
            origins,
            key,
            client,
        })
    }

    fn destination(&self, value: &str) -> Result<Url, FederationError> {
        let url = Url::parse(value)?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || !self.origins.contains(&url.origin().ascii_serialization())
        {
            return Err(FederationError::Origin);
        }
        Ok(url)
    }

    pub fn local_actor(&self) -> Result<RemoteActor, FederationError> {
        Ok(RemoteActor::new(
            self.local.as_str(),
            self.local.join("inbox")?.as_str(),
            self.key.public_pem()?,
        ))
    }

    pub async fn resolve(&self, actor_id: &str) -> Result<RemoteActor, FederationError> {
        let url = self.destination(actor_id)?;
        let mut response = self
            .client
            .get(url)
            .header("accept", "application/activity+json")
            .send()
            .await?
            .error_for_status()?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len().saturating_add(chunk.len()) > 262_144 {
                return Err(FederationError::ResponseSize);
            }
            body.extend_from_slice(&chunk);
        }
        let actor: RemoteActor = serde_json::from_slice(&body)?;
        if actor.id != actor_id || actor.public_key.owner != actor.id {
            return Err(FederationError::Identity);
        }
        self.destination(&actor.inbox)?;
        Ok(actor)
    }

    pub async fn accept_follow(
        &self,
        request: Request<Full<Bytes>>,
    ) -> Result<(), FederationError> {
        let input = signature_input(request.headers())?;
        let follow: Follow = serde_json::from_slice(
            request
                .body()
                .clone()
                .collect()
                .await
                .map_err(|_| FederationError::Signature)?
                .to_bytes()
                .as_ref(),
        )?;
        Url::parse(&follow.id)?;
        if follow.object != self.local.as_str() {
            return Err(FederationError::Identity);
        }
        let host = request
            .headers()
            .get("host")
            .and_then(|value| value.to_str().ok())
            .ok_or(FederationError::Identity)?;
        if host != &self.local[url::Position::BeforeHost..url::Position::AfterPort] {
            return Err(FederationError::Identity);
        }
        let actor = self.resolve(&follow.actor).await?;
        if input.key_id() != actor.public_key.id {
            return Err(FederationError::Identity);
        }
        verify_request(request, &actor).await?;
        let mut accept_id = self.local.clone();
        accept_id
            .path_segments_mut()
            .map_err(|_| FederationError::Identity)?
            .extend(["accepts", &follow.id]);
        let accept = Accept {
            context: "https://www.w3.org/ns/activitystreams",
            id: accept_id.into(),
            kind: "Accept",
            actor: self.local.as_str(),
            object: &follow,
        };
        let request = sign_request(
            &self.key,
            &format!("{}#main-key", self.local),
            &actor.inbox,
            serde_json::to_vec(&accept)?,
        )
        .await?;
        let request = request.map(reqwest::Body::wrap);
        self.client
            .execute(reqwest::Request::try_from(request)?)
            .await?
            .error_for_status()?;
        Ok(())
    }
}
