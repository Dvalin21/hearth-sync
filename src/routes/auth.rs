use crate::auth::{hash_password, issue_access_token, issue_refresh_token, validate_access_token, verify_password};
use crate::db::conn;
use crate::error::{AppError, Result};
use crate::AppState;
use axum::extract::State;
use axum::Json;
use chrono::Utc;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct RegisterReq {
    pub username: String,
    pub password: String,
    pub invite_code: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResp {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserInfo,
}

#[derive(Serialize, Clone)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub role: String,
}

#[derive(Deserialize)]
pub struct RefreshReq {
    pub refresh_token: String,
}

pub async fn register(State(state): State<AppState>, Json(req): Json<RegisterReq>) -> Result<Json<AuthResp>> {
    if req.username.trim().is_empty() || req.password.len() < 8 {
        return Err(AppError::BadRequest("username required and password >= 8 chars".into()));
    }
    let pool = state.db.clone();
    let secret = state.config.jwt_secret();
    let username = req.username.clone();
    let password = req.password.clone();
    let username_resp = username.clone();
    let (user_id, role) = tokio::task::spawn_blocking(move || -> Result<(String, String)> {
        let c = conn(&pool).map_err(AppError::Db)?;
        // first user becomes admin
        let count: i64 = c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0)).unwrap_or(0);
        let role = if count == 0 { "admin" } else { "member" };
        // invite check for non-first
        if count > 0 {
            let code = req.invite_code.clone().unwrap_or_default();
            let valid: Option<String> = c
                .query_row(
                    "SELECT code FROM invites WHERE code = ?1 AND used_by IS NULL",
                    params![code],
                    |r| r.get(0),
                )
                .ok();
            if valid.is_none() {
                return Err(AppError::Forbidden);
            }
            c.execute("UPDATE invites SET used_by = ?1 WHERE code = ?2", params!["pending", code])?;
        }
        let id = Uuid::new_v4().to_string();
        let hash = hash_password(&password).map_err(|e| AppError::Internal(e.to_string()))?;
        c.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, username.trim(), hash, role, Utc::now().timestamp()],
        )
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                AppError::Conflict("username taken".into())
            } else {
                AppError::Db(e)
            }
        })?;
        Ok((id, role.to_string()))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let access = issue_access_token(&secret, &user_id, &username_resp, &role)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let refresh = issue_refresh_token();
    store_refresh(&state, &user_id, &refresh).await?;
    Ok(Json(AuthResp {
        access_token: access,
        refresh_token: refresh,
        user: UserInfo { id: user_id, username: username_resp, role },
    }))
}

pub async fn login(State(state): State<AppState>, Json(req): Json<LoginReq>) -> Result<Json<AuthResp>> {
    let pool = state.db.clone();
    let secret = state.config.jwt_secret();
    let username = req.username.clone();
    let password = req.password.clone();
    let username_resp = username.clone();
    let (user_id, role, hash) = tokio::task::spawn_blocking(move || -> Result<(String, String, String)> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let row: (String, String, String) = c
            .query_row(
                "SELECT id, role, password_hash FROM users WHERE username = ?1",
                params![username.trim()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| AppError::Unauthorized)?;
        Ok(row)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    if !verify_password(&password, &hash) {
        return Err(AppError::Unauthorized);
    }
    let access = issue_access_token(&secret, &user_id, &username_resp, &role)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let refresh = issue_refresh_token();
    store_refresh(&state, &user_id, &refresh).await?;
    Ok(Json(AuthResp {
        access_token: access,
        refresh_token: refresh,
        user: UserInfo { id: user_id, username: username_resp, role },
    }))
}

pub async fn refresh(State(state): State<AppState>, Json(req): Json<RefreshReq>) -> Result<Json<AuthResp>> {
    // validate refresh token exists
    let pool = state.db.clone();
    let user_id: String = tokio::task::spawn_blocking(move || -> Result<String> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let uid: String = c
            .query_row(
                "SELECT user_id FROM refresh_tokens WHERE token_hash = ?1 AND expires_at > ?2",
                params![hash_token(&req.refresh_token), Utc::now().timestamp()],
                |r| r.get(0),
            )
            .map_err(|_| AppError::Unauthorized)?;
        // rotate: delete old
        c.execute("DELETE FROM refresh_tokens WHERE token_hash = ?1", params![hash_token(&req.refresh_token)])?;
        Ok(uid)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    // load user
    let pool = state.db.clone();
    let uid_for_user = user_id.clone();
    let (username, role) = tokio::task::spawn_blocking(move || -> Result<(String, String)> {
        let c = conn(&pool).map_err(AppError::Db)?;
        let r = c
            .query_row("SELECT username, role FROM users WHERE id = ?1", params![uid_for_user], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|_| AppError::Unauthorized)?;
        Ok(r)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let secret = state.config.jwt_secret();
    let access = issue_access_token(&secret, &user_id, &username, &role)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let refresh = issue_refresh_token();
    store_refresh(&state, &user_id, &refresh).await?;
    Ok(Json(AuthResp {
        access_token: access,
        refresh_token: refresh,
        user: UserInfo { id: user_id, username, role },
    }))
}

pub async fn logout(State(state): State<AppState>, Json(req): Json<RefreshReq>) -> Result<Json<serde_json::Value>> {
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        c.execute("DELETE FROM refresh_tokens WHERE token_hash = ?1", params![hash_token(&req.refresh_token)])?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn create_invite(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Result<Json<serde_json::Value>> {
    let uid = {
        let h = headers.get("authorization").ok_or(AppError::Unauthorized)?;
        let s = h.to_str().map_err(|_| AppError::Unauthorized)?;
        let t = s.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;
        let claims = crate::auth::validate_access_token(&state.config.jwt_secret(), t).map_err(|_| AppError::Unauthorized)?;
        if claims.role != "admin" { return Err(AppError::Forbidden); }
        claims.sub
    };
    let pool = state.db.clone();
    let code = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let now = chrono::Utc::now().timestamp();
    let code2 = code.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        c.execute(
            "INSERT INTO invites (code, created_by, created_at) VALUES (?1,?2,?3)",
            params![code2, uid, now],
        )?;
        Ok(())
    }).await.map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(Json(serde_json::json!({"code": code})))
}

pub async fn me(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Result<Json<UserInfo>> {
    let token = bearer(&headers)?;
    let claims = validate_access_token(&state.config.jwt_secret(), token).map_err(|_| AppError::Unauthorized)?;
    Ok(Json(UserInfo {
        id: claims.sub,
        username: claims.username,
        role: claims.role,
    }))
}

// --- helpers ---
fn bearer(headers: &axum::http::HeaderMap) -> Result<&str> {
    let h = headers.get("authorization").ok_or(AppError::Unauthorized)?;
    let s = h.to_str().map_err(|_| AppError::Unauthorized)?;
    s.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)
}

fn hash_token(t: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(t.as_bytes()))
}

async fn store_refresh(state: &AppState, user_id: &str, refresh: &str) -> Result<()> {
    let pool = state.db.clone();
    let uid = user_id.to_string();
    let rh = hash_token(refresh);
    let now = Utc::now().timestamp();
    let exp = now + 30 * 24 * 3600;
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = conn(&pool).map_err(AppError::Db)?;
        c.execute(
            "INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![Uuid::new_v4().to_string(), uid, rh, exp, now],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;
    Ok(())
}
