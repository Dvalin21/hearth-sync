use crate::db::conn;
use crate::error::{AppError, Result};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use r2d2_sqlite::SqliteConnectionManager;
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

#[derive(Deserialize)]
pub struct InviteReq {
    pub permission: Option<String>, // view | add
}

#[derive(Serialize)]
pub struct InviteResp {
    pub code: String,
    pub expires_at: Option<i64>,
}

#[derive(Serialize)]
pub struct Member {
    pub user_id: String,
    pub username: String,
    pub permission: String,
    pub added_at: i64,
}

pub async fn invite(State(state): State<AppState>, headers: HeaderMap, Path(album_id): Path<String>, Json(req): Json<InviteReq>) -> Result<Json<InviteResp>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let perm = req.permission.unwrap_or_else(|| "view".to_string());
    if perm != "view" && perm != "add" { return Err(AppError::BadRequest("permission must be view or add".into())); }
    let code = Uuid::new_v4().to_string()[..8].to_string();
    let code_resp = code.clone();
    let now = Utc::now().timestamp();
    let expires = now + 7 * 24 * 3600;
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let owner: String = c.query_row("SELECT user_id FROM albums WHERE id = ?1", params![album_id], |r| r.get(0)).map_err(|_| AppError::NotFound)?;
        if owner != uid { return Err(AppError::Forbidden); }
        c.execute(
            "INSERT INTO album_invites (code, album_id, created_by, permission, created_at, expires_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![code, album_id, uid, perm, now, expires],
        )?;
        c.execute("UPDATE albums SET is_shared = 1 WHERE id = ?1", params![album_id])?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(InviteResp { code: code_resp, expires_at: Some(expires) }))
}

#[derive(Deserialize)]
pub struct AcceptReq {
    pub code: String,
}

pub async fn accept(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<AcceptReq>) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let now = Utc::now().timestamp();
    let album_id = tokio::task::spawn_blocking(move || -> Result<String> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let (aid, perm, expires, used): (String, String, Option<i64>, Option<String>) = c.query_row(
            "SELECT album_id, permission, expires_at, used_by FROM album_invites WHERE code = ?1",
            params![req.code],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).map_err(|_| AppError::NotFound)?;
        if let Some(e) = expires { if now > e { return Err(AppError::BadRequest("invite expired".into())); } }
        if used.is_some() { return Err(AppError::BadRequest("invite already used".into())); }
        c.execute(
            "INSERT OR IGNORE INTO album_members (album_id, user_id, permission, added_at) VALUES (?1,?2,?3,?4)",
            params![aid, uid, perm, now],
        )?;
        c.execute("UPDATE album_invites SET used_by = ?1 WHERE code = ?2", params![uid, req.code])?;
        Ok(aid)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(json!({"album_id": album_id, "ok": true})))
}

pub async fn members(State(state): State<AppState>, headers: HeaderMap, Path(album_id): Path<String>) -> Result<Json<Vec<Member>>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let members = tokio::task::spawn_blocking(move || -> Result<Vec<Member>> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let owner: String = c.query_row("SELECT user_id FROM albums WHERE id = ?1", params![album_id], |r| r.get(0)).map_err(|_| AppError::NotFound)?;
        if owner != uid {
            // must be a member
            let is_member: i64 = c.query_row("SELECT COUNT(*) FROM album_members WHERE album_id = ?1 AND user_id = ?2", params![album_id, uid], |r| r.get(0)).unwrap_or(0);
            if is_member == 0 { return Err(AppError::Forbidden); }
        }
        let mut stmt = c.prepare(
            "SELECT am.user_id, u.username, am.permission, am.added_at FROM album_members am JOIN users u ON u.id = am.user_id WHERE am.album_id = ?1",
        )?;
        let rows = stmt.query_map(params![album_id], |r| Ok(Member {
            user_id: r.get(0)?,
            username: r.get(1)?,
            permission: r.get(2)?,
            added_at: r.get(3)?,
        }))?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(members))
}

pub async fn remove_member(State(state): State<AppState>, headers: HeaderMap, Path((album_id, user_id)): Path<(String, String)>) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let owner: String = c.query_row("SELECT user_id FROM albums WHERE id = ?1", params![album_id], |r| r.get(0)).map_err(|_| AppError::NotFound)?;
        if owner != uid { return Err(AppError::Forbidden); }
        c.execute("DELETE FROM album_members WHERE album_id = ?1 AND user_id = ?2", params![album_id, user_id])?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(json!({"ok": true})))
}

#[derive(Serialize)]
pub struct SharedAlbum {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    pub permission: String,
    pub photo_count: i64,
}

pub async fn shared_with_me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<SharedAlbum>>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let albums = tokio::task::spawn_blocking(move || -> Result<Vec<SharedAlbum>> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let mut stmt = c.prepare(
            "SELECT a.id, a.name, a.user_id, am.permission, COUNT(ap.photo_id)
             FROM albums a JOIN album_members am ON am.album_id = a.id
             LEFT JOIN album_photos ap ON ap.album_id = a.id
             WHERE am.user_id = ?1 GROUP BY a.id",
        )?;
        let rows = stmt.query_map(params![uid], |r| Ok(SharedAlbum {
            id: r.get(0)?,
            name: r.get(1)?,
            owner_id: r.get(2)?,
            permission: r.get(3)?,
            photo_count: r.get(4)?,
        }))?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(albums))
}
