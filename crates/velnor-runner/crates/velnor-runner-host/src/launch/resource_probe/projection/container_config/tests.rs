use crate::journal::ResourceProbePurpose;

use super::super::{DockerRoot, VerifiedProbeImage};

#[test]
fn probe_purpose_sets_command_labels_and_input_streams() -> Result<(), crate::error::HostError> {
    let root = DockerRoot::parse("/var/lib/docker")?;
    let image = VerifiedProbeImage {
        runtime_id: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        platform: "linux/amd64".to_owned(),
        daemon_architecture: "x86_64".to_owned(),
        source_revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        binding_fingerprint: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
    };
    let request = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let cases = [
        (ResourceProbePurpose::Capacity, None, None, false, None),
        (
            ResourceProbePurpose::OwnedCgroups,
            Some("owned-cgroups"),
            Some("owned_container_cgroup"),
            true,
            Some(request),
        ),
        (
            ResourceProbePurpose::CleanupObserver,
            Some("cleanup-observer"),
            Some("cleanup_observer"),
            true,
            Some(request),
        ),
    ];

    for (purpose, label, mode, accepts_stdin, request_label) in cases {
        let config = super::config(
            &root,
            &image,
            "dddddddddddddddddddddddddddddddd",
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            "engine-1",
            purpose,
            request,
        );
        assert_eq!(config.attach_stdin, Some(accepts_stdin));
        assert_eq!(config.open_stdin, Some(accepts_stdin));
        assert_eq!(config.stdin_once, Some(accepts_stdin));
        assert_eq!(
            config.cmd,
            mode.map(|mode| vec!["--mode".to_owned(), mode.to_owned()])
        );
        let labels = config.labels.ok_or(crate::error::HostError::Identity)?;
        assert_eq!(
            labels.get("velnor.probe.purpose").map(String::as_str),
            label
        );
        assert_eq!(
            labels.get("velnor.probe.request").map(String::as_str),
            request_label
        );
        assert_eq!(config.network_disabled, Some(true));
        let host = config
            .host_config
            .ok_or(crate::error::HostError::Identity)?;
        assert_eq!(host.privileged, Some(false));
        assert_eq!(host.readonly_rootfs, Some(true));
        assert_eq!(host.network_mode.as_deref(), Some("none"));
        assert_eq!(host.cpu_period, Some(100_000));
        assert_eq!(host.cpu_quota, Some(10_000));
        assert_eq!(host.memory, Some(134_217_728));
        assert_eq!(host.memory_swap, Some(134_217_728));
        assert_eq!(host.pids_limit, Some(4));
        assert_eq!(
            host.cap_drop.as_deref(),
            Some(["ALL".to_owned()].as_slice())
        );
        assert_eq!(host.mounts.as_ref().map(Vec::len), Some(2));
    }
    Ok(())
}
