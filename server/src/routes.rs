use application::ServiceError;
use axum::{
    Json,
    body::to_bytes,
    extract::{Path, Query, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use http_body_util::Full;
use kernel::{ActorId, CreateNote, NoteCommand, NoteId};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;

pub struct ApiError(StatusCode);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(json!({"error": self.0.canonical_reason().unwrap_or("request failed")})),
        )
            .into_response()
    }
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        use nitinol_error_mapping::status;
        Self(status(error))
    }
}

mod nitinol_error_mapping {
    use application::ServiceError;
    use axum::http::StatusCode;
    pub fn status(error: ServiceError) -> StatusCode {
        match error {
            ServiceError::IdentityMismatch => StatusCode::BAD_REQUEST,
            ServiceError::Command(error) => {
                if error.retryability() == application::Retryability::Permanent {
                    StatusCode::CONFLICT
                } else {
                    tracing::error!(error = %error, "note command failed");
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }
            ServiceError::Query(error) => {
                tracing::error!(error = %error, "note query failed");
                StatusCode::INTERNAL_SERVER_ERROR
            }
            ServiceError::Stop(error) => {
                tracing::error!(error = %error, "process stop failed");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

pub async fn create(
    State(state): State<AppState>,
    Json(command): Json<CreateNote>,
) -> Result<StatusCode, ApiError> {
    state
        .notes
        .execute(command.id, NoteCommand::Create(command))
        .await?;
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
pub struct Reaction {
    actor: ActorId,
    reaction: String,
}

pub async fn react(
    State(state): State<AppState>,
    Path(id): Path<uuid::Uuid>,
    Json(command): Json<Reaction>,
) -> Result<StatusCode, ApiError> {
    state
        .notes
        .execute(
            NoteId(id),
            NoteCommand::React {
                actor: command.actor,
                reaction: command.reaction,
            },
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Page {
    limit: Option<usize>,
    before: Option<u64>,
}

pub async fn timeline(
    State(state): State<AppState>,
    Query(page): Query<Page>,
) -> Result<Json<Vec<application::TimelineEntry>>, ApiError> {
    let limit = page.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(ApiError(StatusCode::BAD_REQUEST));
    }
    Ok(Json(
        state
            .notes
            .timeline()
            .await
            .into_iter()
            .filter(|entry| page.before.is_none_or(|before| entry.sequence < before))
            .take(limit)
            .collect(),
    ))
}

pub async fn inbox(
    State(state): State<AppState>,
    request: Request,
) -> Result<StatusCode, ApiError> {
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, 262_144)
        .await
        .map_err(|_| ApiError(StatusCode::PAYLOAD_TOO_LARGE))?;
    state
        .federation
        .accept_follow(axum::http::Request::from_parts(parts, Full::new(body)))
        .await
        .map_err(|error| {
            use driver::FederationError;
            let status = match error {
                FederationError::Signature | FederationError::Identity => StatusCode::UNAUTHORIZED,
                FederationError::Origin => StatusCode::FORBIDDEN,
                FederationError::Json(_)
                | FederationError::Url(_)
                | FederationError::Request(_) => StatusCode::BAD_REQUEST,
                FederationError::ResponseSize
                | FederationError::Http(_)
                | FederationError::Rsa(_)
                | FederationError::Pem(_) => {
                    tracing::error!(error = %error, "federation request failed");
                    StatusCode::BAD_GATEWAY
                }
            };
            ApiError(status)
        })?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn actor(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let actor = state.federation.local_actor().map_err(|error| {
        tracing::error!(error = %error, "actor representation failed");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    Ok((
        [(header::CONTENT_TYPE, "application/activity+json")],
        Json(json!({
            "@context": ["https://www.w3.org/ns/activitystreams", "https://w3id.org/security/v1"],
            "type": "Application", "id": actor.id, "inbox": actor.inbox, "publicKey": actor.public_key
        })),
    ))
}
