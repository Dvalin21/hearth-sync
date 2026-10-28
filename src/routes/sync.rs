use crate::db::conn;
use crate::error::{AppError, Result};
use crate::AppState;
use axum::extract::{Query, State};
use axum::Json;
use chrono::Utc;
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

#[derive(Deserialize)]
pub struct ChangesQuery {
    pub since: Option<i64>,
    pub limit: Option<i64>,
    pub device_id: Option<String>,
}

#[derive(Serialize)]
pub struct ChangeItem {
    pub id: String,
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
    pub updated_at: i64,
    pub deleted: bool,
    /// Cursor value. Server-side only; the client never sees or sends this.
    #[serde(skip)]
    pub seq: i64,
}

#[derive(Serialize)]
pub struct ChangesResp {
    pub changes: Vec<ChangeItem>,
    pub next_cursor: i64,
    pub has_more: bool,
}

pub async fn changes(State(state): State<AppState>, headers: axum::http::HeaderMap, Query(q): Query<ChangesQuery>) -> Result<Json<ChangesResp>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let since = q.since.unwrap_or(0);
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let (changes, next, has_more) = tokio::task::spawn_blocking(move || -> Result<(Vec<ChangeItem>, i64, bool)> {
        let c = conn(&pool).map_err(AppError::Db)?;
        // +1 from each table so the merged result can be truncated on a real
        // boundary instead of guessing from a per-table limit.
        let fetch = limit + 1;
        let mut all: Vec<ChangeItem> = Vec::new();

        let mut stmt = c.prepare(
            "SELECT id,content_hash,width,height,taken_at,imported_at,size_bytes,mime_type,is_favorite,is_archived,is_trashed,caption,updated_at,sync_seq
             FROM photos WHERE user_id = ?1 AND sync_seq > ?2 ORDER BY sync_seq ASC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![uid, since, fetch], |r| Ok(ChangeItem {
            id: r.get(0)?,
            content_hash: r.get(1)?,
            width: r.get(2)?,
            height: r.get(3)?,
            taken_at: r.get(4)?,
            imported_at: r.get(5)?,
            size_bytes: r.get(6)?,
            mime_type: r.get(7)?,
            is_favorite: r.get::<_, i64>(8)? != 0,
            is_archived: r.get::<_, i64>(9)? != 0,
            is_trashed: r.get::<_, i64>(10)? != 0,
            caption: r.get(11)?,
            updated_at: r.get(12)?,
            deleted: false,
            seq: r.get(13)?,
        }))?;
        for r in rows { all.push(r?); }

        let mut stmt = c.prepare(
            "SELECT entity_id, deleted_at, sync_seq FROM tombstones
             WHERE user_id = ?1 AND sync_seq > ?2 ORDER BY sync_seq ASC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![uid, since, fetch], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })?;
        for r in rows {
            let (eid, ts, seq) = r?;
            all.push(ChangeItem {
                id: eid,
                content_hash: String::new(),
                width: 0,
                height: 0,
                taken_at: None,
                imported_at: ts,
                size_bytes: 0,
                mime_type: String::new(),
                is_favorite: false,
                is_archived: false,
                is_trashed: true,
                caption: String::new(),
                updated_at: ts,
                deleted: true,
                seq,
            });
        }

        all.sort_by_key(|ch| ch.seq);
        let has_more = all.len() as i64 > limit;
        all.truncate(limit as usize);
        // The cursor is the sequence of the last item actually returned, so a
        // change can never be skipped between two pages.
        let next = all.last().map(|ch| ch.seq).unwrap_or(since);
        Ok((all, next, has_more))
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(Json(ChangesResp { changes, next_cursor: next, has_more }))
}

#[derive(Deserialize)]
pub struct PushItem {
    pub id: String,
    pub content_hash: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub taken_at: Option<i64>,
    pub size_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub is_favorite: Option<bool>,
    pub is_archived: Option<bool>,
    pub is_trashed: Option<bool>,
    pub caption: Option<String>,
    pub updated_at: Option<i64>,
    pub deleted: Option<bool>,
}

#[derive(Deserialize)]
pub struct PushReq {
    pub device_id: String,
    pub items: Vec<PushItem>,
}

#[derive(Serialize)]
pub struct PushResp {
    pub accepted: i64,
    pub conflicts: Vec<String>,
}

pub async fn push(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(req): Json<PushReq>) -> Result<Json<PushResp>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let now = Utc::now().timestamp();
    let (accepted, conflicts) = tokio::task::spawn_blocking(move || -> Result<(i64, Vec<String>)> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let mut accepted = 0i64;
        let mut conflicts = Vec::new();
        for it in &req.items {
            if it.deleted == Some(true) {
                // One tombstone per entity. `id` is the primary key and was
                // random, so repeated pushes of the same deletion piled up
                // duplicate changes.
                c.execute(
                    "DELETE FROM tombstones WHERE user_id = ?1 AND entity_id = ?2",
                    params![uid, it.id],
                )?;
                let seq = crate::db::next_seq(&c, &uid).map_err(AppError::Db)?;
                c.execute(
                    "INSERT INTO tombstones (id, user_id, entity_type, entity_id, deleted_at, sync_seq) VALUES (?1,?2,'photo',?3,?4,?5)",
                    params![Uuid::new_v4().to_string(), uid, it.id, it.updated_at.unwrap_or(now), seq],
                )?;
                // remove photo if present
                c.execute("DELETE FROM photos WHERE id = ?1 AND user_id = ?2", params![it.id, uid])?;
                c.execute("DELETE FROM album_photos WHERE photo_id = ?1", params![it.id])?;
                accepted += 1;
                continue;
            }
            // check conflict: existing photo with same id but different hash
            let existing_hash: Option<String> = c.query_row(
                "SELECT content_hash FROM photos WHERE id = ?1 AND user_id = ?2",
                params![it.id, uid],
                |r| r.get(0),
            ).ok();
            let is_conflict = existing_hash.as_ref().map(|eh| eh != &it.content_hash).unwrap_or(false);
            if is_conflict {
                conflicts.push(it.id.clone());
            }
            let id = if is_conflict {
                Uuid::new_v4().to_string()
            } else {
                it.id.clone()
            };
            let ua = it.updated_at.unwrap_or(now);
            let seq = crate::db::next_seq(&c, &uid).map_err(AppError::Db)?;
            c.execute(
                "INSERT INTO photos (id,user_id,content_hash,width,height,taken_at,imported_at,size_bytes,mime_type,is_favorite,is_archived,is_trashed,caption,camera_make,camera_model,updated_at,sync_seq)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'','',?14,?15)
                 ON CONFLICT(id) DO UPDATE SET
                   content_hash=excluded.content_hash, width=excluded.width, height=excluded.height,
                   taken_at=excluded.taken_at, size_bytes=excluded.size_bytes, mime_type=excluded.mime_type,
                   is_favorite=excluded.is_favorite, is_archived=excluded.is_archived, is_trashed=excluded.is_trashed,
                   caption=excluded.caption, updated_at=excluded.updated_at, sync_seq=excluded.sync_seq",
                params![
                    id, uid, it.content_hash,
                    it.width.unwrap_or(0), it.height.unwrap_or(0),
                    it.taken_at, now, it.size_bytes.unwrap_or(0),
                    it.mime_type.clone().unwrap_or_else(|| "image/jpeg".into()),
                    it.is_favorite.unwrap_or(false) as i64,
                    it.is_archived.unwrap_or(false) as i64,
                    it.is_trashed.unwrap_or(false) as i64,
                    it.caption.clone().unwrap_or_default(),
                    ua,
                    seq
                ],
            )?;
            // FTS upsert
            c.execute("DELETE FROM photos_fts WHERE photo_id = ?1", params![id])?;
            c.execute("INSERT INTO photos_fts (photo_id, caption, camera_make, camera_model) VALUES (?1,?2,'','')", params![id, it.caption.clone().unwrap_or_default()])?;
            accepted += 1;
        }
        Ok((accepted, conflicts))
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(PushResp { accepted, conflicts }))
}

#[derive(Deserialize)]
pub struct AckReq {
    pub device_id: String,
    pub cursor: i64,
}

pub async fn ack(State(state): State<AppState>, headers: axum::http::HeaderMap, Json(req): Json<AckReq>) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let now = Utc::now().timestamp();
        c.execute(
            "INSERT INTO sync_cursors (user_id, device_id, cursor, updated_at) VALUES (?1,?2,?3,?4)
             ON CONFLICT(user_id, device_id) DO UPDATE SET cursor=excluded.cursor, updated_at=excluded.updated_at",
            params![uid, req.device_id, req.cursor, now],
        )?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(json!({"ok": true})))
}
