CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'member',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS invites (
    code TEXT PRIMARY KEY,
    created_by TEXT NOT NULL REFERENCES users(id),
    used_by TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER
);

CREATE TABLE IF NOT EXISTS photos (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    width INTEGER NOT NULL DEFAULT 0,
    height INTEGER NOT NULL DEFAULT 0,
    taken_at INTEGER,
    imported_at INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    mime_type TEXT NOT NULL DEFAULT 'image/jpeg',
    is_favorite INTEGER NOT NULL DEFAULT 0,
    is_archived INTEGER NOT NULL DEFAULT 0,
    is_trashed INTEGER NOT NULL DEFAULT 0,
    trashed_at INTEGER,
    caption TEXT NOT NULL DEFAULT '',
    location_lat REAL,
    location_lon REAL,
    camera_make TEXT NOT NULL DEFAULT '',
    camera_model TEXT NOT NULL DEFAULT '',
    sync_state TEXT NOT NULL DEFAULT 'synced',
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_photos_user ON photos(user_id);
CREATE INDEX IF NOT EXISTS idx_photos_hash ON photos(content_hash);
CREATE INDEX IF NOT EXISTS idx_photos_taken ON photos(taken_at);

CREATE VIRTUAL TABLE IF NOT EXISTS photos_fts USING fts5(
    photo_id UNINDEXED,
    caption,
    camera_make,
    camera_model
);

CREATE TABLE IF NOT EXISTS albums (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    cover_photo_id TEXT
);

CREATE TABLE IF NOT EXISTS album_photos (
    album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    photo_id TEXT NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (album_id, photo_id)
);

CREATE TABLE IF NOT EXISTS sync_cursors (
    user_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    cursor INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, device_id)
);

CREATE TABLE IF NOT EXISTS tombstones (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    deleted_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS creations (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,            -- collage, gif, movie
    content_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    source_photo_ids TEXT NOT NULL  -- comma-joined
);

-- Phase 9: shared albums
CREATE TABLE IF NOT EXISTS album_members (
    album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    permission TEXT NOT NULL DEFAULT 'view',  -- view, add
    added_at INTEGER NOT NULL,
    PRIMARY KEY (album_id, user_id)
);

CREATE TABLE IF NOT EXISTS album_invites (
    code TEXT PRIMARY KEY,
    album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    created_by TEXT NOT NULL REFERENCES users(id),
    permission TEXT NOT NULL DEFAULT 'view',
    created_at INTEGER NOT NULL,
    expires_at INTEGER,
    used_by TEXT
);

-- Phase 10: memories
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,          -- on_this_day, highlight
    title TEXT NOT NULL DEFAULT '',
    photo_ids TEXT NOT NULL,     -- comma-joined
    created_at INTEGER NOT NULL,
    hidden INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_memories_user ON memories(user_id);

-- Phase 8: ML - face data
CREATE TABLE IF NOT EXISTS faces (
    id TEXT PRIMARY KEY,
    photo_id TEXT NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    bbox_x REAL NOT NULL,
    bbox_y REAL NOT NULL,
    bbox_w REAL NOT NULL,
    bbox_h REAL NOT NULL,
    embedding BLOB,
    person_id TEXT,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_faces_photo ON faces(photo_id);
CREATE INDEX IF NOT EXISTS idx_faces_user ON faces(user_id);

CREATE TABLE IF NOT EXISTS people (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    cover_face_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Phase 8: ML - smart search embeddings (stored as BLOB, queried via brute-force cosine for small sets)
CREATE TABLE IF NOT EXISTS photo_embeddings (
    photo_id TEXT PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,
    embedding BLOB NOT NULL,
    model TEXT NOT NULL DEFAULT 'clip-vit-b32',
    created_at INTEGER NOT NULL
);

-- Phase 12: rate limiting buckets (in-memory preferred, but persisted for restart survival)
CREATE TABLE IF NOT EXISTS rate_limit_buckets (
    key TEXT PRIMARY KEY,
    count INTEGER NOT NULL DEFAULT 0,
    window_start INTEGER NOT NULL
);

-- Phase 9: album sharing flag on albums
-- (added via migration below for existing DBs)
