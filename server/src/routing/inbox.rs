use application::{
    ApplicationError,
    services::{AcceptFollowService, DependOnAcceptFollowService},
    transfer::{IdentifiedFollow, InboundFollow},
};
use axum::{
    Extension,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{self, StatusCode, header},
    middleware::Next,
    response::Response,
};
use kernel::interface::{DependOnHttpSignatureVerifier, HttpSignatureVerifier};
use serde::Deserialize;

use crate::{app::AppModule, error::ApiError};

const BODY_LIMIT: usize = 262_144;

/// A Follow whose request signature was verified with the follower's key.
/// Only `verify_signature` creates it, so `accept` cannot run on an
/// unverified request.
#[derive(Clone)]
pub(crate) struct VerifiedFollow(IdentifiedFollow);

/// Middleware of `POST /inbox`. Rejects the request with the answer of the
/// first failing check, in this order: body size, Signature header syntax,
/// Follow JSON, the Follow and its sender (see `AcceptFollowService::identify`),
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
    let follow = app
        .accept_follow_service()
        .identify(inbound_follow(&request)?, &key_id)
        .await?;
    verifier
        .verify(&request, follow.actor().public_key())
        .await
        .map_err(ApplicationError::from_kernel)?;
    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(VerifiedFollow(follow));
    Ok(next.run(Request::from_parts(parts, Body::from(body))).await)
}

/// `POST /inbox`: answers 202 once the Accept was delivered.
pub(crate) async fn accept(
    State(app): State<AppModule>,
    Extension(VerifiedFollow(follow)): Extension<VerifiedFollow>,
) -> Result<StatusCode, ApiError> {
    app.accept_follow_service().accept(follow).await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Activity {
    Follow {
        id: String,
        actor: String,
        object: String,
    },
}

fn inbound_follow(request: &http::Request<Bytes>) -> Result<InboundFollow, ApiError> {
    let Activity::Follow { id, actor, object } =
        serde_json::from_slice(request.body()).map_err(|_| ApiError::unreadable_activity())?;
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    Ok(InboundFollow {
        id,
        actor,
        object,
        host,
    })
}
