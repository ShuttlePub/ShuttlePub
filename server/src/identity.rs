//! `GET /actor`: the local actor.
use application::identity::{DependOnLocalActorService, LocalActorService};
use axum::{
    Json,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use kernel::identity::LocalActor;
use serde::Serialize;

use crate::app::AppModule;

const ACTIVITY_JSON: &str = "application/activity+json";
const CONTEXT: [&str; 2] = [
    "https://www.w3.org/ns/activitystreams",
    "https://w3id.org/security/v1",
];

/// `GET /actor`: the local actor as an ActivityPub `Application`.
pub(crate) async fn show(State(app): State<AppModule>) -> Response {
    let actor = app.local_actor_service().get_local_actor();
    (
        [(header::CONTENT_TYPE, ACTIVITY_JSON)],
        Json(ActorDocument::new(actor)),
    )
        .into_response()
}

#[derive(Serialize)]
struct ActorDocument<'a> {
    #[serde(rename = "@context")]
    context: [&'static str; 2],
    #[serde(rename = "type")]
    kind: &'static str,
    id: &'a str,
    inbox: &'a str,
    #[serde(rename = "publicKey")]
    public_key: PublicKeyDocument<'a>,
}

impl<'a> ActorDocument<'a> {
    fn new(actor: &'a LocalActor) -> Self {
        let key = actor.public_key();
        Self {
            context: CONTEXT,
            kind: "Application",
            id: actor.id().as_str(),
            inbox: actor.inbox().as_str(),
            public_key: PublicKeyDocument {
                id: key.id(),
                owner: key.owner(),
                public_key_pem: key.pem(),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicKeyDocument<'a> {
    id: &'a str,
    owner: &'a str,
    public_key_pem: &'a str,
}
