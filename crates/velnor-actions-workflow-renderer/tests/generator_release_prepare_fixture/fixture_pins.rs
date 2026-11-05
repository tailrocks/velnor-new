// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{GH_VERSION, MBX_VERSION, RUST_VERSION};
use velnor_actions_workflow_renderer::schema2::ProductReleasePins;
use velnor_actions_workflow_renderer::setup::MiseSetup;

use velnor_actions_contract::{Step, StepKind};

pub(super) fn command_argv() -> Vec<String> {
    vec!["true".to_owned()]
}

pub(super) fn fixture_step() -> Step {
    Step {
        name: "fixture".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::default(),
        },
    }
}

pub(super) fn pins() -> ProductReleasePins {
    let setup = MiseSetup {
        uses: "jdx/mise-action@f1f6d8a6c2a1f6a79d5216a1d7e3f1b0e4d7c1a9".to_owned(),
        version: "2026.10.19".to_owned(),
        sha256: "c".repeat(64),
    };
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: command_argv(),
        install_build_tools_argv: command_argv(),
        install_qualify_tools_argv: command_argv(),
        qualify_mbx_action_steps: std::array::from_fn(|_| fixture_step()),
        install_runner_build_tools_argv: command_argv(),
        install_gh_argv: command_argv(),
        build_argv: command_argv(),
        intel_build_argv: command_argv(),
        install_intel_target_argv: command_argv(),
        install_resource_probe_target_argv: command_argv(),
        runner_build_argv: command_argv(),
        attestation_helper_build_argv: command_argv(),
        resource_probe_build_argv: command_argv(),
        install_resource_probe_release_tools_argv: command_argv(),
        resource_probe_release_build_argv: command_argv(),
        resource_probe_release_test_argv: command_argv(),
        lint_argv: command_argv(),
        verify_lint_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script(
                GH_VERSION,
                velnor_actions_workflow_renderer::VERIFY_PATH,
            ),
        ],
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
        rust_version: RUST_VERSION.to_owned(),
        verify_rust_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "{} true",
                velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude()
            ),
        ],
        mr_boxington_version: MBX_VERSION.to_owned(),
        gh_version: GH_VERSION.to_owned(),
    }
}
