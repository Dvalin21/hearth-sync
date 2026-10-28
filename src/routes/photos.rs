use crate::db::conn;
use crate::error::{AppError, Result};
use crate::AppState;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Clone)]
pub struct Photo {
    pub id: String,
    pub user_id: String,
    pub content_hash: String,
    pub width: i64,
    pub height: i64,
    pub taken_at: Option<i64>,
    pub imported_at: i64,
    pub size_bytes: i64,
    pub mime_type: String,
    pub is_favorite: bool,
    pub is_archived: bool,
    pub is_trashed: bool,
    pub caption: String,
    pub camera_make: String,
    pub camera_model: String,
    pub updated_at: i64,
}

fn auth_user(state: &AppState, headers: &axum::http::HeaderMap) -> Result<String> {
    let h = headers.get("authorization").ok_or(AppError::Unauthorized)?;
    let s = h.to_str().map_err(|_| AppError::Unauthorized)?;
    let t = s.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;
    let claims = crate::auth::validate_access_token(&state.config.jwt_secret(), t)
        .map_err(|_| AppError::Unauthorized)?;
    Ok(claims.sub)
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub archived: Option<bool>,
    pub trashed: Option<bool>,
    pub favorite: Option<bool>,
}

pub async fn list(State(state): State<AppState>, headers: axum::http::HeaderMap, Query(q): Query<ListQuery>) -> Result<Json<Vec<Photo>>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let limit = q.limit.unwrap_or(100).min(500);
    let offset = q.offset.unwrap_or(0);
    let photos = tokio::task::spawn_blocking(move || -> Result<Vec<Photo>> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let mut sql = String::from("SELECT id,user_id,content_hash,width,height,taken_at,imported_at,size_bytes,mime_type,is_favorite,is_archived,is_trashed,caption,camera_make,camera_model,updated_at FROM photos WHERE user_id = ?1");
        if q.archived == Some(true) { sql.push_str(" AND is_archived = 1"); }
        else if q.trashed != Some(true) { sql.push_str(" AND is_archived = 0 AND is_trashed = 0"); }
        if q.trashed == Some(true) { sql.push_str(" AND is_trashed = 1"); }
        if q.favorite == Some(true) { sql.push_str(" AND is_favorite = 1"); }
        sql.push_str(" ORDER BY COALESCE(taken_at, imported_at) DESC LIMIT ?2 OFFSET ?3");
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt.query_map(params![uid, limit, offset], |r| {
            Ok(Photo {
                id: r.get(0)?,
                user_id: r.get(1)?,
                content_hash: r.get(2)?,
                width: r.get(3)?,
                height: r.get(4)?,
                taken_at: r.get(5)?,
                imported_at: r.get(6)?,
                size_bytes: r.get(7)?,
                mime_type: r.get(8)?,
                is_favorite: r.get::<_, i64>(9)? != 0,
                is_archived: r.get::<_, i64>(10)? != 0,
                is_trashed: r.get::<_, i64>(11)? != 0,
                caption: r.get(12)?,
                camera_make: r.get(13)?,
                camera_model: r.get(14)?,
                updated_at: r.get(15)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(photos))
}

pub async fn upload(State(state): State<AppState>, headers: axum::http::HeaderMap, mut multipart: Multipart) -> Result<Json<Photo>> {
    let uid = auth_user(&state, &headers)?;
    // read file field + optional metadata fields (PhotoSyncWorker sends them)
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut mime = String::from("image/jpeg");
    let mut taken_at_opt: Option<i64> = None;
    let mut lat_opt: Option<f64> = None;
    let mut lon_opt: Option<f64> = None;
    let mut camera_make = String::new();
    let mut camera_model = String::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::BadRequest(e.to_string()))? {
        match field.name().unwrap_or("") {
            "file" => {
                mime = field.content_type().unwrap_or("image/jpeg").to_string();
                file_bytes = Some(field.bytes().await.map_err(|e| AppError::BadRequest(e.to_string()))?.to_vec());
            }
            "taken_at" => taken_at_opt = field.text().await.ok().and_then(|s| s.parse().ok()),
            "location_lat" => lat_opt = field.text().await.ok().and_then(|s| s.parse().ok()),
            "location_lon" => lon_opt = field.text().await.ok().and_then(|s| s.parse().ok()),
            "camera_make" => camera_make = field.text().await.unwrap_or_default(),
            "camera_model" => camera_model = field.text().await.unwrap_or_default(),
            _ => {} // unknown fields ignored — clients may send extras
        }
    }
    let data = file_bytes.ok_or_else(|| AppError::BadRequest("file field required".into()))?;
    if data.len() as u64 > state.config.max_upload_mb * 1024 * 1024 {
        return Err(AppError::PayloadTooLarge);
    }
    let store = state.store.clone();
    let stored = store.put(&data)?;
    let hash = stored.hash.clone();

    // decode dimensions
    let (w, h) = tokio::task::spawn_blocking({
        let data = data.clone();
        move || {
            image::load_from_memory(&data).map(|i| (i.width() as i64, i.height() as i64)).unwrap_or((0, 0))
        }
    }).await.unwrap_or((0, 0));

    // EXIF taken_at (best effort) — multipart value wins
    let taken_at = taken_at_opt.or_else(|| extract_taken_at(&data));

    let pool = state.db.clone();
    let uid2 = uid.clone();
    let mime2 = mime.clone();
    let data_for_db = data.clone();
    let hash_for_db = hash.clone();
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let metrics = state.metrics.clone();
    let photo = tokio::task::spawn_blocking(move || -> Result<Photo> {
        let c = conn(&pool).map_err(AppError::Db)?;
        // dedup: if same hash already exists for this user, return existing
        let existing: Option<String> = c.query_row(
            "SELECT id FROM photos WHERE user_id = ?1 AND content_hash = ?2",
            params![uid2, hash_for_db],
            |r| r.get(0),
        ).ok();
        if let Some(eid) = existing {
            // backfill metadata the client sent, then return existing row
            let seq = crate::db::next_seq(&c, &uid2).map_err(AppError::Db)?;
            c.execute(
                "UPDATE photos SET taken_at=COALESCE(?1,taken_at), location_lat=COALESCE(?2,location_lat), location_lon=COALESCE(?3,location_lon), camera_make=CASE WHEN ?4='' THEN camera_make ELSE ?4 END, camera_model=CASE WHEN ?5='' THEN camera_model ELSE ?5 END, sync_seq=?7 WHERE id=?6",
                params![taken_at, lat_opt, lon_opt, camera_make, camera_model, eid, seq],
            )?;
            let p = load_photo(&c, &eid)?;
            return Ok(p);
        }
        let seq = crate::db::next_seq(&c, &uid2).map_err(AppError::Db)?;
        c.execute(
            "INSERT INTO photos (id,user_id,content_hash,width,height,taken_at,imported_at,size_bytes,mime_type,is_favorite,is_archived,is_trashed,caption,location_lat,location_lon,camera_make,camera_model,updated_at,sync_seq) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0,0,0,'',?10,?11,?12,?13,?14,?15)",
            params![id, uid2, hash_for_db, w, h, taken_at, now, data_for_db.len() as i64, mime2, lat_opt, lon_opt, camera_make, camera_model, now, seq],
        )?;
        // FTS
        c.execute("INSERT INTO photos_fts (photo_id, caption, camera_make, camera_model) VALUES (?1,'','','')", params![id])?;
        // Count real new photos only; a dedup hit returns the existing row and
        // must not inflate the upload counter.
        metrics.photos_uploaded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        load_photo(&c, &id)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;

    // thumbnail gen (best effort, sync for simplicity)
    let store2 = state.store.clone();
    let data2 = data.clone();
    let hash2 = hash.clone();
    tokio::task::spawn_blocking(move || {
        let img = match image::load_from_memory(&data2) {
            Ok(i) => i,
            Err(_) => return,
        };
        let thumb = img.thumbnail(512, 512);
        let path = store2.thumb_path(&hash2);
        if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
        let _ = thumb.save(&path);
    }).await.ok();

    Ok(Json(photo))
}

pub fn load_photo(c: &rusqlite::Connection, id: &str) -> Result<Photo> {
    c.query_row(
        "SELECT id,user_id,content_hash,width,height,taken_at,imported_at,size_bytes,mime_type,is_favorite,is_archived,is_trashed,caption,camera_make,camera_model,updated_at FROM photos WHERE id = ?1",
        params![id],
        |r| Ok(Photo {
            id: r.get(0)?,
            user_id: r.get(1)?,
            content_hash: r.get(2)?,
            width: r.get(3)?,
            height: r.get(4)?,
            taken_at: r.get(5)?,
            imported_at: r.get(6)?,
            size_bytes: r.get(7)?,
            mime_type: r.get(8)?,
            is_favorite: r.get::<_, i64>(9)? != 0,
            is_archived: r.get::<_, i64>(10)? != 0,
            is_trashed: r.get::<_, i64>(11)? != 0,
            caption: r.get(12)?,
            camera_make: r.get(13)?,
            camera_model: r.get(14)?,
            updated_at: r.get(15)?,
        }),
    ).map_err(|_| AppError::NotFound)
}

pub async fn get(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>) -> Result<Json<Photo>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let photo = tokio::task::spawn_blocking(move || -> Result<Photo> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let p = load_photo(&c, &id)?;
        if p.user_id != uid { return Err(AppError::Forbidden); }
        Ok(p)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(photo))
}

pub async fn delete(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>) -> Result<StatusCode> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let store = state.store.clone();
    let now = Utc::now().timestamp();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let p = load_photo(&c, &id)?;
        if p.user_id != uid { return Err(AppError::Forbidden); }
        // refcount
        let rc: i64 = c.query_row("SELECT COUNT(*) FROM photos WHERE content_hash = ?1", params![p.content_hash], |r| r.get(0))?;
        let seq = crate::db::next_seq(&c, &uid).map_err(AppError::Db)?;
        c.execute("DELETE FROM photos WHERE id = ?1", params![id])?;
        c.execute("DELETE FROM album_photos WHERE photo_id = ?1", params![id])?;
        c.execute("DELETE FROM photos_fts WHERE photo_id = ?1", params![id])?;
        // A hard delete is a change too: without a tombstone, other devices keep
        // showing the photo forever because /sync/changes has nothing to report.
        c.execute("DELETE FROM tombstones WHERE user_id = ?1 AND entity_id = ?2", params![uid, id])?;
        c.execute(
            "INSERT INTO tombstones (id, user_id, entity_type, entity_id, deleted_at, sync_seq) VALUES (?1,?2,'photo',?3,?4,?5)",
            params![uuid::Uuid::new_v4().to_string(), uid, id, now, seq],
        )?;
        store.delete_if_unreferenced(&p.content_hash, rc - 1)?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn content(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>) -> Result<axum::response::Response> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let store = state.store.clone();
    let (hash, mime) = tokio::task::spawn_blocking(move || -> Result<(String, String)> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let p = load_photo(&c, &id)?;
        if p.user_id != uid { return Err(AppError::Forbidden); }
        Ok((p.content_hash, p.mime_type))
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    let data = store.get(&hash)?;
    // Basic Range support: bytes=start-end
    if let Some(range) = headers.get(axum::http::header::RANGE) {
        if let Ok(r) = range.to_str() {
            if let Some(spec) = r.strip_prefix("bytes=") {
                let parts: Vec<&str> = spec.split('-').collect();
                if parts.len() == 2 {
                    let start: usize = parts[0].parse().unwrap_or(0);
                    let end: usize = if parts[1].is_empty() { data.len().saturating_sub(1) } else { parts[1].parse().unwrap_or(data.len().saturating_sub(1)) };
                    let end = end.min(data.len().saturating_sub(1));
                    if start <= end && start < data.len() {
                        let slice = data[start..=end].to_vec();
                        let cr = format!("bytes {}-{}/{}", start, end, data.len());
                        let resp = axum::response::Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header(header::CONTENT_TYPE, mime.clone())
                            .header(header::CONTENT_RANGE, cr)
                            .body(axum::body::Body::from(slice))
                            .unwrap();
                        return Ok(resp);
                    }
                }
            }
        }
    }
    let resp = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .body(axum::body::Body::from(data))
        .unwrap();
    Ok(resp)
}

pub async fn thumb(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>) -> Result<impl IntoResponse> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let store = state.store.clone();
    let hash = tokio::task::spawn_blocking(move || -> Result<String> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let p = load_photo(&c, &id)?;
        if p.user_id != uid { return Err(AppError::Forbidden); }
        Ok(p.content_hash)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    let tp = store.thumb_path(&hash);
    if !tp.exists() {
        return Err(AppError::NotFound);
    }
    let data = std::fs::read(tp)?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg")], data))
}

#[derive(Deserialize)]
pub struct FlagReq {
    pub value: bool,
}

pub async fn favorite(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>, Json(req): Json<FlagReq>) -> Result<StatusCode> {
    set_flag(&state, &headers, &id, "is_favorite", req.value).await
}
pub async fn archive(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>, Json(req): Json<FlagReq>) -> Result<StatusCode> {
    set_flag(&state, &headers, &id, "is_archived", req.value).await
}
pub async fn trash(State(state): State<AppState>, headers: axum::http::HeaderMap, Path(id): Path<String>, Json(req): Json<FlagReq>) -> Result<StatusCode> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let now = Utc::now().timestamp();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let p = load_photo(&c, &id)?;
        if p.user_id != uid { return Err(AppError::Forbidden); }
        if req.value {
            let seq = crate::db::next_seq(&c, &uid).map_err(AppError::Db)?;
            c.execute("UPDATE photos SET is_trashed = 1, trashed_at = ?1, updated_at = ?1, sync_seq = ?3 WHERE id = ?2", params![now, id, seq])?;
        } else {
            let seq = crate::db::next_seq(&c, &uid).map_err(AppError::Db)?;
            c.execute("UPDATE photos SET is_trashed = 0, trashed_at = NULL, updated_at = ?1, sync_seq = ?3 WHERE id = ?2", params![now, id, seq])?;
        }
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_flag(state: &AppState, headers: &axum::http::HeaderMap, id: &str, col: &str, val: bool) -> Result<StatusCode> {
    let uid = auth_user(state, headers)?;
    let pool = state.db.clone();
    let now = Utc::now().timestamp();
    let col = col.to_string();
    let id = id.to_string();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let p = load_photo(&c, &id)?;
        if p.user_id != uid { return Err(AppError::Forbidden); }
        let v = if val { 1 } else { 0 };
        let seq = crate::db::next_seq(&c, &uid).map_err(AppError::Db)?;
        let sql = format!("UPDATE photos SET {} = ?1, updated_at = ?2, sync_seq = ?4 WHERE id = ?3", col);
        c.execute(&sql, params![v, now, id, seq])?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

fn extract_taken_at(_data: &[u8]) -> Option<i64> {
    None
}
