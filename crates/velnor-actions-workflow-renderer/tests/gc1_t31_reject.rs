// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_rejects_writer_queue_shapes() -> Result<(), Box<dyn Error>> {
    let empty = Concurrency {
        group: String::new(),
        cancel_in_progress: WRITER_QUEUE_CANCEL_IN_PROGRESS.to_owned(),
    };
    if empty.is_writer_queue() || empty.validate_writer_queue().is_ok() {
        return Err(fail("writer queue accepted an empty group"));
    }
    let cancelling = Concurrency {
        group: "velnor-cache-writer".to_owned(),
        cancel_in_progress: "${{ github.event_name == 'pull_request' }}".to_owned(),
    };
    if cancelling.is_writer_queue() || cancelling.validate_writer_queue().is_ok() {
        return Err(fail("writer queue accepted cancellation of preparation"));
    }
    for group in [
        "velnor-cache-writer-${{ matrix.job }}",
        "velnor-cache-writer-${{ github.run_id }}",
        "velnor-cache-writer-${{ needs.plan.outputs.key }}",
    ] {
        let nested = Concurrency::writer_queue(group.to_owned());
        if nested.validate_writer_queue().is_ok() {
            return Err(fail(&format!(
                "writer queue accepted a nested lock: {group}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn check_rejects_untrusted_writes() -> Result<(), Box<dyn Error>> {
    let mut jobs = authority_jobs();
    jobs.insert(
        "writer".to_owned(),
        job_with(
            "Writer",
            Some(CacheMode::Write),
            None,
            vec![shell_step("Build")],
        ),
    );
    if event_workflow(jobs).validate().is_ok() {
        return Err(fail("untrusted event path accepted an unconditional write"));
    }
    let mut jobs = authority_jobs();
    if let Some(job) = jobs.get_mut("writer") {
        job.cache_mode = Some(CacheMode::push_write_expression());
    }
    let mut workflow = event_workflow(jobs);
    workflow.cache_mode = Some(CacheMode::WriteOnly);
    if workflow.validate().is_ok() {
        return Err(fail("untrusted event path accepted a workflow write"));
    }
    if CacheMode::parse("write-always").is_ok() || CacheMode::parse("").is_ok() {
        return Err(fail("cache mode accepted an unknown grant"));
    }
    Ok(())
}

pub(crate) fn check_rejects_token_confusion() -> Result<(), Box<dyn Error>> {
    let mut jobs = authority_jobs();
    jobs.insert(
        "cleanup".to_owned(),
        job_with(
            "Cleanup",
            None,
            Some(cleanup_permissions()),
            vec![checkout_step(), shell_step("Delete")],
        ),
    );
    if event_workflow(jobs).validate().is_ok() {
        return Err(fail("actions write kept an application checkout"));
    }
    if render_cache_cleanup_yml("has space", "0.1.55").is_ok()
        || render_cache_cleanup_yml("ubuntu-24.04", "").is_ok()
    {
        return Err(fail("cleanup render accepted an invalid runner or version"));
    }
    Ok(())
}

pub(crate) fn check_authority_rejected(
    ir: &WorkflowIr,
    reader: &[&str],
    writer: &[&str],
    cleanup: &[&str],
    message: &str,
) -> Result<(), Box<dyn Error>> {
    if ir
        .validate_event_authority(&names(reader), &names(writer), &names(cleanup))
        .is_ok()
    {
        return Err(fail(message));
    }
    Ok(())
}

pub(crate) fn check_rejects_authority_mismatch() -> Result<(), Box<dyn Error>> {
    let ir = event_workflow(authority_jobs());
    let valid_reader = ["reader"];
    let valid_writer = ["writer"];
    let valid_cleanup = ["cleanup"];
    check_authority_rejected(
        &ir,
        &["ghost"],
        &valid_writer,
        &valid_cleanup,
        "authority check accepted an unknown reader",
    )?;
    check_authority_rejected(
        &ir,
        &valid_reader,
        &["ghost"],
        &valid_cleanup,
        "authority check accepted an unknown writer",
    )?;
    check_authority_rejected(
        &ir,
        &valid_reader,
        &valid_writer,
        &["ghost"],
        "authority check accepted an unknown cleanup",
    )?;
    let mut jobs = authority_jobs();
    jobs.insert(
        "reader".to_owned(),
        job_with(
            "Reader",
            Some(CacheMode::Write),
            None,
            vec![shell_step("Check")],
        ),
    );
    check_authority_rejected(
        &event_workflow(jobs),
        &valid_reader,
        &valid_writer,
        &valid_cleanup,
        "authority check let a reader publish",
    )?;
    let mut jobs = authority_jobs();
    if let Some(job) = jobs.get_mut("cleanup") {
        job.permissions = None;
    }
    check_authority_rejected(
        &event_workflow(jobs),
        &valid_reader,
        &valid_writer,
        &valid_cleanup,
        "authority check let cleanup run without actions write",
    )?;
    let mut jobs = authority_jobs();
    jobs.insert(
        "extra".to_owned(),
        job_with(
            "Extra",
            None,
            Some(cleanup_permissions()),
            vec![shell_step("Extra")],
        ),
    );
    check_authority_rejected(
        &event_workflow(jobs),
        &valid_reader,
        &valid_writer,
        &valid_cleanup,
        "authority check let a non-cleanup job hold actions write",
    )?;
    Ok(())
}

#[test]
fn gc1_t31_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_writer_queue_shapes()?;
    check_rejects_untrusted_writes()?;
    check_rejects_token_confusion()?;
    check_rejects_authority_mismatch()?;
    Ok(())
}
