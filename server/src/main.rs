use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let bind = std::env::var("SHUTTLEPUB_BIND").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let actor =
        std::env::var("SHUTTLEPUB_ACTOR").unwrap_or_else(|_| "http://127.0.0.1:3000/actor".into());
    let origins = std::env::var("SHUTTLEPUB_REMOTE_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    let key = tokio::task::spawn_blocking(driver::DevelopmentKey::generate).await??;
    let federation = Arc::new(driver::Federation::new(&actor, origins, Arc::new(key))?);
    let notes = Arc::new(application::NoteService::in_memory().await);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(address = %listener.local_addr()?, "development server listening; storage and signing key are ephemeral");
    axum::serve(listener, server::router(notes.clone(), federation))
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "shutdown signal failed");
            }
        })
        .await?;
    notes.stop().await?;
    Ok(())
}
