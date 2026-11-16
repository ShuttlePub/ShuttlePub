use application::config::{AppConfig, DependOnAppConfig};
use driver::{config::Config, keys::DevelopmentKey};
use error_stack::{Report, ResultExt};
use server::{
    app::{AppModule, Handler},
    error::ServerError,
};

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
    let app = AppModule::new(Handler::new(config, key)?);
    let listener = tokio::net::TcpListener::bind(app.app_config().bind_address())
        .await
        .change_context(ServerError::Bind)?;
    let address = listener.local_addr().change_context(ServerError::Bind)?;
    tracing::info!(%address, "development server listening; the signing key is ephemeral");
    axum::serve(listener, server::router(app))
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "shutdown signal failed");
            }
        })
        .await
        .change_context(ServerError::Serve)
}
