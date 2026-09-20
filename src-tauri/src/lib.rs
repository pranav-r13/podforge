mod bin_path;
mod commands;
mod convert;
mod db;
mod import;
mod metadata;

use std::sync::Mutex;

use tauri::Manager;

use convert::job_queue::JobQueue;

/// Keeps the file-appender's flush guard alive for the process lifetime --
/// dropping it early would stop log lines from reaching disk.
struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

fn init_logging(log_dir: &std::path::Path) -> LogGuard {
    std::fs::create_dir_all(log_dir).expect("failed to create log dir");
    let file_appender = tracing_appender::rolling::daily(log_dir, "podforge.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    LogGuard(guard)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let log_dir = app
                .path()
                .app_log_dir()
                .expect("failed to resolve log dir");
            let log_guard = init_logging(&log_dir);
            app.manage(log_guard);
            tracing::info!("Podforge starting");

            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let conn = db::init(&app_data_dir);
            let concurrency = commands::load_settings(&conn).concurrency.max(1) as usize;
            app.manage(db::DbState(Mutex::new(conn)));
            app.manage(JobQueue::new(concurrency));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_folder,
            commands::detect_cd,
            commands::lookup_cd_release,
            commands::rip_and_import_cd,
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
            commands::clear_finished_jobs,
            commands::get_settings,
            commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
