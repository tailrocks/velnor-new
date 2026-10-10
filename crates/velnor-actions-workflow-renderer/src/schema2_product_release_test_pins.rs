use crate::schema2::ProductReleasePins;
use crate::setup::MiseSetup;

pub(super) fn test_pins() -> ProductReleasePins {
    let setup = MiseSetup {
        uses: format!("jdx/mise-action@{}", "a".repeat(40)),
        version: "2026.10.4".to_owned(),
        sha256: "b".repeat(64),
    };
    let (install_resource_probe_target_argv, resource_probe_build_argv) = resource_probe_argvs();
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_build_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_qualify_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_runner_build_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_gh_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            "gh@2.102.0",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        build_argv: vec!["mise".to_owned(), "exec".to_owned()],
        intel_build_argv: vec!["mise".to_owned(), "exec".to_owned()],
        install_intel_target_argv: vec!["mise".to_owned(), "exec".to_owned()],
        install_resource_probe_target_argv,
        runner_build_argv: vec!["mise".to_owned(), "exec".to_owned()],
        runner_attestation_helper_build_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
            "--",
            "cargo",
            "build",
            "--locked",
            "--manifest-path",
            "crates/velnor-runner/Cargo.toml",
            "--release",
            "-p",
            "velnor-runner-attestation",
            "--bin",
            "velnor-runner-attestation-helper",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        resource_probe_build_argv,
        actionlint_argv: vec!["mise".to_owned(), "exec".to_owned()],
        zizmor_argv: vec!["mise".to_owned(), "exec".to_owned()],
        gh_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "gh@2.102.0",
            "--",
            "gh",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        rust_version: "1.98.1".to_owned(),
        mr_boxington_version: "1.21.1".to_owned(),
    }
}

fn resource_probe_argvs() -> (Vec<String>, Vec<String>) {
    let install = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.98.1",
        "--",
        "rustup",
        "target",
        "add",
        "--toolchain",
        "1.98.1",
        "x86_64-unknown-linux-musl",
    ]
    .map(str::to_owned)
    .to_vec();
    let build = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.98.1",
        "--",
        "cargo",
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
    ]
    .map(str::to_owned)
    .to_vec();
    (install, build)
}
