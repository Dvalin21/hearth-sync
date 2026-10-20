# hearth-sync

Optional self-hosted sync server for [HearthBoard](https://github.com/openlight/HearthBoard) (Android family dashboard).

Enables cross-device photo sync, shared albums, smart search (CLIP), face grouping,
memories, and Google-Photos-style creations (collages, GIFs).

## Quick start

```bash
# 1. Set a JWT secret
export HEARTH_SYNC_JWT_SECRET=$(openssl rand -hex 32)

# 2. Run
cargo run --release
# or
docker compose up -d
```

Server listens on `:8080`. Point HearthBoard's sync settings at `http://<host>:8080`
(LAN) or `https://photos.yourdomain.com` (behind a reverse proxy).

## ML features (optional)

Smart search and face detection require Python 3 with torch, pillow, numpy:

```bash
pip install torch pillow numpy
```

The server calls `ml/inference.py` as a helper. Place it next to the binary or
set `HEARTH_SYNC_MODEL_DIR`. ML endpoints return a clear error when unavailable.

## API

All endpoints under `/api/`. Auth via `Authorization: Bearer <token>`.

| Route | Method | Description |
|---|---|---|
| `/healthz` | GET | Liveness |
| `/readyz` | GET | Readiness (DB check) |
| `/api/auth/register` | POST | Register (first user = admin) |
| `/api/auth/login` | POST | Login |
| `/api/auth/refresh` | POST | Refresh token |
| `/api/auth/me` | GET | Current user |
| `/api/photos` | GET/POST | List / upload |
| `/api/photos/:id` | GET/DELETE | Get / delete |
| `/api/photos/:id/content` | GET | Download bytes |
| `/api/photos/:id/thumb` | GET | Thumbnail |
| `/api/photos/:id/favorite` | PUT | Toggle favorite |
| `/api/photos/:id/archive` | PUT | Toggle archive |
| `/api/photos/:id/trash` | PUT | Toggle trash |
| `/api/albums` | GET/POST | List / create |
| `/api/albums/:id` | GET/PUT/DELETE | Get / rename / delete |
| `/api/albums/:id/photos` | POST/DELETE | Add / remove photo |
| `/api/albums/:id/invite` | POST | Create invite |
| `/api/albums/invites/accept` | POST | Accept invite |
| `/api/albums/:id/members` | GET | List members |
| `/api/albums/shared` | GET | Shared with me |
| `/api/sync/changes` | GET | Delta pull |
| `/api/sync/push` | POST | Push changes |
| `/api/sync/ack` | POST | Acknowledge cursor |
| `/api/search` | GET | Metadata + smart search |
| `/api/memories` | GET | List memories |
| `/api/memories/:id/hide` | PUT | Hide/unhide memory |
| `/api/creations/collage` | POST | Create collage |
| `/api/creations/gif` | POST | Create GIF |
| `/api/creations` | GET | List creations |
| `/api/ml/status` | GET | ML availability |
| `/api/ml/smart-search` | POST | CLIP search |
| `/api/ml/people` | GET | Face groups |
| `/api/ml/detect-faces` | POST | Detect faces in photo |
| `/api/ml/index-photo` | POST | Index photo for search |

## License

MIT OR Apache-2.0
EOF
echo done