//! Release-plz coordinator pin and argv cases.
use std::ffi::OsString;
use std::path::PathBuf;

use velnor_actions_mise_catalog::catalog::RELEASE_PLZ_VERSION;
use velnor_actions_mise_catalog::catalog::lock::verify_version_policy;
use velnor_actions_mise_catalog::catalog::release_plz::{
    OidcEnvironment, REGISTRY_TOKEN_ENV, RELEASE_PLZ_CKSUM, REQUIRED_ID_TOKEN_PERMISSION,
    ReleaseAuth, ReleasePrRequest, ReleaseRequest, TRUSTED_PUBLISHING_TOKENS_URL,
    trusted_publishing_engages,
};
use velnor_actions_mise_catalog::{PinnedTool, ToolCatalog, validate_exact_version};
use velnor_actions_mise_core::MiseError;

fn config() -> PathBuf {
    PathBuf::from("release-plz.toml")
}

fn vec_os(items: &[&str]) -> Vec<OsString> {
    items.iter().map(|item| OsString::from(*item)).collect()
}

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

#[test]
fn release_plz_pin_is_exact() {
    assert_eq!(RELEASE_PLZ_VERSION, "0.3.171");
    assert_eq!(pinned().version(PinnedTool::ReleasePlz), "0.3.171");
    assert_eq!(
        pinned().tool_spec(PinnedTool::ReleasePlz),
        "release-plz@0.3.171"
    );
    assert_eq!(PinnedTool::ReleasePlz.tool_name(), "release-plz");
    assert_eq!(
        PinnedTool::from_tool_name("release-plz"),
        Ok(PinnedTool::ReleasePlz)
    );
    assert!(validate_exact_version("release-plz", RELEASE_PLZ_VERSION).is_ok());
    let err = pinned()
        .tool_identity(PinnedTool::ReleasePlz)
        .validate("catalog")
        .expect_err("placeholder digests never validate as trusted");
    assert!(err.to_string().contains("placeholder_digest"), "{err}");
}

#[test]
fn release_plz_cksum_is_full_sha256() {
    assert_eq!(
        RELEASE_PLZ_CKSUM,
        "b3605494506c61964582d52ee72bc6fff05d061010bef394a42262bab0e1f35b"
    );
    assert_eq!(RELEASE_PLZ_CKSUM.len(), 64);
    assert!(
        RELEASE_PLZ_CKSUM
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()),
        "cksum must be full hex, never truncated"
    );
}

#[test]
fn release_pr_argv_base_shape() {
    let request = ReleasePrRequest::new(config()).expect("config");
    assert_eq!(
        request.release_pr_argv(),
        vec_os(&["release-plz", "release-pr", "--config", "release-plz.toml"])
    );
}

#[test]
fn release_pr_argv_full_options() {
    let request = ReleasePrRequest::new(config())
        .expect("config")
        .with_manifest(PathBuf::from("Cargo.toml"))
        .expect("manifest")
        .with_registry("crates-io")
        .expect("registry")
        .with_package("velnor")
        .expect("package")
        .with_json_output();
    assert_eq!(
        request.release_pr_argv(),
        vec_os(&[
            "release-plz",
            "release-pr",
            "--config",
            "release-plz.toml",
            "--manifest-path",
            "Cargo.toml",
            "--registry",
            "crates-io",
            "-p",
            "velnor",
            "-o",
            "json",
        ])
    );
}

#[test]
fn release_argv_shapes_per_constructor() {
    let oidc = ReleaseRequest::release(config()).expect("oidc");
    assert_eq!(
        oidc.release_argv(),
        vec_os(&["release-plz", "release", "--config", "release-plz.toml"])
    );
    assert_eq!(oidc.auth(), ReleaseAuth::Oidc);
    assert!(!oidc.dry_run_active());

    let tokened = ReleaseRequest::release_with_token(config(), "test-token").expect("token");
    assert_eq!(
        tokened.release_argv(),
        vec_os(&[
            "release-plz",
            "release",
            "--config",
            "release-plz.toml",
            "--token",
            "test-token",
        ])
    );
    assert_eq!(tokened.auth(), ReleaseAuth::Token);

    let dry = ReleaseRequest::dry_run(config()).expect("dry");
    assert_eq!(
        dry.release_argv(),
        vec_os(&[
            "release-plz",
            "release",
            "--config",
            "release-plz.toml",
            "--dry-run",
        ])
    );
    assert!(dry.dry_run_active());
    assert_eq!(dry.auth(), ReleaseAuth::Oidc);

    let full = ReleaseRequest::release(config())
        .expect("config")
        .with_manifest(PathBuf::from("Cargo.toml"))
        .expect("manifest")
        .with_registry("crates-io")
        .expect("registry")
        .with_json_output();
    assert_eq!(
        full.release_argv(),
        vec_os(&[
            "release-plz",
            "release",
            "--config",
            "release-plz.toml",
            "--manifest-path",
            "Cargo.toml",
            "--registry",
            "crates-io",
            "-o",
            "json",
        ])
    );
}

#[test]
fn phases_never_share_one_call() {
    let pr = ReleasePrRequest::new(config())
        .expect("config")
        .release_pr_argv();
    assert_eq!(pr[1], OsString::from("release-pr"));
    for forbidden in [
        "release",
        "--dry-run",
        "--token",
        "--no-verify",
        "--allow-dirty",
    ] {
        assert!(
            !pr.iter().any(|arg| arg == forbidden),
            "release-pr must not carry {forbidden}"
        );
    }
    let rel = ReleaseRequest::release(config())
        .expect("config")
        .release_argv();
    assert_eq!(rel[1], OsString::from("release"));
    for forbidden in [
        "release-pr",
        "-p",
        "--package",
        "--no-verify",
        "--allow-dirty",
    ] {
        assert!(
            !rel.iter().any(|arg| arg == forbidden),
            "release must not carry {forbidden}"
        );
    }
}

#[test]
fn explicit_config_is_always_required() {
    let empty = PathBuf::new();
    assert!(matches!(
        ReleasePrRequest::new(empty.clone()),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "config"
    ));
    assert!(matches!(
        ReleaseRequest::release(empty.clone()),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "config"
    ));
    assert!(matches!(
        ReleaseRequest::release_with_token(empty.clone(), "test-token"),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "config"
    ));
    assert!(matches!(
        ReleaseRequest::dry_run(empty),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "config"
    ));
    assert!(matches!(
        ReleasePrRequest::new(config())
            .expect("config")
            .with_manifest(PathBuf::new()),
        Err(MiseError::InvalidManifestPath { .. })
    ));
    assert!(matches!(
        ReleasePrRequest::new(config()).expect("config").with_registry(""),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "registry"
    ));
    assert!(matches!(
        ReleasePrRequest::new(config()).expect("config").with_package(""),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "package"
    ));
    assert!(matches!(
        ReleaseRequest::release_with_token(config(), ""),
        Err(MiseError::InvalidStepInput { field, .. }) if field == "token"
    ));
}

#[test]
fn token_and_oidc_constructors_stay_distinct() {
    let tokened = ReleaseRequest::release_with_token(config(), "sekrit-value").expect("token");
    assert_eq!(tokened.auth(), ReleaseAuth::Token);
    let debug = format!("{tokened:?}");
    assert!(
        !debug.contains("sekrit-value"),
        "Debug must redact token material: {debug}"
    );
    assert!(debug.contains("Token"), "Debug keeps the mode: {debug}");
    for request in [
        ReleaseRequest::release(config()).expect("oidc"),
        ReleaseRequest::dry_run(config()).expect("dry"),
    ] {
        assert_eq!(request.auth(), ReleaseAuth::Oidc);
        assert!(
            !request.release_argv().iter().any(|arg| arg == "--token"),
            "OIDC requests render zero token material"
        );
    }
}

#[test]
fn trusted_publishing_gate_matches_upstream() {
    let engaged = OidcEnvironment::new(true, false);
    assert!(trusted_publishing_engages(
        ReleaseAuth::Oidc,
        None,
        false,
        engaged
    ));
    assert!(trusted_publishing_engages(
        ReleaseAuth::Oidc,
        Some("crates-io"),
        false,
        engaged
    ));
    for (auth, registry, dry_run, env) in [
        (ReleaseAuth::Token, None, false, engaged),
        (ReleaseAuth::Oidc, Some("private"), false, engaged),
        (ReleaseAuth::Oidc, None, true, engaged),
        (
            ReleaseAuth::Oidc,
            None,
            false,
            OidcEnvironment::new(false, false),
        ),
        (
            ReleaseAuth::Oidc,
            None,
            false,
            OidcEnvironment::new(true, true),
        ),
    ] {
        assert!(
            !trusted_publishing_engages(auth, registry, dry_run, env),
            "gate must stay closed: {auth:?} {registry:?} dry={dry_run} {env:?}"
        );
    }
    assert_eq!(REGISTRY_TOKEN_ENV, "CARGO_REGISTRY_TOKEN");
    assert_eq!(REQUIRED_ID_TOKEN_PERMISSION, "write");
    assert_eq!(
        TRUSTED_PUBLISHING_TOKENS_URL,
        "https://crates.io/api/v1/trusted_publishing/tokens"
    );
    assert_eq!(
        OidcEnvironment::from_process_env(),
        OidcEnvironment::from_process_env(),
        "process observation is a stable read"
    );
}

#[test]
fn coordinator_command_selects_pinned_tools() {
    let command = ReleaseRequest::dry_run(config())
        .expect("dry")
        .command(&pinned())
        .expect("command");
    let argv = command.argv();
    assert_eq!(argv[0], OsString::from("mise"));
    let separator = argv
        .iter()
        .position(|arg| arg == "--")
        .expect("payload separator");
    for spec in ["rust@1.98.1", "release-plz@0.3.171"] {
        assert!(
            argv[..separator].iter().any(|arg| arg == spec),
            "missing tool spec before --: {spec}"
        );
    }
    assert_eq!(argv[separator + 1], OsString::from("release-plz"));
    assert!(
        argv[separator..].iter().any(|arg| arg == "--dry-run"),
        "payload carries --dry-run after --"
    );
}

#[test]
fn repo_policy_mirror_covers_release_plz() {
    let path = format!(
        "{}/../../../.velnor/version-policy.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).expect("repo version-policy exists");
    assert!(
        text.contains("release-plz = \"0.3.171\""),
        "policy pins release-plz"
    );
    verify_version_policy(&text, &pinned()).expect("policy mirrors catalog");
}

#[test]
fn freshness_inventory_mirrors_release_plz() {
    let path = format!(
        "{}/../../../.velnor/freshness-inventory.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).expect("freshness inventory exists");
    for needle in [
        "\"name\": \"release-plz\"",
        "\"pinned\": \"0.3.171\"",
        "\"qualified\": \"0.3.171\"",
        "\"source\": \"https://crates.io/api/v1/crates/release-plz\"",
    ] {
        assert!(
            text.contains(needle),
            "inventory mirrors release-plz: {needle}"
        );
    }
}
