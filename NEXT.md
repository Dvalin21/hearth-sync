# hearth-sync — NEXT.md

Companion sync server for HearthBoard (Android family dashboard).
Language: Rust. License: MIT OR Apache-2.0.

Status: v1.2.1 — core feature-complete; optional/enhancement tier in progress.

## Implementation status (updated)

### Core (Phases 0–7, 9, 10, 11) — ✅ implemented & verified
- Auth (register/login/refresh/logout/me, invites, admin role), Argon2id, JWT
- Content-addressed object store (SHA-256, sharded, refcounted delete)
- Thumbnails (512px JPEG), EXIF metadata, backfill on dedup
- Albums CRUD + add/remove + shared album invites/members
- Sync protocol (delta pull/push/ack, tombstones, idempotent)
- FTS5 metadata search + CLIP smart search endpoint
- Creations (collage + GIF), memories (on-this-day, hide/unhide)
- ML endpoints (status, models, smart-search, people, reprocess, detect-faces, index-photo)
- Security headers, rate limiting (persistent, per-IP), input validation, size limits
- Multi-stage Docker build, docker-compose, backup/restore CLI
- Graceful shutdown (SIGTERM + SIGINT), structured JSON logging
- Request IDs (X-Request-Id), CORS from config, fail-loud JWT secret

### Completed this pass
- Integration tests: 12 (healthz, readyz, register, v1 alias, unauthorized, photo upload+list,
  album CRUD, search, memories, creations, ml status, sync changes) — all passing
- `/api/v1/` versioned routes alongside legacy `/api/`
- Backup/restore CLI (`hearth-sync backup|restore`)
- Job queue wired into AppState; worker spawned in main (inference when available)
- Rate limiter made persistent (was resetting per request)
- CI workflow (fmt, clippy, test, release build, audit)
- Version bump 0.1.0 → 1.2.1
- Config: tls_cert/tls_key fields added (for future direct-TLS wiring)

### Optional / enhancement tier — status
- [x] Clippy warnings cleaned (0 warnings; CI `-D warnings` green)
- [x] Prometheus metrics endpoint (`/metrics`, Prometheus text format)
- [x] Config TOML file loading (`--config path.toml` → env vars)
- [x] Streaming download Range support (206 Partial Content)
- [x] OCR endpoint (`POST /api/ml/ocr`) + inference.py ocr command (tesseract/pytesseract)
- [x] Remote ML config (`HEARTH_SYNC_ML_SERVICE_URL`)
- [x] Partner sharing (`POST /api/albums/:id/auto-share` by person/date range)
- [x] Polling notifications (`GET /api/notifications?since=`)
- [x] Additional fuzz target (fuzz_sync_push)
- [ ] Direct TLS serving (rustls) — config fields exist; accept loop not wired (reverse proxy recommended)
- [ ] Job queue: make ML routes enqueue async jobs instead of blocking (queue wired but routes still sync)

### Outstanding production polish
- [ ] cargo audit: clean (only unmaintained `paste` warning allowed)
- [ ] Release binaries via GitHub Actions (x86_64, aarch64)
- [ ] Penetration test of auth + object access (IDOR)
- [ ] Fuzzing for 1 hour without crashes

### Cross-cutting
- Testing: 12 integration tests (was 0) — unit tests still 0
- Observability: tracing spans on sync; request IDs; metrics endpoint TODO
- Compatibility: /api + /api/v1 both served
- Migration: SQLite forward-only migrations tested
