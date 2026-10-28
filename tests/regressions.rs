//! Regression tests for defects found by reading the server against its own
//! documented contract. Each test names the file:line it pins down.
//!
//! These are written to fail before the fix and pass after it.

use hearth_sync::{
    build_router, config::Config, db, middleware::RateLimiter, storage::ObjectStore, AppState,
};
use serde_json::Value;
use std::sync::Arc;
use tempfile::TempDir;
use chrono::Datelike;

struct Harness {
    base: String,
    db: db::DbPool,
    jobs: tokio::sync::mpsc::Receiver<hearth_sync::ml::Job>,
    _tmp: TempDir,
    _handle: tokio::task::JoinHandle<()>,
}

async fn harness() -> Harness {
    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(&data_dir).unwrap();
    let config = Config {
        listen: "0.0.0.0:0".into(),
        data_dir: data_dir.clone(),
        db_name: "hearth.db".into(),
        jwt_secret: Some("test-secret-1234567890".into()),
        max_upload_mb: 10,
        log_level: "error".into(),
        cors_origins: "*".into(),
        tls_cert: None,
        tls_key: None,
        ml_service_url: None,
        trusted_proxies: String::new(),
    };
    let pool = db::init_pool(&config.db_path()).unwrap();
    let store = ObjectStore::new(config.objects_dir(), config.thumbs_dir()).unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let state = AppState {
        config,
        db: pool.clone(),
        store,
        rate_limiter: Arc::new(RateLimiter::new(100_000, 60)),
        auth_rate_limiter: Arc::new(RateLimiter::new(100_000, 60)),
        job_tx: tx,
        metrics: Arc::new(hearth_sync::metrics::Metrics::default()),
    };
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
    Harness { base, db: pool, jobs: rx, _tmp: tmp, _handle: handle }
}

async fn register(client: &reqwest::Client, base: &str, user: &str) -> String {
    let r = client
        .post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({"username": user, "password": "password123"}))
        .send()
        .await
        .unwrap();
    let j: Value = r.json().await.unwrap();
    j["access_token"].as_str().unwrap().to_string()
}

async fn upload(client: &reqwest::Client, base: &str, tok: &str, bytes: &[u8]) -> String {
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(bytes.to_vec())
            .file_name("t.jpg")
            .mime_str("image/jpeg")
            .unwrap(),
    );
    let r = client
        .post(format!("{}/api/photos", base))
        .bearer_auth(tok)
        .multipart(form)
        .send()
        .await
        .unwrap();
    let j: Value = r.json().await.unwrap();
    j["id"].as_str().unwrap().to_string()
}

// ── 1. db/mod.rs:25-33 — PRAGMAs run on the migration connection only.
//    r2d2_sqlite does NOT enable foreign_keys per connection, so every
//    `ON DELETE CASCADE` in schema.sql is inert at runtime.

#[tokio::test]
async fn foreign_keys_are_enabled_on_pooled_connections() {
    let h = harness().await;
    let c = h.db.get().unwrap();
    let on: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    assert_eq!(on, 1, "foreign_keys must be ON for every pooled connection");
}

#[tokio::test]
async fn deleting_an_album_cascades_to_album_photos() {
    let h = harness().await;
    let client = reqwest::Client::new();
    let tok = register(&client, &h.base, "u").await;
    let pid = upload(&client, &h.base, &tok, b"AAA").await;

    let r = client
        .post(format!("{}/api/albums", h.base))
        .bearer_auth(&tok)
        .json(&serde_json::json!({"name": "A"}))
        .send()
        .await
        .unwrap();
    let aid = r.json::<Value>().await.unwrap()["id"].as_str().unwrap().to_string();

    client
        .post(format!("{}/api/albums/{}/photos", h.base, aid))
        .bearer_auth(&tok)
        .json(&serde_json::json!({"photo_id": pid}))
        .send()
        .await
        .unwrap();

    client
        .delete(format!("{}/api/albums/{}", h.base, aid))
        .bearer_auth(&tok)
        .send()
        .await
        .unwrap();

    let c = h.db.get().unwrap();
    let left: i64 = c
        .query_row("SELECT COUNT(*) FROM album_photos WHERE album_id = ?1", [&aid], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0, "album_photos rows must cascade away with the album");
}

// ── 2. routes/sync.rs:111-114 — next_cursor is `max(updated_at)`, and
//    updated_at has one-second resolution. Anything written in the same
//    second as the cursor is never returned again: silent data loss.

#[tokio::test]
async fn sync_cursor_does_not_drop_changes_sharing_a_timestamp() {
    let h = harness().await;
    let client = reqwest::Client::new();
    let tok = register(&client, &h.base, "u").await;

    // Two photos pushed in one batch => identical updated_at.
    let r = client
        .post(format!("{}/api/sync/push", h.base))
        .bearer_auth(&tok)
        .json(&serde_json::json!({
            "device_id": "dev",
            "items": [
                {"id": "p1", "content_hash": "h1", "updated_at": 1000},
                {"id": "p2", "content_hash": "h2", "updated_at": 1000}
            ]
        }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // Paginate one at a time: this is what a client with limit=1 sees.
    let r = client
        .get(format!("{}/api/sync/changes?since=0&limit=1", h.base))
        .bearer_auth(&tok)
        .send()
        .await
        .unwrap();
    let page1: Value = r.json().await.unwrap();
    assert_eq!(page1["changes"].as_array().unwrap().len(), 1);
    let cursor = page1["next_cursor"].as_i64().unwrap();

    let r = client
        .get(format!("{}/api/sync/changes?since={}&limit=1", h.base, cursor))
        .bearer_auth(&tok)
        .send()
        .await
        .unwrap();
    let page2: Value = r.json().await.unwrap();
    assert_eq!(
        page2["changes"].as_array().unwrap().len(),
        1,
        "second change was silently skipped: cursor={} must not swallow its own timestamp",
        cursor
    );
}

// ── 3. routes/sharing.rs:216-245 — `notifications` authenticates the caller,
//    then ignores them: the query returns every photo in every shared album.

#[tokio::test]
async fn notifications_do_not_leak_other_users_albums() {
    let h = harness().await;
    let client = reqwest::Client::new();
    let admin = register(&client, &h.base, "admin").await;
    let pid = upload(&client, &h.base, &admin, b"SECRET").await;

    let r = client
        .post(format!("{}/api/albums", h.base))
        .bearer_auth(&admin)
        .json(&serde_json::json!({"name": "Private"}))
        .send()
        .await
        .unwrap();
    let aid = r.json::<Value>().await.unwrap()["id"].as_str().unwrap().to_string();
    client
        .post(format!("{}/api/albums/{}/photos", h.base, aid))
        .bearer_auth(&admin)
        .json(&serde_json::json!({"photo_id": pid}))
        .send()
        .await
        .unwrap();
    // Marking the album shared is enough to expose it today.
    client
        .post(format!("{}/api/albums/{}/invite", h.base, aid))
        .bearer_auth(&admin)
        .json(&serde_json::json!({"permission": "view"}))
        .send()
        .await
        .unwrap();

    // Second user, not a member of anything.
    let inv = client
        .post(format!("{}/api/auth/invites", h.base))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    let code = inv.json::<Value>().await.unwrap()["code"].as_str().unwrap().to_string();
    let r = client
        .post(format!("{}/api/auth/register", h.base))
        .json(&serde_json::json!({"username": "mallory", "password": "password123", "invite_code": code}))
        .send()
        .await
        .unwrap();
    let other = r.json::<Value>().await.unwrap()["access_token"].as_str().unwrap().to_string();

    let r = client
        .get(format!("{}/api/notifications?since=0", h.base))
        .bearer_auth(&other)
        .send()
        .await
        .unwrap();
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        body["items"].as_array().unwrap().len(),
        0,
        "a non-member must not see photos from someone else's shared album"
    );
}

// ── 4. routes/sharing.rs:208 — INSERT omits `added_at`, which is NOT NULL
//    with no default. Every insert fails, the error is discarded with `let _`,
//    and the endpoint still reports the photos as added.

#[tokio::test]
async fn auto_share_actually_adds_photos() {
    let h = harness().await;
    let client = reqwest::Client::new();
    let tok = register(&client, &h.base, "u").await;
    upload(&client, &h.base, &tok, b"ONE").await;
    upload(&client, &h.base, &tok, b"TWO").await;

    let r = client
        .post(format!("{}/api/albums", h.base))
        .bearer_auth(&tok)
        .json(&serde_json::json!({"name": "Smart"}))
        .send()
        .await
        .unwrap();
    let aid = r.json::<Value>().await.unwrap()["id"].as_str().unwrap().to_string();

    let r = client
        .post(format!("{}/api/albums/{}/auto-share", h.base, aid))
        .bearer_auth(&tok)
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    let c = h.db.get().unwrap();
    let n: i64 = c
        .query_row("SELECT COUNT(*) FROM album_photos WHERE album_id = ?1", [&aid], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2, "auto-share reported success but inserted nothing");
}

// ── 5. routes/search.rs:55-61 — raw user text is passed to FTS5 MATCH.
//    A bare quote is a syntax error, so the request 500s.

#[tokio::test]
async fn search_tolerates_fts_syntax_in_user_input() {
    let h = harness().await;
    let client = reqwest::Client::new();
    let tok = register(&client, &h.base, "u").await;
    for q in ["\"", "a\"b", "AND", "*", "foo:", "(", "NEAR("] {
        let r = client
            .get(format!("{}/api/search", h.base))
            .query(&[("q", q)])
            .bearer_auth(&tok)
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "search q={:?} returned {}", q, r.status());
    }
}

// ── 6. metrics.rs — the counters exist and are rendered, but nothing ever
//    increments them, so /metrics always reports zero.

#[tokio::test]
async fn metrics_count_real_traffic() {
    let h = harness().await;
    let client = reqwest::Client::new();
    let tok = register(&client, &h.base, "u").await;
    upload(&client, &h.base, &tok, b"X").await;
    let _ = client.get(format!("{}/api/photos", h.base)).bearer_auth(&tok).send().await;

    let text = reqwest::get(format!("{}/metrics", h.base)).await.unwrap().text().await.unwrap();
    let line = text
        .lines()
        .find(|l| l.starts_with("hearth_sync_requests_total "))
        .expect("counter present");
    let n: u64 = line.rsplit(' ').next().unwrap().parse().unwrap();
    assert!(n > 0, "requests_total is still 0 after real traffic");

    let line = text
        .lines()
        .find(|l| l.starts_with("hearth_sync_photos_uploaded "))
        .unwrap();
    let n: u64 = line.rsplit(' ').next().unwrap().parse().unwrap();
    assert!(n > 0, "photos_uploaded is still 0 after a real upload");
}

// ── 7. routes/auth.rs:59-72 — the invite is marked used by the literal string
//    "pending" before the user row is inserted, so a duplicate username burns
//    the invite and the real user id is never recorded.

#[tokio::test]
async fn failed_registration_does_not_burn_the_invite() {
    let h = harness().await;
    let client = reqwest::Client::new();
    let admin = register(&client, &h.base, "admin").await;

    let code = client
        .post(format!("{}/api/auth/invites", h.base))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["code"]
        .as_str()
        .unwrap()
        .to_string();

    // First attempt collides with the existing "admin" username.
    let r = client
        .post(format!("{}/api/auth/register", h.base))
        .json(&serde_json::json!({"username": "admin", "password": "password123", "invite_code": code}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409, "duplicate username should conflict");

    // The same invite must still work for a different username.
    let r = client
        .post(format!("{}/api/auth/register", h.base))
        .json(&serde_json::json!({"username": "child", "password": "password123", "invite_code": code}))
        .send()
        .await
        .unwrap();
    assert!(
        r.status().is_success(),
        "invite was consumed by a registration that never happened: {}",
        r.status()
    );

    let c = h.db.get().unwrap();
    let used_by: String = c
        .query_row("SELECT used_by FROM invites WHERE code = ?1", [&code], |r| r.get(0))
        .unwrap();
    let child_id: String = c
        .query_row("SELECT id FROM users WHERE username = 'child'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(used_by, child_id, "used_by must record the user who redeemed it");
}

// ── 8. routes/ml.rs:160,241 — temp path built from a client-supplied photo id.
//    `sync::push` accepts any string as an id, so the id can contain `../`.

#[tokio::test]
async fn ml_temp_files_are_not_built_from_user_supplied_ids() {
    let h = harness().await;
    let client = reqwest::Client::new();
    let tok = register(&client, &h.base, "u").await;

    let evil = "../../../../tmp/hearth_pwn";
    let r = client
        .post(format!("{}/api/sync/push", h.base))
        .bearer_auth(&tok)
        .json(&serde_json::json!({
            "device_id": "dev",
            "items": [{"id": evil, "content_hash": "h1", "updated_at": 1}]
        }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // The row now exists with the hostile id. Face detection must not write
    // outside the temp dir when it stages bytes for Python.
    let marker = std::path::Path::new("/tmp/hearth_pwn.jpg");
    let _ = std::fs::remove_file(marker);

    let _ = client
        .post(format!("{}/api/ml/detect-faces", h.base))
        .bearer_auth(&tok)
        .json(&serde_json::json!({"photo_id": evil}))
        .send()
        .await
        .unwrap();

    assert!(
        !marker.exists(),
        "photo id escaped the temp directory: wrote {}",
        marker.display()
    );
}

// ── 9. routes/memories.rs:63 — "on this day" loaded every dated photo the user
//    owns and compared month/day in Rust. The predicate belongs in SQL, and the
//    rewrite must still match the same calendar day in an earlier year.

#[tokio::test]
async fn memories_on_this_day_matches_the_calendar_day() {
    let h = harness().await;
    let client = reqwest::Client::new();
    let tok = register(&client, &h.base, "u").await;

    let today = chrono::Utc::now();
    // A leap day has no counterpart last year; skip rather than assert a
    // calendar that only exists every fourth year.
    let Some(last_year) = today.with_year(today.year() - 1) else { return };

    let on_the_day = upload(&client, &h.base, &tok, b"ONE").await;
    let day_before = upload(&client, &h.base, &tok, b"TWO").await;
    let c = h.db.get().unwrap();
    c.execute(
        "UPDATE photos SET taken_at = ?1 WHERE id = ?2",
        rusqlite::params![last_year.timestamp(), on_the_day],
    )
    .unwrap();
    c.execute(
        "UPDATE photos SET taken_at = ?1 WHERE id = ?2",
        rusqlite::params![last_year.timestamp() - 86_400, day_before],
    )
    .unwrap();
    drop(c);

    let r = client
        .get(format!("{}/api/memories", h.base))
        .bearer_auth(&tok)
        .send()
        .await
        .unwrap();
    let memories: Value = r.json().await.unwrap();
    let arr = memories.as_array().unwrap();
    assert_eq!(arr.len(), 1, "expected exactly one on-this-day memory: {}", memories);
    let ids = arr[0]["photo_ids"].as_array().unwrap();
    assert_eq!(ids.len(), 1, "only the same calendar day qualifies: {}", arr[0]);
    assert_eq!(ids[0].as_str().unwrap(), on_the_day);
}

// ── 10. routes/ml.rs:128 — reprocess answered {"queued": true} while sending
//     nothing to the worker.

#[tokio::test]
async fn reprocess_enqueues_one_job_per_photo() {
    let mut h = harness().await;
    let client = reqwest::Client::new();
    let tok = register(&client, &h.base, "u").await;
    upload(&client, &h.base, &tok, b"ONE").await;
    upload(&client, &h.base, &tok, b"TWO").await;

    let body: Value = client
        .post(format!("{}/api/ml/reprocess", h.base))
        .bearer_auth(&tok)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["queued"].as_u64(), Some(2), "{}", body);

    // The jobs must actually exist in the queue, not just in the response body.
    let mut queued = 0;
    while let Ok(job) = h.jobs.try_recv() {
        if matches!(job, hearth_sync::ml::Job::ProcessPhoto { .. }) {
            queued += 1;
        }
    }
    assert_eq!(queued, 2, "reprocess reported jobs it never enqueued");
}
