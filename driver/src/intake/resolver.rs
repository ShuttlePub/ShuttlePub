use error_stack::{Report, ResultExt};
use http::header;
use kernel::{KernelError, community::PublicKey, id::ActorId, intake::RemoteActor};
use port::intake::RemoteActorResolver;
use serde::Deserialize;
use url::Url;

use crate::remote::{ACTIVITY_JSON, RemoteClient};

const ACTOR_DOCUMENT_LIMIT: usize = 262_144;

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
