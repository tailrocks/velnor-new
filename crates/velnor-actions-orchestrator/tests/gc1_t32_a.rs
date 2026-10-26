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

#[test]
fn gc1_t32_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_group_classification()?;
    check_tofu_group_without_probe()?;
    check_static_reader_writer_ids()?;
    check_branch_decided_before_compilation()?;
    check_recheck_after_queue_slot()?;
    check_recoverable_optional_cache()?;
    check_tofu_branches_count_against_limits()?;
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
