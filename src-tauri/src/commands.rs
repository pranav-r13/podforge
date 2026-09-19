use std::path::Path;

use tauri::State;

use crate::db::models::Album;
use crate::db::{queries, DbState};
use crate::import::folder_scan;

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
