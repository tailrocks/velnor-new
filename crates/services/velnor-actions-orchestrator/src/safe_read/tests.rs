use std::fs;

use super::{MAX_REPO_FILE_BYTES, RepoRead, read_event_file, read_repo_file};

#[test]
fn repo_read_round_trip_and_absent() {
    let root = tempfile::TempDir::new().expect("temp root");
    let dir = root.path().join(".velnor");
    fs::create_dir_all(&dir).expect("velnor dir");
    fs::write(dir.join("config.toml"), "schema = 1\n").expect("config");
    assert!(matches!(
        read_repo_file(root.path(), ".velnor/config.toml", MAX_REPO_FILE_BYTES)
            .expect("readable"),
        RepoRead::Text(text) if text == "schema = 1\n"
    ));
    assert!(matches!(
        read_repo_file(root.path(), ".velnor/missing.toml", MAX_REPO_FILE_BYTES).expect("absent"),
        RepoRead::Absent
    ));
}

#[test]
#[cfg(unix)]
fn symlinked_parent_escape_fails_closed() {
    let root = tempfile::TempDir::new().expect("temp root");
    let outside = tempfile::TempDir::new().expect("outside root");
    fs::write(outside.path().join("config.toml"), "schema = 1\n").expect("outside");
    std::os::unix::fs::symlink(outside.path(), root.path().join(".velnor")).expect("symlink");
    let err = read_repo_file(root.path(), ".velnor/config.toml", MAX_REPO_FILE_BYTES)
        .expect_err("escape refused");
    assert!(err.to_string().contains("root_escape"), "{err}");
    let err = read_event_file(&outside.path().join("config.toml"), MAX_REPO_FILE_BYTES)
        .expect("outside payload reads");
    assert_eq!(err, "schema = 1\n");
}

#[test]
#[cfg(unix)]
fn repo_symlink_refused_names_reason() {
    let dir = tempfile::TempDir::new().expect("temp root");
    let target = dir.path().join("real.toml");
    fs::write(&target, "schema = 1\n").expect("target");
    let link = dir.path().join("link.toml");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    let err =
        read_repo_file(dir.path(), "link.toml", MAX_REPO_FILE_BYTES).expect_err("symlink refused");
    assert!(err.to_string().contains("symlink_refused"), "{err}");
}

#[test]
fn repo_directory_reports_not_a_file() {
    let dir = tempfile::TempDir::new().expect("temp root");
    fs::create_dir_all(dir.path().join("sub")).expect("subdir");
    let err = read_repo_file(dir.path(), "sub", MAX_REPO_FILE_BYTES).expect_err("dir refused");
    assert!(err.to_string().contains("not_a_file"), "{err}");
}

#[test]
fn repo_oversize_and_bad_utf8_stay_errors() {
    let dir = tempfile::TempDir::new().expect("temp root");
    fs::write(dir.path().join("big.txt"), "0123456789").expect("big");
    let err = read_repo_file(dir.path(), "big.txt", 4).expect_err("oversize refused");
    assert!(err.to_string().contains("oversize"), "{err}");
    fs::write(dir.path().join("bad.txt"), [0xff, 0xfe]).expect("bad");
    let err =
        read_repo_file(dir.path(), "bad.txt", MAX_REPO_FILE_BYTES).expect_err("bad utf-8 refused");
    assert!(
        !matches!(err, crate::OrchestratorError::UnsafePath { .. }),
        "{err}"
    );
}

#[test]
#[cfg(unix)]
fn event_symlink_fails_closed() {
    let dir = tempfile::TempDir::new().expect("temp root");
    let target = dir.path().join("payload.json");
    fs::write(&target, "{}").expect("payload");
    let link = dir.path().join("event.json");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    let err = read_event_file(&link, MAX_REPO_FILE_BYTES).expect_err("symlink refused");
    assert!(
        err.to_string().contains("unreadable_event_payload"),
        "{err}"
    );
}
