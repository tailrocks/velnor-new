use super::{View, initialized_job_facts_match};
use crate::journal::connection::StorageObserver;

fn worker(observer: StorageObserver, target: &str, inode: i64, subpath: &str) -> View {
    View {
        observer,
        target: target.to_owned(),
        container_id: Some(
            match observer {
                StorageObserver::Runner => "runner-container",
                StorageObserver::Dind => "dind-container",
                StorageObserver::Probe => "",
            }
            .to_owned(),
        ),
        read_only: true,
        device: (0, 48),
        statx_device: (0, 48),
        fs_type: 0x9123_683e,
        fsid: "0000000000000048".to_owned(),
        digest: "a".repeat(64),
        mount_root: format!("/metadata/{subpath}"),
        inode,
        subpath: Some(subpath.to_owned()),
    }
}

fn worker_views(job_facts_inode: i64) -> Vec<View> {
    vec![
        worker(
            StorageObserver::Runner,
            "/.velnor-runtime-identity",
            64,
            "runtime-identity",
        ),
        worker(
            StorageObserver::Runner,
            "/opt/velnor/job-facts",
            job_facts_inode,
            "job-facts",
        ),
        worker(
            StorageObserver::Runner,
            "/opt/velnor/source-proof/control",
            67,
            "source-proof/control",
        ),
        worker(
            StorageObserver::Dind,
            "/.velnor-runtime-identity",
            64,
            "runtime-identity",
        ),
        worker(
            StorageObserver::Dind,
            "/opt/velnor/job-facts",
            job_facts_inode,
            "job-facts",
        ),
    ]
}

#[test]
fn initialized_child_anchor_rejects_consistent_same_device_substitution() {
    let captured = worker_views(65);
    assert!(initialized_job_facts_match(&captured, Some(65)));

    let substituted = worker_views(66);
    assert!(!initialized_job_facts_match(&substituted, Some(65)));
    assert!(!initialized_job_facts_match(&substituted, None));
}
