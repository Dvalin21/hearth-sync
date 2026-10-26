pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod routes;
pub mod storage;
pub mod middleware;
pub mod ml;

use crate::config::Config;
use crate::db::DbPool;
use crate::storage::ObjectStore;
use crate::middleware::RateLimiter;
use axum::Router;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: DbPool,
    pub store: ObjectStore,
    pub rate_limiter: std::sync::Arc<RateLimiter>,
    pub auth_rate_limiter: std::sync::Arc<RateLimiter>,
    pub job_tx: tokio::sync::mpsc::Sender<crate::ml::Job>,
}

pub fn build_router(state: AppState) -> Router {
    routes::router(state)
}
