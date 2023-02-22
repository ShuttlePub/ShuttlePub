use axum::{Router, response::IntoResponse, routing::post};

pub fn account() -> Router {
    Router::new()
        .route("/signup", post(signup))
}

async fn signup() -> impl IntoResponse {
    todo!()
}