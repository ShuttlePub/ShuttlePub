//! HTTP layer: the composition root, the ActivityPub endpoints (`POST /inbox`
//! in `intake`, `GET /actor` in `community`), starting the Accept delivery
//! (`delivery`), the PostgreSQL wiring (`database`) and the `/api/v1` contract
//! stubs.
pub mod api_v1;
pub mod app;
mod community;
pub mod database;
pub mod delivery;
pub mod error;
mod intake;
pub mod openapi;

use api_v1::{ApiV1Error, ErrorCode};
use app::AppModule;
use axum::{
    Router,
    http::header,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};

pub fn router(app: AppModule) -> Router {
    Router::new()
        .route(
            "/inbox",
            post(intake::receive).route_layer(middleware::from_fn_with_state(
                app.clone(),
                intake::verify_signature,
            )),
        )
        .route("/actor", get(community::show))
        .route("/openapi.json", get(openapi_json))
        .with_state(app)
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
