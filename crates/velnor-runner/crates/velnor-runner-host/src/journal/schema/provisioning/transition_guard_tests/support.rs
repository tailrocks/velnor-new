use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-provisioning-transition-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    pub(super) fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) async fn open_database(path: &std::path::Path) -> (turso::Database, turso::Connection) {
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("open test database");
    let connection = database.connect().expect("connect test database");
    (database, connection)
}

pub(super) async fn scalar(connection: &turso::Connection, sql: &str) -> i64 {
    let mut rows = connection.query(sql, ()).await.expect("query");
    rows.next()
        .await
        .expect("read query")
        .expect("query row")
        .get(0)
        .expect("integer result")
}
