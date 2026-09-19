mod commands;
mod convert;
mod db;
mod import;
mod metadata;

use std::sync::Mutex;

use tauri::Manager;

use convert::job_queue::JobQueue;

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
            app.manage(JobQueue::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_folder,
            commands::get_library,
            commands::get_album,
            commands::update_track_tags,
            commands::bulk_update_tags,
            commands::apply_ipod_compat_fix,
            commands::lookup_musicbrainz,
            commands::apply_musicbrainz_match,
            commands::set_cover_art,
            commands::enqueue_conversion,
            commands::get_job_status,
            commands::list_jobs,
            commands::cancel_job,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
