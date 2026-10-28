# hearth-sync — NEXT.md

Companion sync server for HearthBoard (Android family dashboard).
Language: Rust. License: MIT OR Apache-2.0.

Status: v1.2.1 — core feature-complete. This file records what is actually
verified, not what was intended.

## Verified working (integration + regression tests)

- Auth: register/login/refresh/logout/me, invites, admin role; Argon2id, JWT.
  Registration and invite redemption are one transaction — a failed attempt
  cannot burn an invite, and `used_by` records the real user id.
- Object store: SHA-256 content-addressed, sharded, refcounted delete.
- Thumbnails (512px JPEG), metadata backfill on dedup upload.
- Albums CRUD + add/remove + shared-album invites/members; `auto-share` by
  person/date range actually inserts rows.
- Sync: delta pull/push, tombstones, idempotent. The cursor is a per-user
  monotonic `sync_seq` (`photos.sync_seq` / `tombstones.sync_seq`, counter in
  the `sync_seq` table), seeded at `SYNC_SEQ_BASE = 1_000_000_000_000` so
  clients holding a legacy seconds-cursor resync once instead of stalling.
  `next_cursor` is the seq of the last change actually returned, so paginating
  cannot skip a change. Hard deletes write a tombstone and drop the FTS row.
- Search: FTS5 over captions/camera fields. User input is quoted and escaped
  before `MATCH`; punctuation-only terms are dropped instead of causing a 500.
- Creations (collage + GIF): image work runs on the blocking pool.
- Memories: "on this day" is a SQL predicate on `strftime('%m'/'%d'/'%Y')`,
  not a full-table scan.
- ML: status/models/smart-search/people/detect-faces/index-photo/ocr. Every
  `python3` call and image decode runs on the blocking pool. `/ml/reprocess`
  really enqueues one `ProcessPhoto` job per photo and reports the count.
- Notifications: only albums the caller is a member of (was an IDOR returning
  every photo in every shared album).
- Security: security headers, size limits, CORS from config, fail-loud JWT
  secret, request ids.
- Rate limiting: per-IP token bucket keyed on the TCP peer address from
  `ConnectInfo`; `X-Forwarded-For` is honoured only when the peer appears in
  `HEARTH_SYNC_TRUSTED_PROXIES`. Expired buckets are swept once the map exceeds
  1024 entries.
- Metrics: `/metrics` in Prometheus text format. `requests_total`,
  `photos_uploaded` (new rows only, not dedup hits) and `auth_failures` are
  incremented on real traffic.
- `--config path.toml`: the file is exported as `HEARTH_SYNC_*` env vars before
  clap binds the config, so it takes effect. Scalars only; nested tables are
  rejected. Precedence: CLI flag > file > default.
- Backup/restore CLI, graceful shutdown, structured JSON logging.

## Tests

- `tests/integration.rs` — 13 endpoint smoke tests.
- `tests/regressions.rs` — 11 tests, each pinning a specific defect that was
  reproduced before it was fixed (cursor loss, invite burn, notifications IDOR,
  auto-share no-op, FTS syntax 500, dead metrics, memories scan, fake reprocess
  queue, sync cursor, FK enforcement).
- `src/middleware.rs` — 4 unit tests for bucket-key derivation.
- Total: 28 tests, 0 failures.

## Known deviations (deliberate, documented, not bugs being hidden)

- **Rate limiting is per-process, in memory.** A shared/persistent store would
  add a write per request for a single-instance family server. Restarting the
  service resets the windows. `rate_limit_buckets` in `schema.sql` is unused.
- **EXIF capture is a stub.** `photos::extract_taken_at` returns `None`, so
  `taken_at` comes only from the client's multipart field. Real EXIF parsing
  needs a dependency (`kamadak-exif`); not added without asking.
- **ML worker is a scaffold.** `ml::run_worker` drains the queue and logs; it
  does not compute embeddings yet (that needs photo bytes, i.e. the object
  store, inside the worker).
- **Direct TLS serving is unwired.** Reverse proxy is the supported deployment.
  Note that the `tls` feature path does not attach `ConnectInfo`, so the rate
  limiter would see no peer address there.
- **`GET /api/photos` flag combinations are undefined.** `archived=false` is
  ignored when `trashed=true`; no client documents what the combination should
  mean, so it is left alone rather than guessed at.

## Outstanding production polish

- [ ] `cargo audit` clean (only the unmaintained `paste` warning allowed)
- [ ] Release binaries via GitHub Actions (x86_64, aarch64)
- [ ] Penetration test of auth + object access
- [ ] Fuzzing for an hour without crashes
- [ ] Build the image with `--features ml` in CI (the Dockerfile compiles that
      feature; the local test suite does not)
- [ ] Person identity: face clustering still assigns `person_<label>` from a
      fresh DBSCAN run, so identities churn between runs
