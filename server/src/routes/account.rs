use axum::{Router, response::IntoResponse, routing::post};

pub fn users() -> Router {
    Router::new()
        .route("/signup", post(signup))
        .route("/login", post(login))
}

async fn signup() -> impl IntoResponse {
    todo!()
}

async fn login() -> impl IntoResponse {
    todo!()
}