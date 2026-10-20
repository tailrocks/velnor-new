use super::*;
use std::{cell::Cell, time::Duration};

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(10)).expect("deadline")
}
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp
        .path()
        .canonicalize()
        .expect("canonical")
        .join("release");
    fs::create_dir(&path).expect("directory");
    for name in [
        "payload.tar.gz",
        "payload.tar.gz.bundle",
        "sbom.json",
        "checksums.txt",
        "release.json",
        "release.json.bundle",
    ] {
        fs::write(path.join(name), b"abc").expect("file");
    }
    (temp, path)
}

#[test]
fn repeated_closure_checks_do_not_repeat_full_admission() {
    let (_temp, path) = fixture();
    let calls = Cell::new(0);
    let ((), closure) = HeldDirectory::admit(&path, deadline(), || {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .expect("admit");
    for _ in 0..40 {
        closure.verify().expect("same complete bytes");
    }
    assert_eq!(calls.get(), 1);
}

#[test]
fn mutation_during_crypto_is_not_blessed_by_a_post_crypto_snapshot() {
    let (_temp, path) = fixture();
    assert!(
        HeldDirectory::admit(&path, deadline(), || {
            fs::write(path.join("release.json.bundle"), b"def").expect("changed proof");
            Ok(())
        })
        .is_err()
    );
}

#[test]
fn every_proof_and_payload_is_held_not_only_receipt_assets() {
    for name in [
        "payload.tar.gz",
        "payload.tar.gz.bundle",
        "sbom.json",
        "checksums.txt",
        "release.json",
        "release.json.bundle",
    ] {
        let (_temp, path) = fixture();
        let closure = HeldDirectory::capture(&path, deadline()).expect("capture");
        fs::write(path.join(name), b"def").expect("same-size mutation");
        assert!(closure.verify().is_err(), "accepted changed {name}");
    }
}

#[test]
fn extra_missing_symlink_and_directory_replacement_are_not_refreshable() {
    for change in ["extra", "missing", "symlink", "directory"] {
        let (_temp, path) = fixture();
        let closure = HeldDirectory::capture(&path, deadline()).expect("capture");
        match change {
            "extra" => fs::write(path.join("unadmitted"), b"abc").expect("extra"),
            "missing" => fs::remove_file(path.join("release.json.bundle")).expect("missing"),
            "symlink" => {
                fs::remove_file(path.join("sbom.json")).expect("remove");
                std::os::unix::fs::symlink("checksums.txt", path.join("sbom.json")).expect("link");
            }
            _ => {
                fs::rename(&path, path.with_extension("old")).expect("move");
                fs::create_dir(&path).expect("replacement");
            }
        }
        assert!(closure.verify().is_err(), "accepted {change}");
    }
}

#[test]
fn ancestor_symlink_cannot_reach_an_otherwise_identical_held_directory() {
    let (temp, path) = fixture();
    let root = temp.path().canonicalize().expect("canonical");
    let ancestor = root.join("ancestor");
    fs::create_dir(&ancestor).expect("ancestor");
    let nested = ancestor.join("release");
    fs::rename(&path, &nested).expect("nested");
    let closure = HeldDirectory::capture(&nested, deadline()).expect("capture");
    let moved = root.join("moved");
    fs::rename(&ancestor, &moved).expect("move ancestor");
    std::os::unix::fs::symlink(&moved, &ancestor).expect("same target alias");
    assert!(closure.verify().is_err());
}

#[test]
fn shared_closure_never_refreshes_identity_or_expired_deadline() {
    let (_temp, path) = fixture();
    let closure = std::sync::Arc::new(HeldDirectory::capture(&path, deadline()).expect("capture"));
    let shared = std::sync::Arc::clone(&closure);
    assert!(std::sync::Arc::ptr_eq(&closure, &shared));
    fs::write(path.join("release.json.bundle"), b"def").expect("changed bundle");
    assert!(closure.verify().is_err());
    assert!(shared.verify().is_err());
    let mut expired = HeldDirectory::capture(&path, deadline()).expect("new admission snapshot");
    expired.deadline = CheckDeadline::from_start(
        std::time::Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("elapsed monotonic clock"),
        Duration::from_secs(30),
    )
    .expect("expired fixed deadline");
    assert!(expired.verify().is_err());
}
