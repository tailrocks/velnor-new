use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    cargo_artifact_executable, validate_qualification_manifest,
    validate_qualification_output_bindings,
};

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

fn write_manifest(path: &Path) {
    fs::write(
        path,
        r#"{"schema":1,"version":"0.1.52","repository":"tailrocks/velnor-new","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","targets":[{"target":"x86_64-unknown-linux-gnu","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-unknown-linux-gnu","sha256":"1111111111111111111111111111111111111111111111111111111111111111"},{"target":"aarch64-apple-darwin","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-aarch64-apple-darwin","sha256":"2222222222222222222222222222222222222222222222222222222222222222"},{"target":"x86_64-apple-darwin","artifact":"https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-apple-darwin","sha256":"3333333333333333333333333333333333333333333333333333333333333333"}]}"#,
    )
    .expect("write manifest");
}

fn valid_workflow() -> &'static str {
    "      VELNOR_ASSET_SHA256: 1111111111111111111111111111111111111111111111111111111111111111\n      VELNOR_ASSET_URL: https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-unknown-linux-gnu\n      VELNOR_RELEASE_COMMIT: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"
}

#[test]
fn qualification_bindings_accept_exact_pairs_and_reject_duplicate_json_keys() {
    let scratch = Scratch::new();
    let manifest = scratch.path("manifest.json");
    write_manifest(&manifest);
    assert!(validate_qualification_manifest(&manifest).is_ok());
    let workflows = scratch.path(".github");
    fs::create_dir(&workflows).expect("create workflow directory");
    fs::write(workflows.join("ci.yml"), valid_workflow()).expect("write workflow");
    assert!(validate_qualification_output_bindings(&manifest, &workflows).is_ok());

    fs::write(&manifest, r#"{"commit":"a","commit":"b","targets":[]}"#)
        .expect("write duplicate-key manifest");
    assert!(
        validate_qualification_output_bindings(&manifest, &workflows)
            .expect_err("duplicate JSON key rejected")
            .contains("cannot read candidate manifest")
    );
}

#[test]
fn qualification_bindings_reject_digest_commit_scope_pair_and_unknown_url_faults() {
    let scratch = Scratch::new();
    let manifest = scratch.path("manifest.json");
    write_manifest(&manifest);
    let workflows = scratch.path(".github");
    fs::create_dir(&workflows).expect("create workflow directory");
    let original = valid_workflow();
    let cases = [
        ("wrong digest", original.replace("1111111111111111111111111111111111111111111111111111111111111111", "4444444444444444444444444444444444444444444444444444444444444444"), "wrong digest for target"),
        ("wrong commit", original.replace("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"), "wrong source commit"),
        ("wrong URL", original.replace("https://github.com/tailrocks/velnor-new/releases/download/v0.1.52/velnor-actions-0.1.52-x86_64-unknown-linux-gnu", "https://example.invalid/unknown"), "unknown release URL"),
        ("scope mismatch", original.replace("      VELNOR_ASSET_URL", "    VELNOR_ASSET_URL"), "binding fields use different scopes"),
        ("missing field", original.lines().filter(|line| !line.contains("VELNOR_RELEASE_COMMIT")).collect::<Vec<_>>().join("\n"), "incomplete binding"),
    ];
    for (name, contents, expected) in cases {
        fs::write(workflows.join("ci.yml"), contents).expect("write fault fixture");
        let error = validate_qualification_output_bindings(&manifest, &workflows).expect_err(name);
        assert!(error.contains(expected), "{name}: {error}");
    }
}
