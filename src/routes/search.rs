use crate::db::conn;
use crate::error::{AppError, Result};
use crate::AppState;
use axum::extract::{Query, State};
use axum::Json;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};

fn auth_user(state: &AppState, headers: &axum::http::HeaderMap) -> Result<String> {
    let h = headers.get("authorization").ok_or(AppError::Unauthorized)?;
    let s = h.to_str().map_err(|_| AppError::Unauthorized)?;
    let t = s.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;
    let claims = crate::auth::validate_access_token(&state.config.jwt_secret(), t)
        .map_err(|_| AppError::Unauthorized)?;
    Ok(claims.sub)
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub id: String,
    pub content_hash: String,
    pub width: i64,
    pub height: i64,
    pub taken_at: Option<i64>,
    pub mime_type: String,
    pub is_favorite: bool,
    pub is_archived: bool,
    pub caption: String,
}

pub async fn search(State(state): State<AppState>, headers: axum::http::HeaderMap, Query(q): Query<SearchQuery>) -> Result<Json<Vec<SearchResult>>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let query = q.q.unwrap_or_default();
    let limit = q.limit.unwrap_or(50).min(200);
    let results = tokio::task::spawn_blocking(move || -> Result<Vec<SearchResult>> {
        let c = conn(&pool).map_err(AppError::Db)?;
        if query.trim().is_empty() {
            // return recent
            let mut stmt = c.prepare(
                "SELECT id,content_hash,width,height,taken_at,mime_type,is_favorite,is_archived,caption FROM photos WHERE user_id = ?1 AND is_trashed = 0 ORDER BY COALESCE(taken_at, imported_at) DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![uid, limit], row_to_result)?;
            let mut out = Vec::new();
            for r in rows { out.push(r?); }
            return Ok(out);
        }
        // FTS search
        let mut stmt = c.prepare(
            "SELECT p.id,p.content_hash,p.width,p.height,p.taken_at,p.mime_type,p.is_favorite,p.is_archived,p.caption
             FROM photos p JOIN photos_fts f ON f.photo_id = p.id
             WHERE p.user_id = ?1 AND p.is_trashed = 0 AND photos_fts MATCH ?2
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![uid, query, limit], row_to_result)?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(results))
}

fn row_to_result(r: &rusqlite::Row) -> rusqlite::Result<SearchResult> {
    Ok(SearchResult {
        id: r.get(0)?,
        content_hash: r.get(1)?,
        width: r.get(2)?,
        height: r.get(3)?,
        taken_at: r.get(4)?,
        mime_type: r.get(5)?,
        is_favorite: r.get::<_, i64>(6)? != 0,
        is_archived: r.get::<_, i64>(7)? != 0,
        caption: r.get(8)?,
    })
}
