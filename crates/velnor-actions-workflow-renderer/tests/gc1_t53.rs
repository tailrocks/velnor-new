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

#[path = "gc1_t53_publish.rs"]
mod gc1_t53_publish;
#[path = "gc1_t53_recover.rs"]
mod gc1_t53_recover;
