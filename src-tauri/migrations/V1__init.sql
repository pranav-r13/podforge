CREATE TABLE albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    album_artist TEXT,
    year INTEGER,
    genre TEXT,
    musicbrainz_release_id TEXT,
    cover_art_path TEXT,
    source_type TEXT NOT NULL DEFAULT 'folder',
    source_path TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    album_id INTEGER NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    disc_number INTEGER,
    track_number INTEGER,
    title TEXT NOT NULL,
    artist TEXT,
    duration_ms INTEGER,
    source_path TEXT NOT NULL,
    output_path TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    musicbrainz_recording_id TEXT,
    codec TEXT,
    container TEXT
);

CREATE INDEX idx_tracks_album_id ON tracks(album_id);

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,
    track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
    album_id INTEGER REFERENCES albums(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued',
    progress REAL NOT NULL DEFAULT 0,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT
);
