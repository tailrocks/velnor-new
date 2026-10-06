//! Adapter-wire cases: event modes, save decisions, identities, freshness,
//! policy headers, archive identity, transfers, inventories, and Mise
//! tool-file routing plus inspection (F2 mise/rust halves).
use velnor_actions_contract::digest_b3;
use velnor_actions_contract_release::FreshnessRequirement;
use velnor_actions_mise::cache::mode_for_event;
use velnor_actions_mise::catalog::lock::{parse_version_policy, verify_policy_header};
use velnor_actions_mise::{
    ARCHIVE_FILE, ArchiveIdentityInputs, MISSING_RECOMMENDED_INPUT, MissReason, NextestArchive,
    NextestDriver, PinnedTool, RUST_VERSION, SaveInputs, TOOLING_INPUT_INVALID, ToolCatalog,
    archive_identity, archive_write_required, check_freshness_requirements, count_inventory_tests,
    inspect_mise_file, is_allowed_mise_subcommand, is_mise_env_symbol, lock_tool_versions,
    requires_archive_transfer, save_decision, save_useful, stack_for_symbol, writers_overlap,
};

#[test]
fn fork_event_is_read_only() {
    use velnor_actions_mise::TaskCacheMode;
    assert_eq!(
        mode_for_event("fork").expect("fork"),
        TaskCacheMode::ReadOnly
    );
    assert_eq!(
        mode_for_event("release").expect("release"),
        TaskCacheMode::Off
    );
    assert!(mode_for_event("bogus").is_err());
}

#[test]
fn save_decision_denies_with_reasons() {
    let save = |trust: &str, event: &str, passed: bool| {
        save_decision(&SaveInputs {
            layer_trust: trust,
            event,
            passed,
            unavailable: false,
            active_writer: false,
        })
    };
    assert_eq!(save("trusted", "push", true), Ok(()));
    assert_eq!(save("pr", "push", true), Ok(()));
    for (trust, event, passed) in [
        ("trusted", "pull_request", true),
        ("trusted", "push", false),
        ("pr", "pull_request", true),
        ("pr", "pull_request", false),
        ("pr", "push", false),
        ("pr", "merge_group", true),
        ("pr", "fork", true),
        ("pr", "release", true),
        ("pr", "local", true),
        ("unknown", "push", true),
    ] {
        assert_eq!(
            save(trust, event, passed),
            Err(MissReason::CACHE_WRITE_DISABLED),
            "{trust} {event} {passed}"
        );
    }
    let down = save_decision(&SaveInputs {
        layer_trust: "pr",
        event: "push",
        passed: true,
        unavailable: true,
        active_writer: false,
    });
    assert_eq!(down, Err(MissReason::CACHE_UNAVAILABLE));
    let overlap = save_decision(&SaveInputs {
        layer_trust: "pr",
        event: "push",
        passed: true,
        unavailable: false,
        active_writer: true,
    });
    assert_eq!(overlap, Err(MissReason::CACHE_WRITE_DISABLED));
}

#[test]
fn save_useful_needs_delta() {
    assert!(save_useful(false, false, false));
    assert!(!save_useful(true, false, false));
    assert!(!save_useful(false, true, false));
    assert!(!save_useful(false, false, true));
}

#[test]
fn writers_overlap_guards_layers() {
    let active = vec!["task".to_owned()];
    assert!(writers_overlap(&active, "task"));
    assert!(!writers_overlap(&active, "sources"));
    assert!(!writers_overlap(&[], "task"));
}

#[test]
fn catalog_identities_validate() {
    let catalog = ToolCatalog::pinned();
    let err = catalog
        .validate_identities()
        .expect_err("unbound digests never validate as trusted");
    assert!(err.to_string().contains("placeholder_digest"), "{err}");
    for tool in PinnedTool::ALL {
        let identity = catalog.tool_identity(tool);
        if tool == PinnedTool::Opentofu {
            assert!(
                identity.validate("catalog").is_ok(),
                "opentofu digest is bound (T15)"
            );
        } else {
            let err = identity
                .validate("catalog")
                .expect_err("placeholder digests never validate as trusted");
            assert!(err.to_string().contains("placeholder_digest"), "{err}");
        }
        assert!(identity.source.starts_with("https://"));
        assert!(!identity.platforms.is_empty());
    }
    assert_eq!(
        catalog.tool_identity(PinnedTool::Rust).version,
        RUST_VERSION
    );
    let custom = ToolCatalog::new(
        "1.98.1", "1.19.0", "2.101.0", "1.7.12", "0.11.0", "1.30.1", "0.9.146", "1.13.0",
    );
    assert!(custom.is_ok());
    assert!(
        ToolCatalog::new(
            "latest", "1.19.0", "2.101.0", "1.7.12", "0.11.0", "1.30.1", "0.9.146", "1.13.0"
        )
        .is_err()
    );
}

#[test]
fn freshness_requirements_enforced() {
    let valid = vec![FreshnessRequirement {
        class: "tools".to_owned(),
        max_age_days: 7,
    }];
    assert_eq!(check_freshness_requirements(&valid), Ok(()));
    assert_eq!(check_freshness_requirements(&[]), Ok(()));
    let unknown = vec![FreshnessRequirement {
        class: "nope".to_owned(),
        max_age_days: 7,
    }];
    assert!(check_freshness_requirements(&unknown).is_err());
    let zero = vec![FreshnessRequirement {
        class: "tools".to_owned(),
        max_age_days: 0,
    }];
    assert!(check_freshness_requirements(&zero).is_err());
}

fn policy_text() -> String {
    [
        "schema = 1",
        "channel = \"stable\"",
        "registry = \"https://example.com/releases\"",
        "check_interval_hours = 24",
        "max_exception_days = 14",
        "",
        "[runner]",
        "default = \"ubuntu-26.04\"",
        "supported = [\"ubuntu-26.04\", \"ubuntu-26.04-arm\"]",
        "",
    ]
    .join("\n")
}

#[test]
fn policy_header_validates_and_rejects_weakening() {
    let text = policy_text();
    assert_eq!(verify_policy_header(&text), Ok(()));
    assert_eq!(
        parse_version_policy(&text).expect("parse").channel,
        "stable"
    );
    let weak = text.replace("check_interval_hours = 24", "check_interval_hours = 48");
    assert!(verify_policy_header(&weak).is_err());
    let no_registry = text.replace("registry = \"https://example.com/releases\"\n", "");
    assert!(verify_policy_header(&no_registry).is_err());
    let bad_label = text.replace("ubuntu-26.04-arm", "ubuntu-latest");
    assert!(verify_policy_header(&bad_label).is_err());
    let dotted = text.replace("[runner]", "[github_runner_images.linux_x64]");
    assert_eq!(verify_policy_header(&dotted), Ok(()));
}

#[test]
fn archive_identity_binds_platform_and_config() {
    assert_eq!(ARCHIVE_FILE, "target/nextest/tests.tar.zst");
    let archive = NextestArchive::new(NextestDriver::Cargo, "demo", &["default".to_owned()], None)
        .expect("archive");
    let source = digest_b3(b"source");
    let toolchain = digest_b3(b"toolchain");
    let platform = digest_b3(b"platform");
    let other_platform = digest_b3(b"other-platform");
    let config = digest_b3(b"config");
    let inputs = ArchiveIdentityInputs {
        source_digest: &source,
        profile: "test",
        toolchain_id: &toolchain,
        runtime: "glibc",
        test_runner: "0.9.146",
        format: "tar.zst",
        platform_id: &platform,
        config_digest: &config,
    };
    let first = archive_identity(&archive, &inputs).expect("identity");
    let mut moved = inputs.clone();
    moved.platform_id = &other_platform;
    let second = archive_identity(&archive, &moved).expect("identity");
    assert_ne!(first, second);
    let mut bad = inputs.clone();
    bad.source_digest = "nope";
    assert!(archive_identity(&archive, &bad).is_err());
}

#[test]
fn single_shard_skips_archive_write_and_transfer() {
    for count in [0, 1] {
        assert!(!archive_write_required(count));
        assert!(!requires_archive_transfer(count));
    }
    for count in [2, 4] {
        assert!(archive_write_required(count));
        assert!(requires_archive_transfer(count));
    }
}

#[test]
fn inventory_count_parses_string_arrays() {
    assert_eq!(count_inventory_tests("[\"a\",\"b\"]").expect("count"), 2);
    assert_eq!(count_inventory_tests("[]").expect("count"), 0);
    assert!(count_inventory_tests("{\"a\":1}").is_err());
    assert!(count_inventory_tests("[\"a\",1]").is_err());
    assert!(count_inventory_tests("nope").is_err());
}

#[test]
fn mise_symbols_route_to_mise() {
    for path in [
        "mise.toml",
        ".mise.toml",
        "mise.lock",
        ".mise.lock",
        "cfg/mise.toml",
    ] {
        assert_eq!(stack_for_symbol(path), Some("mise"), "{path}");
    }
    for path in [
        "rust-toolchain.toml",
        "Cargo.toml",
        "Cargo.lock",
        "crates/a/Cargo.toml",
    ] {
        assert_eq!(stack_for_symbol(path), Some("rust"), "{path}");
    }
    assert_eq!(stack_for_symbol("README.md"), None);
    assert!(is_mise_env_symbol("MISE_TASK_CACHE_DIR"));
    assert!(is_mise_env_symbol("MISE_RUSTUP_HOME"));
    assert!(!is_mise_env_symbol("RUSTUP_TOOLCHAIN"));
    assert!(is_allowed_mise_subcommand("exec"));
}

#[test]
fn inspect_mise_file_never_fails_never_writes() {
    let missing = inspect_mise_file("mise.toml", None).expect("missing");
    assert!(missing.spec.is_none());
    assert_eq!(missing.findings.len(), 1);
    assert_eq!(missing.findings[0].code, MISSING_RECOMMENDED_INPUT);
    assert!(missing.findings[0].recommendation.contains("never creates"));
    let bad = inspect_mise_file("mise.lock", Some("[tools\nrust = ")).expect("malformed");
    assert!(bad.spec.is_none());
    assert_eq!(bad.findings[0].code, TOOLING_INPUT_INVALID);
    assert!(bad.findings[0].recommendation.contains("manually"));
    let good = inspect_mise_file("mise.toml", Some("[tools]\nrust = \"1.98.1\"\n")).expect("valid");
    assert!(good.findings.is_empty());
    let spec = good.spec.expect("spec");
    assert_eq!(spec.tools.get("rust").map(String::as_str), Some("1.98.1"));
    assert!(inspect_mise_file("rust-toolchain.toml", Some("")).is_err());
    let src = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../velnor-actions-mise-catalog/src/toolfiles.rs"),
    )
    .expect("read");
    for token in [
        "fs::write",
        "File::create",
        "OpenOptions",
        "Command::new",
        "std::process",
    ] {
        assert!(!src.contains(token), "write token {token}");
    }
}

#[test]
fn lock_tool_versions_exposes_map() {
    let versions = lock_tool_versions("[tools]\nrust = \"1.98.1\"\ngh = \"2.101.0\"\n");
    assert_eq!(versions.len(), 2);
    assert_eq!(versions.get("rust").map(String::as_str), Some("1.98.1"));
    assert!(lock_tool_versions("[tools\nrust = ").is_empty());
    assert!(lock_tool_versions("[other]\nrust = \"1.98.1\"\n").is_empty());
}

#[test]
fn rust_files_named_only_in_toolfiles() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../velnor-actions-mise-catalog/src");
    let table = std::fs::read_to_string(dir.join("toolfiles.rs")).expect("read");
    for name in [
        "mise.toml",
        "mise.lock",
        "rust-toolchain.toml",
        "Cargo.toml",
        "Cargo.lock",
    ] {
        assert!(table.contains(name), "{name} missing from routing");
    }
    assert!(table.contains("FOREIGN_TOOL_FILES"));
}
