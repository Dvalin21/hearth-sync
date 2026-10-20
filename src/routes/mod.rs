use crate::error::{AppError, Result};
use crate::AppState;
use crate::middleware::rate_limit_middleware;
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
pub mod sharing;
pub mod memories;
pub mod ml;

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
        .route("/api/auth/invites", post(auth::create_invite))
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
        // sharing
        .route("/api/albums/:id/invite", post(sharing::invite))
        .route("/api/albums/invites/accept", post(sharing::accept))
        .route("/api/albums/:id/members", get(sharing::members))
        .route("/api/albums/:id/members/:user_id", delete(sharing::remove_member))
        .route("/api/albums/shared", get(sharing::shared_with_me))
        // memories
        .route("/api/memories", get(memories::list))
        .route("/api/memories/:id/hide", put(memories::hide))
        // ml
        .route("/api/ml/status", get(ml::status))
        .route("/api/ml/models", get(ml::models))
        .route("/api/ml/smart-search", post(ml::smart_search))
        .route("/api/ml/people", get(ml::people))
        .route("/api/ml/reprocess", post(ml::reprocess))
        .route("/api/ml/detect-faces", post(ml::detect_faces))
        .route("/api/ml/index-photo", post(ml::index_photo))
        .layer(RequestBodyLimitLayer::new(max_bytes as usize))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .layer(axum::middleware::from_fn(rate_limit_middleware))
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

async fn security_headers(req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next) -> axum::response::Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    h.insert("X-Frame-Options", "DENY".parse().unwrap());
    h.insert("Referrer-Policy", "no-referrer".parse().unwrap());
    h.insert("X-XSS-Protection", "1; mode=block".parse().unwrap());
    res
}
