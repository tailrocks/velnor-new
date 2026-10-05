//! Typed target-specific build and verification jobs for generator releases.

use std::ffi::OsStr;

use velnor_actions_contract::{GeneratorReleaseSourceBinding, GeneratorReleaseTarget};
use velnor_actions_mise::{
    ToolCatalog, ToolHomes, apple_linker_check_step, apple_sdk_check_step,
    binary_format_architecture_check_step, generator_release_mise_binary_sha256,
    gnu_runtime_abi_check_step, help_smoke_check_step, mbx_cargo_build_step,
    native_host_check_step, rust_toolchain_check_step, setup_rust_steps, version_smoke_check_step,
};
use velnor_actions_rust::{GeneratorBinaryCheck, GeneratorBinaryVerification, GeneratorCargoBuild};

use crate::yaml::Yaml;

use super::{
    ASSET_DIR, MISE_USES, attest_permissions, attest_step, build_permissions, checkout_step,
    finish, run_step, subject_list, target_workflow, trusted_main_dispatch, upload_step,
    with_needs, with_permissions,
};

#[cfg(test)]
#[path = "schema2_generator_release_build_tests.rs"]
mod tests;

/// Construct one typed build and its target-specific attestation job.
pub(super) fn target_jobs(
    target: GeneratorReleaseTarget,
    binding: &GeneratorReleaseSourceBinding,
) -> Result<Vec<(String, Yaml)>, crate::RenderError> {
    let binary = target.binary_filename(binding.version());
    let checksum = target.sidecar_filename(binding.version());
    let files = [binary.as_str(), checksum.as_str()];
    let workflow = target_workflow(target);
    let runner = crate::runs_on::runs_on_yaml(target.runner_label())?;
    let steps = build_steps(target, binding, &binary, &checksum)?;
    let build = finish(
        workflow.build_job_id,
        with_permissions(
            trusted_main_dispatch(super::base(workflow.build_job_name, runner.clone(), 120)),
            build_permissions(),
        ),
        std::iter::once(checkout_step())
            .chain(steps)
            .chain(std::iter::once(upload_step(
                &format!("Upload {} assets", target.triple()),
                workflow.artifact_name,
                &files,
            )))
            .collect(),
    );
    let attest = finish(
        workflow.attest_job_id,
        with_needs(
            with_permissions(
                trusted_main_dispatch(super::base(workflow.attest_job_name, runner, 20)),
                attest_permissions(),
            ),
            &[workflow.build_job_id],
        ),
        vec![
            super::download_step("Download built assets", workflow.artifact_name, ASSET_DIR),
            attest_step(&subject_list(&files)),
        ],
    );
    Ok(vec![build, attest])
}

fn build_steps(
    target: GeneratorReleaseTarget,
    binding: &GeneratorReleaseSourceBinding,
    binary: &str,
    checksum: &str,
) -> Result<Vec<Yaml>, crate::RenderError> {
    let catalog = ToolCatalog::pinned();
    let homes = ToolHomes::runner_temp();
    let build = GeneratorCargoBuild::new(target);
    let verification = build
        .verification(binding, velnor_actions_mise::RUST_VERSION)
        .map_err(render_error)?;
    let mut steps = vec![source_binding_step(binding)];
    let setup = setup_rust_steps(
        MISE_USES,
        target,
        generator_release_mise_binary_sha256(target),
        &homes,
        &catalog,
    )
    .map_err(render_error)?;
    for step in &setup {
        steps.push(crate::steps_plain::plain_step_to_yaml(step)?);
    }
    for check in verification.checks() {
        if matches!(
            check,
            GeneratorBinaryCheck::NativeHostIdentity | GeneratorBinaryCheck::RustToolchainIdentity
        ) {
            steps.push(lower_verification_check(
                *check,
                target,
                &verification,
                &homes,
                &catalog,
            )?);
        }
    }
    let build_step = mbx_cargo_build_step(
        "Build locked release binary through MBX",
        OsStr::new(build.program()),
        &build.args(),
        &homes,
        &catalog,
    )
    .map_err(render_error)?;
    steps.push(crate::steps_plain::plain_step_to_yaml(&build_step)?);
    for check in verification.checks() {
        if !matches!(
            check,
            GeneratorBinaryCheck::NativeHostIdentity | GeneratorBinaryCheck::RustToolchainIdentity
        ) {
            steps.push(lower_verification_check(
                *check,
                target,
                &verification,
                &homes,
                &catalog,
            )?);
        }
    }
    steps.push(run_step(
        "Stage verified release binary",
        &stage_binary_script(&verification, binary),
    ));
    steps.push(run_step(
        "Checksum staged release binary",
        &checksum_script(target, binary, checksum),
    ));
    Ok(steps)
}

fn lower_verification_check(
    check: GeneratorBinaryCheck,
    target: GeneratorReleaseTarget,
    verification: &GeneratorBinaryVerification,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Yaml, crate::RenderError> {
    let binary = verification.binary_relative_path();
    let step = match check {
        GeneratorBinaryCheck::NativeHostIdentity => native_host_check_step(target, homes, catalog),
        GeneratorBinaryCheck::RustToolchainIdentity => rust_toolchain_check_step(
            target,
            verification.rust_toolchain_version(),
            homes,
            catalog,
        ),
        GeneratorBinaryCheck::BinaryFormatArchitecture => {
            binary_format_architecture_check_step(target, binary, homes, catalog)
        }
        GeneratorBinaryCheck::GnuRuntimeAbi => {
            gnu_runtime_abi_check_step(target, binary, homes, catalog)
        }
        GeneratorBinaryCheck::AppleSdk => apple_sdk_check_step(target, homes, catalog),
        GeneratorBinaryCheck::AppleLinker => apple_linker_check_step(target, homes, catalog),
        GeneratorBinaryCheck::VersionSmoke => {
            version_smoke_check_step(binary, verification.version(), homes, catalog)
        }
        GeneratorBinaryCheck::HelpSmoke => help_smoke_check_step(binary, homes, catalog),
    }
    .map_err(render_error)?;
    crate::steps_plain::plain_step_to_yaml(&step)
}

fn source_binding_step(binding: &GeneratorReleaseSourceBinding) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify current workflow source binding"),
        ),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "VELNOR_RELEASE_SOURCE_SHA".to_owned(),
                Yaml::str(binding.source_expression()),
            )]),
        ),
        (
            "run".to_owned(),
            Yaml::str(
                "set -euo pipefail\ncase \"$VELNOR_RELEASE_SOURCE_SHA\" in\n  ''|*[!0-9a-f]*) echo 'invalid workflow source SHA' >&2; exit 1 ;;\nesac\ntest \"${#VELNOR_RELEASE_SOURCE_SHA}\" -eq 40\ntest \"$GITHUB_SHA\" = \"$VELNOR_RELEASE_SOURCE_SHA\"\nsource_commit=\"$(GIT_NO_REPLACE_OBJECTS=1 git rev-parse --verify HEAD)\"\ntest \"$source_commit\" = \"$VELNOR_RELEASE_SOURCE_SHA\"\nsource_tree=\"$(GIT_NO_REPLACE_OBJECTS=1 git rev-parse --verify \"$source_commit^{tree}\")\"\ntest -n \"$source_tree\"",
            ),
        ),
    ])
}

fn stage_binary_script(verification: &GeneratorBinaryVerification, asset: &str) -> String {
    format!(
        "set -euo pipefail\ncp -- {} {}\ntest -s {}",
        shell_quote(&verification.binary_relative_path().display().to_string()),
        shell_quote(asset),
        shell_quote(asset)
    )
}

fn checksum_script(target: GeneratorReleaseTarget, binary: &str, checksum: &str) -> String {
    let command = match target {
        GeneratorReleaseTarget::LinuxX86_64 => "sha256sum",
        GeneratorReleaseTarget::MacosArm64 | GeneratorReleaseTarget::MacosX86_64 => "shasum -a 256",
    };
    format!(
        "set -euo pipefail\n{command} -- {} > {}",
        shell_quote(binary),
        shell_quote(checksum)
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn render_error(error: impl std::fmt::Display) -> crate::RenderError {
    crate::RenderError::InvalidWorkflow(error.to_string())
}
