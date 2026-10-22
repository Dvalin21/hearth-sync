# hearth-sync

Optional self-hosted sync server for [HearthBoard](https://github.com/Dvalin21/Hearthboard) —
an Android family dashboard with photos, checklists, meals, tasks, rewards and more.
Get the app from [HearthBoard releases](https://github.com/Dvalin21/Hearthboard/releases).

Enables cross-device photo sync, shared albums, search, memories, and
Google-Photos-style creations (collages, GIFs).

## Quick start

Docker (image published on Docker Hub):

```bash
docker run -d --name hearth-sync \
  -p 8080:8080 \
  -v hearth-sync-data:/data \
  -e HEARTH_SYNC_JWT_SECRET="$(openssl rand -hex 32)" \
  dvalin21/hearth-sync:latest
```

From source:

```bash
export HEARTH_SYNC_JWT_SECRET="$(openssl rand -hex 32)"
cargo run --release
```

Server listens on `:8080`. Point HearthBoard's sync settings at `http://<host>:8080`
(LAN) or `https://photos.yourdomain.com` (behind a reverse proxy).

## Configuration

| Env var | Default | Purpose |
|---|---|---|
| `HEARTH_SYNC_LISTEN` | `0.0.0.0:8080` | Listen address |
| `HEARTH_SYNC_DATA_DIR` | `./data` | SQLite database + photo blobs + thumbnails |
| `HEARTH_SYNC_JWT_SECRET` | *(required)* | Secret for access-token signing |
| `HEARTH_SYNC_DB` | `hearth.db` | Database file name |
| `HEARTH_SYNC_MAX_UPLOAD_MB` | `100` | Upload size cap |
| `HEARTH_SYNC_LOG` | `info` | Log level |
| `HEARTH_SYNC_CORS_ORIGINS` | `*` | CORS allowlist |

## ML features (optional)

Smart search (CLIP) and face detection ship enabled in the Docker image
(not enabled in plain `cargo build`; build with `--features ml`).
To activate:

```bash
bash scripts/download-models.sh            # prints recommended model sources
```

Place `clip_visual.onnx` and `face_detection.onnx` in the model dir
(`HEARTH_SYNC_MODEL_DIR`, default `./data/models`; `/data/models` in Docker), or run with the
Python helper (`ml/inference.py`, needs `torch pillow numpy`). The ML endpoints
report availability — check `GET /api/ml/status` — and return a clear error
when models are missing.

## API

All endpoints under `/api/`. Auth via `Authorization: Bearer <token>` after
`POST /api/auth/login`.

| Route | Method | Description |
|---|---|---|
| `/healthz` | GET | Liveness |
| `/readyz` | GET | Readiness (DB check) |
| `/api/auth/register` | POST | Register (first user = admin) |
| `/api/auth/login` | POST | Login |
| `/api/auth/refresh` | POST | Refresh token |
| `/api/auth/logout` | POST | Logout |
| `/api/auth/me` | GET | Current user |
| `/api/auth/invites` | POST | Create invite |
| `/api/photos` | GET/POST | List / upload |
| `/api/photos/:id` | GET/DELETE | Get / delete |
| `/api/photos/:id/content` | GET | Download full image |
| `/api/photos/:id/thumb` | GET | Download thumbnail |
| `/api/photos/:id/favorite` | PUT | Toggle favorite |
| `/api/photos/:id/archive` | PUT | Toggle archive |
| `/api/photos/:id/trash` | PUT | Toggle trash |
| `/api/albums` | GET/POST | List / create |
| `/api/albums/:id` | GET/PUT/DELETE | Get / rename / delete |
| `/api/albums/:id/photos` | POST/DELETE | Add / remove photo |
| `/api/albums/:id/invite` | POST | Create invite |
| `/api/albums/invites/accept` | POST | Accept invite |
| `/api/albums/:id/members` | GET | List members |
| `/api/albums/:id/members/:user_id` | DELETE | Remove member |
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
| `/api/ml/models` | GET | List ML models |
| `/api/ml/smart-search` | POST | CLIP search |
| `/api/ml/people` | GET | Face groups |
| `/api/ml/reprocess` | POST | Reprocess photo for ML |
| `/api/ml/detect-faces` | POST | Detect faces |
| `/api/ml/index-photo` | POST | Index photo for search |

## License

MIT OR Apache-2.0
