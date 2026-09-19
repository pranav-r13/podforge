use std::path::Path;

use tauri::State;

use crate::db::models::{Album, Track};
use crate::db::{queries, DbState};
use crate::import::folder_scan;
use crate::metadata::ipod_fix::{self, FixReport};
use crate::metadata::tags::{self, TagPatch};

#[tauri::command]
pub fn scan_folder(state: State<DbState>, path: String) -> Result<Vec<Album>, String> {
    let scanned = folder_scan::scan_folder(Path::new(&path)).map_err(|e| e.to_string())?;

    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut inserted_ids = Vec::new();

    for album in &scanned {
        if queries::album_exists_for_path(&conn, &album.source_path).map_err(|e| e.to_string())? {
            continue; // already imported, skip to avoid duplicates on rescan
        }
        let id = queries::insert_scanned_album(&mut conn, album).map_err(|e| e.to_string())?;
        inserted_ids.push(id);
    }

    inserted_ids
        .into_iter()
        .map(|id| queries::get_album(&conn, id).map_err(|e| e.to_string()))
        .collect()
}

#[tauri::command]
pub fn get_library(state: State<DbState>) -> Result<Vec<Album>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::get_library(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_album(state: State<DbState>, album_id: i64) -> Result<Album, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    queries::get_album(&conn, album_id).map_err(|e| e.to_string())
}

/// Writes `patch` to a track's file tags and mirrors the track-level fields
/// (title/artist/track#/disc#) into the DB.
#[tauri::command]
pub fn update_track_tags(state: State<DbState>, track_id: i64, patch: TagPatch) -> Result<Track, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    apply_patch(&conn, track_id, &patch)?;
    queries::get_track(&conn, track_id).map_err(|e| e.to_string())
}

/// Applies the same shared-field patch (album/album_artist/year/genre, plus
/// any track-level fields given) to every track in `track_ids`.
#[tauri::command]
pub fn bulk_update_tags(state: State<DbState>, track_ids: Vec<i64>, patch: TagPatch) -> Result<Vec<Track>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    for track_id in &track_ids {
        apply_patch(&conn, *track_id, &patch)?;
    }
    track_ids
        .into_iter()
        .map(|id| queries::get_track(&conn, id).map_err(|e| e.to_string()))
        .collect()
}

fn apply_patch(conn: &rusqlite::Connection, track_id: i64, patch: &TagPatch) -> Result<(), String> {
    let track = queries::get_track(conn, track_id).map_err(|e| e.to_string())?;
    let path = Path::new(&track.source_path);
    tags::write_tags(path, patch).map_err(|e| e.to_string())?;
    queries::update_track_fields(conn, track_id, patch).map_err(|e| e.to_string())?;
    if patch.album.is_some() || patch.album_artist.is_some() || patch.year.is_some() || patch.genre.is_some() {
        queries::update_album_fields(conn, track.album_id, patch).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn apply_ipod_compat_fix(state: State<DbState>, album_id: i64) -> Result<FixReport, String> {
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    ipod_fix::apply_ipod_compat_fix(&mut conn, album_id).map_err(|e| e.to_string())
}
