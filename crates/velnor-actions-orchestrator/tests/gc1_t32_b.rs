// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_rejects_missing_branch_links() -> Result<(), Box<dyn Error>> {
    let (branch, reader, writer) = fixture_branches()?;
    let detached = Job {
        needs: vec!["plan".to_owned()],
        ..fixture_job(
            writer.cache_mode.clone(),
            writer.condition.clone(),
            writer.steps.clone(),
        )?
    };
    must_fail!(
        validate_combined_branch(
            &branch,
            Some("gate"),
            true,
            &BranchJobs {
                reader_job: &branch.reader,
                reader: &reader,
                writer_job: &branch.writer,
                writer: &detached,
            }
        ),
        "writer without reader need"
    );
    let untrusted = fixture_job(
        writer.cache_mode.clone(),
        Some("github.event_name != 'push'".to_owned()),
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
                writer: &untrusted,
            }
        ),
        "writer on untrusted event"
    );
    let unconditioned_reader = fixture_job(reader.cache_mode.clone(), None, fixture_base_steps())?;
    must_fail!(
        validate_combined_branch(
            &branch,
            Some("gate"),
            true,
            &BranchJobs {
                reader_job: &branch.reader,
                reader: &unconditioned_reader,
                writer_job: &branch.writer,
                writer: &writer,
            }
        ),
        "static reader without gate"
    );
    let ungated = fixture_job(
        writer.cache_mode.clone(),
        Some(TRUSTED_WRITER_EVENT_GATE.to_owned()),
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
                writer: &ungated,
            }
        ),
        "branch condition without group gate"
    );
    Ok(())
}

fn check_rejects_missing_execution_or_recheck() -> Result<(), Box<dyn Error>> {
    let (branch, reader, writer) = fixture_branches()?;
    let idle_reader = fixture_job(
        reader.cache_mode.clone(),
        reader.condition.clone(),
        vec![shell_step("idle")],
    )?;
    must_fail!(
        validate_combined_branch(
            &branch,
            Some("gate"),
            true,
            &BranchJobs {
                reader_job: &branch.reader,
                reader: &idle_reader,
                writer_job: &branch.writer,
                writer: &writer,
            }
        ),
        "warm group never runs"
    );
    let idle_writer = fixture_job(
        writer.cache_mode.clone(),
        writer.condition.clone(),
        vec![shell_step("idle")],
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
                writer: &idle_writer,
            }
        ),
        "cold group never runs"
    );
    let mut early_executor = writer.steps.clone();
    early_executor.swap(0, 1);
    let rushed = fixture_job(
        writer.cache_mode.clone(),
        writer.condition.clone(),
        early_executor,
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
                writer: &rushed,
            }
        ),
        "compilation precedes recheck"
    );
    Ok(())
}

fn check_rejects_suppressed_writer_failure() -> Result<(), Box<dyn Error>> {
    let (branch, reader, writer) = fixture_branches()?;
    let mut suppressed = writer.steps.clone();
    suppressed.push(Step {
        condition: Some("false".to_owned()),
        ..restore_step("Save Cargo sources", StepRole::CargoSourcesSave)
    });
    let hiding = fixture_job(
        writer.cache_mode.clone(),
        writer.condition.clone(),
        suppressed,
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
                writer: &hiding,
            }
        ),
        "writer hides hard failure"
    );
    Ok(())
}

#[test]
fn gc1_t32_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_branch_identity()?;
    check_rejects_bad_authority()?;
    check_rejects_missing_branch_links()?;
    check_rejects_missing_execution_or_recheck()?;
    check_rejects_suppressed_writer_failure()?;
    Ok(())
}
