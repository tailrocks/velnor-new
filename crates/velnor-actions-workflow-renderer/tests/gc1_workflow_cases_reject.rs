// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{CacheMode, NeedsConclusions, REQUIRED_JOB_ID, RunsOn};
use velnor_actions_workflow_renderer::overlap::{PrepOverlap, wire_prep_join};
use velnor_actions_workflow_renderer::steps::bootstrap_bridge_step;

use super::gc1_workflow_cases_common::*;

fn check_rejects_untrusted_event_writes() -> Result<(), Box<dyn Error>> {
    if mode_workflow(pr_trigger(), Some(CacheMode::Write))
        .validate()
        .is_ok()
    {
        return Err(fail("untrusted event path accepted an unconditional write"));
    }
    let mut workflow = mode_workflow(pr_trigger(), None);
    if let Some(reader) = workflow.jobs.get_mut("reader") {
        reader.cache_mode = None;
    }
    if workflow
        .validate_event_authority(
            &names(&["reader"]),
            &names(&["writer"]),
            &names(&["cleanup"]),
        )
        .is_ok()
    {
        return Err(fail(
            "authority check accepted a reader without explicit read",
        ));
    }
    Ok(())
}

fn check_rejects_authority_mismatch() -> Result<(), Box<dyn Error>> {
    let mut jobs = mode_jobs(None);
    if let Some(cleanup) = jobs.get_mut("cleanup") {
        cleanup.steps.insert(0, checkout_step());
    }
    let mut workflow = mode_workflow(pr_trigger(), None);
    workflow.jobs = jobs;
    if workflow
        .validate_event_authority(
            &names(&["reader"]),
            &names(&["writer"]),
            &names(&["cleanup"]),
        )
        .is_ok()
    {
        return Err(fail(
            "authority check let cleanup hold an application checkout",
        ));
    }
    let mut jobs = mode_jobs(None);
    jobs.insert(
        "extra".to_owned(),
        job_with("Extra", None, Some(cleanup_permissions()), Vec::new(), None),
    );
    let mut workflow = mode_workflow(pr_trigger(), None);
    workflow.jobs = jobs;
    if workflow
        .validate_event_authority(
            &names(&["reader"]),
            &names(&["writer"]),
            &names(&["cleanup"]),
        )
        .is_ok()
    {
        return Err(fail(
            "authority check let a non-cleanup job hold actions write",
        ));
    }
    let mut restricted = pr_trigger();
    restricted.pull_request_types = vec!["pull_request_target".to_owned()];
    if mode_workflow(restricted, None).validate().is_ok() {
        return Err(fail(
            "event path accepted a restricted untrusted actor event",
        ));
    }
    let mut scheduled = schedule_trigger();
    scheduled.schedule = Some(velnor_actions_contract::ScheduleTrigger::single(
        "not-a-cron",
    ));
    if mode_workflow(scheduled, None).validate().is_ok() {
        return Err(fail("event path accepted a malformed schedule"));
    }
    Ok(())
}

fn check_rejects_shadow_confusion() -> Result<(), Box<dyn Error>> {
    let sha = "e".repeat(64);
    let dangling_matched = velnor_actions_workflow_renderer::steps::BootstrapBridge {
        expected_key: None,
        ..guarded_bridge()
    };
    if bootstrap_bridge_step(&dangling_matched, &sha).is_ok() {
        return Err(fail(
            "shadow guard accepted a matched key without its expected key",
        ));
    }
    let dangling_expected = velnor_actions_workflow_renderer::steps::BootstrapBridge {
        matched_key: None,
        ..guarded_bridge()
    };
    if bootstrap_bridge_step(&dangling_expected, &sha).is_ok() {
        return Err(fail(
            "shadow guard accepted an expected key without its matched key",
        ));
    }
    for shadow in [String::new(), "has space".to_owned(), "x".repeat(65)] {
        let bridge = velnor_actions_workflow_renderer::steps::BootstrapBridge {
            shadow_state: Some(shadow),
            ..guarded_bridge()
        };
        if bootstrap_bridge_step(&bridge, &sha).is_ok() {
            return Err(fail("shadow guard accepted an untrusted shadow state"));
        }
    }
    for member in [
        "../escape".to_owned(),
        "/absolute".to_owned(),
        String::new(),
    ] {
        let bridge = velnor_actions_workflow_renderer::steps::BootstrapBridge {
            member,
            ..guarded_bridge()
        };
        if bootstrap_bridge_step(&bridge, &sha).is_ok() {
            return Err(fail("shadow guard accepted an escaping member"));
        }
    }
    for staged in ["relative/staged".to_owned(), String::new()] {
        let bridge = velnor_actions_workflow_renderer::steps::BootstrapBridge {
            staged_path: staged,
            ..guarded_bridge()
        };
        if bootstrap_bridge_step(&bridge, &sha).is_ok() {
            return Err(fail("shadow guard accepted an unowned staged path"));
        }
    }
    for max_bytes in [0, u64::MAX] {
        let bridge = velnor_actions_workflow_renderer::steps::BootstrapBridge {
            max_bytes,
            ..guarded_bridge()
        };
        if bootstrap_bridge_step(&bridge, &sha).is_ok() {
            return Err(fail("shadow guard accepted an unbounded payload"));
        }
    }
    if bootstrap_bridge_step(&guarded_bridge(), "not-a-sha").is_ok() {
        return Err(fail("shadow guard accepted a foreign release digest"));
    }
    Ok(())
}

fn check_rejects_ledger_drift() -> Result<(), Box<dyn Error>> {
    let mut jobs = mode_jobs(None);
    if let Some(required) = jobs.get_mut(REQUIRED_JOB_ID) {
        required.needs = Vec::new();
    }
    if NeedsConclusions::from_finalized_jobs(REQUIRED_JOB_ID, &jobs).is_ok() {
        return Err(fail("ledger check accepted an empty required inventory"));
    }
    if NeedsConclusions::from_finalized_jobs("ghost-gate", &mode_jobs(None)).is_ok() {
        return Err(fail("ledger check accepted a missing required gate"));
    }
    let conclusions = match NeedsConclusions::from_finalized_jobs(REQUIRED_JOB_ID, &mode_jobs(None))
    {
        Ok(conclusions) => conclusions,
        Err(error) => return Err(fail(&format!("ledger check lost its baseline:{error}"))),
    };
    let mut drifted = mode_jobs(None);
    if let Some(required) = drifted.get_mut(REQUIRED_JOB_ID) {
        required.needs = vec!["reader".to_owned()];
    }
    if conclusions.gate_matches(&drifted) {
        return Err(fail("ledger check matched a drifted producer set"));
    }
    Ok(())
}

fn check_rejects_branch_collisions() -> Result<(), Box<dyn Error>> {
    let mut jobs = branch_jobs();
    let spec = PrepOverlap {
        download_job: "download".to_owned(),
        image_job: "download".to_owned(),
        join_job: "join".to_owned(),
    };
    if wire_prep_join(&mut jobs, &spec).is_ok() {
        return Err(fail("static branch accepted colliding ledgers"));
    }
    let spec = PrepOverlap {
        download_job: "ghost".to_owned(),
        image_job: "image".to_owned(),
        join_job: "join".to_owned(),
    };
    if wire_prep_join(&mut jobs, &spec).is_ok() {
        return Err(fail("static branch accepted an unknown ledger"));
    }
    let mut cyclic = branch_jobs();
    if let Some(download) = cyclic.get_mut("download") {
        download.needs = vec!["join".to_owned()];
    }
    if let Some(join) = cyclic.get_mut("join") {
        join.needs = vec!["download".to_owned()];
    }
    let spec = PrepOverlap {
        download_job: "download".to_owned(),
        image_job: "image".to_owned(),
        join_job: "join".to_owned(),
    };
    if wire_prep_join(&mut cyclic, &spec).is_ok() {
        return Err(fail("static branch accepted a ledger cycle"));
    }
    if RunsOn::parse("scale-set:velnor").is_ok() {
        return Err(fail("platform limit accepted a malformed scale-set token"));
    }
    Ok(())
}

#[test]
fn gc1_workflow_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_untrusted_event_writes()?;
    check_rejects_authority_mismatch()?;
    check_rejects_shadow_confusion()?;
    check_rejects_ledger_drift()?;
    check_rejects_branch_collisions()?;
    Ok(())
}
