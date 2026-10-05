use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::mise_bootstrap_guard_script;

#[test]
fn verified_bootstrap_hit_and_corrupt_entries_follow_cold_repair() {
    let root = test_root();
    let binary = bootstrap_path(&root);
    fs::create_dir_all(binary.parent().expect("binary parent")).expect("create cache dirs");
    fs::write(&binary, b"pinned-mise-binary").expect("write fixture executable");
    make_executable(&binary);
    let digest = sha256(&binary);
    let script = mise_bootstrap_guard_script(&digest);

    let hit = run_guard(&script, &root);
    assert!(hit.status.success(), "verified hit: {:?}", hit.stderr);
    assert!(String::from_utf8_lossy(&hit.stdout).contains("verified cached Mise bootstrap"));
    assert!(binary.is_file(), "verified file remains available to setup");

    fs::write(&binary, b"tampered").expect("damage cached binary");
    make_executable(&binary);
    let damaged = run_guard(&script, &root);
    assert!(damaged.status.success(), "bad digest is a cold miss");
    assert!(
        !binary.exists(),
        "unverified file is removed before action execution"
    );

    fs::write(&binary, b"not executable").expect("write non-executable file");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o600)).expect("remove execute bit");
    let non_executable = run_guard(&script, &root);
    assert!(
        non_executable.status.success(),
        "non-executable is a cold miss"
    );
    assert!(!binary.exists(), "non-executable payload is removed");

    symlink(root.join("missing-target"), &binary).expect("create broken cache symlink");
    let broken = run_guard(&script, &root);
    assert!(broken.status.success(), "dangling symlink is a cold miss");
    assert!(
        binary.symlink_metadata().is_err(),
        "dangling link is removed"
    );
    remove_root(&root);
}

#[test]
fn bootstrap_guard_fails_closed_on_symlinked_owned_ancestors() {
    let root = test_root();
    let outside = test_root();
    fs::create_dir_all(&outside).expect("create outside directory");
    symlink(&outside, root.join("velnor")).expect("link owned cache ancestor");
    let digest = "a".repeat(64);
    let result = run_guard(&mise_bootstrap_guard_script(&digest), &root);
    assert!(!result.status.success(), "symlinked ancestor is rejected");
    assert!(
        root.join("velnor").symlink_metadata().is_ok(),
        "link is untouched"
    );
    remove_root(&root);
    remove_root(&outside);
}

#[test]
fn bootstrap_guard_uses_the_trusted_shasum_fallback_when_needed() {
    let root = test_root();
    let binary = bootstrap_path(&root);
    fs::create_dir_all(binary.parent().expect("binary parent")).expect("create cache dirs");
    fs::write(&binary, b"pinned-mise-binary").expect("write fixture executable");
    make_executable(&binary);
    let digest = sha256(&binary);
    let fake_shasum = root.join("qualified-shasum");
    let delegate = if Path::new("/usr/bin/sha256sum").is_file() {
        "exec /usr/bin/sha256sum -c -"
    } else {
        "exec /usr/bin/shasum -a 256 -c -"
    };
    fs::write(
        &fake_shasum,
        format!(
            "#!/bin/sh\n[ \"$1\" = -a ] && [ \"$2\" = 256 ] && [ \"$3\" = -c ] && [ \"$4\" = - ] || exit 19\n{delegate}\n"
        ),
    )
    .expect("write fallback shim");
    make_executable(&fake_shasum);
    let script = mise_bootstrap_guard_script(&digest)
        .replace("/usr/bin/sha256sum", "/missing/qualified-sha256sum")
        .replace("/usr/bin/shasum", &shell_quote(&fake_shasum));
    let result = run_guard(&script, &root);
    assert!(
        result.status.success(),
        "fallback verifies: {:?}",
        result.stderr
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("verified cached Mise bootstrap"));
    assert!(binary.is_file(), "verified fallback hit remains available");
    remove_root(&root);
}

fn run_guard(script: &str, runner_temp: &Path) -> Output {
    Command::new("bash")
        .args(["-c", script])
        .env("RUNNER_TEMP", runner_temp)
        .output()
        .expect("run bootstrap guard")
}

fn bootstrap_path(root: &Path) -> PathBuf {
    root.join("velnor/mise-bootstrap/bin/mise")
}

fn sha256(path: &Path) -> String {
    let mut command = if Path::new("/usr/bin/sha256sum").is_file() {
        let mut command = Command::new("/usr/bin/sha256sum");
        command.arg(path);
        command
    } else {
        let mut command = Command::new("/usr/bin/shasum");
        command.args(["-a", "256"]).arg(path);
        command
    };
    let output = command.output().expect("run trusted digest utility");
    assert!(output.status.success(), "digest utility exits successfully");
    String::from_utf8(output.stdout)
        .expect("digest output is UTF-8")
        .split_whitespace()
        .next()
        .expect("digest exists")
        .to_owned()
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn make_executable(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("set executable mode");
}

fn test_root() -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    loop {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-bootstrap-guard-{}-{unique}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create isolated runner temp: {error}"),
        }
    }
}

fn remove_root(path: &Path) {
    fs::remove_dir_all(path).expect("remove isolated runner temp");
}
