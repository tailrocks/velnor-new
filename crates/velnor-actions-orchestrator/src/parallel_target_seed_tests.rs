use super::execution::finalize_staged_result;
use super::execution::{SeedError, Stage};
use super::filesystem::{
    cleanup_stages, open_dir_path, openat_dir, rename_noreplace, seed_io_error, statat,
};
use super::inventory::{Entry, Identity, Snapshot};
use super::*;
use rustix::fs::{self as ufs, AtFlags, Mode};
use std::{
    fs,
    os::unix::fs::{MetadataExt, symlink},
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "velnor-parallel-seed-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&path).expect("temporary root");
        let target = path.join("target/debug/deps");
        fs::create_dir_all(&target).expect("debug tree");
        fs::write(target.join("sample.rlib"), b"native artifact\0").expect("artifact");
        fs::write(path.join("target/debug/.cargo-lock"), b"lock").expect("lock");
        fs::write(path.join("target/.rustc_info.json"), b"{}").expect("rustc info");
        Self(path)
    }

    fn target(&self) -> PathBuf {
        self.0.join("target")
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn lanes() -> [ParallelTargetLane; 2] {
    [ParallelTargetLane::Clippy, ParallelTargetLane::Doc]
}

#[test]
fn cold_lanes_are_independent_and_keep_metadata_without_cargo_lock() {
    let tree = Tree::new();
    let source = tree.target().join("debug/deps/sample.rlib");
    let source_meta = fs::metadata(&source).expect("source metadata");
    let outcome = seed_parallel_targets(&tree.0, "target", &lanes()).expect("seed");
    assert!(
        matches!(outcome, SeedOutcome::Seeded { lanes, .. } if lanes == ["velnor-clippy", "velnor-doc"])
    );
    for lane in ["velnor-clippy", "velnor-doc"] {
        let copy = tree.target().join(lane).join("debug/deps/sample.rlib");
        let copy_meta = fs::metadata(&copy).expect("copied artifact");
        assert_eq!(fs::read(&copy).expect("copy bytes"), b"native artifact\0");
        assert_eq!(copy_meta.mtime(), source_meta.mtime());
        assert_ne!(
            (copy_meta.dev(), copy_meta.ino()),
            (source_meta.dev(), source_meta.ino())
        );
        assert_eq!(copy_meta.nlink(), 1);
        assert!(!tree.target().join(lane).join("debug/.cargo-lock").exists());
    }
    fs::write(
        tree.target().join("velnor-clippy/debug/deps/sample.rlib"),
        b"changed",
    )
    .expect("modify first lane");
    assert_eq!(
        fs::read(&source).expect("untouched source"),
        b"native artifact\0"
    );
    assert_eq!(
        fs::read(tree.target().join("velnor-doc/debug/deps/sample.rlib")).expect("other lane"),
        b"native artifact\0"
    );
}

#[test]
fn existing_lanes_are_preserved_and_only_cargo_lock_is_excluded() {
    let tree = Tree::new();
    let empty = tree.target().join("velnor-clippy");
    let warm = tree.target().join("velnor-doc");
    fs::create_dir(&empty).expect("empty lane");
    fs::create_dir(&warm).expect("warm lane");
    fs::write(warm.join("foreign"), b"preserve").expect("foreign file");
    let before = (
        fs::metadata(&empty).expect("empty metadata").ino(),
        fs::metadata(&warm).expect("warm metadata").ino(),
    );
    assert_eq!(
        seed_parallel_targets(&tree.0, "target", &lanes()).expect("seed"),
        SeedOutcome::Cold
    );
    assert_eq!(
        before,
        (
            fs::metadata(&empty).expect("empty metadata").ino(),
            fs::metadata(&warm).expect("warm metadata").ino()
        )
    );
    assert_eq!(
        fs::read(warm.join("foreign")).expect("foreign bytes"),
        b"preserve"
    );

    let nested = tree.target().join("debug/build/pkg/out");
    fs::create_dir_all(&nested).expect("nested build output");
    fs::write(nested.join(".cargo-lock"), b"payload, not Cargo's lock").expect("payload");
    fs::remove_dir(empty).expect("remove cold lane");
    fs::remove_dir_all(warm).expect("remove warm lane");
    seed_parallel_targets(&tree.0, "target", &lanes()).expect("seed cold lanes");
    for lane in ["velnor-clippy", "velnor-doc"] {
        assert_eq!(
            fs::read(
                tree.target()
                    .join(lane)
                    .join("debug/build/pkg/out/.cargo-lock")
            )
            .expect("nested payload"),
            b"payload, not Cargo's lock"
        );
        assert!(!tree.target().join(lane).join("debug/.cargo-lock").exists());
    }
}

#[test]
fn path_traversal_and_noncanonical_lanes_are_rejected() {
    let tree = Tree::new();
    for bad in [
        "/target",
        "../target",
        "target/../target",
        "a//target",
        "other",
    ] {
        assert!(
            seed_parallel_targets(&tree.0, bad, &[ParallelTargetLane::Clippy]).is_err(),
            "{bad}"
        );
    }
    assert!(parse_lanes("").is_err());
    assert!(parse_lanes("velnor-doc,velnor-clippy").is_err());
    assert!(parse_lanes("velnor-clippy,velnor-clippy").is_err());
    assert!(parse_lanes("velnor-doc;id").is_err());
}

#[test]
fn symlink_and_special_baseline_entries_fail_closed_without_lane_admission() {
    let tree = Tree::new();
    let source = tree.target().join("debug/deps/sample.rlib");
    let alias = tree.target().join("debug/deps/alias");
    symlink(&source, &alias).expect("symlink");
    assert!(seed_parallel_targets(&tree.0, "target", &[ParallelTargetLane::Clippy]).is_err());
    assert!(!tree.target().join("velnor-clippy").exists());
    fs::remove_file(alias).expect("remove symlink");

    let lane = tree.target().join("velnor-doc");
    symlink(tree.target().join("debug"), &lane).expect("lane symlink");
    assert!(seed_parallel_targets(&tree.0, "target", &[ParallelTargetLane::Doc]).is_err());
    assert!(
        lane.symlink_metadata()
            .expect("lane still exists")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn workspace_scoped_target_uses_only_its_own_baseline() {
    let tree = Tree::new();
    let nested = tree.0.join("crates/member");
    fs::create_dir_all(&nested).expect("workspace member");
    fs::rename(tree.target(), nested.join("target")).expect("move target");
    let outcome =
        seed_parallel_targets(&tree.0, "crates/member/target", &[ParallelTargetLane::Doc])
            .expect("scoped seed");
    assert!(matches!(outcome, SeedOutcome::Seeded { .. }));
    assert!(
        nested
            .join("target/velnor-doc/debug/deps/sample.rlib")
            .is_file()
    );
    assert!(!tree.0.join("target").exists());
}

#[test]
fn byte_entry_elapsed_and_reserve_limits_skip_without_admitting_lanes() {
    let tree = Tree::new();
    for limits in [
        SeedLimits {
            bytes: 1,
            ..DEFAULT_LIMITS
        },
        SeedLimits {
            entries: 0,
            ..DEFAULT_LIMITS
        },
        SeedLimits {
            seconds: Duration::ZERO,
            ..DEFAULT_LIMITS
        },
        SeedLimits {
            reserve: u64::MAX,
            ..DEFAULT_LIMITS
        },
    ] {
        let outcome = seed_parallel_targets_with_limits(
            &tree.0,
            "target",
            &[ParallelTargetLane::Clippy],
            limits,
        )
        .expect("budget is optional");
        assert!(matches!(outcome, SeedOutcome::Skipped { .. }));
        assert!(!tree.target().join("velnor-clippy").exists());
        assert!(fs::read(tree.target().join("debug/deps/sample.rlib")).is_ok());
    }
}

#[test]
fn unsupported_or_racing_admission_never_replaces_foreign_lane() {
    let tree = Tree::new();
    let target = open_dir_path(&tree.target()).expect("target fd");
    let source = ".stage";
    let destination = "velnor-clippy";
    ufs::mkdirat(&target, source, Mode::from_raw_mode(0o700)).expect("source stage");
    ufs::mkdirat(&target, destination, Mode::from_raw_mode(0o700)).expect("foreign destination");
    fs::write(tree.target().join(destination).join("foreign"), b"preserve").expect("foreign file");
    assert!(rename_noreplace(&target, source, destination).is_err());
    assert_eq!(
        fs::read(tree.target().join(destination).join("foreign")).expect("foreign bytes"),
        b"preserve"
    );
    ufs::unlinkat(&target, source, AtFlags::REMOVEDIR).expect("remove unused stage");
}

#[test]
fn source_mutation_after_inventory_is_rejected() {
    let tree = Tree::new();
    let target = open_dir_path(&tree.target()).expect("target fd");
    let debug = openat_dir(&target, "debug").expect("debug fd");
    let mut snapshot = Snapshot::new(Instant::now(), DEFAULT_LIMITS);
    snapshot.entries.push(Entry::new(
        vec!["debug".to_owned()],
        statat(&target, "debug").expect("debug stat"),
    ));
    snapshot
        .inventory(&debug, &mut vec!["debug".to_owned()])
        .expect("inventory");
    fs::write(tree.target().join("debug/deps/sample.rlib"), b"mutated").expect("mutate baseline");
    assert!(snapshot.validate_source(&target).is_err());
}

#[test]
fn metadata_and_stat_keep_platform_timestamp_values_losslessly() {
    let tree = Tree::new();
    let target = open_dir_path(&tree.target()).expect("target fd");
    for relative in ["debug", "debug/deps/sample.rlib"] {
        let metadata = fs::metadata(tree.target().join(relative)).expect("metadata");
        let stat = statat(&target, relative).expect("stat");
        let from_metadata = Identity::from_metadata(&metadata);
        let from_stat = Identity::from_stat(&stat);
        assert_eq!(from_metadata, from_stat);
        assert_eq!(
            from_metadata.atime,
            (metadata.atime(), i128::from(metadata.atime_nsec()))
        );
        assert_eq!(
            from_stat.atime,
            (stat.st_atime, i128::from(stat.st_atime_nsec))
        );
    }
}

#[test]
fn raw_mode_conversion_accepts_platform_width_and_rejects_narrowing() {
    assert_eq!(
        super::inventory::checked_raw_mode::<u16>(0o750).expect("mode"),
        0o750
    );
    assert!(super::inventory::checked_raw_mode::<u16>(u32::MAX).is_err());
    assert_eq!(
        super::inventory::checked_raw_mode::<u32>(u32::MAX).expect("wide mode"),
        u32::MAX
    );
}

#[test]
fn timestamp_conversion_preserves_pre_epoch_time_and_rejects_invalid_nanos() {
    let expected = UNIX_EPOCH
        .checked_sub(Duration::from_nanos(1))
        .expect("representable timestamp");
    assert_eq!(
        super::inventory::system_time((-1, 999_999_999)).expect("pre-epoch time"),
        expected
    );
    assert!(super::inventory::system_time((0, -1)).is_err());
    assert!(super::inventory::system_time((0, 1_000_000_000)).is_err());
}

#[test]
fn inventory_stops_retaining_names_at_the_configured_entry_limit() {
    let tree = Tree::new();
    let debug_path = tree.target().join("debug");
    for index in 0..256 {
        fs::write(debug_path.join(format!("entry-{index:04}")), b"x")
            .expect("extra baseline entry");
    }
    let target = open_dir_path(&tree.target()).expect("target fd");
    let debug = openat_dir(&target, "debug").expect("debug fd");
    let limits = SeedLimits {
        entries: 8,
        ..DEFAULT_LIMITS
    };
    let mut snapshot = Snapshot::new(Instant::now(), limits);
    snapshot.entries.push(Entry::new(
        vec!["debug".to_owned()],
        statat(&target, "debug").expect("debug stat"),
    ));
    assert!(matches!(
        snapshot.inventory(&debug, &mut vec!["debug".to_owned()]),
        Err(SeedError::Skip("entry limit"))
    ));
    assert!(snapshot.entries.len() <= limits.entries);
}

#[test]
fn capacity_io_skips_only_after_owned_stage_cleanup() {
    let tree = Tree::new();
    let target = open_dir_path(&tree.target()).expect("target fd");
    let stage_name = ".velnor-seed-injected-capacity";
    ufs::mkdirat(&target, stage_name, Mode::from_raw_mode(0o700)).expect("stage directory");
    fs::write(tree.target().join(stage_name).join("partial"), b"partial")
        .expect("partial staged file");
    let fd = openat_dir(&target, stage_name).expect("stage fd");
    let identity = Identity::from_stat(&ufs::fstat(&fd).expect("stage stat"));
    let mut stages = vec![Stage {
        name: stage_name.to_owned(),
        fd,
        identity,
    }];

    let outcome = finalize_staged_result(
        &target,
        &mut stages,
        Err(seed_io_error(std::io::Error::from_raw_os_error(28))),
        &[ParallelTargetLane::Clippy],
        7,
    )
    .expect("capacity exhaustion is optional");
    assert_eq!(
        outcome,
        SeedOutcome::Skipped {
            reason: "storage capacity exhausted"
        }
    );
    assert!(!tree.target().join(stage_name).exists());
}

#[test]
fn stage_replacement_is_never_removed_as_owned_cleanup() {
    let tree = Tree::new();
    let target = open_dir_path(&tree.target()).expect("target fd");
    ufs::mkdirat(&target, ".velnor-seed-owned", Mode::from_raw_mode(0o700)).expect("owned stage");
    let fd = openat_dir(&target, ".velnor-seed-owned").expect("stage fd");
    let identity = Identity::from_stat(&ufs::fstat(&fd).expect("stage stat"));
    fs::rename(
        tree.target().join(".velnor-seed-owned"),
        tree.0.join("held-stage"),
    )
    .expect("hold original stage");
    ufs::mkdirat(&target, ".velnor-seed-owned", Mode::from_raw_mode(0o700))
        .expect("foreign replacement");
    fs::write(
        tree.target().join(".velnor-seed-owned/foreign"),
        b"preserve",
    )
    .expect("foreign file");
    let mut stages = vec![Stage {
        name: ".velnor-seed-owned".to_owned(),
        fd,
        identity,
    }];
    assert!(cleanup_stages(&target, &mut stages).is_err());
    assert_eq!(
        fs::read(tree.target().join(".velnor-seed-owned/foreign")).expect("foreign bytes"),
        b"preserve"
    );
}
