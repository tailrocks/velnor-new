// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_tofu_branches_count_against_limits() -> Result<(), Box<dyn Error>> {
    let first = combined_branch_for("tofu-a")?;
    let second = combined_branch_for("tofu-b")?;
    let gate_a = "!contains(needs.plan.outputs.not_affected_jobs, ',tofu-a,')";
    let gate_b = "!contains(needs.plan.outputs.not_affected_jobs, ',tofu-b,')";
    let mut jobs: Vec<(String, Job)> = Vec::new();
    for (branch, gate) in [(&first, gate_a), (&second, gate_b)] {
        jobs.push((
            branch.reader.clone(),
            fixture_job(
                Some(CacheMode::explicit_read()),
                Some(reader_branch_condition(Some(gate))?),
                fixture_base_steps(),
            )?,
        ));
        jobs.push((
            branch.writer.clone(),
            fixture_job(
                Some(CacheMode::push_write_expression()),
                Some(writer_branch_condition(Some(gate))?),
                fixture_base_steps(),
            )?,
        ));
    }
    stage_tofu_root_jobs(&mut jobs, &[first.clone(), second.clone()], 1);
    let by_id: BTreeMap<&str, &Job> = jobs.iter().map(|(id, job)| (id.as_str(), job)).collect();
    for (branch, gate, chained) in [(&first, gate_a, false), (&second, gate_b, true)] {
        let reader = by_id
            .get(branch.reader.as_str())
            .ok_or_else(|| fail("reader lost"))?;
        let writer = by_id
            .get(branch.writer.as_str())
            .ok_or_else(|| fail("writer lost"))?;
        for job in [reader, writer] {
            let condition = job
                .condition
                .as_deref()
                .ok_or_else(|| fail("lane dropped gate"))?;
            if !condition.contains(gate) {
                return Err(fail("lane staging dropped the branch decision"));
            }
            if chained && !condition.contains(STAGED_TOFU_JOB_CONDITION) {
                return Err(fail("chained lane lost its queue signal"));
            }
        }
    }
    let second_reader = by_id
        .get(second.reader.as_str())
        .ok_or_else(|| fail("staged reader"))?;
    if second_reader.needs != vec!["plan".to_owned(), first.reader.clone()] {
        return Err(fail("reader lane ignores platform limits"));
    }
    let second_writer = by_id
        .get(second.writer.as_str())
        .ok_or_else(|| fail("staged writer"))?;
    if !second_writer.needs.contains(&first.writer) {
        return Err(fail("writer lane ignores platform limits"));
    }
    Ok(())
}

fn check_reader_first_execution() -> Result<(), Box<dyn Error>> {
    let gate = "!contains(needs.plan.outputs.not_affected_jobs, ',rust-demo,')";
    let reader_condition = reader_branch_condition(Some(gate))?;
    if reader_condition.contains(UNTRUSTED_READER_EVENT_GATE) {
        return Err(fail(
            "reader still gated off push; push writers need a running reader",
        ));
    }
    if reader_condition.contains(TRUSTED_WRITER_EVENT_GATE) {
        return Err(fail("reader gated to push only; pull runs lose coverage"));
    }
    if !reader_condition.contains(",rust-demo,") {
        return Err(fail("reader lost the affected group gate"));
    }
    if reader_branch_condition(None)? != "true" {
        return Err(fail("gateless reader must run on every event"));
    }
    let writer_condition = writer_branch_condition(Some(gate))?;
    if !writer_condition.contains(TRUSTED_WRITER_EVENT_GATE)
        || !writer_condition.contains(",rust-demo,")
    {
        return Err(fail("writer lost trusted push gate"));
    }
    Ok(())
}

#[test]
fn gc1_t32_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_group_classification()?;
    check_tofu_group_without_probe()?;
    check_static_reader_writer_ids()?;
    check_branch_decided_before_compilation()?;
    check_recheck_after_queue_slot()?;
    check_recheck_reads_reader_outputs()?;
    check_recheck_reader_matcher_rejects_lookalikes()?;
    check_recoverable_optional_cache()?;
    check_tofu_branches_count_against_limits()?;
    check_reader_first_execution()?;
    Ok(())
}

pub(crate) fn check_rejects_bad_branch_identity() -> Result<(), Box<dyn Error>> {
    must_fail!(combined_branch_for(""), "empty group");
    must_fail!(combined_branch_for("Rust-demo"), "uppercase group");
    must_fail!(combined_branch_for("rust demo"), "spaced group");
    must_fail!(combined_branch_for("rust\ndemo"), "multiline group");
    must_fail!(reader_branch_condition(Some("")), "empty reader gate");
    must_fail!(
        reader_branch_condition(Some("a\nb")),
        "multiline reader gate"
    );
    must_fail!(writer_branch_condition(Some("")), "empty writer gate");
    must_fail!(
        writer_branch_condition(Some("a\rb")),
        "multiline writer gate"
    );
    must_fail!(
        writer_branch_needs(&["plan".to_owned()], "not a job"),
        "malformed reader need"
    );
    must_fail!(
        writer_branch_needs(&["not a job".to_owned()], "rust-demo-read"),
        "malformed external need"
    );
    Ok(())
}

pub(crate) fn check_rejects_bad_authority() -> Result<(), Box<dyn Error>> {
    let (branch, reader, writer) = fixture_branches()?;
    must_fail!(
        validate_combined_branch(
            &branch,
            Some("gate"),
            true,
            &BranchJobs {
                reader_job: &branch.writer,
                reader: &reader,
                writer_job: &branch.reader,
                writer: &writer,
            }
        ),
        "swapped branch identity"
    );
    let writable_reader = fixture_job(
        Some(CacheMode::push_write_expression()),
        reader.condition.clone(),
        fixture_base_steps(),
    )?;
    must_fail!(
        validate_combined_branch(
            &branch,
            Some("gate"),
            true,
            &BranchJobs {
                reader_job: &branch.reader,
                reader: &writable_reader,
                writer_job: &branch.writer,
                writer: &writer,
            }
        ),
        "reader without explicit read"
    );
    let readable_writer = fixture_job(
        Some(CacheMode::explicit_read()),
        writer.condition.clone(),
        writer.steps.clone(),
    )?;
    must_fail!(
        validate_combined_branch(
            &branch,
            Some("gate"),
            true,
            &BranchJobs {
                reader_job: &branch.reader,
                reader: &reader,
                writer_job: &branch.writer,
                writer: &readable_writer,
            }
        ),
        "writer without cache write"
    );
    let unconditioned = fixture_job(writer.cache_mode.clone(), None, writer.steps.clone())?;
    must_fail!(
        validate_combined_branch(
            &branch,
            Some("gate"),
            true,
            &BranchJobs {
                reader_job: &branch.reader,
                reader: &reader,
                writer_job: &branch.writer,
                writer: &unconditioned,
            }
        ),
        "writer without branch condition"
    );
    Ok(())
}

fn check_recheck_reads_reader_outputs() -> Result<(), Box<dyn Error>> {
    use velnor_actions_contract::workflow::cache_family_mbx::{
        MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_KEY_JOB_OUTPUT, MBX_BUNDLE_RESTORE_KEYS_EXPR,
        MBX_BUNDLE_RESTORE_KEYS_JOB_OUTPUT, is_mbx_bundle_key_needs_expr,
        is_mbx_bundle_restore_keys_needs_expr, mbx_bundle_key_needs_expr,
        mbx_bundle_recheck_condition, mbx_bundle_restore_keys_needs_expr,
    };
    let mut with = BTreeMap::new();
    with.insert("key".to_owned(), MBX_BUNDLE_KEY_EXPR.to_owned());
    with.insert(
        "restore-keys".to_owned(),
        MBX_BUNDLE_RESTORE_KEYS_EXPR.to_owned(),
    );
    with.insert("path".to_owned(), "transport".to_owned());
    let probe = Step {
        name: "Restore state snapshot".to_owned(),
        id: Some(StepId::MbxBundleRestore),
        role: Some(StepRole::MbxBundleRestore),
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: "cache-restore".to_owned(),
            with,
            env: BTreeMap::new(),
        },
    };
    let Some(mut recheck) = writer_recheck_step(std::slice::from_ref(&probe)) else {
        return Err(fail("bundle probe has no recheck"));
    };
    rewrite_recheck_step_keys(&mut recheck, "rust-demo-read");
    let StepKind::Action { with, .. } = &recheck.kind else {
        return Err(fail("recheck lost its action"));
    };
    // The recheck runs before the writer's own key step, so it must read
    // the reader branch outputs through `needs`, never a later step.
    let key = with.get("key").map(String::as_str).unwrap_or_default();
    let keys = with
        .get("restore-keys")
        .map(String::as_str)
        .unwrap_or_default();
    if key != mbx_bundle_key_needs_expr("rust-demo-read")
        || keys != mbx_bundle_restore_keys_needs_expr("rust-demo-read")
    {
        return Err(fail("recheck does not read the reader outputs"));
    }
    if !is_mbx_bundle_key_needs_expr(key) || !is_mbx_bundle_restore_keys_needs_expr(keys) {
        return Err(fail("recheck needs expressions rejected"));
    }
    if with.get("path").map(String::as_str) != Some("transport") {
        return Err(fail("recheck rewrite touched non-key inputs"));
    }
    // The reader is skipped on writer events, so the recheck must skip
    // instead of restoring with an empty key.
    if recheck.condition.as_deref() != Some(&mbx_bundle_recheck_condition("rust-demo-read")[..]) {
        return Err(fail("recheck missing the empty-key guard"));
    }
    // A probe without bundle key references is left untouched.
    let Some(mut plain) = writer_recheck_step(&fixture_base_steps()) else {
        return Err(fail("probe has no recheck"));
    };
    rewrite_recheck_step_keys(&mut plain, "rust-demo-read");
    if plain.condition.is_some() {
        return Err(fail("rewrite guarded a keyless recheck"));
    }
    if with
        .values()
        .any(|value| value.contains("steps.mbx-bundle-key"))
    {
        return Err(fail("recheck still references the later key step"));
    }
    // Pin the published output names: they must be valid GitHub job-output
    // names and match what the reader publishes.
    if MBX_BUNDLE_KEY_JOB_OUTPUT != "mbx_bundle_key"
        || MBX_BUNDLE_RESTORE_KEYS_JOB_OUTPUT != "mbx_bundle_restore_keys"
    {
        return Err(fail("reader output names drifted"));
    }
    Ok(())
}

fn check_recheck_reader_matcher_rejects_lookalikes() -> Result<(), Box<dyn Error>> {
    use velnor_actions_contract::workflow::cache_family_mbx::{
        MBX_BUNDLE_KEY_EXPR, is_mbx_bundle_key_needs_expr, is_mbx_bundle_restore_keys_needs_expr,
        mbx_bundle_key_needs_expr, mbx_bundle_restore_keys_needs_expr,
    };
    // The matcher accepts only well-formed `needs.<job>.outputs.<name>`
    // references and rejects lookalikes.
    if !is_mbx_bundle_key_needs_expr(&mbx_bundle_key_needs_expr("a-b_c9"))
        || !is_mbx_bundle_restore_keys_needs_expr(&mbx_bundle_restore_keys_needs_expr("a-b_c9"))
    {
        return Err(fail("valid reader reference rejected"));
    }
    for bad in [
        MBX_BUNDLE_KEY_EXPR,
        "${{ needs.UPPER.outputs.mbx_bundle_key }}",
        "${{ needs.dot.name.outputs.mbx_bundle_key }}",
        "${{ needs.a.outputs.wrong_name }}",
        "needs.a.outputs.mbx_bundle_key",
        "${{ needs.a.outputs.mbx_bundle_key }} ",
    ] {
        if is_mbx_bundle_key_needs_expr(bad) || is_mbx_bundle_restore_keys_needs_expr(bad) {
            return Err(fail("malformed reader reference accepted"));
        }
    }
    Ok(())
}
