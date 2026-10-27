// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;

use velnor_actions_actionlint::actions::MISE_ACTION_SHA;
use velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script;
use velnor_actions_contract::{
    ReleaseTarget, RoutingWorkflow, Step, StepKind, require_release_version,
};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};
use velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude;
use velnor_actions_workflow_renderer::{
    ProductReleasePins, RenderedFile, Schema2WorkflowRequest, VERIFY_PATH, render_schema2_workflows,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn forbid(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Err(fail(&format!("{label} kept:{needle}")))
    } else {
        Ok(())
    }
}

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
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

fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn verify_rust_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &format!("{} exit 0", credential_unset_prelude()),
    ])
}

fn verify_lint_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &jactionlint_verifier_script("${VELNOR_TOOL_JACTIONLINT}", VERIFY_PATH),
    ])
}

fn pins() -> ProductReleasePins {
    let setup = mise_setup();
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: argv(&["mise", "install", "gh@2.103.2"]),
        install_build_tools_argv: argv(&["mise", "install", "rust@1.91.0", "mr-boxington@1.0.0"]),
        install_qualify_tools_argv: argv(&["mise", "install", "opentofu@1.13.2"]),
        qualify_mbx_action_steps: [fixture_step(), fixture_step(), fixture_step()],
        install_runner_build_tools_argv: Vec::new(),
        install_gh_argv: argv(&["mise", "install", "gh@2.103.2"]),
        build_argv: argv(&[
            "mbx",
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]),
        intel_build_argv: argv(&[
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
        ]),
        install_intel_target_argv: argv(&["rustup", "target", "add", "x86_64-apple-darwin"]),
        install_resource_probe_target_argv: Vec::new(),
        runner_build_argv: Vec::new(),
        resource_probe_build_argv: Vec::new(),
        install_resource_probe_release_tools_argv: Vec::new(),
        resource_probe_release_build_argv: Vec::new(),
        resource_probe_release_test_argv: Vec::new(),
        lint_argv: argv(&["bash", "-c", "exit 0"]),
        verify_lint_argv: verify_lint_argv(),
        gh_argv: argv(&["gh"]),
        rust_version: "1.91.0".to_owned(),
        verify_rust_argv: verify_rust_argv(),
        mr_boxington_version: "1.0.0".to_owned(),
        gh_version: "2.103.2".to_owned(),
    }
}

fn request() -> Result<Schema2WorkflowRequest, Box<dyn Error>> {
    Ok(Schema2WorkflowRequest {
        version: "0.1.55".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(pins()),
    })
}

fn rendered() -> Result<Vec<RenderedFile>, Box<dyn Error>> {
    let request = request()?;
    Ok(render_schema2_workflows(&request)?)
}

fn rendered_with(pins: ProductReleasePins) -> Result<Vec<RenderedFile>, Box<dyn Error>> {
    let mut request = request()?;
    request.product_release = Some(pins);
    Ok(render_schema2_workflows(&request)?)
}

fn find_file<'a>(files: &'a [RenderedFile], path: &str) -> Result<&'a str, Box<dyn Error>> {
    files
        .iter()
        .find(|file| file.path == path)
        .map(|file| file.bytes.as_str())
        .ok_or_else(|| fail(&format!("release file missing:{path}")))
}

fn job_section<'a>(workflow: &'a str, job_id: &str) -> Result<&'a str, Box<dyn Error>> {
    let start = workflow
        .find(&format!("\n  {job_id}:\n"))
        .ok_or_else(|| fail(&format!("release job missing:{job_id}")))?;
    let rest = &workflow[start + 1..];
    let mut end = rest.len();
    let mut offset = 0;
    let mut first = true;
    for line in rest.split_inclusive('\n') {
        if first {
            first = false;
        } else if line.starts_with("  ") && !line.starts_with("   ") && line.contains(':') {
            end = offset;
            break;
        }
        offset += line.len();
    }
    Ok(&rest[..end])
}

fn build_target_of(job: &str) -> Result<ReleaseTarget, Box<dyn Error>> {
    match job {
        "build-linux" => Ok(ReleaseTarget::LinuxX86_64),
        "build-macos" => Ok(ReleaseTarget::MacosArm64),
        "build-macos-intel" => Ok(ReleaseTarget::MacosX86_64),
        _ => Err(fail(&format!("unknown release build job:{job}"))),
    }
}

fn check_build_jobs_share_context(workflow: &str) -> Result<(), Box<dyn Error>> {
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

fn check_build_actions_bind_outputs(files: &[RenderedFile]) -> Result<(), Box<dyn Error>> {
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

fn check_signers_publishers_free(workflow: &str) -> Result<(), Box<dyn Error>> {
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

fn check_eligibility_and_qualification_retained(workflow: &str) -> Result<(), Box<dyn Error>> {
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

#[test]
fn gc1_t42_preserves_contract() -> Result<(), Box<dyn Error>> {
    let files = rendered()?;
    let workflow = find_file(&files, ".github/workflows/product-release-generator.yml")?;
    check_build_jobs_share_context(workflow)?;
    check_build_actions_bind_outputs(&files)?;
    check_signers_publishers_free(workflow)?;
    check_eligibility_and_qualification_retained(workflow)?;
    let again = rendered()?;
    let repeat = find_file(&again, ".github/workflows/product-release-generator.yml")?;
    if workflow != repeat {
        return Err(fail("release render drifted between renders"));
    }
    Ok(())
}

fn expect_render_error(pins: ProductReleasePins, needle: &str) -> Result<(), Box<dyn Error>> {
    match rendered_with(pins) {
        Ok(_) => Err(fail(&format!("invalid release pins accepted:{needle}"))),
        Err(err) => require(&err.to_string(), needle, "release pins"),
    }
}

fn check_rejects_missing_pins() -> Result<(), Box<dyn Error>> {
    let mut request = request()?;
    request.product_release = None;
    match render_schema2_workflows(&request) {
        Ok(_) => Err(fail("missing release pins accepted")),
        Err(err) => require(
            &err.to_string(),
            "product_release_pins_missing",
            "missing pins",
        ),
    }
}

fn check_rejects_empty_build_argv() -> Result<(), Box<dyn Error>> {
    let mut altered = pins();
    altered.build_argv = Vec::new();
    expect_render_error(altered, "empty_argv")
}

fn check_rejects_bad_mise_setup() -> Result<(), Box<dyn Error>> {
    let mut altered = pins();
    "not-hex".clone_into(&mut altered.linux_x86_64_setup.sha256);
    expect_render_error(altered, "bad_mise_sha256")
}

fn check_rejects_bad_release_version() -> Result<(), Box<dyn Error>> {
    match require_release_version("not-a-version", "release.yml") {
        Ok(()) => Err(fail("non-release build accepted")),
        Err(err) => require(&err.to_string(), "non_release_build", "release version"),
    }
}

#[test]
fn gc1_t42_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_missing_pins()?;
    check_rejects_empty_build_argv()?;
    check_rejects_bad_mise_setup()?;
    check_rejects_bad_release_version()?;
    Ok(())
}
