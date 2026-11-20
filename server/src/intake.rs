use std::time::SystemTime;

use application::{
    ApplicationError,
    intake::{
        DependOnIntakeService, IdentifiedCreate, IdentifiedFollow, InboundCreate, InboundFollow,
        IntakeService,
    },
};
use axum::{
    Extension,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{self, StatusCode, header},
    middleware::Next,
    response::Response,
};
use kernel::intake::{DependOnHttpSignatureVerifier, HttpSignatureVerifier, RemoteActor};
use serde::Deserialize;
use serde_json::Value;

use crate::{app::AppModule, error::ApiError};

const BODY_LIMIT: usize = 262_144;

/// An activity whose sender was resolved; its signature is not verified yet.
#[derive(Clone)]
enum IdentifiedActivity {
    Follow(IdentifiedFollow),
    Create(IdentifiedCreate),
}

impl IdentifiedActivity {
    fn sender(&self) -> &RemoteActor {
        match self {
            Self::Follow(follow) => follow.sender(),
            Self::Create(create) => create.sender(),
        }
    }
}

/// An activity whose request signature was verified with its sender's key.
/// Only `verify_signature` creates it, so `receive` cannot run on an
/// unverified request.
#[derive(Clone)]
pub(crate) struct VerifiedActivity(IdentifiedActivity);

/// Middleware of `POST /inbox`. Rejects the request with the answer of the
/// first failing check, in this order: body size, Signature header syntax,
/// activity JSON, the activity and its sender (see
/// `IntakeService::identify_follow` and `IntakeService::identify_create`),
/// then the signature itself.
pub(crate) async fn verify_signature(
    State(app): State<AppModule>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| ApiError::payload_too_large())?;
    let request = http::Request::from_parts(parts, body);
    let verifier = app.http_signature_verifier();
    let key_id = verifier
        .inspect(&request)
        .map_err(ApplicationError::from_kernel)?;
    let intake = app.intake_service();
    let activity = match inbound(&request)? {
        Inbound::Follow(follow) => {
            IdentifiedActivity::Follow(intake.identify_follow(follow, &key_id).await?)
        }
        Inbound::Create(create) => {
            IdentifiedActivity::Create(intake.identify_create(create, &key_id).await?)
        }
    };
    verifier
        .verify(&request, activity.sender().public_key())
        .await
        .map_err(ApplicationError::from_kernel)?;
    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(VerifiedActivity(activity));
    Ok(next.run(Request::from_parts(parts, Body::from(body))).await)
}

/// `POST /inbox`: answers 202 once what the activity means is recorded. The
/// Accept of a Follow is delivered afterwards, by the reaction to the record.
pub(crate) async fn receive(
    State(app): State<AppModule>,
    Extension(VerifiedActivity(activity)): Extension<VerifiedActivity>,
) -> Result<StatusCode, ApiError> {
    let intake = app.intake_service();
    match activity {
        IdentifiedActivity::Follow(follow) => intake.accept_follow(follow).await?,
        IdentifiedActivity::Create(create) => {
            intake.accept_create(create, SystemTime::now()).await?
        }
    }
    Ok(StatusCode::ACCEPTED)
}

enum Inbound {
    Follow(InboundFollow),
    Create(InboundCreate),
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Activity {
    Follow {
        id: String,
        actor: String,
        object: String,
    },
    Create {
        id: String,
        actor: String,
        object: EmbeddedObject,
        to: Option<Value>,
        cc: Option<Value>,
    },
}

/// The object a Create carries: only an embedded Note is understood.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum EmbeddedObject {
    Note {
        id: String,
        #[serde(rename = "attributedTo")]
        attributed_to: String,
    },
}

fn inbound(request: &http::Request<Bytes>) -> Result<Inbound, ApiError> {
    let body = request.body();
    let activity: Activity =
        serde_json::from_slice(body).map_err(|_| ApiError::unreadable_activity())?;
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    Ok(match activity {
        Activity::Follow { id, actor, object } => Inbound::Follow(InboundFollow {
            id,
            actor,
            object,
            host,
        }),
        Activity::Create {
            id,
            actor,
            object:
                EmbeddedObject::Note {
                    id: note,
                    attributed_to,
                },
            to,
            cc,
        } => Inbound::Create(InboundCreate {
            id,
            actor,
            note,
            attributed_to,
            audience: [audience(to), audience(cc)].concat(),
            raw: std::str::from_utf8(body)
                .map_err(|_| ApiError::unreadable_activity())?
                .to_owned(),
            host,
        }),
    })
}

/// The entries of a `to` or `cc`, which is a single IRI or a list of them.
/// Anything else names no one this server could recognise, so it adds none.
fn audience(field: Option<Value>) -> Vec<String> {
    match field {
        Some(Value::String(entry)) => vec![entry],
        Some(Value::Array(entries)) => entries
            .into_iter()
            .filter_map(|entry| match entry {
                Value::String(entry) => Some(entry),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}
