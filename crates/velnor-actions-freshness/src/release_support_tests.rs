use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::cargo_artifact_executable;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        for _ in 0..32 {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "velnor-release-support-{}-{id}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create isolated test directory: {error}"),
            }
        }
        panic!("could not allocate isolated test directory after 32 attempts")
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("make executable");
}

#[test]
fn cargo_artifact_selection_requires_one_matching_executable_record() {
    let scratch = Scratch::new();
    let manifest = scratch.path("Cargo.toml");
    fs::write(&manifest, "[package]\n").expect("write manifest");
    let executable = scratch.path("velnor-actions");
    fs::write(&executable, b"binary").expect("write executable");
    #[cfg(unix)]
    make_executable(&executable);
    let manifest = fs::canonicalize(manifest).expect("canonical manifest");
    let executable = fs::canonicalize(executable).expect("canonical executable");
    let record = format!(
        r#"{{"reason":"compiler-artifact","manifest_path":{},"target":{{"name":"velnor-actions","kind":["bin"]}},"executable":{}}}"#,
        serde_json::to_string(&manifest).expect("encode manifest path"),
        serde_json::to_string(&executable).expect("encode executable path"),
    );
    let messages = scratch.path("messages.jsonl");
    fs::write(
        &messages,
        format!("{{\"reason\":\"build-finished\"}}\n{record}\n"),
    )
    .expect("write Cargo messages");

    let selected = cargo_artifact_executable(&scratch.0, &messages, &manifest, "velnor-actions")
        .expect("select exact executable");
    assert_eq!(selected, executable);
}

#[test]
fn cargo_artifact_selection_rejects_missing_and_ambiguous_records() {
    let scratch = Scratch::new();
    let manifest = scratch.path("Cargo.toml");
    fs::write(&manifest, "[package]\n").expect("write manifest");
    let manifest = fs::canonicalize(manifest).expect("canonical manifest");
    let missing = scratch.path("missing.jsonl");
    fs::write(&missing, "{\"reason\":\"build-finished\"}\n").expect("write messages");
    assert!(
        cargo_artifact_executable(&scratch.0, &missing, &manifest, "velnor-actions")
            .expect_err("missing artifact rejected")
            .contains("found 0")
    );

    let executable = scratch.path("velnor-actions");
    fs::write(&executable, b"binary").expect("write executable");
    #[cfg(unix)]
    make_executable(&executable);
    let executable = fs::canonicalize(executable).expect("canonical executable");
    let record = format!(
        r#"{{"reason":"compiler-artifact","manifest_path":{},"target":{{"name":"velnor-actions","kind":["bin"]}},"executable":{}}}"#,
        serde_json::to_string(&manifest).expect("encode manifest path"),
        serde_json::to_string(&executable).expect("encode executable path"),
    );
    let ambiguous = scratch.path("ambiguous.jsonl");
    fs::write(&ambiguous, format!("{record}\n{record}\n")).expect("write messages");
    assert!(
        cargo_artifact_executable(&scratch.0, &ambiguous, &manifest, "velnor-actions")
            .expect_err("duplicate artifact rejected")
            .contains("found 2")
    );
}
