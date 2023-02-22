use axum::Router;

mod account;

use self::account::account;

// http://api.shuttle.pub/v0/accounts
pub fn v0() -> Router {
    Router::new()
        .nest("/accounts", account())
}