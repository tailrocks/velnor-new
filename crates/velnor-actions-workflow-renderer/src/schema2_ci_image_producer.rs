// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_actionlint::UPLOAD_ARTIFACT_ACTION_USES;
use velnor_actions_contract::Step;

use super::ProductReleasePins;
use crate::{RenderError, action_step, ambient_shell_step};

const MAIN_PUSH: &str = "github.event_name == 'push' && github.ref == 'refs/heads/main'";
pub const SDK_CI_IMAGE_LOOKUP_SCRIPT: &str = include_str!("schema2_ci_image_lookup.sh");
pub const SDK_CI_IMAGE_ARTIFACT_SCRIPT: &str = include_str!("schema2_ci_image_artifact.py");
pub const SDK_CI_IMAGE_DOWNLOAD_SCRIPT: &str = include_str!("schema2_ci_image_download.sh");
pub(super) const SCRUB_EXEC: &str = r#"unset_args=(); while IFS= read -r -d '' entry; do name=${entry%%=*}; case "$name" in *TOKEN*|*PASSWORD*|*SECRET*|GH_HOST|GH_CONFIG_DIR|BASH_ENV|ENV|BASH_FUNC_*) unset_args+=(-u "$name");; esac; done < <(/usr/bin/env -0); exec /usr/bin/env "${unset_args[@]}" "$@""#;

fn build_argvs(rust: &str) -> (Vec<String>, Vec<String>) {
    let prefix = ["mise", "--no-config", "--no-env", "--no-hooks", "exec"]
        .into_iter()
        .map(str::to_owned)
        .chain([rust.to_owned(), "--".to_owned()])
        .collect::<Vec<_>>();
    let mut build = prefix.clone();
    build.extend(
        ["bash", "scripts/ci-native-image-tool-build.sh", "cargo"]
            .into_iter()
            .map(str::to_owned),
    );
    build.extend(
        [
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    let mut probe = prefix;
    probe.push("cargo".to_owned());
    probe.extend(
        [
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
        .into_iter()
        .map(str::to_owned),
    );
    (build, probe)
}

fn upload_steps() -> Result<Vec<Step>, RenderError> {
    let mut steps = Vec::new();
    for (name, artifact, path) in [
        (
            "Upload Job image candidate",
            "velnor-job-image-candidate-${{ github.run_attempt }}",
            "${{ runner.temp }}/velnor-ci-image-candidates/job/",
        ),
        (
            "Upload Runner image candidates",
            "image-assets-${{ github.run_attempt }}",
            "${{ runner.temp }}/velnor-ci-image-candidates/images/",
        ),
    ] {
        let mut step = action_step(
            name,
            UPLOAD_ARTIFACT_ACTION_USES,
            BTreeMap::from([
                ("name".to_owned(), artifact.to_owned()),
                ("path".to_owned(), path.to_owned()),
                ("if-no-files-found".to_owned(), "error".to_owned()),
                ("retention-days".to_owned(), "7".to_owned()),
            ]),
        )?;
        step.condition = Some(format!("success() && {MAIN_PUSH}"));
        steps.push(step);
    }
    Ok(steps)
}

pub fn sdk_ci_image_steps(
    pins: &ProductReleasePins,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<Vec<Step>, RenderError> {
    let rust = velnor_actions_mise::rust_install_spec(&pins.rust_version);
    let (build, probe) = build_argvs(&rust);
    let mut steps = vec![
        ambient_shell_step(
            "Install pinned Linux musl target",
            scrubbed_argv(&[
                "mise".to_owned(),
                "--no-config".to_owned(),
                "--no-env".to_owned(),
                "--no-hooks".to_owned(),
                "exec".to_owned(),
                rust,
                "--".to_owned(),
                "rustup".to_owned(),
                "target".to_owned(),
                "add".to_owned(),
                "x86_64-unknown-linux-musl".to_owned(),
            ]),
            scrub_env(),
        )?,
        verify_cross_target_toolchain(catalog)?,
        ambient_shell_step(
            "Build exact-source image maintenance",
            scrubbed_argv(&build),
            native_target_env(),
        )?,
        ambient_shell_step(
            "Build locked resource probe",
            scrubbed_argv(&probe),
            native_target_env(),
        )?,
        ambient_shell_step(
            "Build and inspect CI image candidates",
            scrubbed_argv(&[
                "bash".to_owned(),
                "scripts/ci-native-image-candidates.sh".to_owned(),
            ]),
            native_target_env(),
        )?,
    ];
    steps.extend(upload_steps()?);
    Ok(steps)
}

fn scrubbed_argv(argv: &[String]) -> Vec<String> {
    let mut out = [
        "bash",
        "--noprofile",
        "--norc",
        "-p",
        "scripts/ci-native-image-credential-exec.sh",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    out.extend_from_slice(argv);
    out
}

fn native_target_env() -> BTreeMap<String, String> {
    let mut env = scrub_env();
    env.insert(
        "CARGO_TARGET_DIR".to_owned(),
        "${{ runner.temp }}/velnor-ci-image-target".to_owned(),
    );
    env
}

fn scrub_env() -> BTreeMap<String, String> {
    let mut env = crate::toolchain_env::credential_scrub();
    let homes = velnor_actions_mise::ToolHomes::runner_temp();
    env.insert("RUSTUP_HOME".to_owned(), homes.rustup_home().to_owned());
    env.insert("CARGO_HOME".to_owned(), homes.cargo_home().to_owned());
    env.insert(
        "MISE_RUSTUP_HOME".to_owned(),
        homes.rustup_home().to_owned(),
    );
    env.insert("MISE_CARGO_HOME".to_owned(), homes.cargo_home().to_owned());
    env.insert("BASH_ENV".to_owned(), String::new());
    env.insert("ENV".to_owned(), String::new());
    env
}

fn verify_cross_target_toolchain(
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<Step, RenderError> {
    let homes = velnor_actions_mise::ToolHomes::runner_temp();
    crate::verifier_composite::verifier_shell_step(
        velnor_actions_contract::StepRole::VerifyRustManifest,
        "Verify Rust toolchain after cross-target install",
        crate::verifier_builders::verify_rust_argv(catalog),
        BTreeMap::from([
            ("RUSTUP_HOME".to_owned(), homes.rustup_home().to_owned()),
            ("CARGO_HOME".to_owned(), homes.cargo_home().to_owned()),
        ]),
    )
}
