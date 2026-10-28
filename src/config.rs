use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "hearth-sync", about = "HearthBoard sync server")]
pub struct Config {
    #[arg(long, env = "HEARTH_SYNC_LISTEN", default_value = "0.0.0.0:8080")]
    pub listen: String,

    #[arg(long, env = "HEARTH_SYNC_DATA_DIR", default_value = "./data")]
    pub data_dir: PathBuf,

    #[arg(long, env = "HEARTH_SYNC_DB", default_value = "hearth.db")]
    pub db_name: String,

    #[arg(long, env = "HEARTH_SYNC_JWT_SECRET")]
    pub jwt_secret: Option<String>,

    #[arg(long, env = "HEARTH_SYNC_MAX_UPLOAD_MB", default_value = "100")]
    pub max_upload_mb: u64,

    #[arg(long, env = "HEARTH_SYNC_LOG", default_value = "info")]
    pub log_level: String,

    #[arg(long, env = "HEARTH_SYNC_CORS_ORIGINS", default_value = "*")]
    pub cors_origins: String,

    #[arg(long, env = "HEARTH_SYNC_ML_SERVICE_URL")]
    pub ml_service_url: Option<String>,

    /// Comma-separated IP addresses of reverse proxies whose `X-Forwarded-For`
    /// header may be trusted. Empty means trust no proxy.
    #[arg(long, env = "HEARTH_SYNC_TRUSTED_PROXIES", default_value = "")]
    pub trusted_proxies: String,

    #[arg(long, env = "HEARTH_SYNC_TLS_CERT")]
    pub tls_cert: Option<PathBuf>,

    #[arg(long, env = "HEARTH_SYNC_TLS_KEY")]
    pub tls_key: Option<PathBuf>,
}

impl Config {
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join(&self.db_name)
    }
    pub fn objects_dir(&self) -> PathBuf {
        self.data_dir.join("objects")
    }
    pub fn thumbs_dir(&self) -> PathBuf {
        self.data_dir.join("thumbs")
    }
    pub fn jwt_secret(&self) -> String {
        self.jwt_secret
            .clone()
            .or_else(|| std::env::var("HEARTH_SYNC_JWT_SECRET").ok())
            .unwrap_or_else(|| {
                // Fail loudly rather than silently running with an insecure key.
                panic!("HEARTH_SYNC_JWT_SECRET is required; generate one with: openssl rand -hex 32")
            })
    }
}
