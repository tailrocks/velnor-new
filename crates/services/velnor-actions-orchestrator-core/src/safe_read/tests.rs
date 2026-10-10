use std::fs;

use super::{
    MAX_REPO_FILE_BYTES, RepoRead, read_event_file, read_repo_file,
    stream::{open_repo_parent, stream_repo_file_from_parent},
    stream_repo_file,
};

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

#[test]
#[cfg(unix)]
fn streamed_repo_file_refuses_an_intermediate_symlink() {
    let root = tempfile::TempDir::new().expect("temp root");
    let outside = tempfile::TempDir::new().expect("outside root");
    std::fs::write(outside.path().join("image.tar"), b"outside").expect("outside output");
    std::os::unix::fs::symlink(outside.path(), root.path().join("dist")).expect("parent link");

    let mut streamed = false;
    let error = stream_repo_file(root.path(), "dist/image.tar", 64, |_| {
        streamed = true;
        Ok(())
    })
    .expect_err("intermediate symlink refused");
    let detail = error.to_string();
    assert!(
        detail.contains("symlink_refused") || detail.contains("not_a_directory"),
        "{error}"
    );
    assert!(!streamed, "bytes must not be read through the symlink");
}

#[test]
#[cfg(unix)]
fn streamed_repo_file_stays_on_pinned_parent_after_path_replacement() {
    let root = tempfile::TempDir::new().expect("temp root");
    let outside = tempfile::TempDir::new().expect("outside root");
    let original = root.path().join("dist");
    std::fs::create_dir(&original).expect("parent");
    std::fs::write(original.join("image.tar"), b"pinned original").expect("original output");
    std::fs::write(outside.path().join("image.tar"), b"outside replacement").expect("outside");

    let pinned = open_repo_parent(root.path(), "dist/image.tar").expect("pinned parent");
    std::fs::rename(&original, root.path().join("dist-moved")).expect("rename parent");
    std::os::unix::fs::symlink(outside.path(), &original).expect("replace parent with symlink");

    let mut observed = Vec::new();
    stream_repo_file_from_parent(&pinned, 64, &mut |chunk| {
        observed.extend_from_slice(chunk);
        Ok(())
    })
    .expect("read pinned directory handle");
    assert_eq!(observed, b"pinned original");
}

#[test]
#[cfg(unix)]
fn streamed_repo_file_opens_a_replaced_fifo_nonblocking_then_rejects_it() {
    use std::sync::mpsc;
    use std::time::Duration;

    let root = tempfile::TempDir::new().expect("temp root");
    let parent = root.path().join("dist");
    std::fs::create_dir(&parent).expect("parent");
    let _parent_fd = rustix::fs::open(
        &parent,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .expect("open parent");
    create_fifo(&parent, "image.tar").expect("create fifo");
    let root_path = root.path().to_path_buf();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = stream_repo_file(&root_path, "dist/image.tar", 64, |_| Ok(()));
        let text = result.expect_err("fifo is not a regular file").to_string();
        let _ = sender.send(text);
    });
    let outcome = match receiver.recv_timeout(Duration::from_secs(1)) {
        Ok(outcome) => outcome,
        Err(error) => {
            // If a regression blocks on open, connect a writer so the test can
            // join cleanly before reporting that the nonblocking guarantee failed.
            let _writer = std::fs::OpenOptions::new()
                .write(true)
                .open(parent.join("image.tar"))
                .expect("unblock a regressed blocking open");
            worker.join().expect("worker thread");
            panic!("FIFO open did not return nonblocking: {error}");
        }
    };
    worker.join().expect("worker thread");
    assert!(outcome.contains("not_a_file"), "{outcome}");
}

#[cfg(unix)]
fn create_fifo(directory: &std::path::Path, name: &str) -> Result<(), String> {
    let path = directory.join(name);
    let outcome = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .map_err(|error| format!("run mkfifo: {error}"))?;
    if outcome.success() {
        Ok(())
    } else {
        Err(format!("mkfifo failed for {}", path.display()))
    }
}

#[test]
#[cfg(unix)]
fn streamed_repo_file_rejects_traversal_before_opening_components() {
    let root = tempfile::TempDir::new().expect("temp root");
    let error = stream_repo_file(root.path(), "../outside", 64, |_| Ok(()))
        .expect_err("traversal rejected");
    assert!(
        error.to_string().contains("unsafe_relative_path"),
        "{error}"
    );
}
