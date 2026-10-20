pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod routes;
pub mod storage;

use crate::config::Config;
use crate::db::DbPool;
use crate::storage::ObjectStore;
use axum::Router;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: DbPool,
    pub store: ObjectStore,
}

pub fn build_router(state: AppState) -> Router {
    routes::router(state)
}
