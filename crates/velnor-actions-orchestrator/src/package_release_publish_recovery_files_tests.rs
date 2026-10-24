use super::{
    create_owned_input_directory, parse_canonical_positive_decimal,
    parse_canonical_positive_decimal_u32,
};
use crate::filesystem_owner::held_directory::HeldDirectory;
use std::{cell::Cell, fs, sync::Arc, time::Duration};
use velnor_actions_mise::CheckDeadline;

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(10)).expect("deadline")
}

#[test]
fn preview_manual_run_identity_requires_canonical_positive_decimal() {
    for invalid in [
        "",
        "0",
        "00",
        "01",
        "+1",
        " 1",
        "1 ",
        "18446744073709551616",
    ] {
        assert!(
            parse_canonical_positive_decimal(invalid).is_err(),
            "accepted noncanonical or out-of-range run id {invalid:?}"
        );
    }
    assert_eq!(
        parse_canonical_positive_decimal("1").expect("canonical run id"),
        1
    );
    assert_eq!(
        parse_canonical_positive_decimal("18446744073709551615").expect("maximum canonical run id"),
        u64::MAX
    );

    for invalid in ["0", "01", "4294967296"] {
        assert!(
            parse_canonical_positive_decimal_u32(invalid).is_err(),
            "accepted noncanonical or out-of-range attempt {invalid:?}"
        );
    }
    assert_eq!(
        parse_canonical_positive_decimal_u32("2").expect("canonical attempt"),
        2
    );
}

#[test]
fn retained_verifications_get_unique_owned_input_directories() {
    let runner_root = tempfile::tempdir().expect("runner temp root");
    let root = runner_root.path().canonicalize().expect("canonical root");
    let preexisting = root.join("release-recovery-input-preexisting");
    fs::create_dir(&preexisting).expect("preexisting directory");
    fs::write(preexisting.join("keep"), b"original").expect("preexisting marker");

    let first = create_owned_input_directory(&root).expect("first owned input");
    fs::write(first.path().join("release.json"), b"first receipt").expect("first receipt");
    let first_path = first.path().to_path_buf();
    let first_calls = Cell::new(0);
    let (first_bytes, first_closure) = HeldDirectory::admit_owned(first, deadline(), || {
        first_calls.set(first_calls.get() + 1);
        fs::read(first_path.join("release.json")).map_err(|error| {
            crate::internal::internal(&format!("first fixture read failed: {error}"))
        })
    })
    .expect("first custody admission");
    let first_custody = Arc::new(first_closure);
    let first_clone = Arc::clone(&first_custody);

    let second = create_owned_input_directory(&root).expect("second owned input");
    fs::write(second.path().join("release.json"), b"second receipt").expect("second receipt");
    let second_path = second.path().to_path_buf();
    let second_calls = Cell::new(0);
    let (second_bytes, second_closure) = HeldDirectory::admit_owned(second, deadline(), || {
        second_calls.set(second_calls.get() + 1);
        fs::read(second_path.join("release.json")).map_err(|error| {
            crate::internal::internal(&format!("second fixture read failed: {error}"))
        })
    })
    .expect("second custody admission in the same root");
    let second_custody = Arc::new(second_closure);

    assert_ne!(first_path, second_path);
    assert!(first_path.starts_with(&root));
    assert!(second_path.starts_with(&root));
    assert_ne!(first_path, preexisting);
    assert_ne!(second_path, preexisting);
    assert!(first_path.is_dir() && second_path.is_dir());
    assert_eq!(first_bytes, b"first receipt");
    assert_eq!(second_bytes, b"second receipt");
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 1);
    first_custody.verify().expect("first bytes remain admitted");
    second_custody
        .verify()
        .expect("second bytes remain admitted");
    drop(first_custody);
    assert!(
        first_path.is_dir(),
        "Arc clone retains the first TempDir owner"
    );
    first_clone
        .verify()
        .expect("cloned first custody remains valid");
    drop(first_clone);
    assert!(
        !first_path.exists(),
        "final Arc drop removes the first input"
    );
    drop(second_custody);
    assert!(
        !second_path.exists(),
        "final custody drop removes the second input"
    );
    assert_eq!(
        fs::read(preexisting.join("keep")).expect("kept marker"),
        b"original"
    );
}

#[test]
fn failed_owned_admission_removes_only_its_input_directory() {
    let runner_root = tempfile::tempdir().expect("runner temp root");
    let root = runner_root.path().canonicalize().expect("canonical root");
    let preexisting = root.join("release-recovery-input-preexisting");
    fs::create_dir(&preexisting).expect("preexisting directory");
    fs::write(preexisting.join("keep"), b"original").expect("preexisting marker");
    let owner = create_owned_input_directory(&root).expect("owned input");
    fs::write(owner.path().join("release.json"), b"rejected receipt").expect("receipt fixture");
    let owned_path = owner.path().to_path_buf();

    let result = HeldDirectory::admit_owned(owner, deadline(), || {
        Err::<(), _>(crate::internal::internal("fixture verification rejection"))
    });

    assert!(result.is_err());
    assert!(!owned_path.exists(), "failed admission drops its TempDir");
    assert_eq!(
        fs::read(preexisting.join("keep")).expect("kept marker"),
        b"original"
    );
}

#[cfg(unix)]
#[test]
fn owned_input_creation_does_not_follow_a_preexisting_symlink() {
    let runner_root = tempfile::tempdir().expect("runner temp root");
    let target = runner_root.path().join("retained-data");
    fs::create_dir(&target).expect("target directory");
    fs::write(target.join("keep"), b"original").expect("target marker");
    let symlink = runner_root.path().join("release-recovery-input-link");
    std::os::unix::fs::symlink(&target, &symlink).expect("preexisting symlink");

    let owned = create_owned_input_directory(runner_root.path()).expect("unique owned input");

    assert_ne!(owned.path(), symlink);
    assert!(
        fs::symlink_metadata(&symlink)
            .expect("kept symlink")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read(target.join("keep")).expect("kept target"),
        b"original"
    );
}
