use super::*;

#[test]
fn only_explicit_tap_reads_can_use_direct_or_retained_authority() {
    for op in [
        "admit-homebrew-tap-v1",
        "write-homebrew-tap-pr-v1",
        "merge-homebrew-tap-pr-v1",
    ] {
        for event in ["workflow_run", "workflow_dispatch"] {
            assert_eq!(
                route(Some(op), Some(event), None, Some("")).expect("direct"),
                VerificationRoute::DirectTap
            );
        }
        for event in ["workflow_dispatch", "schedule"] {
            assert_eq!(
                route(Some(op), Some(event), Some("11"), Some("2")).expect("retained"),
                VerificationRoute::Retained
            );
        }
        for (event, id, attempt) in [
            ("workflow_run", Some("11"), Some("2")),
            ("schedule", None, None),
            ("push", None, None),
            ("workflow_dispatch", Some("11"), None),
        ] {
            assert!(route(Some(op), Some(event), id, attempt).is_err());
        }
    }
    for op in [
        None,
        Some("sign-package-release-v1"),
        Some("publish-package-release-v1"),
        Some("prepare-homebrew-tap-v1"),
        Some("recover-package-release-v1"),
        Some("unknown"),
    ] {
        assert_eq!(
            route(op, Some("workflow_dispatch"), Some("11"), Some("2")).expect("signing path"),
            VerificationRoute::Admitted
        );
    }
}

#[test]
fn direct_tap_entry_rejects_cross_channel_events_and_stable_nonrelease_before_http() {
    for channel in [
        PackageReleaseChannel::Preview,
        PackageReleaseChannel::Stable,
    ] {
        for operation in [
            "admit-homebrew-tap-v1",
            "write-homebrew-tap-pr-v1",
            "merge-homebrew-tap-pr-v1",
        ] {
            let temp = tempfile::tempdir().expect("private fixture");
            let runner = temp.path().canonicalize().expect("canonical runner");
            let root = runner.join("velnor");
            fs::create_dir(&root).expect("root");
            let mut command = fixture(&runner, &root, channel);
            command
                .env("VELNOR_INTERNAL_OP", operation)
                .env("VELNOR_DIRECT_NEGATIVE", "1")
                .env(
                    "GITHUB_EVENT_NAME",
                    if channel == PackageReleaseChannel::Preview {
                        "workflow_dispatch"
                    } else {
                        "workflow_run"
                    },
                );
            assert!(command.status().expect("cross-channel child").success());
            if channel == PackageReleaseChannel::Stable {
                command.env("GITHUB_EVENT_NAME", "workflow_dispatch");
                for phase in ["prepare", "", "invalid"] {
                    assert!(
                        command
                            .env("VELNOR_STABLE_OPERATION", phase)
                            .status()
                            .expect("phase child")
                            .success()
                    );
                }
            }
        }
    }
}
