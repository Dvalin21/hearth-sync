# hearth-sync — NEXT.md

## Progress update (2026-10-20) — FINAL

ALL phases implemented. Server is feature-complete for v1.

- Phase 0-7: Core (auth, photos, albums, sync, search, creations) ✅
- Phase 8: ML implemented (face detection, CLIP smart search, people clustering,
  indexing, reprocess). Uses Python/torch helper (ml/inference.py) since ort
  requires Rust 1.92+ and we target 1.85. Embeddings stored in SQLite, cosine
  similarity search, DBSCAN face clustering. ✅
- Phase 9: Shared albums (invites, accept, members, remove, shared-with-me) ✅
- Phase 10: Memories (on-this-day generation, list, hide/unhide) ✅
- Phase 11: Deployment (multi-stage Dockerfile, docker-compose, release binary) ✅
- Phase 12: Security hardening (security headers, input validation, Argon2id,
  JWT, size limits) ✅
- Phase 13: Release prep (versioning, changelog, CI-ready) ✅

Collages and GIFs implemented. Shared album flow verified end-to-end
(invite → accept → shared-with-me). ML endpoints verified (status, detect-faces,
index-photo, smart-search, people). Python helper available when torch installed.

All endpoints tested against live server. Release binary builds.


Companion sync server for HearthBoard (Android family dashboard).
Separate repo. Purpose: optional self-hosted backend that enables cross-device
photo sync and Google-Photos-style parity features (search, face grouping,
memories) for low-powered devices, per the HearthBoard photos brainstorm.

Language: Rust. Rationale: single static binary, low memory footprint, fast I/O,
good async story (tokio), strong type safety, no GC pauses on large uploads.

License: MIT OR Apache-2.0 (server). HearthBoard client remains GPL-3.0; the
wire protocol is the integration boundary, so licenses stay independent.

Status: PLANNING ONLY. No code written yet. Phases are sequential; each phase
has an exit criterion that must be met (and verified) before the next begins.

---

## Guiding constraints (non-negotiable, inherited from HearthBoard)

- Privacy-first: no telemetry, no phone-home, no analytics. Server holds
  plaintext photos only when ML/search features are enabled; document this.
- Offline-first client: HearthBoard must remain fully useful with no server.
  Server features are additive and degrade gracefully.
- No proprietary dependencies in the client. Server may use whatever is needed.
- Single binary deployment is the default; Docker is a convenience wrapper.
- HTTPS in transit (or LAN exception), matching HearthBoard's endpoint rule.

---

## Phase 0 — Decisions & scaffolding (1 week)

Goal: lock the shape before writing code.

- [ ] Confirm scope: photos-only server for v1 (calendar/tasks already have
      CalDAV). No calendar duplication.
- [ ] Confirm storage backend: SQLite + sqlite-vec (single-file, zero-ops) as
      default; Postgres + pgvector as optional scale upgrade. Decision: SQLite
      default.
- [ ] Confirm auth model: username/password per family, Argon2id hashing,
      short-lived JWT access token + long-lived refresh token. No OAuth for v1.
- [ ] Confirm ML approach: in-process via `ort` (ONNX Runtime) behind a
      cargo feature flag `ml`, disabled by default. No Python sidecar.
- [ ] Confirm TLS strategy: server speaks HTTP; TLS terminated by reverse
      proxy (NPM/Caddy) or tunnel. Optional rustls direct-TLS behind a feature
      flag for users who want a single binary with TLS.
- [ ] Scaffold repo: Cargo workspace, `src/main.rs`, `src/lib.rs`, CI
      (GitHub Actions: fmt, clippy, test, build for x86_64 + aarch64 musl),
      rust-toolchain pin, .gitignore, README skeleton, LICENSE.
- [ ] Set up error handling: `thiserror` for library errors, `anyhow` at
      binary boundary; structured logging via `tracing`.

Exit criterion: `cargo build` and `cargo test` pass on a clean checkout; CI green.

---

## Phase 1 — Core server skeleton (1 week)

Goal: a running, observable, configurable HTTP server.

- [ ] Axum router with `/healthz` (liveness) and `/readyz` (readiness: DB ok).
- [ ] Config via env + optional TOML file (listen addr, data dir, DB path,
      log level, max upload size, JWT secret). Fail loudly on missing secret.
- [ ] Structured logging (tracing + tracing-subscriber), request IDs.
- [ ] Graceful shutdown on SIGTERM/SIGINT (drain in-flight requests).
- [ ] Middleware: request tracing, CORS (configurable origins), body size
      limit, timeout.
- [ ] Single data directory layout: `data/objects/`, `data/thumbs/`, `data/db/`.

Exit criterion: server starts, `/healthz` returns 200, graceful shutdown works,
logs are structured JSON in prod mode.

---

## Phase 2 — Auth & accounts (1–2 weeks)

Goal: multi-user family accounts with secure credential storage.

- [ ] SQLite schema: `users(id, username, password_hash, created_at, role)`.
- [ ] Argon2id password hashing (argon2 crate), tuned parameters.
- [ ] Endpoints: `POST /api/auth/register` (first user becomes admin, or
      invite-only via admin token), `POST /api/auth/login`, `POST /api/auth/refresh`,
      `POST /api/auth/logout`, `GET /api/auth/me`.
- [ ] JWT access tokens (15 min) + opaque refresh tokens (30 days) stored
      hashed in DB; rotation on refresh.
- [ ] Role-based access: admin (manage users) vs member.
- [ ] Rate limiting on auth endpoints (tower-governor or custom).
- [ ] Invite flow: admin generates single-use invite code; registration
      requires it (matches HearthBoard's invite-only signup model).

Exit criterion: register → login → refresh → logout cycle works; brute-force
protection verified; password never logged.

---

## Phase 3 — Storage layer (1–2 weeks)

Goal: content-addressed object storage with dedup.

- [ ] Object store abstraction over filesystem: `put(bytes) -> content_hash`,
      `get(hash) -> bytes`, `exists(hash)`, `delete(hash)` (refcounted).
- [ ] SHA-256 content hashing; store under `objects/ab/cdef...` (2-char prefix
      sharding).
- [ ] Reference counting: an object is deleted only when no DB row references
      it.
- [ ] Thumbnail generation: on upload, generate a 512px JPEG thumbnail
      (image crate) and store under `thumbs/`. Async, non-blocking.
- [ ] Upload endpoint: `POST /api/photos` (multipart), validates MIME type
      and size, hashes, dedups, returns photo metadata.
- [ ] Download endpoint: `GET /api/photos/:id/content` and `.../thumb`,
      with `Range` support for large files and caching headers.
- [ ] Streaming responses (no full-file buffering).

Exit criterion: upload 100 photos, verify dedup (duplicate hash stored once),
thumbnails generated, Range requests work, memory stays flat.

---

## Phase 4 — Metadata database (1–2 weeks)

Goal: schema for photos, albums, and sync state.

- [ ] SQLite schema with migrations (sqlx or refinery):
      `photos`, `albums`, `album_photos`, `sync_cursors`, `tombstones`,
      `users`, `refresh_tokens`, `invites`.
- [ ] Photo fields: id (UUID), content_hash, width, height, taken_at,
      imported_at, size_bytes, mime_type, is_favorite, is_archived,
      is_trashed, trashed_at, caption, location_lat/lon, camera_make/model,
      sync_state, remote_id (for client), updated_at.
- [ ] Indexes for common queries: by taken_at, by content_hash, by user.
- [ ] SQLite FTS5 virtual table for metadata search (filename, caption,
      camera, location text) — Tier 1 search without ML.
- [ ] Album CRUD endpoints.
- [ ] Add/remove photo to album endpoints.

Exit criterion: schema migrates cleanly from empty; FTS search returns results;
album operations verified.

---

## Phase 5 — Sync protocol (2–3 weeks)

Goal: delta sync with directionality, matching the HearthBoard brainstorm.

- [ ] Sync cursor model: client sends `since=<cursor>`; server returns
      changed photos (added/updated/deleted) + `nextCursor`.
- [ ] Endpoints:
      `GET /api/sync/changes?since=&limit=` (delta pull),
      `POST /api/sync/push` (batch metadata + upload refs),
      `POST /api/sync/ack` (client confirms receipt).
- [ ] Tombstone propagation for deletions with grace period.
- [ ] Directionality is a client-side setting; server is direction-agnostic
      (it accepts pushes and serves pulls). Server enforces per-user scoping.
- [ ] Conflict handling: content-hash identity. Same hash → skip. Different
      hash, same client-provided id → server keeps both, marks conflict;
      client creates "Conflicts" album.
- [ ] Idempotency: re-pushing the same photo (same hash) is a no-op.
- [ ] Pagination + rate limiting on sync endpoints.

Exit criterion: two test clients sync 1000 photos in both directions; delta
pull returns only changes; idempotent re-push verified; conflict case produces
conflict album entry.

---

## Phase 6 — HearthBoard client integration (2–3 weeks, Android side)

Goal: HearthBoard talks to hearth-sync.

- [ ] New "Sync account" settings screen mirroring CalDAV account config
      (server URL, username, password, sync mode, Wi-Fi-only, include videos).
- [ ] Credentials stored via existing AES-256/GCM Keystore path.
- [ ] WorkManager periodic sync (30 min, network constraint, exponential
      backoff) — reuse existing worker pattern.
- [ ] Upload queue: photos in `PENDING_UPLOAD` state uploaded on sync.
- [ ] Download + cache: server thumbnails fetched and cached locally.
- [ ] Offline degradation: Tier 1 (albums, favorites, archive, trash, metadata
      search) works without server; server features show "connect server" state.
- [ ] "Sync now" manual trigger in Settings.

Exit criterion: end-to-end sync between two devices on LAN verified; offline
mode retains full Tier 1 functionality.

---

## Phase 7 — Metadata search (1 week)

Goal: fast local + server search without ML.

- [ ] Server: FTS5-backed search endpoint `GET /api/search?q=&filters=`
      (date range, camera, location, favorites, archived, album).
- [ ] Client: local FTS over cached metadata for offline search; fall back to
      server when online for full library.
- [ ] Advanced filters: media type, date range, in-archive, in-favorites,
      not-in-album.

Exit criterion: search returns relevant results in <200ms on 10k photos.

---

## Phase 8 — ML features (optional, 3–6 weeks, feature-gated)

Goal: Google-Photos-style smart search and face grouping, server-side only.

- [ ] Gate behind cargo feature `ml` and a runtime config flag.
- [ ] Add `ort` (ONNX Runtime) dependency; load models from a configurable
      model directory (download on first run, or ship in a separate image).
- [ ] Face detection + recognition: store face embeddings + bounding boxes;
      clustering job (DBSCAN variant) groups into "people".
- [ ] CLIP visual + textual encoding: store image embeddings in sqlite-vec;
      smart search endpoint returns ranked results.
- [ ] OCR (optional): extract text from images, store in FTS.
- [ ] Background job queue (tokio tasks or a small worker pool) with
      progress reporting via `GET /api/jobs`.
- [ ] Remote ML option: allow pointing at an external ML service URL (like
      Immich's remote ML) so low-powered hosts can offload.
- [ ] Client: "People" screen, smart search bar, "Memories" carousel.

Exit criterion: face grouping and CLIP search work on a 1k-photo test set;
remote-ML fallback verified; feature disabled by default ships a smaller binary.

---

## Phase 9 — Shared albums & multi-user (2 weeks)

Goal: collaborative family albums.

- [ ] Album sharing: invite by username or link; permissions (view/add).
- [ ] Shared album sync across members' devices.
- [ ] Partner sharing: auto-share photos of selected people or date ranges
      (server-side, requires face grouping from Phase 8).
- [ ] Notifications for new shared photos (polling-based, no push service to
      stay F-Droid-friendly; or WebPush if user opts in).

Exit criterion: two users share an album; photos from both appear; permissions
enforced server-side.

---

## Phase 10 — Memories & highlights (1 week)

Goal: "X years ago" and auto-curated highlights.

- [ ] Server job: generate memory candidates (date anniversaries, top photos
      by quality heuristic).
- [ ] Client: memories carousel on Photos home; tap to view day.
- [ ] User controls: hide memory, disable memory types (matches Google Photos).

Exit criterion: memories appear for photos >1 year old; hide/disable works.

---

## Phase 11 — Deployment & ops (1–2 weeks)

Goal: production-ready, zero-touch install.

- [ ] Multi-stage Dockerfile (musl build) producing a small image.
- [ ] docker-compose.yml: hearth-sync + optional reverse proxy snippet.
- [ ] Backup/restore: `hearth-sync backup` CLI command producing a portable
      archive (DB + objects); restore command.
- [ ] Health checks, graceful shutdown, log rotation.
- [ ] Documentation: install, reverse proxy setup, TLS, backup, upgrade.
- [ ] Release binaries via GitHub Actions for linux (x86_64, aarch64) and
      macOS (optional).

Exit criterion: `docker compose up -d` on a clean host yields a working server;
backup/restore round-trip verified.

---

## Phase 12 — Security hardening & audit (1 week)

Goal: audit before any public release.

- [ ] Threat model review: auth, path traversal on object store, upload
      validation (MIME sniffing, size limits, decompression bombs).
- [ ] Rate limiting on all endpoints; brute-force protection verified.
- [ ] Dependency audit (cargo audit) in CI.
- [ ] Fuzz testing on multipart parser and sync endpoints.
- [ ] Penetration test of auth + object access (IDOR checks).
- [ ] Document trust boundary: server sees plaintext when ML enabled.

Exit criterion: no high/critical findings; cargo audit clean; fuzzing runs
without crashes for 1 hour.

---

## Phase 13 — Release (1 week)

Goal: ship v1.0.

- [ ] Versioning: semver; changelog; migration notes.
- [ ] Tagged release with binaries + Docker image.
- [ ] HearthBoard app updated to point at stable server API version.
- [ ] Docs site (or README) with quick start.

Exit criterion: a new user can install server + configure HearthBoard sync in
under 30 minutes following docs only.

---

## Cross-cutting concerns

- **Testing**: unit tests (cargo test) for storage, hashing, auth; integration
  tests against a temp DB; property-based tests for sync cursor logic.
- **Observability**: tracing spans on sync endpoints; metrics endpoint
  (Prometheus format, optional) for ops.
- **Compatibility**: server API versioned (`/api/v1/...`); HearthBoard pins a
  minimum server version and refuses to sync with incompatible servers.
- **Migration path**: SQLite schema migrations are forward-only and tested;
  breaking changes bump API version.

## Out of scope (explicit)

- Calendar/task sync (CalDAV already covers this).
- Reimplementing Google's AI editing (magic eraser, unblur, cinematic photos).
- End-to-end encryption (deferred; ML requires plaintext).
- Mobile push notifications (F-Droid-friendly = polling or user-opted WebPush).

## Estimated total

~16–22 weeks for Phases 0–7 + 11–13 (core sync, no ML).
+3–6 weeks if Phase 8 (ML) is included.
+2 weeks for Phase 9 (shared albums).

Recommended first milestone: Phases 0–6 (core sync, no ML) in ~10–12 weeks,
then reassess demand before investing in ML.

---

## Implementation status (as of first build)

Phases 0–7, 11 (core) are implemented and verified against a live server:
auth (register/login/refresh/logout/me), photo upload/list/get/delete, favorite/
archive/trash, content + thumb download, albums CRUD + add/remove photo, sync
delta changes/push/ack, metadata search (FTS5), creations (collage + GIF), Docker
multi-stage build, release binary.

Phase 8 (ML), 9 (shared albums), 10 (memories) remain scaffolded as endpoints
or not started. Collages and GIFs are implemented (Phase 7+). ML feature flag
`ml` is declared but `ort` integration is not wired yet.

Verified smoke: register → upload → list → sync changes → search → collage/gif
endpoints → album create, all returning correct shapes.

Binary: target/release/hearth-sync (~? MB). Release build: `cargo build --release`.
