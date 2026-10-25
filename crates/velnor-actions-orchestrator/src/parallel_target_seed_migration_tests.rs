// These fixtures need a byte-name filesystem; APFS rejects their creation with EILSEQ.
#[cfg(target_os = "linux")]
use std::{ffi::OsString, os::unix::ffi::OsStringExt};

use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    process::Command,
};

#[cfg(target_os = "linux")]
use super::super::filesystem::remove_tree;

use super::super::{
    ParallelTargetLane, SeedOutcome,
    execution::{SeedError, Stage, finalize_staged_result},
    filesystem::{open_dir_path, openat_dir},
    inventory::Identity,
    seed_parallel_targets,
};
use super::{Tree, lanes};
use rustix::fs::{self as ufs, Mode};

#[test]
fn absent_target_or_debug_baseline_keeps_all_lanes_cold() {
    let tree = Tree::new();
    fs::remove_dir_all(tree.target()).expect("remove target");
    assert_eq!(
        seed_parallel_targets(&tree.0, "target", &lanes()).expect("missing target is optional"),
        SeedOutcome::Cold
    );

    fs::create_dir(tree.target()).expect("target without debug baseline");
    assert_eq!(
        seed_parallel_targets(&tree.0, "target", &lanes()).expect("missing debug is optional"),
        SeedOutcome::Cold
    );
    assert!(!tree.target().join("velnor-clippy").exists());
    assert!(!tree.target().join("velnor-doc").exists());
}

#[test]
fn target_symlink_is_rejected_without_admitting_lanes() {
    let tree = Tree::new();
    symlink(&tree.0, tree.0.join("alias")).expect("workspace alias");

    assert!(seed_parallel_targets(&tree.0, "alias/target", &lanes()).is_err());
    assert!(!tree.target().join("velnor-clippy").exists());
    assert!(!tree.target().join("velnor-doc").exists());
}

#[test]
fn readable_target_with_foreign_owner_is_accepted_when_chown_is_available() {
    let tree = Tree::new();
    let owner = rustix::process::geteuid().as_raw();
    let foreign_owner = if owner == 0 { 65_534 } else { 0 };
    let target = tree.target();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o777))
        .expect("writable target fixture");
    if let Err(error) = std::os::unix::fs::chown(&target, Some(foreign_owner), None) {
        eprintln!("foreign-owner fixture unavailable: {error}");
        return;
    }
    assert_eq!(
        fs::symlink_metadata(&target)
            .expect("target metadata")
            .uid(),
        foreign_owner
    );

    assert!(matches!(
        seed_parallel_targets(&tree.0, "target", &[ParallelTargetLane::Clippy])
            .expect("readable real target does not require current-user ownership"),
        SeedOutcome::Seeded { .. }
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_baseline_names_are_copied_without_loss() {
    let tree = Tree::new();
    let directory = OsString::from_vec(b"build-\xff".to_vec());
    let filename = OsString::from_vec(b"artifact-\xfe.rlib".to_vec());
    let source = tree.target().join("debug").join(&directory);
    fs::create_dir(&source).expect("non-UTF-8 baseline directory");
    fs::write(source.join(&filename), b"raw filesystem name").expect("non-UTF-8 baseline filename");

    assert!(matches!(
        seed_parallel_targets(&tree.0, "target", &[ParallelTargetLane::Clippy])
            .expect("non-UTF-8 baseline names are supported"),
        SeedOutcome::Seeded { .. }
    ));
    assert_eq!(
        fs::read(
            tree.target()
                .join("velnor-clippy/debug")
                .join(&directory)
                .join(&filename)
        )
        .expect("copied entry with its original raw name"),
        b"raw filesystem name"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn staging_cleanup_removes_non_utf8_names() {
    let tree = Tree::new();
    let target = open_dir_path(&tree.target()).expect("target fd");
    let stage_path = tree.target().join(".velnor-seed-raw-name");
    fs::create_dir(&stage_path).expect("stage directory");
    let filename = OsString::from_vec(b"partial-\xff".to_vec());
    fs::write(stage_path.join(filename), b"partial").expect("raw-name partial file");
    let stage = openat_dir(&target, ".velnor-seed-raw-name").expect("stage fd");

    remove_tree(&stage).expect("cleanup accepts raw filesystem names");
    assert!(
        fs::read_dir(&stage_path)
            .expect("stage directory remains openable")
            .next()
            .is_none()
    );
    fs::remove_dir(stage_path).expect("remove empty stage root");
}

#[test]
fn existing_lane_file_is_rejected_and_preserved() {
    let tree = Tree::new();
    let lane = tree.target().join("velnor-doc");
    fs::write(&lane, b"foreign file").expect("foreign lane file");

    assert!(seed_parallel_targets(&tree.0, "target", &[ParallelTargetLane::Doc]).is_err());
    assert_eq!(
        fs::read(&lane).expect("preserved lane file"),
        b"foreign file"
    );
}

#[test]
fn missing_compiler_metadata_is_optional_for_a_valid_debug_baseline() {
    let tree = Tree::new();
    fs::remove_file(tree.target().join(".rustc_info.json")).expect("remove metadata");

    assert!(matches!(
        seed_parallel_targets(&tree.0, "target", &[ParallelTargetLane::Clippy])
            .expect("missing compiler metadata is optional"),
        SeedOutcome::Seeded { .. }
    ));
    assert!(
        tree.target()
            .join("velnor-clippy/debug/deps/sample.rlib")
            .is_file()
    );
}

#[test]
fn nonregular_compiler_metadata_is_rejected_without_lane_admission() {
    let tree = Tree::new();
    fs::remove_file(tree.target().join(".rustc_info.json")).expect("remove metadata");
    symlink(
        tree.target().join("debug/deps/sample.rlib"),
        tree.target().join(".rustc_info.json"),
    )
    .expect("metadata symlink");

    assert!(seed_parallel_targets(&tree.0, "target", &[ParallelTargetLane::Clippy]).is_err());
    assert!(!tree.target().join("velnor-clippy").exists());
}

#[test]
fn fifo_baseline_entry_is_rejected_without_lane_admission() {
    let tree = Tree::new();
    let fifo = tree.target().join("debug/deps/fifo");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo command");
    assert!(status.success(), "mkfifo returned {status}");

    assert!(seed_parallel_targets(&tree.0, "target", &[ParallelTargetLane::Clippy]).is_err());
    assert!(!tree.target().join("velnor-clippy").exists());
}

#[test]
fn interrupted_partial_stage_is_cleaned_without_changing_the_baseline() {
    let tree = Tree::new();
    let source = tree.target().join("debug/deps/sample.rlib");
    let source_bytes = fs::read(&source).expect("baseline bytes");
    let target = open_dir_path(&tree.target()).expect("target fd");
    let name = ".velnor-seed-injected-interruption";
    ufs::mkdirat(&target, name, Mode::from_raw_mode(0o700)).expect("stage directory");
    fs::write(tree.target().join(name).join("partial"), b"partial").expect("partial copied file");
    let fd = openat_dir(&target, name).expect("stage fd");
    let identity = Identity::from_stat(&ufs::fstat(&fd).expect("stage stat"));
    let mut stages = vec![Stage {
        name: name.to_owned(),
        fd,
        identity,
    }];

    assert_eq!(
        finalize_staged_result(
            &target,
            &mut stages,
            Err(SeedError::Skip("injected interruption")),
            &[ParallelTargetLane::Clippy],
            7,
        )
        .expect("optional interruption"),
        SeedOutcome::Skipped {
            reason: "injected interruption"
        }
    );
    assert!(!tree.target().join(name).exists());
    assert!(!tree.target().join("velnor-clippy").exists());
    assert_eq!(fs::read(source).expect("unchanged baseline"), source_bytes);
}
