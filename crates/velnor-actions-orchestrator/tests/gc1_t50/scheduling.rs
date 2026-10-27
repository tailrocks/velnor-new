// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use velnor_actions_contract::{CacheMode, Job, JobTimeout, Step, StepKind};

use super::crate_jobs_stage::{
    BranchJobs, CombinedBranch, TRUSTED_WRITER_EVENT_GATE, UNTRUSTED_READER_EVENT_GATE,
    combined_branch_for, reader_branch_condition, validate_combined_branch,
    writer_branch_condition, writer_branch_needs,
};

pub(super) fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

pub(super) fn affected_gate() -> String {
    "!contains(needs.plan.outputs.not_affected_jobs, ',rust-f7-job,')".to_owned()
}

pub(super) fn execution_step() -> Step {
    Step {
        name: "Run lane".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::TaskExecution {
            argv: vec!["mbx".to_owned(), "build".to_owned()],
            env: BTreeMap::new(),
            task_id: "internal/build/dev".to_owned(),
            task_digest: "digest".to_owned(),
            toolchain_inputs: velnor_actions_contract::cachekey::ToolchainInputs {
                tools: vec!["rust".to_owned()],
                components: Vec::new(),
                compile_driver: "cargo".to_owned(),
                test_runner: "cargo".to_owned(),
            },
            matrix_id: "matrix".to_owned(),
            matrix_key: "key".to_owned(),
            report_helper_version: "0.1.0".to_owned(),
            matrix_max_parallel: None,
        },
    }
}

pub(super) fn base_job(
    cache_mode: CacheMode,
    condition: String,
    needs: Vec<String>,
) -> Result<Job, Box<dyn Error>> {
    Ok(Job {
        display_name: "Rust / f7".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::new(10).map_err(|_| fail("bad fixture timeout"))?,
        needs,
        condition: Some(condition),
        permissions: None,
        cache_mode: Some(cache_mode),
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        steps: vec![execution_step()],
    })
}

pub(super) fn reader_first_pair() -> Result<(CombinedBranch, Job, Job), Box<dyn Error>> {
    let branch = combined_branch_for("rust-f7-job")?;
    let gate = affected_gate();
    let reader = base_job(
        CacheMode::explicit_read(),
        reader_branch_condition(Some(&gate))?,
        vec!["plan".to_owned()],
    )?;
    let writer = base_job(
        CacheMode::push_write_expression(),
        writer_branch_condition(Some(&gate))?,
        writer_branch_needs(&["plan".to_owned()], &branch.reader)?,
    )?;
    Ok((branch, reader, writer))
}

pub(super) fn check_reader_runs_where_writer_runs() -> Result<(), Box<dyn Error>> {
    let gate = affected_gate();
    let reader_condition = reader_branch_condition(Some(&gate))?;
    let writer_condition = writer_branch_condition(Some(&gate))?;
    if reader_condition.contains(UNTRUSTED_READER_EVENT_GATE)
        || reader_condition.contains(TRUSTED_WRITER_EVENT_GATE)
    {
        return Err(fail(
            "reader carries an event gate; push writers would need a skipped job",
        ));
    }
    let expected = format!("{reader_condition} && {TRUSTED_WRITER_EVENT_GATE}");
    if writer_condition != expected {
        return Err(fail(
            "writer gate is not the reader gate plus the push trust gate; \
             a writer could run while its reader is skipped",
        ));
    }
    if reader_branch_condition(None)? != "true" {
        return Err(fail("gateless reader must run on every event"));
    }
    if writer_branch_condition(None)? != TRUSTED_WRITER_EVENT_GATE {
        return Err(fail(
            "gateless writer must keep exactly the push trust gate",
        ));
    }
    let (branch, reader, writer) = reader_first_pair()?;
    if !writer.needs.contains(&branch.reader) {
        return Err(fail("writer lost its needs edge on the reader"));
    }
    validate_combined_branch(
        &branch,
        Some(&gate),
        false,
        &BranchJobs {
            reader_job: &branch.reader,
            reader: &reader,
            writer_job: &branch.writer,
            writer: &writer,
        },
    )
    .map_err(|error| fail(&format!("reader-first pair rejected: {error:?}")))?;
    Ok(())
}

pub(super) fn check_push_skipped_reader_rejected() -> Result<(), Box<dyn Error>> {
    let branch = combined_branch_for("rust-f7-job")?;
    let gate = affected_gate();
    let stale_reader_condition = format!("({gate}) && {UNTRUSTED_READER_EVENT_GATE}");
    let reader = base_job(
        CacheMode::explicit_read(),
        stale_reader_condition,
        vec!["plan".to_owned()],
    )?;
    let writer = base_job(
        CacheMode::push_write_expression(),
        writer_branch_condition(Some(&gate))?,
        writer_branch_needs(&["plan".to_owned()], &branch.reader)?,
    )?;
    let error = match validate_combined_branch(
        &branch,
        Some(&gate),
        false,
        &BranchJobs {
            reader_job: &branch.reader,
            reader: &reader,
            writer_job: &branch.writer,
            writer: &writer,
        },
    ) {
        Ok(()) => return Err(fail("push-skipped reader passed validation")),
        Err(error) => error,
    };
    if !format!("{error:?}").contains("reader_excludes_trusted_event") {
        return Err(fail(&format!(
            "push-skipped reader rejected for the wrong reason: {error:?}"
        )));
    }
    Ok(())
}

pub(super) fn check_signal_flows_as_data_not_gate() -> Result<(), Box<dyn Error>> {
    let gate = affected_gate();
    for condition in [
        reader_branch_condition(Some(&gate))?,
        writer_branch_condition(Some(&gate))?,
        reader_branch_condition(None)?,
        writer_branch_condition(None)?,
    ] {
        if condition.contains("needs_prepare") || condition.contains(".result") {
            return Err(fail(
                "branch condition gates on coverage data; the needs_prepare signal \
                 must flow as ledger data inside the writer, not as a job gate",
            ));
        }
    }
    let reader_condition = reader_branch_condition(Some(&gate))?;
    if reader_condition.contains("github.event_name") {
        return Err(fail(
            "reader excludes an event; readers must run on push, pull_request, and merge_group",
        ));
    }
    let writer_condition = writer_branch_condition(Some(&gate))?;
    if !writer_condition.contains(TRUSTED_WRITER_EVENT_GATE) {
        return Err(fail("writer lost the trusted push gate"));
    }
    Ok(())
}
