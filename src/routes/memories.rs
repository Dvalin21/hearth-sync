use crate::db::conn;
use crate::error::{AppError, Result};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{Datelike, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

fn auth_user(state: &AppState, headers: &HeaderMap) -> Result<String> {
    let h = headers.get("authorization").ok_or(AppError::Unauthorized)?;
    let s = h.to_str().map_err(|_| AppError::Unauthorized)?;
    let t = s.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;
    let claims = crate::auth::validate_access_token(&state.config.jwt_secret(), t)
        .map_err(|_| AppError::Unauthorized)?;
    Ok(claims.sub)
}

#[derive(Serialize)]
pub struct Memory {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub photo_ids: Vec<String>,
    pub created_at: i64,
    pub hidden: bool,
}

#[derive(Deserialize)]
pub struct HideReq {
    pub hidden: bool,
}

pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<Memory>>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let items = tokio::task::spawn_blocking(move || -> Result<Vec<Memory>> {
        let c = conn(&pool).map_err(AppError::Db)?;
        // Generate "on this day" memories if none exist recently
        generate_on_this_day(&c, &uid)?;
        let mut stmt = c.prepare("SELECT id, kind, title, photo_ids, created_at, hidden FROM memories WHERE user_id = ?1 AND hidden = 0 ORDER BY created_at DESC")?;
        let rows = stmt.query_map(params![uid], |r| {
            let ids: String = r.get(3)?;
            Ok(Memory {
                id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get(2)?,
                photo_ids: ids.split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
                created_at: r.get(4)?,
                hidden: r.get::<_, i64>(5)? != 0,
            })
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(items))
}

fn generate_on_this_day(c: &rusqlite::Connection, uid: &str) -> Result<()> {
    use rusqlite::params;
    let now = Utc::now();
    let today_month = now.month();
    let today_day = now.day();
    // find photos taken on this day in previous years
    let mut stmt = c.prepare(
        "SELECT id, taken_at FROM photos WHERE user_id = ?1 AND is_trashed = 0 AND taken_at IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![uid], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut matching: Vec<String> = Vec::new();
    for r in rows {
        let (id, ts) = r?;
        let dt = chrono::DateTime::from_timestamp(ts, 0);
        if let Some(d) = dt {
            if d.month() == today_month && d.day() == today_day && d.year() < now.year() {
                matching.push(id);
            }
        }
    }
    if matching.is_empty() { return Ok(()); }
    // check if already generated today
    let today_start = now.date_naive().and_hms_opt(0,0,0).unwrap().and_utc().timestamp();
    let exists: i64 = c.query_row(
        "SELECT COUNT(*) FROM memories WHERE user_id = ?1 AND kind = 'on_this_day' AND created_at >= ?2",
        params![uid, today_start],
        |r| r.get(0),
    ).unwrap_or(0);
    if exists > 0 { return Ok(()); }
    let id = Uuid::new_v4().to_string();
    let ids = matching.join(",");
    let title = "On this day".to_string();
    c.execute(
        "INSERT INTO memories (id, user_id, kind, title, photo_ids, created_at, hidden) VALUES (?1,?2,'on_this_day',?3,?4,?5,0)",
        params![id, uid, title, ids, now.timestamp()],
    )?;
    Ok(())
}

pub async fn hide(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>, Json(req): Json<HideReq>) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let owner: String = c.query_row("SELECT user_id FROM memories WHERE id = ?1", params![id], |r| r.get(0)).map_err(|_| AppError::NotFound)?;
        if owner != uid { return Err(AppError::Forbidden); }
        let v = if req.hidden { 1 } else { 0 };
        c.execute("UPDATE memories SET hidden = ?1 WHERE id = ?2", params![v, id])?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(json!({"ok": true})))
}
