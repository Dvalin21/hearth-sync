use crate::AppState;
use axum::extract::State;
use axum::Json;
use serde_json::json;

pub async fn healthz() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

pub async fn readyz(State(state): State<AppState>) -> Json<serde_json::Value> {
    // check DB
    let ok = db_ok(&state).await;
    Json(json!({"status": if ok { "ready" } else { "not ready" }, "db": ok}))
}

async fn db_ok(state: &AppState) -> bool {
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get().ok()?;
        conn.query_row("SELECT 1", [], |_| Ok(())).ok()?;
        Some(())
    })
    .await
    .ok()
    .flatten()
    .is_some()
}
