use crate::db::conn;
use crate::error::{AppError, Result};
use crate::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
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
pub struct CreationReq {
    pub photo_ids: Vec<String>,
}

#[derive(Serialize)]
pub struct CreationResp {
    pub id: String,
    pub kind: String,
    pub content_hash: String,
}

#[derive(Serialize)]
pub struct CreationItem {
    pub id: String,
    pub kind: String,
    pub content_hash: String,
    pub created_at: i64,
}

pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Vec<CreationItem>>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let items = tokio::task::spawn_blocking(move || -> Result<Vec<CreationItem>> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let mut stmt = c.prepare("SELECT id, kind, content_hash, created_at FROM creations WHERE user_id = ?1 ORDER BY created_at DESC")?;
        let rows = stmt.query_map(params![uid], |r| Ok(CreationItem {
            id: r.get(0)?,
            kind: r.get(1)?,
            content_hash: r.get(2)?,
            created_at: r.get(3)?,
        }))?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(items))
}

pub async fn collage(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<CreationReq>) -> Result<Json<CreationResp>> {
    if req.photo_ids.is_empty() || req.photo_ids.len() > 9 {
        return Err(AppError::BadRequest("photo_ids must have 1..=9 items".into()));
    }
    let uid = auth_user(&state, &headers)?;
    let store = state.store.clone();
    let pool = state.db.clone();
    let ids = req.photo_ids.clone();

    // Decoding, resizing and JPEG encoding are CPU-bound. Running them straight
    // in the handler blocked the async runtime for the whole collage build.
    let hash = {
        let pool = pool.clone();
        let store = store.clone();
        let uid = uid.clone();
        let ids = ids.clone();
        tokio::task::spawn_blocking(move || -> Result<String> {
            // load image bytes
            let mut images: Vec<image::DynamicImage> = Vec::new();
            for pid in &ids {
                let p = load_photo_bytes(&pool, &store, &uid, pid)?;
                let img = image::load_from_memory(&p).map_err(|e| AppError::BadRequest(e.to_string()))?;
                images.push(img);
            }

            // build grid collage
            let n = images.len();
            let cols = ((n as f64).sqrt().ceil() as u32).max(1);
            let rows = (n as u32).div_ceil(cols);
            let cell = 512u32;
            let w = cols * cell;
            let h = rows * cell;
            let mut canvas = image::RgbaImage::new(w, h);
            for (i, img) in images.iter().enumerate() {
                let r = (i as u32) / cols;
                let ccol = (i as u32) % cols;
                let resized = img.resize_exact(cell, cell, image::imageops::FilterType::Lanczos3);
                let rgba = resized.to_rgba8();
                for y in 0..cell {
                    for x in 0..cell {
                        canvas.put_pixel(ccol * cell + x, r * cell + y, *rgba.get_pixel(x, y));
                    }
                }
            }
            let dynimg = image::DynamicImage::ImageRgba8(canvas);
            let mut buf = Vec::new();
            dynimg.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg)
                .map_err(|e| AppError::Internal(e.to_string()))?;
            let stored = store.put(&buf)?;
            Ok(stored.hash.clone())
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))??
    };

    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let source = ids.join(",");
    let id2 = id.clone();
    let hash2 = hash.clone();
    let uid2 = uid.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        c.execute(
            "INSERT INTO creations (id, user_id, kind, content_hash, created_at, source_photo_ids) VALUES (?1,?2,'collage',?3,?4,?5)",
            params![id2, uid2, hash2, now, source],
        )?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(CreationResp { id, kind: "collage".into(), content_hash: hash }))
}

pub async fn gif(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<CreationReq>) -> Result<Json<CreationResp>> {
    if req.photo_ids.is_empty() || req.photo_ids.len() > 20 {
        return Err(AppError::BadRequest("photo_ids must have 1..=20 items".into()));
    }
    let uid = auth_user(&state, &headers)?;
    let store = state.store.clone();
    let pool = state.db.clone();
    let ids = req.photo_ids.clone();

    // Same reason as `collage`: decode + GIF encode is CPU work.
    let hash = {
        let pool = pool.clone();
        let store = store.clone();
        let uid = uid.clone();
        let ids = ids.clone();
        tokio::task::spawn_blocking(move || -> Result<String> {
            let mut frames: Vec<image::DynamicImage> = Vec::new();
            for pid in &ids {
                let p = load_photo_bytes(&pool, &store, &uid, pid)?;
                let img = image::load_from_memory(&p).map_err(|e| AppError::BadRequest(e.to_string()))?;
                frames.push(img.resize(480, 480, image::imageops::FilterType::Lanczos3));
            }

            let mut buf = Vec::new();
            {
                use gif::{Encoder, Frame, Repeat};
                let mut encoder = Encoder::new(&mut buf, 480, 480, &[]).map_err(|e| AppError::Internal(e.to_string()))?;
                encoder.set_repeat(Repeat::Infinite).map_err(|e| AppError::Internal(e.to_string()))?;
                for img in &frames {
                    let rgba = img.to_rgba8();
                    let mut frame = Frame::from_rgba_speed(480, 480, &mut rgba.into_raw(), 10);
                    frame.delay = 20;
                    encoder.write_frame(&frame).map_err(|e| AppError::Internal(e.to_string()))?;
                }
            }
            let stored = store.put(&buf)?;
            Ok(stored.hash.clone())
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))??
    };

    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let source = ids.join(",");
    let id2 = id.clone();
    let hash2 = hash.clone();
    let uid2 = uid.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        c.execute(
            "INSERT INTO creations (id, user_id, kind, content_hash, created_at, source_photo_ids) VALUES (?1,?2,'gif',?3,?4,?5)",
            params![id2, uid2, hash2, now, source],
        )?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(CreationResp { id, kind: "gif".into(), content_hash: hash }))
}

fn load_photo_bytes(pool: &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, store: &crate::storage::ObjectStore, uid: &str, photo_id: &str) -> Result<Vec<u8>> {
    let c = conn(pool).map_err(AppError::Db)?;
    let p: (String, String) = c.query_row(
        "SELECT content_hash, user_id FROM photos WHERE id = ?1",
        params![photo_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(|_| AppError::NotFound)?;
    if p.1 != uid { return Err(AppError::Forbidden); }
    store.get(&p.0)
}
