use axum::Router;

mod account;

use self::account::users;

// http://api.shuttle.pub/v0/account
pub fn v0() -> Router {
    Router::new()
        .nest("/account", users())
}