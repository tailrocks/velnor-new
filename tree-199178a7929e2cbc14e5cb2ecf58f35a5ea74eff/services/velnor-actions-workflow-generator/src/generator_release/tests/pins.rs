use crate::generator_release_pins::GeneratorReleasePins;
use velnor_actions_workflow_steps::setup::MiseSetup;

pub(super) const PINNED_MISE_ARGUMENTS: &str = "--no-config --no-env --no-hooks exec gh@2.102.0 --";

fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn macos_x86_64_cross_build_argv() -> Vec<String> {
    argv(&[
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.98.1",
        "mr-boxington@1.21.1",
        "--",
        "mbx",
        "build",
        "--release",
        "--locked",
        "--package",
        "velnor-actions-cli",
        "--bin",
        "velnor-actions",
        "--target",
        "x86_64-apple-darwin",
    ])
}

fn install_macos_x86_64_target_argv() -> Vec<String> {
    argv(&[
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
        "1.98.1-aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ])
}

pub(super) fn test_pins() -> GeneratorReleasePins {
    let setup = MiseSetup {
        uses: format!("jdx/mise-action@{}", "a".repeat(40)),
        version: "2026.9.18".to_owned(),
        sha256: "b".repeat(64),
    };
    GeneratorReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_build_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
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
        macos_x86_64_cross_build_argv: macos_x86_64_cross_build_argv(),
        install_macos_x86_64_target_argv: install_macos_x86_64_target_argv(),
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
