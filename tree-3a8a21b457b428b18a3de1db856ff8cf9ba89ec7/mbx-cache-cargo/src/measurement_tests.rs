use super::*;

#[test]
fn full_probe_preserves_owner_selectors_and_stays_offline_locked() {
    let input = [
        "build",
        "--manifest-path",
        "sub/Cargo.toml",
        "-Falpha,beta",
        "--target=wasm32-unknown-unknown",
        "--no-default-features",
        "--",
        "--all-features",
    ];
    let args = measurement_arguments(&input.map(str::to_string)).expect("selectors");
    assert!(!args.iter().any(|arg| arg == "--no-deps"));
    for flag in [
        "--locked",
        "--offline",
        "--no-default-features",
        "--features",
        "--filter-platform",
    ] {
        assert!(args.iter().any(|arg| arg == flag), "{flag}");
    }
    assert!(!args.iter().any(|arg| arg == "--all-features"));
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--manifest-path", "sub/Cargo.toml"])
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--features", "alpha,beta"])
    );
}

#[test]
fn other_flag_values_never_become_feature_selectors() {
    let input = [
        "build",
        "--bin",
        "--all-features",
        "--config",
        "build.target=\"x\"",
    ];
    let args = measurement_arguments(&input.map(str::to_string)).expect("selectors");
    assert!(!args.iter().any(|arg| arg == "--all-features"));
}

#[test]
fn opaque_id_labels_do_not_establish_package_provenance() {
    let value = serde_json::json!({"workspace_members":["registry+fake#member"],"packages":[
        {"id":"registry+fake#member","source":null,"manifest_path":"/workspace/member/Cargo.toml","targets":[{"src_path":"/workspace/member/src/lib.rs"}]},
        {"id":"path+fake#dependency","source":"registry+https://example.invalid/index","manifest_path":"/cache/dep/Cargo.toml","targets":[{"src_path":"/cache/dep/src/lib.rs"}]},
        {"id":"registry+fake#path","source":null,"manifest_path":"/outside/Cargo.toml","targets":[]},
        {"id":"unknown","source":"future+source","manifest_path":"/future/Cargo.toml","targets":[]}
    ]});
    let packages = parse_metadata(value.to_string().as_bytes()).expect("metadata");
    assert_eq!(
        packages
            .iter()
            .map(|package| package.origin)
            .collect::<Vec<_>>(),
        [
            PackageOrigin::Workspace,
            PackageOrigin::Registry,
            PackageOrigin::Path,
            PackageOrigin::Unknown
        ]
    );
}

#[test]
fn incomplete_membership_and_relative_paths_are_unavailable() {
    let mut value = serde_json::json!({"workspace_members":["missing"],"packages":[]});
    assert!(parse_metadata(value.to_string().as_bytes()).is_none());
    value["workspace_members"] = serde_json::json!([]);
    value["packages"] = serde_json::json!([{"id":"id","source":null,"manifest_path":"relative/Cargo.toml","targets":[]}]);
    assert!(parse_metadata(value.to_string().as_bytes()).is_none());
    value["packages"] =
        serde_json::json!([{"id":"id","manifest_path":"/actual/Cargo.toml","targets":[]}]);
    assert!(parse_metadata(value.to_string().as_bytes()).is_none());
}

#[test]
fn unresolved_toolchain_selectors_and_unbounded_values_are_unsupported() {
    assert!(measurement_arguments(&["+nightly".into(), "build".into()]).is_none());
    assert!(
        measurement_arguments(&["build".into(), format!("-F{}", "x".repeat(MAX_TEXT + 1))])
            .is_none()
    );
    let result = measurement_metadata(OsStr::new("cargo"), &["build".into()], Path::new("/"));
    assert_eq!(result.observation, MetadataObservation::Unsupported);
}

#[test]
fn ordered_directory_globals_retain_owner_selection() {
    let input = [
        "-C",
        "/first",
        "--directory",
        "/second",
        "--config",
        "build.target=\"x\"",
        "-C",
        "/last",
        "build",
    ];
    let args = measurement_arguments(&input.map(str::to_string)).expect("selectors");
    assert_eq!(&args[..8], &input[..8]);
}

#[cfg(unix)]
#[test]
fn hung_metadata_cannot_hold_the_workload_open() {
    let started = Instant::now();
    let result = metadata_output_with_timeout(
        OsStr::new("/bin/sh"),
        &["-c".into(), "exec sleep 10".into()],
        Path::new("/"),
        Duration::from_millis(20),
    );
    assert!(result.is_none());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[cfg(unix)]
#[test]
fn unavailable_probe_returns_no_authority_without_publishing_diagnostics() {
    let result = measurement_metadata(
        OsStr::new("/does-not-exist/cargo"),
        &["build".into()],
        Path::new("/"),
    );
    assert_eq!(result.observation, MetadataObservation::Unavailable);
    assert!(result.packages.is_empty());
}
