use clap::{Parser, Subcommand};
use hearth_sync::{build_router, config::Config, db, middleware::RateLimiter, storage::ObjectStore, AppState};
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Optional TOML config file (values become env vars).
    #[arg(long)]
    config: Option<std::path::PathBuf>,
    #[command(flatten)]
    config_opts: Config,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a portable backup archive of the data directory.
    Backup { #[arg(long)] output: std::path::PathBuf },
    /// Restore a backup archive into the data directory.
    Restore { #[arg(long)] input: std::path::PathBuf },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // Load optional TOML config into env vars before parsing Config.
    if let Some(path) = &cli.config {
        let text = std::fs::read_to_string(path)?;
        let table: toml::Table = text.parse()?;
        for (k, v) in table {
            let env_key = format!("HEARTH_SYNC_{}", k.to_uppercase());
            std::env::set_var(env_key, v.to_string().trim_matches('"'));
        }
    }
    let config = cli.config_opts;

    // Backup/restore are synchronous CLI ops; handle before server startup.
    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Backup { output } => {
                std::fs::create_dir_all(&config.data_dir)?;
                let status = std::process::Command::new("tar")
                    .arg("-czf").arg(&output)
                    .arg("-C").arg(&config.data_dir).arg(".")
                    .status()?;
                if !status.success() { anyhow::bail!("backup failed"); }
                println!("backup written to {}", output.display());
                return Ok(());
            }
            Commands::Restore { input } => {
                std::fs::create_dir_all(&config.data_dir)?;
                let status = std::process::Command::new("tar")
                    .arg("-xzf").arg(&input)
                    .arg("-C").arg(&config.data_dir)
                    .status()?;
                if !status.success() { anyhow::bail!("restore failed"); }
                println!("restored from {}", input.display());
                return Ok(());
            }
        }
    }

    let secret_ok = config
        .jwt_secret
        .as_ref()
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

    let (job_queue, job_rx) = hearth_sync::ml::JobQueue::new();
    let job_tx = job_queue.sender.clone();

    // Spawn background ML worker.
    let worker_model_dir = config.data_dir.join("models");
    let worker_db = db::init_pool(&config.db_path())?;
    tokio::spawn(hearth_sync::ml::run_worker(job_rx, worker_model_dir, worker_db));

    let state = AppState {
        config: config.clone(),
        db,
        store,
        rate_limiter: std::sync::Arc::new(RateLimiter::new(100, 60)),
        auth_rate_limiter: std::sync::Arc::new(RateLimiter::new(10, 60)),
        job_tx,
        metrics: std::sync::Arc::new(hearth_sync::metrics::Metrics::default()),
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
