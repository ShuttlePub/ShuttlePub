use application::config::{AppConfig, DependOnAppConfig};
use driver::{
    config::Config,
    identity::DevelopmentKey,
    persistence::{Database, DatabaseSettings, EventSourcing},
    relay::{FollowerJournal, start_follower_projection},
};
use error_stack::{Report, ResultExt};
use server::{
    app::{AppModule, Handler},
    error::ServerError,
};

/// Starts in this order: settings, signing key, database connection and
/// migrations, the event sourcing system, the follower projection, then the
/// listener, so that the server never listens without a usable database. On
/// shutdown the listener stops first and the projection after it.
#[tokio::main]
async fn main() -> Result<(), Report<ServerError>> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let lookup = |name: &str| std::env::var(name).ok();
    let config = Config::from_lookup(lookup).change_context(ServerError::Config)?;
    let database_settings =
        DatabaseSettings::from_lookup(lookup).change_context(ServerError::Config)?;
    let key = tokio::task::spawn_blocking(DevelopmentKey::generate)
        .await
        .change_context(ServerError::Key)?
        .change_context(ServerError::Key)?;
    let database = Database::connect(&database_settings)
        .await
        .change_context(ServerError::Database)?;
    database
        .migrate()
        .await
        .change_context(ServerError::Database)?;
    let events = EventSourcing::start(
        database
            .event_store()
            .change_context(ServerError::Database)?,
    )
    .await;
    let projection = start_follower_projection(&events, &database).await;
    let app = AppModule::new(Handler::new(config, key, FollowerJournal::new(&events))?);
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
    if let Err(error) = projection.stop().await {
        tracing::error!(?error, "the follower projection could not be stopped");
    }
    served
}
