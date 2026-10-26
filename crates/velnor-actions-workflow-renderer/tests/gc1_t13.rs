// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use velnor_actions_contract::{Job, JobTimeout, StepId, StepKind, StepRole};
use velnor_actions_workflow_renderer::cache_elect::elect_cache_writers;
use velnor_actions_workflow_renderer::cache_p08::{ToolsCacheInputs, ToolsCachePayload};
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
        ),
        version: velnor_actions_mise::MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

fn payload(
    setup: &MiseSetup,
    runs_on: &str,
    target: &str,
    specs: &[&str],
    toolchain: Option<&str>,
    components: &[&str],
) -> Result<ToolsCachePayload, Box<dyn Error>> {
    let tool_specs: Vec<String> = specs.iter().map(|spec| (*spec).to_owned()).collect();
    let rustup_components: Vec<String> = components
        .iter()
        .map(|component| (*component).to_owned())
        .collect();
    Ok(ToolsCachePayload::new(ToolsCacheInputs {
        runs_on,
        target,
        mise_setup: setup,
        tool_specs: &tool_specs,
        rustup_toolchain: toolchain,
        rustup_components: &rustup_components,
    })?)
}

fn base_payload(setup: &MiseSetup) -> Result<ToolsCachePayload, Box<dyn Error>> {
    payload(
        setup,
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2", "opentofu@1.13.2"],
        None,
        &[],
    )
}

fn job(payload: &ToolsCachePayload) -> Result<Job, Box<dyn Error>> {
    Ok(Job {
        display_name: "gc1-t13".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        services: Vec::new(),
        steps: vec![payload.runtime_prelude_step()?, payload.restore_step()?],
    })
}

fn save_steps(job: &Job) -> Vec<usize> {
    job.steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(StepRole::ToolsCacheSave))
        .map(|(index, _)| index)
        .collect()
}

fn check_union_and_determinism(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    let base = base_payload(setup)?;
    let same = base_payload(setup)?;
    if base.static_digest() != same.static_digest()
        || base.identity_input() != same.identity_input()
    {
        return Err(fail("host tool set digest drifted"));
    }
    let shuffled = payload(
        setup,
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["opentofu@1.13.2", "gh@2.103.2", "gh@2.103.2"],
        None,
        &[],
    )?;
    if shuffled.static_digest() != base.static_digest()
        || shuffled.identity_input() != base.identity_input()
    {
        return Err(fail("host tool set union kept order or duplicates"));
    }
    if !base.runtime_identity_supported() {
        return Err(fail("supported host lane lost runtime identity"));
    }
    if base.key_expression() != "mise-tools-v2-${{steps.v2.outputs.identity}}" {
        return Err(fail("host tool set key left the shared expression"));
    }
    let key = base.key_for_runtime_identity(&"ab".repeat(32))?;
    if key != format!("mise-tools-v2-{}", "ab".repeat(32)) {
        return Err(fail("host tool set key rejected its runtime identity"));
    }
    if base.paths().is_empty() || !base.paths().contains(&"~/.local/share/mise".to_owned()) {
        return Err(fail("host tool set dropped an approved path"));
    }
    Ok(())
}

fn check_host_distinctions_survive(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    let base = base_payload(setup)?;
    let other_lane = payload(
        setup,
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2", "opentofu@1.13.2"],
        None,
        &[],
    )?;
    if other_lane.static_digest() == base.static_digest() {
        return Err(fail("host distinction collapsed across lanes"));
    }
    let other_target = payload(
        setup,
        "ubuntu-24.04",
        "aarch64-apple-darwin",
        &["gh@2.103.2", "opentofu@1.13.2"],
        None,
        &[],
    )?;
    if other_target.static_digest() == base.static_digest() {
        return Err(fail("host distinction collapsed across targets"));
    }
    if other_target.runtime_identity_supported() {
        return Err(fail("unsupported target kept runtime identity"));
    }
    let clippy = payload(
        setup,
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["rust@1.99.1"],
        Some("1.99.1"),
        &["clippy"],
    )?;
    let full = payload(
        setup,
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        &["rust@1.99.1"],
        Some("1.99.1"),
        &["clippy", "rustfmt"],
    )?;
    if clippy.static_digest() == full.static_digest() {
        return Err(fail("component distinction collapsed"));
    }
    clippy.restore_step()?;
    full.restore_step()?;
    let restore = base.restore_step()?;
    if restore.role != Some(StepRole::ToolsCacheRestore) || restore.id != Some(StepId::ToolsRestore)
    {
        return Err(fail("restore step lost its tools identity"));
    }
    let prelude = base.runtime_prelude_step()?;
    if prelude.role != Some(StepRole::ToolsCacheIdentity)
        || prelude.id != Some(StepId::ToolsCacheIdentity)
    {
        return Err(fail("prelude step lost its tools identity"));
    }
    Ok(())
}

fn check_common_policy_save(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    let owned = base_payload(setup)?;
    let mut jobs: BTreeMap<String, Job> = BTreeMap::from([("worker-b".to_owned(), job(&owned)?)]);
    jobs.insert("worker-a".to_owned(), job(&owned)?);
    elect_cache_writers(&mut jobs)?;
    let saves: Vec<(String, Vec<usize>)> = jobs
        .iter()
        .map(|(id, job)| (id.clone(), save_steps(job)))
        .collect();
    if saves.iter().map(|(_, steps)| steps.len()).sum::<usize>() != 1 {
        return Err(fail("compatible jobs did not elect one common save"));
    }
    let winner = jobs
        .get("worker-a")
        .ok_or_else(|| fail("winner job missing"))?;
    let winner_saves = save_steps(winner);
    if winner_saves.len() != 1 {
        return Err(fail("common save landed outside the elected job"));
    }
    let save = &winner.steps[winner_saves[0]];
    if save.role != Some(StepRole::ToolsCacheSave) {
        return Err(fail("elected save lost its tools role"));
    }
    let StepKind::Action { with, .. } = &save.kind else {
        return Err(fail("elected save is not an action step"));
    };
    if with.get("key").map(String::as_str) != Some(owned.key_expression().as_str()) {
        return Err(fail("elected save left the shared host key"));
    }
    let condition = save
        .condition
        .as_deref()
        .ok_or_else(|| fail("save lost its gate"))?;
    if !condition.contains("success()")
        || !condition.contains("steps.tools-restore.outputs.cache-hit != 'true'")
    {
        return Err(fail("elected save bypassed the common policy gate"));
    }
    elect_cache_writers(&mut jobs)?;
    if saves.iter().map(|(_, steps)| steps.len()).sum::<usize>()
        != jobs
            .values()
            .map(|job| save_steps(job).len())
            .sum::<usize>()
    {
        return Err(fail("common save was not stable under re-election"));
    }
    Ok(())
}

#[test]
fn gc1_t13_preserves_contract() -> Result<(), Box<dyn Error>> {
    let setup = mise_setup();
    setup.validate()?;
    check_union_and_determinism(&setup)?;
    check_host_distinctions_survive(&setup)?;
    check_common_policy_save(&setup)?;
    Ok(())
}

#[path = "gc1_t13_reject.rs"]
mod gc1_t13_reject;
