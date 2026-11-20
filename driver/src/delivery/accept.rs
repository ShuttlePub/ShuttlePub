use error_stack::{Report, ResultExt};
use kernel::{KernelError, producer::Accept};
use port::delivery::ActivityDelivery;
use serde::Serialize;
use url::Url;

use crate::remote::RemoteClient;

const ACTIVITY_STREAMS: &str = "https://www.w3.org/ns/activitystreams";

impl ActivityDelivery for RemoteClient {
    async fn deliver(&self, inbox: &str, accept: &Accept) -> Result<(), Report<KernelError>> {
        // The inbox was checked when the Follow arrived, but it may be
        // delivered after a restart under different origin settings.
        let inbox = self.destination(Url::parse(inbox).change_context(KernelError::Malformed)?)?;
        let body = serde_json::to_vec(&AcceptDocument::new(accept))
            .change_context(KernelError::Internal)?;
        let request = self.signer.sign_post(inbox.as_str(), body).await?;
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
