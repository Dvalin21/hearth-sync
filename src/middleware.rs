use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use crate::AppState;
use axum::response::Response;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Simple in-memory rate limiter (token bucket per IP).
/// Limits: 100 requests per 60 seconds per IP for general endpoints.
/// Auth endpoints: 10 requests per 60 seconds per IP.
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, (u32, Instant)>>,
    max_requests: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    pub fn check(&self, key: &str) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let now = Instant::now();
        let entry = buckets.entry(key.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) > self.window {
            entry.0 = 0;
            entry.1 = now;
        }
        if entry.0 >= self.max_requests {
            return false;
        }
        entry.0 += 1;
        true
    }
}

/// Rate limiting middleware. Applies per-IP limits.
pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract client IP from headers or connection
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .unwrap_or("unknown")
        .trim()
        .to_string();

    let path = req.uri().path();
    let is_auth = path.starts_with("/api/auth/") || path.starts_with("/api/v1/auth/");
    let limiter = if is_auth { &state.auth_rate_limiter } else { &state.rate_limiter };
    if !limiter.check(&ip) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    Ok(next.run(req).await)
}

/// Attach a unique request id to each response.
pub async fn request_id_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let id = uuid::Uuid::new_v4().to_string();
    let mut res = next.run(req).await;
    if let Ok(v) = id.parse() {
        res.headers_mut().insert("X-Request-Id", v);
    }
    res
}
