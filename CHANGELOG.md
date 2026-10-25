# Changelog

## 1.2.0 (2026-10)
- Photo metadata: accept taken_at/location/camera multipart fields; backfill on dedup.
- Raise multipart body limit for photo uploads.
- Dockerfile: trixie-slim runtime for glibc compatibility with ort.
- Add ort ML inference, model management, rate limiting, security audit (cargo audit clean), fuzz targets.
- ML infrastructure, shared albums, memories, rate limiting, model management, Python inference helper.
- Security headers, input validation, Argon2id, JWT, size limits.

## 1.0.0 (2026-10)
- Initial release: auth, photos, albums, sync, search, creations, Docker.
