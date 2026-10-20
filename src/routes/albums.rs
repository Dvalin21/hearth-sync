use crate::db::conn;
use crate::error::{AppError, Result};
use axum::http::StatusCode;
use axum::http::HeaderMap;
use crate::AppState;
use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

fn auth_user(state: &AppState, headers: &axum::http::HeaderMap) -> Result<String> {
    let h = headers.get("authorization").ok_or(AppError::Unauthorized)?;
    let s = h.to_str().map_err(|_| AppError::Unauthorized)?;
    let t = s.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;
    let claims = crate::auth::validate_access_token(&state.config.jwt_secret(), t)
        .map_err(|_| AppError::Unauthorized)?;
    Ok(claims.sub)
}

#[derive(Serialize, Clone)]
pub struct Album {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub cover_photo_id: Option<String>,
    pub photo_count: i64,
}

#[derive(Deserialize)]
pub struct CreateAlbumReq {
    pub name: String,
}

#[derive(Deserialize)]
pub struct RenameAlbumReq {
    pub name: String,
}

pub async fn list(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Result<Json<Vec<Album>>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let albums = tokio::task::spawn_blocking(move || -> Result<Vec<Album>> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let mut stmt = c.prepare(
            "SELECT a.id, a.user_id, a.name, a.created_at, a.updated_at, a.cover_photo_id, COUNT(ap.photo_id) FROM albums a LEFT JOIN album_photos ap ON ap.album_id = a.id WHERE a.user_id = ?1 GROUP BY a.id ORDER BY a.updated_at DESC",
        )?;
        let rows = stmt.query_map(params![uid], |r| Ok(Album {
            id: r.get(0)?,
            user_id: r.get(1)?,
            name: r.get(2)?,
            created_at: r.get(3)?,
            updated_at: r.get(4)?,
            cover_photo_id: r.get(5)?,
            photo_count: r.get(6)?,
        }))?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(albums))
}

pub async fn create(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(req): Json<CreateAlbumReq>) -> Result<Json<Album>> {
    let uid = auth_user(&state, &headers)?;
    if req.name.trim().is_empty() { return Err(AppError::BadRequest("name required".into())); }
    let pool = state.db.clone();
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let name = req.name.trim().to_string();
    let album = tokio::task::spawn_blocking(move || -> Result<Album> {
        let c = conn(&pool).map_err(AppError::Db)?;
        c.execute(
            "INSERT INTO albums (id, user_id, name, created_at, updated_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, uid, name, now, now],
        )?;
        Ok(Album { id, user_id: uid, name, created_at: now, updated_at: now, cover_photo_id: None, photo_count: 0 })
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(album))
}

pub async fn get(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>) -> Result<Json<Album>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let album = tokio::task::spawn_blocking(move || -> Result<Album> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let a: Album = c.query_row(
            "SELECT a.id, a.user_id, a.name, a.created_at, a.updated_at, a.cover_photo_id, COUNT(ap.photo_id) FROM albums a LEFT JOIN album_photos ap ON ap.album_id = a.id WHERE a.id = ?1 GROUP BY a.id",
            params![id],
            |r| Ok(Album {
                id: r.get(0)?,
                user_id: r.get(1)?,
                name: r.get(2)?,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
                cover_photo_id: r.get(5)?,
                photo_count: r.get(6)?,
            }),
        ).map_err(|_| AppError::NotFound)?;
        if a.user_id != uid { return Err(AppError::Forbidden); }
        Ok(a)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(album))
}

pub async fn update(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>, Json(req): Json<RenameAlbumReq>) -> Result<StatusCode> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let now = Utc::now().timestamp();
    let name = req.name.trim().to_string();
    if name.is_empty() { return Err(AppError::BadRequest("name required".into())); }
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let owner: String = c.query_row("SELECT user_id FROM albums WHERE id = ?1", params![id], |r| r.get(0)).map_err(|_| AppError::NotFound)?;
        if owner != uid { return Err(AppError::Forbidden); }
        c.execute("UPDATE albums SET name = ?1, updated_at = ?2 WHERE id = ?3", params![name, now, id])?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>) -> Result<StatusCode> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let owner: String = c.query_row("SELECT user_id FROM albums WHERE id = ?1", params![id], |r| r.get(0)).map_err(|_| AppError::NotFound)?;
        if owner != uid { return Err(AppError::Forbidden); }
        c.execute("DELETE FROM albums WHERE id = ?1", params![id])?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AlbumPhotoReq {
    pub photo_id: String,
}

pub async fn add_photo(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>, Json(req): Json<AlbumPhotoReq>) -> Result<StatusCode> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let now = Utc::now().timestamp();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let owner: String = c.query_row("SELECT user_id FROM albums WHERE id = ?1", params![id], |r| r.get(0)).map_err(|_| AppError::NotFound)?;
        if owner != uid { return Err(AppError::Forbidden); }
        // verify photo ownership
        let powner: String = c.query_row("SELECT user_id FROM photos WHERE id = ?1", params![req.photo_id], |r| r.get(0)).map_err(|_| AppError::NotFound)?;
        if powner != uid { return Err(AppError::Forbidden); }
        c.execute("INSERT OR IGNORE INTO album_photos (album_id, photo_id, added_at) VALUES (?1,?2,?3)", params![id, req.photo_id, now])?;
        c.execute("UPDATE albums SET updated_at = ?1 WHERE id = ?2", params![now, id])?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_photo(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>, Json(req): Json<AlbumPhotoReq>) -> Result<StatusCode> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let now = Utc::now().timestamp();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let owner: String = c.query_row("SELECT user_id FROM albums WHERE id = ?1", params![id], |r| r.get(0)).map_err(|_| AppError::NotFound)?;
        if owner != uid { return Err(AppError::Forbidden); }
        c.execute("DELETE FROM album_photos WHERE album_id = ?1 AND photo_id = ?2", params![id, req.photo_id])?;
        c.execute("UPDATE albums SET updated_at = ?1 WHERE id = ?2", params![now, id])?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}
