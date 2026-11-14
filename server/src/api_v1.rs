//! `/api/v1` contract surface: shared DTOs, the structured error format and
//! explicit `501 Not Implemented` stub handlers.
//!
//! This module only defines routes and types. No handler performs real work;
//! every contract route answers `501` with the `not_implemented` error code so
//! that clients can never mistake a stub for a successful response.

pub mod personal_domain;
pub mod quick_reaction;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

/// Path prefix shared by every contract route.
pub const PREFIX: &str = "/api/v1";

/// Machine readable error code carried by every error body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request is malformed or fails validation.
    InvalidRequest,
    /// Authentication is required or failed.
    Unauthorized,
    /// The caller is authenticated but not allowed to perform the operation.
    Forbidden,
    /// The addressed resource does not exist.
    NotFound,
    /// The request conflicts with the current resource state.
    Conflict,
    /// The `expected_revision` of an optimistic update is stale; the body
    /// carries the revision currently stored.
    RevisionConflict,
    /// An emoji needed by the request is unavailable (deleted or disabled).
    EmojiUnavailable,
    /// The emoji exists and is available but may not be used this way: an
    /// external (other server) custom emoji is only usable by pressing an
    /// existing reaction chip of the Note (`usage: existing_reaction_only`).
    EmojiUsageNotAllowed,
    /// The server failed unexpectedly.
    Internal,
    /// The route is part of the API contract but has no implementation yet.
    NotImplemented,
    /// A search hit its response time limit without any resumable progress; with
    /// progress the answer is a partial 200 instead (`page.truncated`).
    SearchTimeout,
}

impl ErrorCode {
    /// HTTP status that accompanies this code.
    pub fn status(self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict | Self::RevisionConflict => StatusCode::CONFLICT,
            Self::EmojiUnavailable | Self::EmojiUsageNotAllowed => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotImplemented => StatusCode::NOT_IMPLEMENTED,
            Self::SearchTimeout => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

/// A single field level validation problem.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FieldViolation {
    /// Name or JSON pointer of the offending field.
    pub field: String,
    /// Human readable explanation.
    pub message: String,
}

/// Structured error body returned by `/api/v1` handlers for every error they report.
///
/// Responses produced by the framework itself (unmatched route, wrong method)
/// do not go through a handler and therefore do not use this body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ErrorBody {
    /// Stable machine readable code; clients should branch on this value.
    pub code: ErrorCode,
    /// Human readable description intended for developers, not end users.
    pub message: String,
    /// Optional per-field problems, mainly for `invalid_request`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<FieldViolation>,
    /// Optional S8 outbound block refusal. code remains forbidden (403); absent
    /// for all existing S0/S3 errors. No mutation, implicit unblock or Undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_block: Option<personal_domain::OutboundDomainBlock>,
}

/// Error response: an [`ErrorBody`] with the HTTP status implied by its code.
#[derive(Clone, Debug)]
pub struct ApiV1Error(pub ErrorBody);

impl ApiV1Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self(ErrorBody {
            code,
            message: message.into(),
            details: Vec::new(),
            domain_block: None,
        })
    }

    /// Explicit stub answer for routes that exist only as contract.
    pub fn not_implemented() -> Self {
        Self::new(
            ErrorCode::NotImplemented,
            "this endpoint is defined by the API contract but not implemented yet",
        )
    }
}

impl IntoResponse for ApiV1Error {
    fn into_response(self) -> Response {
        (self.0.code.status(), Json(self.0)).into_response()
    }
}

/// Query parameters for cursor paginated collections.
#[derive(Clone, Debug, Default, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct PageParams {
    /// Maximum number of items to return; the server picks a default when omitted.
    #[param(minimum = 1)]
    #[schema(minimum = 1)]
    pub limit: Option<u32>,
    /// Opaque cursor taken from a previous `next_cursor`.
    pub cursor: Option<String>,
}

/// Pagination metadata embedded in collection responses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PageInfo {
    /// Cursor for the next page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Body of the example contract route.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ApiStatus {
    /// API generation served, for example `v1`.
    pub api_version: String,
}

/// Example contract route (stub: always answers 501)
#[utoipa::path(
    get,
    path = "/api/v1/status",
    operation_id = "getApiStatus",
    tag = "meta",
    responses(
        (status = 200, description = "Contract only: never returned while the endpoint is a stub", body = ApiStatus),
        (status = 501, description = "Not implemented yet", body = ErrorBody),
    )
)]
pub async fn get_status() -> ApiV1Error {
    ApiV1Error::not_implemented()
}

/// Every contract route, registered once.
///
/// `routes!` takes the HTTP method and path from each handler's
/// `#[utoipa::path]`, so the axum route and the OpenAPI operation come from the
/// same definition. Stub handlers must take no extractors so that they answer
/// `501` regardless of the request.
pub fn router() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(get_status))
        .merge(quick_reaction::router())
        .merge(personal_domain::router())
}
