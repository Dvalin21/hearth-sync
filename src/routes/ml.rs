use crate::error::{AppError, Result};
use crate::AppState;
use axum::extract::{State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

fn auth_user(state: &AppState, headers: &HeaderMap) -> Result<String> {
    let h = headers.get("authorization").ok_or(AppError::Unauthorized)?;
    let s = h.to_str().map_err(|_| AppError::Unauthorized)?;
    let t = s.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;
    let claims = crate::auth::validate_access_token(&state.config.jwt_secret(), t)
        .map_err(|_| AppError::Unauthorized)?;
    Ok(claims.sub)
}

#[derive(Serialize)]
pub struct MlStatus {
    pub enabled: bool,
    pub features: Vec<String>,
}

pub async fn status(State(_state): State<AppState>, headers: HeaderMap) -> Result<Json<MlStatus>> {
    let _ = auth_user(&_state, &headers)?;
    #[cfg(feature = "ml")]
    {
        Ok(Json(MlStatus { enabled: true, features: vec!["face-detection".into(), "clip-search".into(), "ocr".into()] }))
    }
    #[cfg(not(feature = "ml"))]
    {
        Ok(Json(MlStatus { enabled: false, features: vec![] }))
    }
}

#[derive(Deserialize)]
pub struct SmartSearchReq {
    pub query: String,
}

pub async fn smart_search(State(state): State<AppState>, headers: HeaderMap, Json(_req): Json<SmartSearchReq>) -> Result<Json<serde_json::Value>> {
    let _ = auth_user(&state, &headers)?;
    #[cfg(not(feature = "ml"))]
    {
        return Err(AppError::BadRequest("ML feature not enabled. Rebuild with --features ml".into()));
    }
    #[cfg(feature = "ml")]
    {
        // Placeholder: real implementation would encode text via CLIP, query photo_embeddings
        Ok(Json(json!({"results": [], "note": "ML scaffold - implement inference"})))
    }
}

pub async fn people(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<serde_json::Value>> {
    let _ = auth_user(&state, &headers)?;
    #[cfg(not(feature = "ml"))]
    {
        return Err(AppError::BadRequest("ML feature not enabled".into()));
    }
    #[cfg(feature = "ml")]
    {
        Ok(Json(json!({"people": []})))
    }
}

pub async fn reprocess(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<serde_json::Value>> {
    let _ = auth_user(&state, &headers)?;
    #[cfg(not(feature = "ml"))]
    {
        return Err(AppError::BadRequest("ML feature not enabled".into()));
    }
    #[cfg(feature = "ml")]
    {
        Ok(Json(json!({"queued": true, "note": "ML scaffold - implement job queue"})))
    }
}
