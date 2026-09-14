//! Development REST API.
mod routes;

use application::NoteService;
use axum::{
    Router,
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
        .route(
            "/openapi.json",
            get(|| async {
                (
                    [("content-type", "application/json")],
                    include_str!("../../docs/openapi.json"),
                )
            }),
        )
        .with_state(AppState { notes, federation })
}
