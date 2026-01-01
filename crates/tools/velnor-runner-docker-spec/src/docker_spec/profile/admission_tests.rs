use std::time::{SystemTime, UNIX_EPOCH};

use super::{RunnerImageProfile, resolve_from_pins, valid_profile_pin};
use velnor_runner_journal::HostError;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn synthetic_ubuntu26_pin() -> RunnerImageProfile {
    RunnerImageProfile {
        key: "ubuntu-26.04-amd64",
        scale_set_name: "ubuntu-26.04-scale-set",
        platform: "linux/amd64",
        runner_image: "ghcr.io/actions/actions-runner@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        runner_manifest_digest: DIGEST,
        runner_index_digest: DIGEST,
        runner_config_digest: DIGEST,
        runner_os: "ubuntu26",
        runner_version: "2.338.0",
        runner_release_published_at: "2026-10-06T13:55:11Z",
        runner_requalify_by: "2026-11-05T13:55:11Z",
        runner_requalify_by_unix: 1_793_886_911,
        dind_image: "docker.io/library/docker@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        dind_manifest_digest: DIGEST,
        dind_index_digest: DIGEST,
        dind_config_digest: DIGEST,
        dind_version: "29.8.2",
        dind_source: "docker-library/official-images@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa;docker-library/docker@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:29/dind",
        dind_entrypoint_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        runner_uid: 1_001,
        runner_gid: 1_001,
        runner_docker_gid: 123,
        dind_socket_group: "docker",
        dind_socket_gid: 2_375,
    }
}

fn before_deadline() -> SystemTime {
    UNIX_EPOCH + std::time::Duration::from_secs(1_793_886_910)
}

#[test]
fn synthetic_ubuntu26_source_pin_can_resolve_but_is_not_compiled_for_production() {
    let pin = synthetic_ubuntu26_pin();
    assert!(valid_profile_pin(&pin));
    assert_eq!(
        resolve_from_pins(
            "ubuntu-26.04-amd64",
            "ubuntu-26.04-scale-set",
            before_deadline(),
            Some(&pin),
        ),
        Ok(pin)
    );
}

#[test]
fn production_resolver_has_no_fabricated_ubuntu26_pin_or_ubuntu24_fallback() {
    assert_eq!(
        super::resolve_linux_admission_profile("ubuntu-26.04-amd64", "ubuntu-26.04-scale-set",),
        Err(HostError::Config)
    );
    assert_eq!(
        super::resolve_linux_admission_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set",),
        Err(HostError::Config)
    );
    let legacy =
        super::super::resolve_runner_profile("ubuntu-24.04-amd64", "ubuntu-24.04-scale-set")
            .expect("legacy static identity remains available for plan fixtures");
    assert_eq!(
        super::validate_linux_admission_profile(legacy),
        Err(HostError::Config)
    );
}

#[test]
fn only_exact_ubuntu26_selector_and_scale_set_can_resolve() {
    let pin = synthetic_ubuntu26_pin();
    assert_eq!(
        resolve_from_pins(
            "ubuntu-24.04-amd64",
            "ubuntu-24.04-scale-set",
            before_deadline(),
            Some(&pin),
        ),
        Err(HostError::Config)
    );
    assert_eq!(
        resolve_from_pins(
            "ubuntu-26.04-amd64",
            "ubuntu-24.04-scale-set",
            before_deadline(),
            Some(&pin),
        ),
        Err(HostError::Config)
    );
}

#[test]
fn synthetic_ubuntu26_profile_cannot_build_a_plan_without_a_compiled_pin() {
    let profile = synthetic_ubuntu26_pin();
    assert_eq!(
        super::super::runner_plan_for_profile("worker_a", &profile),
        Err(HostError::Config)
    );
}

#[test]
fn synthetic_profile_rejects_unpinned_or_wrong_image_facts() {
    let pin = synthetic_ubuntu26_pin();
    let invalid = [
        RunnerImageProfile {
            runner_image: "ghcr.io/actions/actions-runner:ubuntu26",
            ..pin
        },
        RunnerImageProfile {
            runner_manifest_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ..pin
        },
        RunnerImageProfile {
            runner_index_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaA",
            ..pin
        },
        RunnerImageProfile {
            runner_os: "ubuntu24",
            ..pin
        },
        RunnerImageProfile {
            platform: "linux/arm64",
            ..pin
        },
        RunnerImageProfile {
            runner_version: "2.338",
            ..pin
        },
        RunnerImageProfile {
            dind_source: "docker-library/docker@floating:29/dind",
            ..pin
        },
        RunnerImageProfile {
            runner_uid: 0,
            ..pin
        },
    ];
    for candidate in invalid {
        assert!(!valid_profile_pin(&candidate));
    }
}

#[test]
fn synthetic_profile_is_stale_at_its_exact_requalification_cutoff() {
    let pin = synthetic_ubuntu26_pin();
    let deadline = UNIX_EPOCH + std::time::Duration::from_secs(pin.runner_requalify_by_unix);
    assert_eq!(
        resolve_from_pins(
            "ubuntu-26.04-amd64",
            "ubuntu-26.04-scale-set",
            deadline,
            Some(&pin),
        ),
        Err(HostError::Config)
    );
}
