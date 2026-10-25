use clap::Parser;
use hearth_sync::{build_router, config::Config, db, middleware::RateLimiter, storage::ObjectStore, AppState};
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::parse();

    let secret_ok = config
        .jwt_secret
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("HEARTH_SYNC_JWT_SECRET").map(|s| !s.trim().is_empty()).unwrap_or(false);
    if !secret_ok {
        eprintln!("ERROR: HEARTH_SYNC_JWT_SECRET is required and non-empty. Generate one with: openssl rand -hex 32");
        std::process::exit(1);
    }

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(&config.log_level))
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    std::fs::create_dir_all(&config.data_dir)?;
    let db = db::init_pool(&config.db_path())?;
    let store = ObjectStore::new(config.objects_dir(), config.thumbs_dir())?;

    let state = AppState {
        config: config.clone(),
        db,
        store,
        rate_limiter: std::sync::Arc::new(RateLimiter::new(100, 60)),
    };

    let app = build_router(state);

    let addr: SocketAddr = config.listen.parse()?;
    tracing::info!("hearth-sync listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async { let _ = tokio::signal::ctrl_c().await; };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutdown signal received");
}
