// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_contract::{StepKind, StepRole};

use super::state_producer::{ROOTS, build, job};
use super::state_skeleton_rig::{HASH, LANES, OTHER_HASH, Rig, read};

#[test]
fn restore_returns_the_predecessor_prediction_and_removes_only_member_outputs()
-> Result<(), Box<dyn Error>> {
    let rig = Rig::new()?;
    rig.seed_predecessor()?;
    rig.stub()?;
    rig.simulate_phase_one()?;
    rig.seed_member_and_registry_outputs()?;
    assert_eq!(read(&rig.workspace.join("foo/src/lib.rs"))?, "");

    let restore = rig.restore()?;
    assert!(
        restore.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&restore.stderr)
    );

    assert_eq!(
        read(&rig.manifests().join("v1/project.json"))?,
        "predecessor"
    );
    assert!(!rig.manifests().join("v1/stub-only.json").exists());
    assert!(!rig.store().join("incremental").exists());
    assert!(rig.store().join("actions/cas").is_dir());
    assert_eq!(
        read(&rig.workspace.join("foo/src/lib.rs"))?,
        "pub fn f() {}\n"
    );
    for lane in LANES {
        for gone in [
            format!("deps/libfoo-{HASH}.rlib"),
            format!("deps/libfoo-{HASH}.rmeta"),
            format!("deps/foo-{HASH}.d"),
            format!("deps/it-{HASH}"),
            format!("deps/it-{HASH}.d"),
            format!("deps/libmy_crate-{HASH}.rlib"),
            format!("deps/my_crate-{HASH}.d"),
            format!("deps/my_crate-{HASH}"),
            format!("build/foo-{HASH}"),
            format!("build/foo-{OTHER_HASH}"),
            format!("build/my-crate-{HASH}"),
        ] {
            assert!(
                !rig.exists(&format!("{lane}/{gone}")),
                "{lane}/{gone} stayed"
            );
        }
        for kept in [
            format!("deps/libserde-{HASH}.rlib"),
            format!("deps/serde-{HASH}.d"),
            format!("deps/libfoo_bar-{HASH}.rlib"),
            format!("deps/foo_bar-{HASH}.d"),
            format!("deps/libfoo-{HASH}.d"),
            format!("deps/build_script_build-{HASH}"),
            format!("deps/foo-{HASH}x"),
            format!("build/foo-bar-{HASH}/output"),
            format!("build/serde-{HASH}/output"),
            format!("build/foo-extra-{HASH}/output"),
        ] {
            assert!(
                rig.exists(&format!("{lane}/{kept}")),
                "{lane}/{kept} was removed"
            );
        }
    }
    Ok(())
}

#[test]
fn a_store_without_predecessor_manifests_ends_without_the_stub_prediction()
-> Result<(), Box<dyn Error>> {
    let rig = Rig::new()?;
    fs::create_dir_all(rig.store().join("actions"))?;
    rig.stub()?;
    rig.simulate_phase_one()?;
    rig.touch("target/debug/deps/placeholder")?;

    let restore = rig.restore()?;
    assert!(
        restore.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&restore.stderr)
    );

    assert!(!rig.manifests().join("v1/stub-only.json").exists());
    assert!(!rig.manifests().join("v1/project.json").exists());
    Ok(())
}

#[test]
fn restore_without_the_metadata_record_changes_nothing() -> Result<(), Box<dyn Error>> {
    let rig = Rig::new()?;
    rig.seed_predecessor()?;
    rig.stub()?;
    rig.simulate_phase_one()?;
    rig.seed_member_and_registry_outputs()?;
    fs::remove_file(rig.temp.path().join("velnor/state-skeleton/metadata.json"))?;

    let restore = rig.restore()?;

    assert!(!restore.status.success(), "restore ran without metadata");
    assert_eq!(
        read(&rig.manifests().join("v1/project.json"))?,
        "stub-inputs"
    );
    assert!(rig.store().join("incremental/state").is_file());
    assert_eq!(read(&rig.workspace.join("foo/src/lib.rs"))?, "");
    for lane in LANES {
        assert!(rig.exists(&format!("{lane}/deps/libfoo-{HASH}.rlib")));
        assert!(rig.exists(&format!("{lane}/build/my-crate-{HASH}")));
    }
    assert_eq!(
        read(
            &rig.temp
                .path()
                .join("velnor/state-skeleton/task-manifests/v1/project.json")
        )?,
        "predecessor"
    );
    Ok(())
}

#[test]
fn restore_fails_closed_when_the_recorded_target_directory_does_not_exist()
-> Result<(), Box<dyn Error>> {
    let rig = Rig::new()?;
    rig.seed_predecessor()?;
    rig.stub()?;
    rig.simulate_phase_one()?;
    rig.touch("elsewhere/debug/deps/libfoo-0123456789abcdef.rlib")?;

    let restore = rig.restore()?;

    assert!(
        !restore.status.success(),
        "restore ran without the target directory"
    );
    assert_eq!(
        read(&rig.manifests().join("v1/project.json"))?,
        "stub-inputs"
    );
    assert_eq!(read(&rig.workspace.join("foo/src/lib.rs"))?, "");
    assert!(rig.exists("elsewhere/debug/deps/libfoo-0123456789abcdef.rlib"));
    Ok(())
}

#[test]
fn a_registry_crate_that_shares_a_members_name_loses_its_outputs() -> Result<(), Box<dyn Error>> {
    let rig = Rig::new()?;
    rig.seed_predecessor()?;
    rig.stub()?;
    rig.simulate_phase_one()?;
    rig.touch(&format!("target/debug/deps/libfoo-{OTHER_HASH}.rlib"))?;
    rig.touch(&format!("target/debug/deps/foo-{OTHER_HASH}.d"))?;

    let restore = rig.restore()?;
    assert!(
        restore.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&restore.stderr)
    );

    assert!(!rig.exists(&format!("target/debug/deps/libfoo-{OTHER_HASH}.rlib")));
    assert!(!rig.exists(&format!("target/debug/deps/foo-{OTHER_HASH}.d")));
    Ok(())
}

#[test]
fn every_producer_lane_builds_under_the_target_directory_the_restore_cleans()
-> Result<(), Box<dyn Error>> {
    let built = build()?;
    for (producer_id, _) in ROOTS {
        let producer = job(&built.jobs, producer_id)?;
        let root = producer
            .steps
            .iter()
            .find_map(|step| match (&step.role, &step.kind) {
                (Some(StepRole::StateSkeleton), StepKind::Shell { env, .. }) => {
                    env.get("VELNOR_MBX_BUNDLE_ROOT").cloned()
                }
                _ => None,
            })
            .ok_or("producer lost its skeleton step")?;
        let target = if root.is_empty() || root == "." {
            "target".to_owned()
        } else {
            format!("{root}/target")
        };
        let mut redirected = 0;
        for step in &producer.steps {
            let StepKind::TaskExecution { env, .. } = &step.kind else {
                continue;
            };
            if let Some(dir) = env.get("CARGO_TARGET_DIR") {
                assert!(
                    dir == &target || dir.starts_with(&format!("{target}/")),
                    "{producer_id}: lane builds in {dir}, outside {target}"
                );
                redirected += 1;
            }
        }
        assert!(
            redirected > 0,
            "{producer_id}: no lane sets a target directory"
        );
    }
    Ok(())
}
