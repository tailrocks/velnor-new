use super::{install_argv, resource_probe_build_argv, runner_build_argv};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

fn strings(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn assert_catalog_mbx_build(args: &[String], expected_build_args: &[&str]) {
    let actual = strings(args);
    let catalog = ToolCatalog::pinned();
    let rust = format!("rust@{}", catalog.version(PinnedTool::Rust));
    let mbx = format!("mr-boxington@{}", catalog.version(PinnedTool::MrBoxington));
    let expected_prefix = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        rust.as_str(),
        mbx.as_str(),
        "--",
        "mbx",
    ];

    assert_eq!(&actual[..expected_prefix.len()], expected_prefix);
    assert_eq!(&actual[expected_prefix.len()..], expected_build_args);
    assert!(!actual.contains(&"cargo"));
}

#[test]
fn product_release_installs_catalog_mbx_for_runner_builds() {
    let catalog = ToolCatalog::pinned();
    let argv = install_argv(&[PinnedTool::Rust, PinnedTool::MrBoxington], &catalog).unwrap();
    let actual = strings(&argv);
    let rust = format!("rust@{}", catalog.version(PinnedTool::Rust));
    let mbx = format!("mr-boxington@{}", catalog.version(PinnedTool::MrBoxington));

    assert_eq!(
        actual,
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            rust.as_str(),
            mbx.as_str(),
        ]
    );
}

#[test]
fn macos_runner_binary_build_uses_catalog_mbx_and_pinned_rust() {
    let catalog = ToolCatalog::pinned();
    let argv = runner_build_argv(&catalog).unwrap();

    assert_catalog_mbx_build(
        &argv,
        &[
            "build",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--release",
            "--package",
            "velnor-runner-cli",
        ],
    );
}

#[test]
fn linux_resource_probe_build_uses_catalog_mbx_and_preserves_target() {
    let catalog = ToolCatalog::pinned();
    let argv = resource_probe_build_argv(&catalog).unwrap();

    assert_catalog_mbx_build(
        &argv,
        &[
            "build",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--package",
            "velnor-resource-probe",
            "--bin",
            "velnor-resource-probe",
            "--release",
            "--target",
            "x86_64-unknown-linux-musl",
        ],
    );
}
