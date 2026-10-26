use crate::error::{AppError, Result};
use crate::AppState;
use crate::middleware::{rate_limit_middleware, request_id_middleware};
use axum::extract::DefaultBodyLimit;
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
    let cors = if state.config.cors_origins.trim() == "*" {
        CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any)
    } else {
        let origins: Vec<axum::http::HeaderValue> = state
            .config
            .cors_origins
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        CorsLayer::new().allow_origin(origins).allow_methods(Any).allow_headers(Any)
    };

    let max_bytes = state.config.max_upload_mb * 1024 * 1024;

    // API routes (stateless router; state provided by outer .with_state).
    let api = Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/invites", post(auth::create_invite))
        .route("/photos", get(photos::list).post(photos::upload))
        .route("/photos/:id", get(photos::get).delete(photos::delete))
        .route("/photos/:id/content", get(photos::content))
        .route("/photos/:id/thumb", get(photos::thumb))
        .route("/photos/:id/favorite", put(photos::favorite))
        .route("/photos/:id/archive", put(photos::archive))
        .route("/photos/:id/trash", put(photos::trash))
        .route("/albums", get(albums::list).post(albums::create))
        .route("/albums/:id", get(albums::get).put(albums::update).delete(albums::delete))
        .route("/albums/:id/photos", post(albums::add_photo).delete(albums::remove_photo))
        .route("/sync/changes", get(sync::changes))
        .route("/sync/push", post(sync::push))
        .route("/sync/ack", post(sync::ack))
        .route("/search", get(search::search))
        .route("/creations/collage", post(creations::collage))
        .route("/creations/gif", post(creations::gif))
        .route("/creations", get(creations::list))
        .route("/albums/:id/invite", post(sharing::invite))
        .route("/albums/invites/accept", post(sharing::accept))
        .route("/albums/:id/members", get(sharing::members))
        .route("/albums/:id/members/:user_id", delete(sharing::remove_member))
        .route("/albums/shared", get(sharing::shared_with_me))
        .route("/memories", get(memories::list))
        .route("/memories/:id/hide", put(memories::hide))
        .route("/ml/status", get(ml::status))
        .route("/ml/models", get(ml::models))
        .route("/ml/smart-search", post(ml::smart_search))
        .route("/ml/people", get(ml::people))
        .route("/ml/reprocess", post(ml::reprocess))
        .route("/ml/detect-faces", post(ml::detect_faces))
        .route("/ml/index-photo", post(ml::index_photo))
        .layer(RequestBodyLimitLayer::new(max_bytes as usize))
        .layer(DefaultBodyLimit::max(max_bytes as usize))
        .layer(TraceLayer::new_for_http())
        .layer(cors.clone());

    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .nest("/api", api.clone())
        .nest("/api/v1", api)
        .layer(axum::middleware::from_fn_with_state(state.clone(), rate_limit_middleware))
        .layer(axum::middleware::from_fn(security_headers))
        .layer(axum::middleware::from_fn(request_id_middleware))
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
