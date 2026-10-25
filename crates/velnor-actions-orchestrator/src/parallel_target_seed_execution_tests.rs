use std::{
    fs,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use super::*;

struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "velnor-parallel-seed-execution-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(path.join("target/debug/deps")).expect("debug tree");
        fs::write(path.join("target/debug/deps/sample.rlib"), b"artifact")
            .expect("baseline artifact");
        Self(path)
    }

    fn target(&self, lanes: &[ParallelTargetLane]) -> SeedTarget {
        prepare_target(&self.0, "target", lanes)
            .expect("target validation")
            .expect("baseline target")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn stages(target: &SeedTarget) -> Vec<Stage> {
    target
        .cold
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let name = format!(".velnor-seed-test-{index}");
            fs::create_dir(target.path.join(&name)).expect("stage directory");
            let fd = super::super::filesystem::openat_dir(&target.fd, &name).expect("stage fd");
            let identity = super::super::inventory::Identity::from_stat(
                &rustix::fs::fstat(&fd).expect("stage identity"),
            );
            Stage { name, fd, identity }
        })
        .collect()
}

fn snapshot() -> Snapshot {
    Snapshot::new(Instant::now(), super::super::DEFAULT_LIMITS)
}

#[test]
fn second_lane_admission_skip_preserves_the_first_completed_lane() {
    let tree = Tree::new();
    let target = tree.target(&[ParallelTargetLane::Clippy, ParallelTargetLane::Doc]);
    let mut stages = stages(&target);
    let snapshot = snapshot();
    let result = admit_stages_with_rename(&target, &snapshot, &stages, |target, lane, stage| {
        if lane == ParallelTargetLane::Doc {
            Err(SeedError::Skip("injected second lane limit"))
        } else {
            rename_stage(target, lane, stage)
        }
    });

    assert_eq!(
        finalize_staged_result(
            &target.fd,
            &mut stages,
            result,
            &target.cold,
            snapshot.bytes,
        )
        .expect("optional second lane limit"),
        SeedOutcome::Skipped {
            reason: "injected second lane limit"
        }
    );
    assert!(tree.0.join("target/velnor-clippy").is_dir());
    assert!(!tree.0.join("target/velnor-doc").exists());
    assert!(!tree.0.join("target/.velnor-seed-test-1").exists());
}

#[test]
fn unsupported_atomic_rename_skips_and_cleans_all_unadmitted_stages() {
    let tree = Tree::new();
    let target = tree.target(&[ParallelTargetLane::Clippy]);
    let mut stages = stages(&target);
    let snapshot = snapshot();
    let result = admit_stages_with_rename(&target, &snapshot, &stages, |_, _, _| {
        Err(SeedError::Skip("atomic exclusive rename unsupported"))
    });

    assert_eq!(
        finalize_staged_result(
            &target.fd,
            &mut stages,
            result,
            &target.cold,
            snapshot.bytes,
        )
        .expect("unsupported rename is optional"),
        SeedOutcome::Skipped {
            reason: "atomic exclusive rename unsupported"
        }
    );
    assert!(!tree.0.join("target/velnor-clippy").exists());
    assert!(!tree.0.join("target/.velnor-seed-test-0").exists());
}

#[test]
fn replaced_staging_name_fails_before_copy_and_preserves_foreign_contents() {
    let tree = Tree::new();
    let target = tree.target(&[ParallelTargetLane::Clippy]);
    let mut stages = stages(&target);
    let stage = stages.pop().expect("single stage");
    let name = target.path.join(&stage.name);
    fs::rename(&name, tree.0.join("held-stage")).expect("hold original stage");
    fs::create_dir(&name).expect("replacement stage");
    fs::write(name.join("foreign"), b"preserve").expect("foreign contents");

    assert!(ensure_stage_identity(&target.fd, &stage).is_err());
    assert_eq!(
        fs::read(name.join("foreign")).expect("replacement remains untouched"),
        b"preserve"
    );
}

#[test]
fn replacement_renamed_into_lane_is_detected_after_atomic_admission() {
    let tree = Tree::new();
    let target = tree.target(&[ParallelTargetLane::Clippy]);
    let mut stages = stages(&target);
    let snapshot = snapshot();
    let result = admit_stages_with_rename(&target, &snapshot, &stages, |target, lane, stage| {
        let name = target.path.join(&stage.name);
        fs::rename(&name, tree.0.join("held-stage")).expect("hold original stage");
        fs::create_dir(&name).expect("replacement stage");
        fs::write(name.join("foreign"), b"preserve").expect("foreign stage content");
        super::super::filesystem::rename_noreplace(&target.fd, &stage.name, lane.as_str())
            .map_err(super::super::filesystem::seed_io_error)
    });

    assert!(
        finalize_staged_result(
            &target.fd,
            &mut stages,
            result,
            &target.cold,
            snapshot.bytes,
        )
        .is_err()
    );
    assert_eq!(
        fs::read(tree.0.join("target/velnor-clippy/foreign")).expect("foreign lane content"),
        b"preserve"
    );
}
