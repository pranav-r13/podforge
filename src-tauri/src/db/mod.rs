pub mod models;
pub mod queries;

use rusqlite::Connection;
use std::sync::Mutex;

pub struct DbState(pub Mutex<Connection>);

refinery::embed_migrations!("migrations");

pub fn init(app_data_dir: &std::path::Path) -> Connection {
    std::fs::create_dir_all(app_data_dir).expect("failed to create app data dir");
    let db_path = app_data_dir.join("library.sqlite3");
    let mut conn = Connection::open(db_path).expect("failed to open sqlite database");
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrations::runner()
        .run(&mut conn)
        .expect("failed to run database migrations");
    conn
}
