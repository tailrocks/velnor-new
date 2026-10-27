// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! PUBLISH-path snapshot regressions (threads 4389237062, 4389351106).
//!
//! The export step must measure the uncompressed state root with `du`
//! before packing and refuse it over the family ceiling, and the
//! publication record plus the `published` print must move out of the
//! export step into a record step conditioned on the save outcome, so a
//! rejected or failed save can never leave PASS-looking evidence.

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::io::Write as _;

use velnor_actions_actionlint::actions::MISE_ACTION_SHA;
use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_KEY_EXPR;
use velnor_actions_contract::{
    PullRequestCachePolicy, RustSuiteConfig, RustSuiteTrigger, StepKind,
};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::rust_suites::{RustSuitesRenderSpec, render_suites_workflow};
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};
use velnor_actions_workflow_renderer::steps::{
    CHECKOUT_USES, CompileDriver, MbxTransport, mbx_steps_for_driver,
};
use velnor_actions_workflow_renderer::toolchain_env::{checked_task_env, credential_unset_prelude};

const MBX_VERSION: &str = "1.91.0";
const RUST_VERSION: &str = "1.89.0";
const NEXTEST_VERSION: &str = "0.9.150";
const EXPORT_STEP: &str = "Publish state snapshot";
const SAVE_STEP: &str = "Save state snapshot";
const RECORD_STEP: &str = "Record state publication";
const SAVE_OUTCOME_GATE: &str = "steps.velnor-optional-mbx-bundle-save.outcome == 'success'";
const RECORD_CONDITION: &str =
    "always() && !cancelled() && steps.velnor-optional-mbx-bundle-save.outcome == 'success'";
/// 400 MiB of real bytes: over the 384 MiB family ceiling with margin.
const OVER_CEILING_BYTES: u64 = 400 * 1024 * 1024;

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

fn require_order(
    haystack: &str,
    first: &str,
    second: &str,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let first_pos = haystack
        .find(first)
        .ok_or_else(|| fail(&format!("{label} lost:{first}")))?;
    let second_pos = haystack
        .find(second)
        .ok_or_else(|| fail(&format!("{label} lost:{second}")))?;
    if first_pos < second_pos {
        Ok(())
    } else {
        Err(fail(&format!("{label} misordered:{first}:{second}")))
    }
}

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

fn spec() -> RustSuitesRenderSpec {
    let mut suite = RustSuiteConfig::new("t53snap", "default");
    suite.triggers = vec![RustSuiteTrigger::Push, RustSuiteTrigger::WorkflowDispatch];
    RustSuitesRenderSpec {
        suites: vec![suite],
        workflow_dispatch: None,
        default_runs_on: "ubuntu-24.04".to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        generator_version: "0.1.55".to_owned(),
        mise: mise_setup(),
        rust_version: RUST_VERSION.to_owned(),
        cargo_nextest_version: NEXTEST_VERSION.to_owned(),
        compile_driver: CompileDriver::Mbx,
        mbx_action: Some((
            format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
            MBX_VERSION.to_owned(),
        )),
        pull_request_cache_policy: PullRequestCachePolicy::default(),
        verify_restored_rust_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("{} exit 0", credential_unset_prelude()),
        ],
        verify_rust_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("{} exit 0", credential_unset_prelude()),
        ],
        export_rustup_toolchain: false,
    }
}

fn render_text() -> Result<String, Box<dyn Error>> {
    render_suites_workflow(&spec())?.ok_or_else(|| fail("render lost its suites workflow"))
}

/// Slice one rendered step: from its `name:` marker to the next step's
/// `- name:` entry (or the end of the job text).
fn step_section<'a>(text: &'a str, name: &str) -> Result<&'a str, Box<dyn Error>> {
    let begin = text
        .find(name)
        .ok_or_else(|| fail(&format!("render lost its step:{name}")))?;
    let tail = &text[begin..];
    let close = tail
        .find("\n      - ")
        .map_or(text.len(), |pos| pos + begin);
    Ok(&text[begin..close])
}

/// Decode one `run: "..."` double-quoted scalar from a step section. The
/// emitter only escapes `\\` and `\"` inside single-line scripts, so any
/// other escape fails the extraction loudly instead of silently mangling
/// the script under test.
fn extract_run(section: &str, label: &str) -> Result<String, Box<dyn Error>> {
    let marker = "run: \"sh -c '";
    let begin = section
        .find(marker)
        .ok_or_else(|| fail(&format!("{label} lost its inline script")))?;
    let mut out = String::from("sh -c '");
    let mut chars = section[begin + marker.len()..].chars();
    loop {
        match chars.next() {
            None => return Err(fail(&format!("{label} left an unterminated script"))),
            Some('"') => break,
            Some('\\') => match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    return Err(fail(&format!(
                        "{label} left an unsupported escape:\\{other}"
                    )));
                }
                None => return Err(fail(&format!("{label} left an unterminated script"))),
            },
            Some(ch) => {
                if ch == '\n' {
                    return Err(fail(&format!("{label} left a multiline script")));
                }
                out.push(ch);
            }
        }
    }
    if !out.starts_with("sh -c 'set -eu;") && !out.contains("; set -eu;") {
        return Err(fail(&format!("{label} lost its shell preamble")));
    }
    Ok(out)
}

fn check_shell_syntax(script: &str, label: &str) -> Result<(), Box<dyn Error>> {
    let output = std::process::Command::new("sh")
        .arg("-n")
        .arg("-c")
        .arg(script)
        .output()
        .map_err(|error| fail(&format!("shell check failed to run: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(fail(&format!(
            "{label} left invalid shell: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

fn check_export_bounds_uncompressed(export: &str) -> Result<(), Box<dyn Error>> {
    for marker in [
        "du -sb",
        "over_ceiling",
        "state-publication.staged",
        "exported=true",
        "readback verified",
    ] {
        require(export, marker, "export step")?;
    }
    // The publication record and the `published` print must not be emitted
    // before the save outcome is known.
    forbid(export, "state-publication.record", "export step")?;
    forbid(export, "state snapshot published (", "export step")?;
    Ok(())
}

fn check_record_after_save(text: &str, record: &str) -> Result<(), Box<dyn Error>> {
    require_order(text, EXPORT_STEP, SAVE_STEP, "postlude order")?;
    require_order(text, SAVE_STEP, RECORD_STEP, "postlude order")?;
    require(record, RECORD_CONDITION, "record gate")?;
    for marker in [
        SAVE_OUTCOME_GATE,
        "state-publication.record",
        "state snapshot published (",
        "state-publication.staged",
    ] {
        require(record, marker, "record step")?;
    }
    Ok(())
}

#[test]
fn gc1_t53_publish_binds_evidence_to_save() -> Result<(), Box<dyn Error>> {
    let text = render_text()?;
    require(&text, EXPORT_STEP, "postlude")?;
    require(&text, SAVE_STEP, "postlude")?;
    require(&text, RECORD_STEP, "postlude")?;
    let export = step_section(&text, EXPORT_STEP)?.to_owned();
    let record = step_section(&text, RECORD_STEP)?.to_owned();
    check_export_bounds_uncompressed(&export)?;
    check_record_after_save(&text, &record)?;
    check_shell_syntax(&extract_run(&export, "export")?, "export")?;
    check_shell_syntax(&extract_run(&record, "record")?, "record")?;
    Ok(())
}

fn driver_env() -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    Ok(checked_task_env(
        &BTreeMap::new(),
        "${{ runner.temp }}/velnor/rustup",
        "${{ runner.temp }}/velnor/cargo",
        "1.89.0",
    )?)
}

fn shell_script_of(step: &velnor_actions_contract::Step) -> Result<String, Box<dyn Error>> {
    match &step.kind {
        StepKind::Shell { run, .. } => run
            .get(2)
            .cloned()
            .ok_or_else(|| fail("shell step left its inline script position")),
        _ => Err(fail("expected a shell step")),
    }
}

fn check_recover_bounds_uncompressed() -> Result<(), Box<dyn Error>> {
    let steps = mbx_steps_for_driver(
        &format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
        CompileDriver::Mbx,
        "1.2.3",
        "1.89.0",
        driver_env()?,
        MbxTransport::SeedRead,
        None,
    )?
    .ok_or_else(|| fail("mbx driver produced no steps"))?;
    let script = shell_script_of(&steps[5])?;
    require(&script, "du -sb", "recover step")?;
    require(&script, "cache_corrupt", "recover step")?;
    check_shell_syntax(&script, "recover")?;
    Ok(())
}

#[test]
fn gc1_t53_recover_bounds_uncompressed() -> Result<(), Box<dyn Error>> {
    check_recover_bounds_uncompressed()
}

fn shell_scratch(name: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!(
        "velnor-t53-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| fail(&format!("clock unreadable: {error}")))?
            .as_nanos()
    ));
    std::fs::create_dir_all(&root)
        .map_err(|error| fail(&format!("scratch unwritable: {error}")))?;
    Ok(root)
}

fn github_output_value(path: &std::path::Path, key: &str) -> Result<String, Box<dyn Error>> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| fail(&format!("github output unreadable: {error}")))?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix(&format!("{key}=")) {
            return Ok(value.to_owned());
        }
    }
    Err(fail(&format!("github output lost: {key}")))
}

struct PublishSetup {
    scratch: std::path::PathBuf,
    runner: std::path::PathBuf,
    workspace: std::path::PathBuf,
    github_output: std::path::PathBuf,
    export: String,
    record: String,
}

fn publish_setup() -> Result<PublishSetup, Box<dyn Error>> {
    let text = render_text()?;
    let export = extract_run(step_section(&text, EXPORT_STEP)?, "export")?;
    let record = extract_run(step_section(&text, RECORD_STEP)?, "record")?;
    let scratch = shell_scratch("publish")?;
    let runner = scratch.join("runner");
    let workspace = scratch.join("workspace");
    std::fs::create_dir_all(runner.join("velnor"))
        .map_err(|error| fail(&format!("runner unwritable: {error}")))?;
    std::fs::create_dir_all(&workspace)
        .map_err(|error| fail(&format!("workspace unwritable: {error}")))?;
    std::fs::write(workspace.join("Cargo.lock"), "lock-fixture")
        .map_err(|error| fail(&format!("lock unwritable: {error}")))?;
    Ok(PublishSetup {
        github_output: scratch.join("github-output"),
        scratch,
        runner,
        workspace,
        export,
        record,
    })
}

/// Run the discovery prelude so the export/record scripts see a trusted
/// discovery record with a matching compat digest.
fn publish_discover(setup: &PublishSetup) -> Result<(), Box<dyn Error>> {
    let steps = mbx_steps_for_driver(
        &format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
        CompileDriver::Mbx,
        "1.2.3",
        "1.89.0",
        driver_env()?,
        MbxTransport::SeedRead,
        None,
    )?
    .ok_or_else(|| fail("mbx driver produced no steps"))?;
    let discover = shell_script_of(&steps[3])?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&discover)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .env("GITHUB_OUTPUT", &setup.github_output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_PROTECTED", "true")
        .env("VELNOR_DEFAULT_REF", "refs/heads/main")
        .env("VELNOR_MBX_BUNDLE_VERSION", "1.2.3")
        .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.89.0")
        .env("VELNOR_MBX_BUNDLE_ROOT", ".")
        .status()
        .map_err(|error| fail(&format!("discover failed to run: {error}")))?;
    if !status.success() {
        return Err(fail("discover script failed on trusted push"));
    }
    if github_output_value(&setup.github_output, "provenance")? != "trusted" {
        return Err(fail("discover misclassified a trusted push"));
    }
    // The producer built the state root in-job: record its success exactly
    // like the producer-mark step does.
    std::fs::write(setup.runner.join("velnor/state-producer-ok"), "")
        .map_err(|error| fail(&format!("mark unwritable: {error}")))?;
    Ok(())
}

fn write_state_file(setup: &PublishSetup, name: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let root = setup.runner.join("velnor-gc1/state");
    std::fs::create_dir_all(&root)
        .map_err(|error| fail(&format!("state root unwritable: {error}")))?;
    std::fs::write(root.join(name), bytes)
        .map_err(|error| fail(&format!("state unwritable: {error}")))?;
    Ok(())
}

/// Fill the state root with real bytes over the family ceiling. Real bytes
/// (not a sparse hole) so `du` reports them on every platform.
fn write_over_ceiling_state(setup: &PublishSetup) -> Result<(), Box<dyn Error>> {
    let root = setup.runner.join("velnor-gc1/state");
    std::fs::create_dir_all(&root)
        .map_err(|error| fail(&format!("state root unwritable: {error}")))?;
    let chunk = vec![0u8; 1024 * 1024];
    let mut file = std::fs::File::create(root.join("bulk"))
        .map_err(|error| fail(&format!("bulk unwritable: {error}")))?;
    for _ in 0..(OVER_CEILING_BYTES / 1024 / 1024) {
        file.write_all(&chunk)
            .map_err(|error| fail(&format!("bulk unwritable: {error}")))?;
    }
    Ok(())
}

fn run_export(setup: &PublishSetup) -> Result<String, Box<dyn Error>> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&setup.export)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .env("GITHUB_OUTPUT", &setup.github_output)
        .env("MBX_CACHE_EXPORT_GROUP", "velnor")
        .output()
        .map_err(|error| fail(&format!("export failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail(&format!(
            "export script failed the step: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn run_record(setup: &PublishSetup) -> Result<String, Box<dyn Error>> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&setup.record)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .output()
        .map_err(|error| fail(&format!("record failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail(&format!(
            "record script failed the step: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

#[test]
#[cfg(unix)]
fn gc1_t53_export_stages_for_save() -> Result<(), Box<dyn Error>> {
    let setup = publish_setup()?;
    publish_discover(&setup)?;
    write_state_file(&setup, "payload", b"state bytes")?;
    let output = run_export(&setup)?;
    require(&output, "readback verified", "export behavior")?;
    forbid(&output, "state snapshot published (", "export behavior")?;
    if github_output_value(&setup.github_output, "exported")? != "true" {
        return Err(fail("export must admit a small snapshot"));
    }
    if !setup
        .runner
        .join("velnor/state-publication.staged")
        .is_file()
    {
        return Err(fail("export must stage its attestation"));
    }
    if setup
        .runner
        .join("velnor/state-publication.record")
        .exists()
    {
        return Err(fail("export must not write the publication record"));
    }
    // The save succeeded: the record step now publishes the evidence.
    let recorded = run_record(&setup)?;
    require(&recorded, "state snapshot published (", "record behavior")?;
    if !setup
        .runner
        .join("velnor/state-publication.record")
        .is_file()
    {
        return Err(fail("record must write the publication record"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t53_record_without_save_writes_nothing() -> Result<(), Box<dyn Error>> {
    let setup = publish_setup()?;
    publish_discover(&setup)?;
    write_state_file(&setup, "payload", b"state bytes")?;
    // The save never ran: no staged attestation exists.
    let output = run_record(&setup)?;
    forbid(&output, "state snapshot published (", "record behavior")?;
    if setup
        .runner
        .join("velnor/state-publication.record")
        .exists()
    {
        return Err(fail("record must not publish without a save"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t53_export_refuses_uncompressed_over_ceiling() -> Result<(), Box<dyn Error>> {
    let setup = publish_setup()?;
    publish_discover(&setup)?;
    write_over_ceiling_state(&setup)?;
    let output = run_export(&setup)?;
    // Highly compressible zeros: the compressed tarball would fit, but the
    // uncompressed root must be refused before packing.
    require(&output, "not published (over_ceiling)", "export ceiling")?;
    forbid(&output, "state snapshot published (", "export ceiling")?;
    if setup
        .runner
        .join("velnor-gc1/transport/state/payload.tar.gz")
        .exists()
    {
        return Err(fail("over-ceiling root must never be packed"));
    }
    if setup
        .runner
        .join("velnor/state-publication.staged")
        .exists()
        || setup
            .runner
            .join("velnor/state-publication.record")
            .exists()
    {
        return Err(fail("over-ceiling root must leave no evidence"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

fn run_discover_key(
    discover: &str,
    runner: &std::path::Path,
    workspace: &std::path::Path,
    scratch: &std::path::Path,
) -> Result<String, Box<dyn Error>> {
    let github_output = scratch.join("github-output");
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(discover)
        .env("RUNNER_TEMP", runner)
        .env("GITHUB_WORKSPACE", workspace)
        .env("GITHUB_OUTPUT", &github_output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_PROTECTED", "true")
        .env("VELNOR_DEFAULT_REF", "refs/heads/main")
        .env("VELNOR_MBX_BUNDLE_VERSION", "1.2.3")
        .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.89.0")
        .env("VELNOR_MBX_BUNDLE_ROOT", ".")
        .status()
        .map_err(|error| fail(&format!("discover failed to run: {error}")))?;
    if !status.success() {
        return Err(fail("discover script failed on trusted push"));
    }
    github_output_value(&github_output, "key")
}

fn pack_over_ceiling_fixture(
    scratch: &std::path::Path,
    runner: &std::path::Path,
) -> Result<(), Box<dyn Error>> {
    // Pack a state root whose uncompressed bytes exceed the ceiling with
    // highly compressible content, so only an uncompressed cap can refuse
    // it after extraction.
    let pack_parent = scratch.join("pack");
    let pack_root = pack_parent.join("state");
    std::fs::create_dir_all(&pack_root)
        .map_err(|error| fail(&format!("pack root unwritable: {error}")))?;
    let chunk = vec![0u8; 1024 * 1024];
    let mut bulk = std::fs::File::create(pack_root.join("bulk"))
        .map_err(|error| fail(&format!("bulk unwritable: {error}")))?;
    for _ in 0..(OVER_CEILING_BYTES / 1024 / 1024) {
        bulk.write_all(&chunk)
            .map_err(|error| fail(&format!("bulk unwritable: {error}")))?;
    }
    drop(bulk);
    std::fs::create_dir_all(runner.join("velnor-gc1/transport/state"))
        .map_err(|error| fail(&format!("transport unwritable: {error}")))?;
    let packed = std::process::Command::new("tar")
        .arg("-czf")
        .arg(runner.join("velnor-gc1/transport/state/payload.tar.gz"))
        .arg("-C")
        .arg(&pack_parent)
        .arg("state")
        .status()
        .map_err(|error| fail(&format!("fixture pack failed to run: {error}")))?;
    if !packed.success() {
        return Err(fail("fixture pack failed"));
    }
    Ok(())
}

fn run_recover_cold(
    recover: &str,
    runner: &std::path::Path,
    workspace: &std::path::Path,
) -> Result<String, Box<dyn Error>> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(recover)
        .env("RUNNER_TEMP", runner)
        .env("GITHUB_WORKSPACE", workspace)
        .output()
        .map_err(|error| fail(&format!("recover failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail("over-ceiling archive must go cold, not fail the step"));
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_uncompressed_over_ceiling() -> Result<(), Box<dyn Error>> {
    let steps = mbx_steps_for_driver(
        &format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
        CompileDriver::Mbx,
        "1.2.3",
        "1.89.0",
        driver_env()?,
        MbxTransport::SeedRead,
        None,
    )?
    .ok_or_else(|| fail("mbx driver produced no steps"))?;
    let scratch = shell_scratch("recover-ceiling")?;
    let runner = scratch.join("runner");
    let workspace = scratch.join("workspace");
    std::fs::create_dir_all(runner.join("velnor"))
        .map_err(|error| fail(&format!("runner unwritable: {error}")))?;
    std::fs::create_dir_all(&workspace)
        .map_err(|error| fail(&format!("workspace unwritable: {error}")))?;
    std::fs::write(workspace.join("Cargo.lock"), "lock-fixture")
        .map_err(|error| fail(&format!("lock unwritable: {error}")))?;
    let discover = shell_script_of(&steps[3])?;
    let key = run_discover_key(&discover, &runner, &workspace, &scratch)?;
    pack_over_ceiling_fixture(&scratch, &runner)?;
    let recover = shell_script_of(&steps[5])?
        .replace(MBX_BUNDLE_KEY_EXPR, &key)
        .replace(
            "${{steps.mbx-bundle-restore.outputs.cache-matched-key}}",
            &key,
        );
    let combined = run_recover_cold(&recover, &runner, &workspace)?;
    require(&combined, "cache_corrupt", "recover ceiling")?;
    if runner.join("velnor/state-recovery.record").exists() {
        return Err(fail("over-ceiling archive must leave no recovery record"));
    }
    if runner.join("velnor-gc1/state").exists() {
        return Err(fail("over-ceiling archive must remove the state root"));
    }
    std::fs::remove_dir_all(&scratch).map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}
