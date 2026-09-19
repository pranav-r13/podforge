use rusqlite::{params, Connection};

use super::models::{Album, ScannedAlbum, Track};

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
