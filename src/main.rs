use clap::Parser;
use hearth_sync::{build_router, config::Config, db, storage::ObjectStore, AppState};
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::parse();

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
    };

    let app = build_router(state);

    let addr: SocketAddr = config.listen.parse()?;
    tracing::info!("hearth-sync listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
