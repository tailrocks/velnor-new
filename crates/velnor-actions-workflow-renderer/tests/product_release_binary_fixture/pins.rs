// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn pins() -> ProductReleasePins {
    let setup = MiseSetup {
        uses: "jdx/mise-action@f1f6d8a6c2a1f6a79d5216a1d7e3f1b0e4d7c1a9".to_owned(),
        version: "2026.10.19".to_owned(),
        sha256: "c".repeat(64),
    };
    let verify_lint_script =
        velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script(
            "2.83.1",
            velnor_actions_workflow_renderer::VERIFY_PATH,
        );
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: argv(),
        install_build_tools_argv: argv(),
        install_qualify_tools_argv: argv(),
        qualify_mbx_action_steps: std::array::from_fn(|_| fixture_step()),
        install_runner_build_tools_argv: argv(),
        install_gh_argv: argv(),
        build_argv: argv(),
        intel_build_argv: argv(),
        install_intel_target_argv: argv(),
        install_resource_probe_target_argv: argv(),
        runner_build_argv: build_argv(HOST_PACKAGE, HOST_NAME),
        attestation_helper_build_argv: build_argv(HELPER_PACKAGE, HELPER_BIN),
        resource_probe_build_argv: argv(),
        install_resource_probe_release_tools_argv: argv(),
        resource_probe_release_build_argv: argv(),
        resource_probe_release_test_argv: argv(),
        lint_argv: argv(),
        verify_lint_argv: vec!["sh".to_owned(), "-c".to_owned(), verify_lint_script],
        gh_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            &format!(
                "gh@{}",
                velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64.version
            ),
            "--",
            "gh",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        rust_version: "1.99.1".to_owned(),
        verify_rust_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "{} true",
                velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude()
            ),
        ],
        mr_boxington_version: "1.29.2".to_owned(),
        gh_version: velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64
            .version
            .to_owned(),
    }
}

fn build_argv(package: &str, binary: &str) -> Vec<String> {
    [
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--release",
        "--package",
        package,
        "--bin",
        binary,
    ]
    .map(str::to_owned)
    .to_vec()
}

fn argv() -> Vec<String> {
    vec!["true".to_owned()]
}

fn fixture_step() -> Step {
    Step {
        name: "fixture".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}
