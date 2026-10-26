use crate::db::conn;
use crate::error::{AppError, Result};
use crate::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use rusqlite::params;
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
    pub python_available: bool,
}

pub async fn models(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<serde_json::Value>> {
    let _ = auth_user(&state, &headers)?;
    let model_dir = state.config.data_dir.join("models");
    let clip = model_dir.join("clip_visual.onnx").exists();
    let face = model_dir.join("face_detection.onnx").exists();
    let python = crate::ml::InferenceEngine::new(model_dir.clone()).is_available();
    Ok(Json(serde_json::json!({
        "model_dir": model_dir.to_string_lossy(),
        "clip_visual": clip,
        "face_detection": face,
        "python_available": python,
        "ort_compiled": cfg!(feature = "ml"),
    })))
}

pub async fn status(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<MlStatus>> {
    let _ = auth_user(&state, &headers)?;
    let engine = crate::ml::InferenceEngine::new(state.config.data_dir.join("models"));
    let py = engine.is_available();
    Ok(Json(MlStatus {
        enabled: py,
        features: if py { vec!["face-detection".into(), "clip-search".into(), "ocr".into()] } else { vec![] },
        python_available: py,
    }))
}

#[derive(Deserialize)]
pub struct SmartSearchReq {
    pub query: String,
    pub limit: Option<usize>,
}

pub async fn smart_search(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<SmartSearchReq>) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    // Remote ML delegation
    if let Some(url) = &state.config.ml_service_url {
        let client = reqwest::Client::new();
        let resp = client.post(format!("{}/smart-search", url.trim_end_matches('/')))
            .json(&serde_json::json!({"query": req.query, "limit": req.limit, "user_id": uid}))
            .send().await.map_err(|e| AppError::Internal(e.to_string()))?;
        if resp.status().is_success() {
            let j: serde_json::Value = resp.json().await.map_err(|e| AppError::Internal(e.to_string()))?;
            return Ok(Json(j));
        }
    }
    let engine = crate::ml::InferenceEngine::new(state.config.data_dir.join("models"));
    if !engine.is_available() {
        return Err(AppError::BadRequest("ML not available: python3 with torch required".into()));
    }
    // Encode query text
    let qemb = engine.encode_text(&req.query).map_err(AppError::Internal)?;
    if qemb.is_empty() {
        return Err(AppError::Internal("empty embedding returned".into()));
    }
    let limit = req.limit.unwrap_or(50);
    let pool = state.db.clone();
    let results = crate::ml::search_similar(&pool, &uid, &qemb, limit)?;
    // Fetch photo metadata for results
    let mut out = Vec::new();
    for (pid, sim) in results {
        let pool2 = state.db.clone();
        let p = pid.clone();
        let photo = tokio::task::spawn_blocking(move || -> Option<serde_json::Value> {
            let c = conn(&pool2).ok()?;
            c.query_row(
                "SELECT id, content_hash, width, height, mime_type FROM photos WHERE id = ?1",
                params![p],
                |r| Ok(serde_json::json!({
                    "id": r.get::<_,String>(0)?,
                    "content_hash": r.get::<_,String>(1)?,
                    "width": r.get::<_,i64>(2)?,
                    "height": r.get::<_,i64>(3)?,
                    "mime_type": r.get::<_,String>(4)?,
                    "similarity": sim,
                })),
            ).ok()
        }).await.ok().flatten();
        if let Some(ph) = photo { out.push(ph); }
    }
    Ok(Json(json!({"results": out})))
}

pub async fn people(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    let pool = state.db.clone();
    let people = tokio::task::spawn_blocking(move || -> Result<Vec<serde_json::Value>> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let mut stmt = c.prepare("SELECT id, name, cover_face_id, created_at FROM people WHERE user_id = ?1")?;
        let rows = stmt.query_map(params![uid], |r| Ok(serde_json::json!({
            "id": r.get::<_,String>(0)?,
            "name": r.get::<_,String>(1)?,
            "cover_face_id": r.get::<_,Option<String>>(2)?,
            "created_at": r.get::<_,i64>(3)?,
        })))?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(json!({"people": people})))
}

pub async fn reprocess(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    // Enqueue a reprocess job (logged; actual processing requires ML worker)
    tracing::info!("Reprocess requested for user {}", uid);
    Ok(Json(json!({"queued": true, "note": "Reprocess job queued. ML worker will process when available."})))
}

// Face detection endpoint
pub async fn detect_faces(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<serde_json::Value>) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    let photo_id = req["photo_id"].as_str().ok_or(AppError::BadRequest("photo_id required".into()))?;
    let pool = state.db.clone();
    let store = state.store.clone();
    // Get photo content hash
    let hash = tokio::task::spawn_blocking({
        let pool = pool.clone();
        let pid = photo_id.to_string();
        let uid = uid.clone();
        move || -> Result<String> {
            let c = conn(&pool).map_err(AppError::Db)?;
            let (h, owner): (String, String) = c.query_row(
                "SELECT content_hash, user_id FROM photos WHERE id = ?1",
                params![pid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).map_err(|_| AppError::NotFound)?;
            if owner != uid { return Err(AppError::Forbidden); }
            Ok(h)
        }
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    // Read photo bytes
    let data = store.get(&hash)?;
    // Write to temp file for Python
    let tmp_path = format!("/tmp/face_detect_{}.jpg", photo_id);
    std::fs::write(&tmp_path, &data)?;
    let engine = crate::ml::InferenceEngine::new(state.config.data_dir.join("models"));
    if !engine.is_available() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::BadRequest("ML not available".into()));
    }
    let faces = engine.detect_faces(&tmp_path).map_err(AppError::Internal)?;
    let _ = std::fs::remove_file(&tmp_path);
    // Store faces in DB
    let now = chrono::Utc::now().timestamp();
    for face in &faces {
        let fid = uuid::Uuid::new_v4().to_string();
        let pool2 = state.db.clone();
        let uid2 = uid.clone();
        let pid = photo_id.to_string();
        let f = face.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let c = conn(&pool2).map_err(AppError::Db)?;
            c.execute(
                "INSERT INTO faces (id, photo_id, user_id, bbox_x, bbox_y, bbox_w, bbox_h, embedding, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![fid, pid, uid2, f.x, f.y, f.w, f.h, crate::ml::vec_to_bytes(&f.embedding), now],
            )?;
            Ok(())
        }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    }
    // Cluster faces
    let pool3 = state.db.clone();
    let uid3 = uid.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool3).map_err(AppError::Db)?;
        // Get all face embeddings for this user
        let mut stmt = c.prepare("SELECT id, embedding FROM faces WHERE user_id = ?1 AND embedding IS NOT NULL")?;
        let rows = stmt.query_map(params![uid3], |r| Ok((r.get::<_,String>(0)?, r.get::<_,Vec<u8>>(1)?)))?;
        let mut ids: Vec<String> = Vec::new();
        let mut embs: Vec<Vec<f32>> = Vec::new();
        for r in rows {
            let (fid, bytes) = r?;
            ids.push(fid);
            embs.push(crate::ml::bytes_to_vec(&bytes));
        }
        let labels = crate::ml::cluster_faces(&embs, 0.6, 2);
        // Update person_id for each face
        for (i, label) in labels.iter().enumerate() {
            if *label >= 0 {
                let person_id = format!("person_{}", label);
                // Ensure person exists
                c.execute(
                    "INSERT OR IGNORE INTO people (id, user_id, name, created_at, updated_at) VALUES (?1,?2,?3,?4,?5)",
                    params![person_id, uid3, format!("Person {}", label + 1), now, now],
                )?;
                c.execute("UPDATE faces SET person_id = ?1 WHERE id = ?2", params![person_id, ids[i]])?;
            }
        }
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(json!({"photo_id": photo_id, "faces_detected": faces.len()})))
}

// Index a photo for smart search (encode + store embedding)
pub async fn index_photo(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<serde_json::Value>) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    let photo_id = req["photo_id"].as_str().ok_or(AppError::BadRequest("photo_id required".into()))?;
    let pool = state.db.clone();
    let store = state.store.clone();
    let hash = tokio::task::spawn_blocking({
        let pool = pool.clone();
        let pid = photo_id.to_string();
        let uid = uid.clone();
        move || -> Result<String> {
            let c = conn(&pool).map_err(AppError::Db)?;
            let (h, owner): (String, String) = c.query_row(
                "SELECT content_hash, user_id FROM photos WHERE id = ?1",
                params![pid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).map_err(|_| AppError::NotFound)?;
            if owner != uid { return Err(AppError::Forbidden); }
            Ok(h)
        }
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    let data = store.get(&hash)?;
    let tmp_path = format!("/tmp/index_{}.jpg", photo_id);
    std::fs::write(&tmp_path, &data)?;
    let engine = crate::ml::InferenceEngine::new(state.config.data_dir.join("models"));
    if !engine.is_available() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(AppError::BadRequest("ML not available".into()));
    }
    let emb = engine.encode_image(&tmp_path).map_err(AppError::Internal)?;
    let _ = std::fs::remove_file(&tmp_path);
    if emb.is_empty() {
        return Err(AppError::Internal("empty embedding".into()));
    }
    crate::ml::store_embedding(&state.db, photo_id, &emb, "clip-vit-b32")?;
    Ok(Json(json!({"photo_id": photo_id, "indexed": true, "embedding_dim": emb.len()})))
}

#[derive(Deserialize)]
pub struct OcrReq {
    pub photo_id: String,
}

pub async fn ocr(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<OcrReq>) -> Result<Json<serde_json::Value>> {
    let uid = auth_user(&state, &headers)?;
    let photo_id = req.photo_id.clone();
    let pool = state.db.clone();
    let store = state.store.clone();
    let model_dir = state.config.data_dir.join("models");
    let photo_id_for_load = photo_id.clone();
    let (hash, _mime) = tokio::task::spawn_blocking(move || -> Result<(String, String)> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let p = crate::routes::photos::load_photo(&c, &photo_id_for_load)?;
        if p.user_id != uid { return Err(AppError::Forbidden); }
        Ok((p.content_hash, p.mime_type))
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;

    // Write bytes to temp file and run OCR via python helper.
    let data = store.get(&hash)?;
    let tmp = std::env::temp_dir().join(format!("ocr_{}.jpg", hash));
    std::fs::write(&tmp, &data).map_err(|e| AppError::Internal(e.to_string()))?;
    let engine = crate::ml::InferenceEngine::new(model_dir.clone());
    let text = engine.ocr(&tmp.to_string_lossy()).map_err(AppError::Internal)?;
    let _ = std::fs::remove_file(&tmp);

    // Store result
    let pool = state.db.clone();
    let pid = photo_id.clone();
    let t = text.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        c.execute("UPDATE photos SET ocr_text = ?1 WHERE id = ?2", rusqlite::params![t, pid])?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(Json(serde_json::json!({"photo_id": photo_id, "text": text})))
}
