//! Promotion failures never silently enter fresh Cargo planning.

use super::*;

struct Fixture(PathBuf);
use std::path::PathBuf;
impl Fixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = env::temp_dir().join(format!(
            "velnor-dispatch-plan-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("temp directory");
        Self(path)
    }
    fn request(&self) -> PathBuf {
        self.0.join("plan-v1-request.json")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("fixture cleanup");
    }
}

#[test]
fn invalid_mode_never_enters_fresh_planner() {
    let fixture = Fixture::new();
    let error = plan_response_with_mode(&fixture.request(), "{}", &fixture.0, Some("invalid"))
        .expect_err("invalid mode");
    assert!(error.to_string().contains("early_output_invalid"));
}

#[test]
fn missing_ready_response_is_error_without_fallback() {
    let fixture = Fixture::new();
    let error = plan_response_with_mode(&fixture.request(), "{}", &fixture.0, Some("false"))
        .expect_err("missing ready response");
    assert!(error.to_string().contains("early_response_read"), "{error}");
    assert!(!fixture.0.join("velnor").exists());
}

#[test]
fn malformed_ready_response_never_enters_fresh_planner() {
    let fixture = Fixture::new();
    let sibling = response_path_for(&fixture.request()).expect("response path");
    fs::write(&sibling, "{").expect("malformed fixture");
    assert!(plan_response_with_mode(&fixture.request(), "{}", &fixture.0, Some("false")).is_err());
    assert_eq!(fs::read_to_string(&sibling).expect("unchanged bytes"), "{");
    assert!(!fixture.0.join("velnor").exists());
}

#[test]
fn oversized_ready_response_is_rejected_before_request_decode() {
    let fixture = Fixture::new();
    let sibling = response_path_for(&fixture.request()).expect("response path");
    fs::write(&sibling, vec![b' '; 8 * 1024 * 1024 + 1]).expect("oversize fixture");
    let error = plan_response_with_mode(&fixture.request(), "{}", &fixture.0, Some("false"))
        .expect_err("bounded read");
    assert!(error.to_string().contains("early_response_read"), "{error}");
}

#[test]
#[cfg(unix)]
fn ready_symlink_is_rejected_without_reading_or_fallback() {
    let fixture = Fixture::new();
    let target = fixture.0.join("target.json");
    fs::write(&target, "{}").expect("target");
    let sibling = response_path_for(&fixture.request()).expect("response path");
    std::os::unix::fs::symlink(&target, &sibling).expect("symlink");
    let error = plan_response_with_mode(&fixture.request(), "{}", &fixture.0, Some("false"))
        .expect_err("symlink rejection");
    assert!(error.to_string().contains("early_response_read"), "{error}");
    assert_eq!(fs::read_to_string(target).expect("target unchanged"), "{}");
}

#[test]
fn early_stage_is_exclusive_and_preserves_existing_bytes() {
    let fixture = Fixture::new();
    let sibling = response_path_for(&fixture.request()).expect("response path");
    stage_early_response(&sibling, "first").expect("first stage");
    assert!(stage_early_response(&sibling, "second").is_err());
    assert_eq!(fs::read_to_string(sibling).expect("first bytes"), "first");
}

#[test]
#[cfg(unix)]
fn early_stage_refuses_symlink_without_writing_target() {
    let fixture = Fixture::new();
    let target = fixture.0.join("target.json");
    fs::write(&target, "original").expect("target");
    let sibling = response_path_for(&fixture.request()).expect("response path");
    std::os::unix::fs::symlink(&target, &sibling).expect("symlink");
    assert!(stage_early_response(&sibling, "replacement").is_err());
    assert_eq!(
        fs::read_to_string(target).expect("target bytes"),
        "original"
    );
}
