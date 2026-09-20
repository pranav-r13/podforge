use rusqlite::{params, Connection};

use super::models::{Album, Job, PlannedTrack, ScannedAlbum, Track};
use crate::metadata::tags::TagPatch;

/// Inserts a scanned album and its tracks in a single transaction.
/// Returns the new album's id.
pub fn insert_scanned_album(conn: &mut Connection, album: &ScannedAlbum) -> rusqlite::Result<i64> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO albums (title, album_artist, year, genre, source_type, source_path)
         VALUES (?1, ?2, ?3, ?4, 'folder', ?5)",
        params![
            album.title,
            album.album_artist,
            album.year,
            album.genre,
            album.source_path,
        ],
    )?;
    let album_id = tx.last_insert_rowid();

    for track in &album.tracks {
        tx.execute(
            "INSERT INTO tracks (
                album_id, disc_number, track_number, title, artist,
                duration_ms, source_path, status, codec, container
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9)",
            params![
                album_id,
                track.disc_number,
                track.track_number,
                track.title,
                track.artist,
                track.duration_ms,
                track.source_path,
                track.codec,
                track.container,
            ],
        )?;
    }

    tx.commit()?;
    Ok(album_id)
}

/// Inserts a CD album header row. Tracks are inserted separately via
/// `insert_planned_track` once this call's id is known, since each track's
/// planned WAV path is namespaced by album id.
pub fn insert_cd_album(
    conn: &Connection,
    title: &str,
    album_artist: Option<&str>,
    device: &str,
    release_id: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO albums (title, album_artist, source_type, source_path, musicbrainz_release_id)
         VALUES (?1, ?2, 'cd', ?3, ?4)",
        params![title, album_artist, device, release_id],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Inserts one not-yet-ripped CD track row. `source_path` points at where
/// the WAV file will land once the rip job for it completes.
pub fn insert_planned_track(conn: &Connection, album_id: i64, track: &PlannedTrack) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO tracks (album_id, track_number, title, artist, duration_ms, source_path, status, codec, container)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 'pcm_s16le', 'wav')",
        params![
            album_id,
            track.track_number,
            track.title,
            track.artist,
            track.duration_ms,
            track.source_path,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn set_track_ripped(conn: &Connection, track_id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE tracks SET status = 'ripped' WHERE id = ?1", params![track_id])?;
    Ok(())
}

fn row_to_track(row: &rusqlite::Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id: row.get("id")?,
        album_id: row.get("album_id")?,
        disc_number: row.get("disc_number")?,
        track_number: row.get("track_number")?,
        title: row.get("title")?,
        artist: row.get("artist")?,
        duration_ms: row.get("duration_ms")?,
        source_path: row.get("source_path")?,
        output_path: row.get("output_path")?,
        status: row.get("status")?,
        musicbrainz_recording_id: row.get("musicbrainz_recording_id")?,
        codec: row.get("codec")?,
        container: row.get("container")?,
    })
}

pub fn get_tracks_for_album(conn: &Connection, album_id: i64) -> rusqlite::Result<Vec<Track>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM tracks WHERE album_id = ?1 ORDER BY disc_number, track_number, title",
    )?;
    let rows = stmt.query_map(params![album_id], row_to_track)?;
    rows.collect()
}

pub fn get_track(conn: &Connection, track_id: i64) -> rusqlite::Result<Track> {
    conn.query_row(
        "SELECT * FROM tracks WHERE id = ?1",
        params![track_id],
        row_to_track,
    )
}

/// Applies the track-level fields of `patch` (title/artist/track#/disc#) to
/// the DB row. Fields left `None` are untouched via `COALESCE`.
pub fn update_track_fields(conn: &Connection, track_id: i64, patch: &TagPatch) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tracks SET
            title = COALESCE(?1, title),
            artist = COALESCE(?2, artist),
            track_number = COALESCE(?3, track_number),
            disc_number = COALESCE(?4, disc_number)
         WHERE id = ?5",
        params![
            patch.title,
            patch.artist,
            patch.track_number,
            patch.disc_number,
            track_id,
        ],
    )?;
    Ok(())
}

/// Applies the album-level fields of `patch` (album/album_artist/year/genre)
/// to the album row. Fields left `None` are untouched via `COALESCE`.
pub fn update_album_fields(conn: &Connection, album_id: i64, patch: &TagPatch) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE albums SET
            title = COALESCE(?1, title),
            album_artist = COALESCE(?2, album_artist),
            year = COALESCE(?3, year),
            genre = COALESCE(?4, genre)
         WHERE id = ?5",
        params![patch.album, patch.album_artist, patch.year, patch.genre, album_id],
    )?;
    Ok(())
}

pub fn set_album_musicbrainz_release(conn: &Connection, album_id: i64, release_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE albums SET musicbrainz_release_id = ?1 WHERE id = ?2",
        params![release_id, album_id],
    )?;
    Ok(())
}

pub fn set_album_cover_art_path(conn: &Connection, album_id: i64, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE albums SET cover_art_path = ?1 WHERE id = ?2",
        params![path, album_id],
    )?;
    Ok(())
}

pub fn update_track_codec(conn: &Connection, track_id: i64, codec: &str, container: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tracks SET codec = ?1, container = ?2 WHERE id = ?3",
        params![codec, container, track_id],
    )?;
    Ok(())
}

pub fn set_track_output_path(conn: &Connection, track_id: i64, output_path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tracks SET output_path = ?1, status = 'converted' WHERE id = ?2",
        params![output_path, track_id],
    )?;
    Ok(())
}

fn row_to_job(row: &rusqlite::Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: row.get("id")?,
        job_type: row.get("type")?,
        track_id: row.get("track_id")?,
        album_id: row.get("album_id")?,
        status: row.get("status")?,
        progress: row.get("progress")?,
        error: row.get("error")?,
        created_at: row.get("created_at")?,
        completed_at: row.get("completed_at")?,
    })
}

/// Inserts a queued job row. Progress tracking (status/progress/error)
/// happens separately via `update_job_progress` as the background worker runs.
pub fn insert_job(conn: &Connection, job_type: &str, track_id: Option<i64>, album_id: Option<i64>) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO jobs (type, track_id, album_id, status, progress) VALUES (?1, ?2, ?3, 'queued', 0)",
        params![job_type, track_id, album_id],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Updates a job's live status/progress/error. Stamps `completed_at` once
/// the job reaches a terminal state (done/error/cancelled).
pub fn update_job_progress(conn: &Connection, job_id: i64, status: &str, progress: f64, error: Option<&str>) -> rusqlite::Result<()> {
    let terminal = matches!(status, "done" | "error" | "cancelled");
    conn.execute(
        "UPDATE jobs SET
            status = ?1,
            progress = ?2,
            error = ?3,
            completed_at = CASE WHEN ?4 THEN datetime('now') ELSE completed_at END
         WHERE id = ?5",
        params![status, progress, error, terminal, job_id],
    )?;
    Ok(())
}

pub fn get_job(conn: &Connection, job_id: i64) -> rusqlite::Result<Job> {
    conn.query_row("SELECT * FROM jobs WHERE id = ?1", params![job_id], row_to_job)
}

/// Removes every job in a terminal state (done/error/cancelled). Queued and
/// running jobs are left alone so an in-flight conversion can't vanish from
/// under the Jobs drawer.
pub fn clear_finished_jobs(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM jobs WHERE status IN ('done', 'error', 'cancelled')",
        [],
    )?;
    Ok(())
}

pub fn list_jobs(conn: &Connection, status: Option<&str>) -> rusqlite::Result<Vec<Job>> {
    match status {
        Some(status) => {
            let mut stmt = conn.prepare("SELECT * FROM jobs WHERE status = ?1 ORDER BY created_at DESC")?;
            let rows = stmt.query_map(params![status], row_to_job)?;
            rows.collect()
        }
        None => {
            let mut stmt = conn.prepare("SELECT * FROM jobs ORDER BY created_at DESC")?;
            let rows = stmt.query_map([], row_to_job)?;
            rows.collect()
        }
    }
}

fn row_to_album(conn: &Connection, row: &rusqlite::Row) -> rusqlite::Result<Album> {
    let id: i64 = row.get("id")?;
    Ok(Album {
        id,
        title: row.get("title")?,
        album_artist: row.get("album_artist")?,
        year: row.get("year")?,
        genre: row.get("genre")?,
        musicbrainz_release_id: row.get("musicbrainz_release_id")?,
        cover_art_path: row.get("cover_art_path")?,
        source_type: row.get("source_type")?,
        source_path: row.get("source_path")?,
        created_at: row.get("created_at")?,
        tracks: get_tracks_for_album(conn, id)?,
    })
}

pub fn get_library(conn: &Connection) -> rusqlite::Result<Vec<Album>> {
    let mut stmt = conn.prepare("SELECT * FROM albums ORDER BY album_artist, year, title")?;
    let rows = stmt.query_map([], |row| row_to_album(conn, row))?;
    rows.collect()
}

pub fn get_album(conn: &Connection, album_id: i64) -> rusqlite::Result<Album> {
    let mut stmt = conn.prepare("SELECT * FROM albums WHERE id = ?1")?;
    stmt.query_row(params![album_id], |row| row_to_album(conn, row))
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0))
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
}

/// Upserts one setting. `settings` has no default rows, so every key is
/// either absent (caller falls back to a hardcoded default) or explicitly set.
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Returns true if an album with this exact source_path is already imported,
/// so re-scanning a folder doesn't create duplicates.
pub fn album_exists_for_path(conn: &Connection, source_path: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM albums WHERE source_path = ?1",
        params![source_path],
        |_| Ok(()),
    )
    .map(|_| true)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(false),
        other => Err(other),
    })
}
