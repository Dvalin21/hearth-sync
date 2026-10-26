use hearth_sync::{build_router, config::Config, db, storage::ObjectStore, AppState, middleware::RateLimiter};
use serde_json::Value;
use std::sync::Arc;
use tempfile::TempDir;

fn make_state(tmp: &TempDir) -> AppState {
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
    };
    let db = db::init_pool(&config.db_path()).unwrap();
    let store = ObjectStore::new(config.objects_dir(), config.thumbs_dir()).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    AppState {
        config,
        db,
        store,
        rate_limiter: Arc::new(RateLimiter::new(10000, 60)),
        auth_rate_limiter: Arc::new(RateLimiter::new(10000, 60)),
        job_tx: tx,
    }
}

async fn spawn_server(state: AppState) -> (String, tokio::task::JoinHandle<()>) {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{}", addr), handle)
}

#[tokio::test]
async fn healthz_ok() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let resp = reqwest::get(format!("{}/healthz", base)).await.unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn readyz_ok() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let resp = reqwest::get(format!("{}/readyz", base)).await.unwrap();
    assert!(resp.status().is_success());
    let j: Value = resp.json().await.unwrap();
    assert_eq!(j["db"], true);
}

#[tokio::test]
async fn register_first_user_is_admin() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let client = reqwest::Client::new();
    let resp = client.post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({"username":"admin","password":"password123"}))
        .send().await.unwrap();
    assert!(resp.status().is_success());
    let j: Value = resp.json().await.unwrap();
    assert_eq!(j["user"]["role"], "admin");
}

#[tokio::test]
async fn v1_alias_works() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let client = reqwest::Client::new();
    let resp = client.post(format!("{}/api/v1/auth/register", base))
        .json(&serde_json::json!({"username":"u","password":"password123"}))
        .send().await.unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn unauthorized_without_token() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let resp = reqwest::get(format!("{}/api/photos", base)).await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn photo_upload_and_list() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let client = reqwest::Client::new();
    // register
    let r = client.post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({"username":"u","password":"password123"}))
        .send().await.unwrap();
    let j: Value = r.json().await.unwrap();
    let tok = j["access_token"].as_str().unwrap();
    // upload
    let form = reqwest::multipart::Form::new()
        .part("file", reqwest::multipart::Part::bytes(b"FAKEJPEGDATA".to_vec())
            .file_name("t.jpg").mime_str("image/jpeg").unwrap());
    let r = client.post(format!("{}/api/photos", base))
        .bearer_auth(tok)
        .multipart(form).send().await.unwrap();
    assert!(r.status().is_success() || r.status().as_u16() == 201, "upload: {}", r.status());
    // list
    let r = client.get(format!("{}/api/photos", base)).bearer_auth(tok).send().await.unwrap();
    assert!(r.status().is_success());
    let arr: Value = r.json().await.unwrap();
    assert!(arr.as_array().unwrap().len() >= 1);
}

#[tokio::test]
async fn album_crud() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let client = reqwest::Client::new();
    let r = client.post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({"username":"u","password":"password123"})).send().await.unwrap();
    let j: Value = r.json().await.unwrap();
    let tok = j["access_token"].as_str().unwrap();
    // create album
    let r = client.post(format!("{}/api/albums", base)).bearer_auth(tok)
        .json(&serde_json::json!({"name":"Vacation"})).send().await.unwrap();
    assert!(r.status().is_success());
    let j: Value = r.json().await.unwrap();
    let aid = j["id"].as_str().unwrap();
    // list
    let r = client.get(format!("{}/api/albums", base)).bearer_auth(tok).send().await.unwrap();
    assert!(r.status().is_success());
    // get
    let r = client.get(format!("{}/api/albums/{}", base, aid)).bearer_auth(tok).send().await.unwrap();
    assert!(r.status().is_success());
    // delete
    let r = client.delete(format!("{}/api/albums/{}", base, aid)).bearer_auth(tok).send().await.unwrap();
    assert!(r.status().is_success() || r.status().as_u16() == 204);
}

#[tokio::test]
async fn search_works() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let client = reqwest::Client::new();
    let r = client.post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({"username":"u","password":"password123"})).send().await.unwrap();
    let j: Value = r.json().await.unwrap();
    let tok = j["access_token"].as_str().unwrap();
    let r = client.get(format!("{}/api/search?q=test", base)).bearer_auth(tok).send().await.unwrap();
    assert!(r.status().is_success());
}

#[tokio::test]
async fn memories_list() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let client = reqwest::Client::new();
    let r = client.post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({"username":"u","password":"password123"})).send().await.unwrap();
    let j: Value = r.json().await.unwrap();
    let tok = j["access_token"].as_str().unwrap();
    let r = client.get(format!("{}/api/memories", base)).bearer_auth(tok).send().await.unwrap();
    assert!(r.status().is_success());
}

#[tokio::test]
async fn creations_list() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let client = reqwest::Client::new();
    let r = client.post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({"username":"u","password":"password123"})).send().await.unwrap();
    let j: Value = r.json().await.unwrap();
    let tok = j["access_token"].as_str().unwrap();
    let r = client.get(format!("{}/api/creations", base)).bearer_auth(tok).send().await.unwrap();
    assert!(r.status().is_success());
}

#[tokio::test]
async fn ml_status() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let client = reqwest::Client::new();
    let r = client.post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({"username":"u","password":"password123"})).send().await.unwrap();
    let j: Value = r.json().await.unwrap();
    let tok = j["access_token"].as_str().unwrap();
    let r = client.get(format!("{}/api/ml/status", base)).bearer_auth(tok).send().await.unwrap();
    assert!(r.status().is_success());
}

#[tokio::test]
async fn sync_changes_empty() {
    let tmp = TempDir::new().unwrap();
    let (base, _h) = spawn_server(make_state(&tmp)).await;
    let client = reqwest::Client::new();
    let r = client.post(format!("{}/api/auth/register", base))
        .json(&serde_json::json!({"username":"u","password":"password123"})).send().await.unwrap();
    let j: Value = r.json().await.unwrap();
    let tok = j["access_token"].as_str().unwrap();
    let r = client.get(format!("{}/api/sync/changes", base)).bearer_auth(tok).send().await.unwrap();
    assert!(r.status().is_success());
}
