//! Development REST API plus the `/api/v1` contract stubs.
pub mod api_v1;
pub mod openapi;
mod routes;

use api_v1::{ApiV1Error, ErrorCode};
use application::NoteService;
use axum::{
    Router,
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use driver::Federation;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    notes: Arc<NoteService>,
    federation: Arc<Federation>,
}

pub fn router(notes: Arc<NoteService>, federation: Arc<Federation>) -> Router {
    Router::new()
        .route("/api/notes", post(routes::create))
        .route("/api/notes/{id}/reactions", post(routes::react))
        .route("/api/timeline", get(routes::timeline))
        .route("/inbox", post(routes::inbox))
        .route("/actor", get(routes::actor))
        .route("/openapi.json", get(openapi_json))
        .with_state(AppState { notes, federation })
        .merge(openapi::contract().0)
}

/// Serves the generated `/api/v1` contract; identical to `openapi/shuttlepub.json`.
async fn openapi_json() -> Response {
    match openapi::render() {
        Ok(body) => ([(header::CONTENT_TYPE, "application/json")], body).into_response(),
        Err(error) => {
            tracing::error!(%error, "openapi rendering failed");
            ApiV1Error::new(ErrorCode::Internal, "openapi rendering failed").into_response()
        }
    }
}
