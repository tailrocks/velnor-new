// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn build_target_of(job: &str) -> Result<ReleaseTarget, Box<dyn Error>> {
    match job {
        "build-linux" => Ok(ReleaseTarget::LinuxX86_64),
        "build-macos" => Ok(ReleaseTarget::MacosArm64),
        "build-macos-intel" => Ok(ReleaseTarget::MacosX86_64),
        _ => Err(fail(&format!("unknown release build job:{job}"))),
    }
}

pub(super) fn check_build_jobs_share_context(workflow: &str) -> Result<(), Box<dyn Error>> {
    for job in ["build-linux", "build-macos", "build-macos-intel"] {
        let section = job_section(workflow, job)?;
        let lane = format!(
            "$RUNNER_TEMP/velnor/target/release-{}",
            build_target_of(job)?.triple()
        );
        require(section, "CARGO_TARGET_DIR", job)?;
        require(section, &lane, job)?;
        require(section, "verify-release-source", job)?;
    }
    Ok(())
}

fn check_native_action(
    files: &[RenderedFile],
    logical: &str,
    command: &str,
) -> Result<(), Box<dyn Error>> {
    let action = find_file(files, &format!(".github/actions/{logical}/action.yml"))?;
    require(action, command, logical)?;
    require(action, "$CARGO_TARGET_DIR/release/velnor-actions", logical)?;
    require(action, "missing shared release target dir", logical)?;
    forbid(action, "cp target/release/velnor-actions", logical)?;
    forbid(
        action,
        "cp target/x86_64-apple-darwin/release/velnor-actions",
        logical,
    )?;
    Ok(())
}

pub(super) fn check_build_actions_bind_outputs(
    files: &[RenderedFile],
) -> Result<(), Box<dyn Error>> {
    let native_command =
        "mbx build --release --locked --package velnor-actions-cli --bin velnor-actions";
    check_native_action(files, "generator-release-build-linux", native_command)?;
    check_native_action(files, "generator-release-build-macos", native_command)?;
    let intel = find_file(
        files,
        ".github/actions/generator-release-build-macos-intel/action.yml",
    )?;
    require(
        intel,
        "mbx build --release --locked --package velnor-actions-cli --bin velnor-actions --target x86_64-apple-darwin",
        "generator-release-build-macos-intel",
    )?;
    require(
        intel,
        "rustup target add x86_64-apple-darwin",
        "intel target",
    )?;
    require(
        intel,
        "Install x86_64-apple-darwin Rust target",
        "intel target",
    )?;
    require(
        intel,
        "$CARGO_TARGET_DIR/x86_64-apple-darwin/release/velnor-actions",
        "generator-release-build-macos-intel",
    )?;
    forbid(
        intel,
        "$CARGO_TARGET_DIR/release/velnor-actions",
        "generator-release-build-macos-intel",
    )?;
    require(intel, "missing shared release target dir", "intel target")?;
    Ok(())
}

pub(super) fn check_signers_publishers_free(workflow: &str) -> Result<(), Box<dyn Error>> {
    for job in [
        "attest-linux",
        "attest-macos",
        "attest-macos-intel",
        "attest-manifest",
        "publish-generator",
    ] {
        let section = job_section(workflow, job)?;
        for marker in [
            "actions/cache/restore",
            "actions/cache/save",
            "CARGO_TARGET_DIR",
            "CARGO_HOME",
            "RUSTUP_HOME",
            "MISE_CARGO_HOME",
            "MISE_RUSTUP_HOME",
        ] {
            forbid(section, marker, job)?;
        }
    }
    Ok(())
}

pub(super) fn check_eligibility_and_qualification_retained(
    workflow: &str,
) -> Result<(), Box<dyn Error>> {
    for job in [
        "verify-release-source",
        "candidate-manifest",
        "qualify-linux",
        "qualify-macos",
        "qualify-macos-intel",
        "qualify-homebrew-macos",
        "attest-linux",
        "attest-macos",
        "attest-macos-intel",
        "attest-manifest",
        "publish-generator",
        "build-linux",
        "build-macos",
        "build-macos-intel",
    ] {
        job_section(workflow, job)?;
    }
    let qualify = job_section(workflow, "qualify-linux")?;
    require(qualify, "build-linux", "qualify-linux")?;
    require(qualify, "candidate-manifest", "qualify-linux")?;
    Ok(())
}
