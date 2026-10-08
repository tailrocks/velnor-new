//! Release coordinator modes: dry-run, token/OIDC split, argv snapshots.
use std::ffi::OsString;
use std::path::PathBuf;

use velnor_actions_mise::catalog::release_plz::{ReleaseAuth, ReleasePrRequest, ReleaseRequest};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

fn config() -> PathBuf {
    PathBuf::from("release-plz.toml")
}

fn vec_os(items: &[&str]) -> Vec<OsString> {
    items.iter().map(|item| OsString::from(*item)).collect()
}

#[test]
fn dry_run_argv_snapshot_never_carries_token() {
    let dry = ReleaseRequest::dry_run(config()).expect("dry");
    assert_eq!(
        dry.release_argv(),
        vec_os(&[
            "release-plz",
            "release",
            "--config",
            "release-plz.toml",
            "--dry-run"
        ])
    );
    assert!(dry.dry_run_active());
    assert_eq!(dry.auth(), ReleaseAuth::Oidc);
    let full = ReleaseRequest::dry_run(config())
        .expect("dry")
        .with_manifest(PathBuf::from("crates/a/Cargo.toml"))
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
            "crates/a/Cargo.toml",
            "--registry",
            "crates-io",
            "--dry-run",
            "-o",
            "json",
        ])
    );
    assert!(!full.release_argv().iter().any(|arg| arg == "--token"));
    assert!(!full.release_argv().iter().any(|arg| arg == "-p"));
}

#[test]
fn auth_modes_never_blend_or_fall_back() {
    let tokened = ReleaseRequest::release_with_token(config(), "sekrit").expect("token");
    assert_eq!(tokened.auth(), ReleaseAuth::Token);
    assert!(!tokened.dry_run_active());
    let dry = ReleaseRequest::dry_run(config()).expect("dry");
    assert_eq!(dry.auth(), ReleaseAuth::Oidc);
    let oidc = ReleaseRequest::release(config()).expect("oidc");
    assert_eq!(oidc.auth(), ReleaseAuth::Oidc);
    assert!(!oidc.dry_run_active());
    let token_argv = tokened.release_argv();
    let positions: Vec<usize> = token_argv
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| (arg == "--token").then_some(index))
        .collect();
    assert_eq!(positions.len(), 1);
    assert_eq!(token_argv[positions[0] + 1], OsString::from("sekrit"));
}

#[test]
fn release_pr_takes_exactly_one_package() {
    let once = ReleasePrRequest::new(config())
        .expect("config")
        .with_package("alpha")
        .expect("package");
    let argv = once.release_pr_argv();
    assert_eq!(argv.iter().filter(|arg| *arg == "-p").count(), 1);
    let twice = ReleasePrRequest::new(config())
        .expect("config")
        .with_package("alpha")
        .expect("first")
        .with_package("beta")
        .expect("second");
    let argv = twice.release_pr_argv();
    assert_eq!(argv.iter().filter(|arg| *arg == "-p").count(), 1);
    assert!(argv.iter().any(|arg| arg == "beta"));
    assert!(
        !argv.iter().any(|arg| arg == "alpha"),
        "last wins, never repeated"
    );
}

#[test]
fn argv_preserves_spaced_paths_without_shell() {
    let spaced = PathBuf::from("my dir/release plz.toml");
    let request = ReleasePrRequest::new(spaced.clone()).expect("config");
    let argv = request.release_pr_argv();
    assert_eq!(argv.len(), 4);
    assert_eq!(argv[3], spaced.into_os_string(), "single argv element");
    let with_manifest = ReleaseRequest::release(config())
        .expect("release")
        .with_manifest(PathBuf::from("crates/my crate/Cargo.toml"))
        .expect("manifest");
    let argv = with_manifest.release_argv();
    assert!(argv.iter().any(|arg| arg == "crates/my crate/Cargo.toml"));
}

#[test]
fn coordinator_payload_equals_release_argv_after_separator() {
    let catalog = ToolCatalog::pinned();
    let request = ReleaseRequest::release(config()).expect("release");
    let command = request.command(&catalog).expect("command");
    let argv = command.argv();
    assert_eq!(argv[0], OsString::from("mise"));
    let separator = argv.iter().position(|arg| arg == "--").expect("separator");
    assert!(
        argv[..separator]
            .iter()
            .any(|arg| arg == "release-plz@0.3.170"),
        "pinned coordinator before --"
    );
    assert!(
        argv[..separator].iter().any(|arg| arg == "rust@1.98.1"),
        "pinned rust before --"
    );
    assert_eq!(&argv[separator + 1..], request.release_argv().as_slice());
    assert_eq!(catalog.version(PinnedTool::ReleasePlz), "0.3.170");
}
