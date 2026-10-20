use crate::error::{AppError, Result};
use crate::AppState;
use axum::Router;
use axum::routing::{get, post, delete, put};
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

pub mod health;
pub mod auth;
pub mod photos;
pub mod albums;
pub mod sync;
pub mod search;
pub mod creations;

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let max_bytes = state.config.max_upload_mb * 1024 * 1024;

    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        // auth
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/refresh", post(auth::refresh))
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/me", get(auth::me))
        // photos
        .route("/api/photos", get(photos::list).post(photos::upload))
        .route("/api/photos/:id", get(photos::get).delete(photos::delete))
        .route("/api/photos/:id/content", get(photos::content))
        .route("/api/photos/:id/thumb", get(photos::thumb))
        .route("/api/photos/:id/favorite", put(photos::favorite))
        .route("/api/photos/:id/archive", put(photos::archive))
        .route("/api/photos/:id/trash", put(photos::trash))
        // albums
        .route("/api/albums", get(albums::list).post(albums::create))
        .route("/api/albums/:id", get(albums::get).put(albums::update).delete(albums::delete))
        .route("/api/albums/:id/photos", post(albums::add_photo).delete(albums::remove_photo))
        // sync
        .route("/api/sync/changes", get(sync::changes))
        .route("/api/sync/push", post(sync::push))
        .route("/api/sync/ack", post(sync::ack))
        // search
        .route("/api/search", get(search::search))
        // creations
        .route("/api/creations/collage", post(creations::collage))
        .route("/api/creations/gif", post(creations::gif))
        .route("/api/creations", get(creations::list))
        .layer(RequestBodyLimitLayer::new(max_bytes as usize))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
