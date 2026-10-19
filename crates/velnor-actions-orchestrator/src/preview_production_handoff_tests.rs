//! Deterministic ownership and mutation checks on actual held input files.
use super::*;
use std::time::{Duration, Instant};

fn deadline() -> CheckDeadline {
    CheckDeadline::from_start(Instant::now(), Duration::from_secs(5)).expect("deadline")
}

fn asset(root: &Path) -> (PathBuf, AssetProof) {
    let path = root
        .canonicalize()
        .expect("canonical owned fixture")
        .join("payload.tar.gz");
    fs::write(&path, b"abc").expect("asset");
    (
        path,
        AssetProof {
            name: "payload.tar.gz".to_owned(),
            sha256: sha256(b"abc"),
            size: 3,
        },
    )
}

#[test]
fn admitted_file_rejects_same_bytes_at_replaced_inode() {
    let root = tempfile::tempdir().expect("root");
    let (path, proof) = asset(root.path());
    let held = PinnedAsset::new(&path, &proof, deadline()).expect("pin");
    held.verify(deadline()).expect("unchanged");
    fs::rename(&path, root.path().join("original")).expect("move");
    fs::write(&path, b"abc").expect("same bytes new inode");
    assert!(held.verify(deadline()).is_err());
}

#[test]
fn admitted_file_rejects_in_place_mutation_and_symbolic_replacement() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().expect("root");
    let (path, proof) = asset(root.path());
    let held = PinnedAsset::new(&path, &proof, deadline()).expect("pin");
    fs::write(&path, b"abd").expect("mutate");
    assert!(held.verify(deadline()).is_err());
    fs::rename(&path, root.path().join("other")).expect("move");
    symlink(root.path().join("other"), &path).expect("link");
    assert!(held.verify(deadline()).is_err());
}

#[test]
fn admission_refuses_changed_verified_bytes_and_never_creates_success_asset() {
    let root = tempfile::tempdir().expect("root");
    let (path, proof) = asset(root.path());
    let out = fresh_directory(root.path(), "output").expect("output");
    fs::write(&path, b"bad").expect("mutation");
    assert!(read_verified(&path, &proof, deadline()).is_err());
    assert!(!out.join(proof.name).exists());
}

#[test]
fn expired_staging_does_not_publish_asset_and_fresh_directory_never_reuses() {
    let root = tempfile::tempdir().expect("root");
    let (path, proof) = asset(root.path());
    let out = fresh_directory(root.path(), "output").expect("output");
    assert!(fresh_directory(root.path(), "output").is_err());
    let expired = CheckDeadline::from_start(
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("one second before test clock"),
        Duration::from_millis(1),
    )
    .expect("expired");
    assert!(read_verified(&path, &proof, expired).is_err());
    assert!(!out.join(proof.name).exists());
}

#[test]
fn private_upload_file_becomes_exact_0644_without_changing_bytes() {
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let root = tempfile::tempdir().expect("root");
    let path = root
        .path()
        .canonicalize()
        .expect("canonical")
        .join("payload");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .expect("private file");
    file.write_all(b"abc").expect("bytes");
    let before = file.metadata().expect("before");
    assert_eq!(before.mode() & 0o7777, 0o600);
    let proof = AssetProof {
        name: "payload".into(),
        sha256: sha256(b"abc"),
        size: 3,
    };
    normalize_upload_file(&path, &proof, deadline()).expect("literal upload mode");
    let after = fs::symlink_metadata(&path).expect("after");
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    assert_eq!(after.mode() & 0o7777, 0o644);
    assert_eq!(fs::read(&path).expect("unchanged bytes"), b"abc");
    recheck(&path, &proof, deadline()).expect("size and digest unchanged");
}

#[test]
fn hardlink_after_pin_cannot_be_admitted_by_a_refreshed_metadata_snapshot() {
    use std::os::unix::fs::MetadataExt;
    let root = tempfile::tempdir().expect("root");
    let (path, proof) = asset(root.path());
    let mut held = PinnedAsset::new(&path, &proof, deadline()).expect("pin");
    fs::hard_link(&path, root.path().join("alias")).expect("second link");
    // Model the post-fchmod snapshot window, so an old ctime alone cannot make
    // this negative pass: the current snapshot itself must reject two links.
    held.original = held.file.metadata().expect("refreshed snapshot");
    assert_eq!(held.original.nlink(), 2);
    assert!(held.verify(deadline()).is_err());
    assert!(normalize_upload_file(&path, &proof, deadline()).is_err());
    assert_eq!(fs::read(&path).expect("original remains"), b"abc");
    assert_eq!(
        fs::read(root.path().join("alias")).expect("alias remains"),
        b"abc"
    );
}
