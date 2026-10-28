use axum::extract::{ConnectInfo, State};
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use crate::AppState;
use axum::response::Response;
use std::collections::HashMap;
use std::net::SocketAddr;
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
        // One entry per source address, forever, is a memory leak any scanner
        // can drive. Sweep expired windows once the map is worth sweeping.
        if buckets.len() > 1024 {
            buckets.retain(|_, (_, start)| now.duration_since(*start) <= self.window);
        }
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

/// Resolve the rate-limit bucket key for a request.
///
/// The TCP peer address is authoritative. `X-Forwarded-For` is only believed
/// when that peer is a reverse proxy the operator named in
/// `trusted_proxies`; otherwise any client could choose its own bucket by
/// setting the header, and requests without it all shared one bucket.
pub fn client_ip(peer: SocketAddr, xff: Option<&str>, trusted_proxies: &str) -> String {
    let peer_ip = peer.ip().to_string();
    let is_trusted = trusted_proxies
        .split(',')
        .map(str::trim)
        .any(|t| !t.is_empty() && t == peer_ip);
    if is_trusted {
        if let Some(first) = xff.and_then(|v| v.split(',').next()).map(str::trim) {
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }
    peer_ip
}

/// Rate limiting middleware. Applies per-IP limits.
pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let xff = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok());
    let ip = client_ip(peer, xff, &state.config.trusted_proxies);

    let path = req.uri().path();
    let is_auth = path.starts_with("/api/auth/") || path.starts_with("/api/v1/auth/");
    let limiter = if is_auth { &state.auth_rate_limiter } else { &state.rate_limiter };
    if !limiter.check(&ip) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    // Every request that reaches the router is counted here; /metrics would
    // otherwise always report zero because nothing else touches the counter.
    state.metrics.requests_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::client_ip;

    fn peer(s: &str) -> std::net::SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn forwarded_for_is_ignored_from_an_untrusted_peer() {
        // The attacker is the direct peer and sets the header itself.
        assert_eq!(
            client_ip(peer("203.0.113.9:5000"), Some("1.2.3.4"), ""),
            "203.0.113.9"
        );
    }

    #[test]
    fn forwarded_for_is_honoured_from_a_trusted_proxy() {
        assert_eq!(
            client_ip(peer("10.0.0.27:5000"), Some("1.2.3.4, 10.0.0.27"), "10.0.0.27"),
            "1.2.3.4"
        );
    }

    #[test]
    fn a_trusted_proxy_without_the_header_falls_back_to_its_own_address() {
        assert_eq!(client_ip(peer("10.0.0.27:5000"), None, "10.0.0.27"), "10.0.0.27");
    }

    #[test]
    fn distinct_peers_get_distinct_buckets() {
        assert_ne!(
            client_ip(peer("10.0.0.5:1"), None, ""),
            client_ip(peer("10.0.0.6:1"), None, "")
        );
    }
}
