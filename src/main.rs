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
    // First pass: resolve only `--config`, because the file's values are
    // exported as environment variables and clap reads those while binding
    // Config. Reading the file *after* `Cli::parse()` was a no-op: every
    // option already had its final value by then.
    let first = Cli::parse();
    if let Some(path) = &first.config {
        load_toml_env(path)?;
    }
    let cli = Cli::parse_from(std::env::args());
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

    // Optional direct TLS (feature "tls").
    #[cfg(feature = "tls")]
    if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key) {
        use axum_server::tls_rustls::RustlsConfig;
        let tls = RustlsConfig::from_pem_file(cert, key).await.map_err(|e| anyhow::anyhow!("{}", e))?;
        axum_server::bind_rustls(addr, tls)
            .serve(app.into_make_service())
            .await?;
        return Ok(());
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    // Connect info is what the rate limiter keys on. Without it every request
    // would land in a single bucket.
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Export a flat TOML file as `HEARTH_SYNC_*` environment variables.
///
/// Only flat tables of scalars map to env vars; nested tables have no sensible
/// single-variable representation, so they are rejected instead of silently
/// ignored.
fn load_toml_env(path: &std::path::Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let table: toml::Table = text.parse()?;
    for (k, v) in table {
        let env_key = format!("HEARTH_SYNC_{}", k.to_uppercase());
        let value = match v {
            toml::Value::String(s) => s,
            toml::Value::Integer(i) => i.to_string(),
            toml::Value::Float(f) => f.to_string(),
            toml::Value::Boolean(b) => b.to_string(),
            other => anyhow::bail!("config key '{}': unsupported value {}", k, other),
        };
        std::env::set_var(env_key, value);
    }
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
