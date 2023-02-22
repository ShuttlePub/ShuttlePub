use std::net::SocketAddr;

use axum::Router;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, Layer, util::SubscriberInitExt};

use server::di;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let appender = tracing_appender::rolling::daily(std::path::Path::new("./logs/"), "debug.log");
    let (non_blocking_appender, _guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer()
            .with_filter(tracing_subscriber::EnvFilter::new(std::env::var("RUST_LOG").unwrap_or_else(|_| "driver=debug,server=debug".into())))
            .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG))
        .with(tracing_subscriber::fmt::Layer::default()
            .with_writer(non_blocking_appender)
            .with_ansi(false)
            .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG))
        .init();

    di::inject().await?;

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let routes = Router::new()
        .nest("/v0/", server::routes::v0())
        .into_make_service();

    #[allow(clippy::let_unit_value)]
    let _ = axum::Server::bind(&addr)
        .serve(routes)
        .await?;
    
    Ok(())
}
