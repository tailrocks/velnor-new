use super::read_text;
use crate::CheckDeadline;
use std::path::PathBuf;
use std::time::Duration;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("velnor-check-read-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&path).expect("temp directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("velnor_fixture_cleanup_failed:{:?}", error.kind());
        }
    }
}

#[test]
fn exact_byte_cap_is_accepted_and_one_extra_byte_refused() {
    let temp = TestDirectory::new();
    let path = temp.0.join("projection.toml");
    std::fs::write(&path, b"1234").expect("write");
    assert_eq!(
        read_text(&path, 4, None, "projection").expect("exact bound"),
        "1234"
    );
    std::fs::write(&path, b"12345").expect("oversize");
    assert!(read_text(&path, 4, None, "projection").is_err());
}

#[test]
fn expired_shared_deadline_stops_projection_read() {
    let temp = TestDirectory::new();
    let path = temp.0.join("projection.toml");
    std::fs::write(&path, b"[tasks.x]\nrun='true'\n").expect("write");
    let deadline = CheckDeadline::after(Duration::ZERO).expect("deadline");
    assert!(read_text(&path, 1024, Some(deadline), "projection").is_err());
}
