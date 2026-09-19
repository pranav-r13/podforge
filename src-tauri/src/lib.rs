mod commands;
mod db;
mod import;
mod metadata;

use std::sync::Mutex;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let conn = db::init(&app_data_dir);
            app.manage(db::DbState(Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_folder,
            commands::get_library,
            commands::get_album,
            commands::update_track_tags,
            commands::bulk_update_tags,
            commands::apply_ipod_compat_fix,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
