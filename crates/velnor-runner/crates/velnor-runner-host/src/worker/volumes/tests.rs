use std::collections::HashMap;

use bollard::models::Volume;
use velnor_runner_core::runner_work_path;

use super::{WorkerVolume, WorkerVolumeRole, owns};

fn sample(name: &str, labels: &[(&str, &str)]) -> Volume {
    Volume {
        name: name.to_owned(),
        driver: "local".to_owned(),
        mountpoint: "/var/lib/docker/volumes/test".to_owned(),
        created_at: None,
        status: None,
        labels: labels
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
        scope: None,
        cluster_volume: None,
        options: HashMap::new(),
        usage_data: None,
    }
}

fn expected() -> WorkerVolume {
    WorkerVolume {
        worker: "w135".to_owned(),
        name: "w135-work".to_owned(),
        role: WorkerVolumeRole::Work,
    }
}

#[test]
fn unlabeled_exact_name_is_kept() {
    let volume = sample("w135-work", &[]);
    assert!(!owns(&expected(), &volume));
}

#[test]
fn exact_labels_are_owned() {
    let volume = sample(
        "w135-work",
        &[("velnor.worker", "w135"), ("velnor.role", "work")],
    );
    assert!(owns(&expected(), &volume));
}

#[test]
fn metadata_volume_has_engine_metadata_role_and_no_container_target() {
    let role = WorkerVolumeRole::RunnerMetadata;
    let volume = super::worker_volume("w135", role).expect("correct worker identity");
    assert_eq!(volume.name, "w135-metadata");
    assert_eq!(role.label(), "engine-metadata");
    assert_eq!(super::target(role, super::ContainerKind::Runner), None);
    assert_eq!(super::target(role, super::ContainerKind::Dind), None);
}

#[test]
fn runner_and_dind_keep_home_and_work_as_distinct_shared_volumes() {
    let runner = super::mounts_for_runner("w135").expect("runner mounts");
    let runner_plan = super::mounts_for_runner_plan("w135").expect("runner plan mounts");
    let dind = super::mounts_for_dind("w135").expect("dind mounts");

    assert_eq!(
        runner
            .iter()
            .map(|mount| mount.source.as_str())
            .collect::<Vec<_>>(),
        ["volume:w135", "volume:w135-home", "volume:w135-work"]
    );
    assert_eq!(
        runner
            .iter()
            .map(|mount| mount.target.as_str())
            .collect::<Vec<_>>(),
        ["/run", "/home/runner", runner_work_path().as_str()]
    );
    assert_eq!(
        runner_plan
            .iter()
            .map(|mount| mount.target.as_str())
            .collect::<Vec<_>>(),
        ["/run", runner_work_path().as_str()]
    );
    assert_eq!(
        dind.iter()
            .map(|mount| mount.source.as_str())
            .collect::<Vec<_>>(),
        [
            "volume:w135",
            "volume:w135-home",
            "volume:w135-work",
            "volume:w135-docker"
        ]
    );
    assert!(
        !runner
            .iter()
            .any(|mount| mount.source.ends_with("-metadata"))
    );
    assert!(!dind.iter().any(|mount| mount.source.ends_with("-metadata")));
}

#[test]
fn foreign_worker_label_is_kept() {
    let volume = sample(
        "w135-work",
        &[("velnor.worker", "other"), ("velnor.role", "work")],
    );
    assert!(!owns(&expected(), &volume));
}

#[test]
fn partial_label_is_kept() {
    let volume = sample("w135-work", &[("velnor.worker", "w135")]);
    assert!(!owns(&expected(), &volume));
}

#[test]
fn extra_label_is_kept() {
    let volume = sample(
        "w135-work",
        &[
            ("velnor.worker", "w135"),
            ("velnor.role", "work"),
            ("other", "1"),
        ],
    );
    assert!(!owns(&expected(), &volume));
}

#[test]
fn wrong_name_is_kept() {
    let volume = sample("other", &[]);
    assert!(!owns(&expected(), &volume));
}
