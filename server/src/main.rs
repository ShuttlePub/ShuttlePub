use application::config::{AppConfig, DependOnAppConfig};
use driver::{config::Config, keys::DevelopmentKey};
use error_stack::{Report, ResultExt};
use server::{
    app::{AppModule, Handler},
    database::{self, PoolTransactions},
    delivery,
    error::ServerError,
};

/// Starts in this order: settings, signing key, database connection and
/// migration, event-sourcing system, Accept delivery, HTTP listener; and on
/// Ctrl-C stops the listener before the Accept delivery.
#[tokio::main]
async fn main() -> Result<(), Report<ServerError>> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let config =
        Config::from_lookup(|name| std::env::var(name).ok()).change_context(ServerError::Config)?;
    let key = tokio::task::spawn_blocking(DevelopmentKey::generate)
        .await
        .change_context(ServerError::Key)?
        .change_context(ServerError::Key)?;
    let database_url = config
        .database_url()
        .ok_or_else(|| {
            Report::new(ServerError::Config).attach("SHUTTLEPUB_DATABASE_URL is not set")
        })?
        .to_owned();
    let pool = database::connect(&database_url).await?;
    let system = database::event_source_system(pool.clone()).await?;
    let app = AppModule::new(Handler::new(config, key, system)?);
    let accept_delivery = delivery::spawn_accept_delivery(&app, PoolTransactions::new(pool)).await;
    let listener = tokio::net::TcpListener::bind(app.app_config().bind_address())
        .await
        .change_context(ServerError::Bind)?;
    let address = listener.local_addr().change_context(ServerError::Bind)?;
    tracing::info!(%address, "development server listening; the signing key is ephemeral");
    let served = axum::serve(listener, server::router(app))
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "shutdown signal failed");
            }
        })
        .await
        .change_context(ServerError::Serve);
    let stopped = accept_delivery.stop().await;
    served?;
    stopped
}
