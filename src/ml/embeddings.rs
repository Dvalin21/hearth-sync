use crate::db::conn;
use crate::error::{AppError, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::sync::Arc;

/// Compute cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = (na.sqrt() * nb.sqrt()).max(1e-8);
    dot / denom
}

/// Convert f32 slice to bytes (little-endian).
pub fn vec_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Convert bytes to f32 vec.
pub fn bytes_to_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Store an embedding for a photo.
pub fn store_embedding(
    pool: &Pool<SqliteConnectionManager>,
    photo_id: &str,
    embedding: &[f32],
    model: &str,
) -> Result<()> {
    let c = conn(pool).map_err(AppError::Db)?;
    let now = chrono::Utc::now().timestamp();
    let bytes = vec_to_bytes(embedding);
    c.execute(
        "INSERT INTO photo_embeddings (photo_id, embedding, model, created_at) VALUES (?1,?2,?3,?4)
         ON CONFLICT(photo_id) DO UPDATE SET embedding=excluded.embedding, model=excluded.model, created_at=excluded.created_at",
        params![photo_id, bytes, model, now],
    )?;
    Ok(())
}

/// Get embedding for a photo.
pub fn get_embedding(pool: &Pool<SqliteConnectionManager>, photo_id: &str) -> Result<Option<Vec<f32>>> {
    let c = conn(pool).map_err(AppError::Db)?;
    let bytes: Option<Vec<u8>> = c
        .query_row("SELECT embedding FROM photo_embeddings WHERE photo_id = ?1", params![photo_id], |r| r.get(0))
        .ok();
    Ok(bytes.map(|b| bytes_to_vec(&b)))
}

/// Search for photos similar to a query embedding. Returns (photo_id, similarity) sorted desc.
pub fn search_similar(
    pool: &Pool<SqliteConnectionManager>,
    user_id: &str,
    query_emb: &[f32],
    limit: usize,
) -> Result<Vec<(String, f32)>> {
    let c = conn(pool).map_err(AppError::Db)?;
    // Join photo_embeddings with photos to filter by user
    let mut stmt = c.prepare(
        "SELECT pe.photo_id, pe.embedding FROM photo_embeddings pe
         JOIN photos p ON p.id = pe.photo_id
         WHERE p.user_id = ?1 AND p.is_trashed = 0",
    )?;
    let rows = stmt.query_map(params![user_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    let mut results: Vec<(String, f32)> = Vec::new();
    for r in rows {
        let (pid, bytes) = r?;
        let emb = bytes_to_vec(&bytes);
        if emb.len() == query_emb.len() {
            let sim = cosine_similarity(query_emb, &emb);
            results.push((pid, sim));
        }
    }
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);
    Ok(results)
}
