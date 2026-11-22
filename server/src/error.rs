use application::ApplicationError;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use error_stack::Report;
use serde_json::json;

/// Reasons the server process fails to start or stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ServerError {
    #[error("the settings could not be loaded")]
    Config,
    #[error("the signing key could not be generated")]
    Key,
    #[error("the application could not be assembled")]
    Assembly,
    #[error("the database could not be reached, migrated or opened")]
    Database,
    #[error("the listener could not be bound")]
    Bind,
    #[error("the HTTP server failed")]
    Serve,
    #[error("the Accept delivery could not be stopped")]
    Reaction,
}

/// Error answer of the ActivityPub endpoints: the status with
/// `{"error": <reason phrase>}`. This is the only place that maps failures to
/// HTTP statuses.
pub(crate) struct ApiError(StatusCode);

impl ApiError {
    pub(crate) fn payload_too_large() -> Self {
        Self(StatusCode::PAYLOAD_TOO_LARGE)
    }

    pub(crate) fn unreadable_activity() -> Self {
        Self(StatusCode::BAD_REQUEST)
    }
}

impl From<Report<ApplicationError>> for ApiError {
    fn from(report: Report<ApplicationError>) -> Self {
        let status = match report.current_context() {
            ApplicationError::InvalidSignature | ApplicationError::UntrustedIdentity => {
                StatusCode::UNAUTHORIZED
            }
            ApplicationError::OriginNotAllowed | ApplicationError::ProducerNotAccepted => {
                StatusCode::FORBIDDEN
            }
            ApplicationError::Malformed => StatusCode::BAD_REQUEST,
            ApplicationError::RemoteFailure => StatusCode::BAD_GATEWAY,
            ApplicationError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status.is_server_error() {
            tracing::error!(error = ?report, "federation request failed");
        }
        Self(status)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let reason = self.0.canonical_reason().unwrap_or("request failed");
        (self.0, Json(json!({ "error": reason }))).into_response()
    }
}
