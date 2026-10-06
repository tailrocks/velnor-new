//! P08 Mise cache policy: paths, transport, sources, trust (C1,C3-C6,C9-C12).

use velnor_actions_contract::StepRole;
use velnor_actions_mise::cache_sources as sources;
use velnor_actions_mise::cache_transport as transport;
use velnor_actions_mise::cache_trust as trust;
use velnor_actions_mise::runtime_paths as paths;

#[test]
fn c1_inventory_lists_every_runtime_path_with_one_owner() {
    let inv = paths::inventory();
    for id in [
        "mise-installs",
        "rustup-toolchains",
        "cargo-registry-sources",
        "cargo-git-sources",
        "cargo-install-binaries",
        "cargo-install-receipt",
        "cargo-install-receipt-json",
        "cargo-target",
        "mbx-objects",
        "tofu-provider-cache",
    ] {
        assert!(paths::is_known_id(id), "missing {id}");
        assert!(!paths::owner_for(id).expect("owner").is_empty());
    }
    assert_eq!(inv.len(), 11, "exact inventory size");
    assert_eq!(
        paths::owner_for("cargo-registry-sources").expect("registry owner"),
        "velnor/sources"
    );
    assert_eq!(
        paths::owner_for("cargo-install-binaries").expect("binary owner"),
        "catalog/tools"
    );
    assert_eq!(
        paths::owner_for("tofu-provider-cache").expect("owner"),
        "velnor/tofu-providers"
    );
}

#[test]
fn c1_dangling_symlink_never_counts_as_warm() {
    assert!(paths::is_warm_toolchain(true, true, Some(true)));
    assert!(!paths::is_warm_toolchain(true, true, Some(false)));
    assert!(!paths::is_warm_toolchain(true, true, None));
    assert!(!paths::is_warm_toolchain(false, true, Some(true)));
    assert!(!paths::is_warm_toolchain(true, false, Some(true)));
    assert!(paths::validate_no_credentials("/cargo/registry/cache/x").is_ok());
    assert!(paths::validate_no_credentials("/cargo/credentials.toml").is_err());
    assert!(paths::owner_for("nope").is_err());
}

#[test]
fn c3_subset_lives_at_real_home_without_credentials() {
    let home = "${{ runner.temp }}/velnor/cargo";
    let got = sources::sources_cache_paths(home).expect("paths");
    assert_eq!(got.len(), 3);
    assert!(sources::validate_sources_subset(&got, home).is_ok());
    assert!(sources::sources_cache_paths("").is_err());
    for bad in [
        format!("{home}/credentials.toml"),
        format!("{home}/registry/src/x"),
        format!("{home}/../escape"),
        "~/.cargo/registry/cache".to_owned(),
    ] {
        assert!(
            sources::validate_sources_subset(std::slice::from_ref(&bad), home).is_err(),
            "must reject {bad}"
        );
    }
}

#[test]
fn c3_single_trusted_writer_never_races() {
    assert!(sources::is_trusted_writer("plan"));
    for role in ["rust-a", "rust-b", "lint", "required", ""] {
        assert!(!sources::is_trusted_writer(role), "{role} must not save");
    }
}

#[test]
fn c4_restore_and_mbx_precede_fetch_with_offline_skip() {
    let good = [
        Some(StepRole::Checkout),
        Some(StepRole::PreparePinnedTools),
        Some(StepRole::CargoSourcesRestore),
        Some(StepRole::MbxCache),
        Some(StepRole::CargoSourcesFetch),
        None,
    ];
    assert!(sources::check_restore_before_fetch(&good, true).is_ok());
    let good_names = [
        "Checkout",
        "Prepare pinned tools",
        "Restore Cargo sources",
        "Setup MBX",
        "Fetch Cargo sources",
        "Clippy",
    ]
    .iter()
    .map(ToString::to_string)
    .collect::<Vec<_>>();
    assert!(
        sources::check_steps_before_fetch(&good_names, &["Restore Cargo sources", "Setup MBX"])
            .is_ok()
    );
    let fetch_first = [
        Some(StepRole::Checkout),
        Some(StepRole::CargoSourcesFetch),
        Some(StepRole::MbxCache),
        Some(StepRole::CargoSourcesRestore),
    ];
    assert!(sources::check_restore_before_fetch(&fetch_first, true).is_err());
    let fetch_first_names = [
        "Checkout",
        "Fetch Cargo sources",
        "Setup MBX",
        "Restore Cargo sources",
    ]
    .iter()
    .map(ToString::to_string)
    .collect::<Vec<_>>();
    assert!(
        sources::check_steps_before_fetch(
            &fetch_first_names,
            &["Restore Cargo sources", "Setup MBX"]
        )
        .is_err()
    );
    assert_eq!(
        sources::fetch_decision(true, "no_entry").expect("skip"),
        sources::FetchDecision::OfflineSkip
    );
    let miss = sources::fetch_decision(false, "no_entry").expect("fetch");
    assert!(matches!(miss, sources::FetchDecision::ExplicitFetch { .. }));
    assert!(sources::fetch_decision(false, "bogus").is_err());
}

#[test]
fn c5_qualified_transport_is_objects_plus_shared_with_numbers() {
    assert_eq!(
        transport::QUALIFIED_TRANSPORT,
        transport::MbxTransport::ObjectsPlusSharedSources
    );
    assert!(transport::is_qualified(
        transport::MbxTransport::ObjectsPlusSharedSources
    ));
    assert!(!transport::is_qualified(
        transport::MbxTransport::TargetPerCrate
    ));
    let target = transport::numbers_for(transport::MbxTransport::TargetPerCrate);
    let objects = transport::numbers_for(transport::MbxTransport::ObjectsPlusSharedSources);
    assert!(objects.stored_mib < target.stored_mib, "stored must shrink");
    assert!(
        objects.duplicate_mib < target.duplicate_mib,
        "duplicates must shrink"
    );
    assert!(
        objects.transfer_mib < target.transfer_mib,
        "transfer must shrink"
    );
    assert_eq!(transport::CRATE_JOBS, 7);
}

#[test]
fn c6_no_path_has_two_owners() {
    let disjoint = [
        ("~/.local/share/mise", "catalog/tools"),
        ("$CARGO_HOME/registry", "velnor/sources"),
        ("mr-boxington-action/objects", "mr-boxington/MBX"),
    ];
    assert!(transport::check_no_double_owner(&disjoint).is_ok());
    let doubled = [
        ("$CARGO_HOME/registry", "velnor/sources"),
        ("$CARGO_HOME/registry/cache", "other/cargo"),
    ];
    assert!(transport::check_no_double_owner(&doubled).is_err());
    let same_owner = [
        ("$CARGO_HOME/registry", "velnor/sources"),
        ("$CARGO_HOME/registry/cache", "velnor/sources"),
    ];
    assert!(transport::check_no_double_owner(&same_owner).is_ok());
}

#[test]
fn c9_pr_action_capability_keeps_forks_read_only() {
    assert!(!trust::pr_save_allowed(false, false, "pull_request"));
    assert!(trust::pr_save_allowed(true, false, "pull_request"));
    assert!(!trust::pr_save_allowed(true, true, "pull_request"));
    assert!(!trust::pr_save_allowed(true, false, "push"));
    assert!(trust::is_read_only(true));
    assert!(!trust::is_read_only(false));
    assert!(trust::pr_outputs_trusted("trusted"));
    assert!(!trust::pr_outputs_trusted("pr"));
    assert_eq!(
        velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION,
        "success() && github.event_name == 'push'"
    );
}

#[test]
fn c10_save_needs_success_delta_scope_and_writers() {
    use velnor_actions_mise::cache_trust::SaveGate;
    let open = SaveGate {
        producer_passed: true,
        useful_delta: true,
        trust_scope_ok: true,
        writers_finished: true,
    };
    assert!(trust::save_after_success(open));
    for gate in [
        SaveGate {
            producer_passed: false,
            ..open
        },
        SaveGate {
            useful_delta: false,
            ..open
        },
        SaveGate {
            trust_scope_ok: false,
            ..open
        },
        SaveGate {
            writers_finished: false,
            ..open
        },
    ] {
        assert!(!trust::save_after_success(gate), "{gate:?}");
    }
    for (cache_err, passed) in [(true, true), (true, false), (false, true)] {
        assert!(
            !trust::cache_error_fails_verification(cache_err, passed),
            "cache errors never fail verification"
        );
    }
}

#[test]
fn c10b_trusted_save_authorizes_only_the_push_only_gate() {
    use velnor_actions_mise::restore::{SaveInputs, save_decision};

    let gate = trust::authorize_trusted_save().expect("authorized");
    assert_eq!(
        gate,
        velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION
    );
    // Step-level `if:` replaces the default `success()`; without it the
    // save would run after a failed producer and poison the trusted
    // layer with failed output.
    assert!(
        gate.contains("success()"),
        "emitted gate restates success(): {gate}"
    );
    let trusted_push = SaveInputs {
        layer_trust: "trusted",
        event: "push",
        passed: true,
        unavailable: false,
        active_writer: false,
    };
    assert!(save_decision(&trusted_push).is_ok());
    assert!(velnor_actions_mise::cache::save_allowed(
        "trusted", "push", true
    ));
    // An action's PR-save capability does not authorize trusted PR writes.
    assert!(trust::pr_save_allowed(true, false, "pull_request"));
    assert!(!trust::pr_save_allowed(true, true, "pull_request"));

    for (event, source) in [
        ("pull_request", "same-repository or fork PR"),
        ("pull_request_target", "PR target"),
        ("merge_group", "merge queue"),
        ("schedule", "scheduled run"),
        ("workflow_dispatch", "manual run"),
        ("workflow_call", "reusable workflow"),
        ("Push", "case variant"),
        ("", "unknown event"),
    ] {
        let denied = SaveInputs {
            event,
            ..trusted_push
        };
        assert!(save_decision(&denied).is_err(), "{source} must not save");
        assert!(
            !velnor_actions_mise::cache::save_allowed("trusted", event, true),
            "{source} must not pass the save allowlist"
        );
    }
}

#[test]
fn c11_quota_from_service_data_not_hardcoded() {
    let body = r#"[{"sizeInBytes":50513706},{"sizeInBytes":54077706}]"#;
    let usage = trust::parse_service_usage(body).expect("usage");
    assert_eq!(usage.active_bytes, 50_513_706 + 54_077_706);
    assert_eq!(usage.count, 2);
    assert!(trust::parse_service_usage("nope").is_err());
    assert_eq!(trust::headroom_bytes(100, 1000).expect("room"), 900);
    assert!(trust::headroom_bytes(1001, 1000).is_err());
    let (stored, transfer) = trust::stored_vs_transfer(100, 7);
    assert_eq!((stored, transfer), (100, 700));
}

#[test]
fn c12_remote_mbx_backends_rejected() {
    for ok in ["github", "local"] {
        assert!(transport::assert_no_remote_cache(ok).is_ok());
    }
    for bad in ["server", "remote", "s3", "remote-cache"] {
        assert!(transport::assert_no_remote_cache(bad).is_err());
    }
}

#[test]
fn c13_usage_report_composes_service_parse_quota_and_transfer() {
    // Live `gh cache list --json` shape (fixed format sample, not a
    // measurement): one shared sources entry plus two V2 tools entries.
    let body = r#"[{"key":"velnor-v1-sources-x86_64-unknown-linux-gnu-1.98.1-aa","sizeInBytes":17568922},{"key":"mise-tools-v2-typed-runtime-bb","sizeInBytes":65857248},{"key":"mise-tools-v2-typed-runtime-cc","sizeInBytes":54077706}]"#;
    let report = trust::summarize_cache_usage(body, 10_737_418_240, 17_568_922, 8).expect("report");
    assert_eq!(report.active_bytes, 17_568_922 + 65_857_248 + 54_077_706);
    assert_eq!(report.count, 3);
    assert_eq!(report.limit_bytes, 10_737_418_240);
    assert_eq!(report.headroom_bytes, 10_737_418_240 - report.active_bytes);
    assert_eq!(report.stored_bytes, 17_568_922);
    assert_eq!(report.restoring_jobs, 8);
    assert_eq!(report.aggregate_transfer_bytes, 17_568_922 * 8);
    assert!(trust::summarize_cache_usage("nope", 1000, 10, 1).is_err());
    assert!(trust::summarize_cache_usage(body, 1, 10, 1).is_err());
}
