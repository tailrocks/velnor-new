// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;

use velnor_actions_contract::workflow::ir::WRITER_QUEUE_CANCEL_IN_PROGRESS;
use velnor_actions_contract::{
    CacheMode, Concurrency, Job, JobTimeout, PermissionLevel, Permissions, Step, StepKind, Trigger,
    WorkflowIr,
};
use velnor_actions_workflow_renderer::cache_cleanup::{
    CACHE_CLEANUP_PATH, render_cache_cleanup_yml,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn shell_step(name: &str) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned(), "-c".to_owned(), "printf ok".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn checkout_step() -> Step {
    Step {
        name: "Checkout".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

fn job_with(
    display: &str,
    cache_mode: Option<CacheMode>,
    permissions: Option<Permissions>,
    steps: Vec<Step>,
) -> Job {
    Job {
        display_name: display.to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions,
        cache_mode,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        steps,
    }
}

fn cleanup_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::Write,
        packages: PermissionLevel::None,
    }
}

fn event_triggers() -> Trigger {
    Trigger {
        pull_request_types: vec!["opened".to_owned()],
        push_branches: vec!["main".to_owned()],
        merge_group: true,
        workflow_dispatch: None,
        schedule: None,
    }
}

fn event_workflow(jobs: BTreeMap<String, Job>) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: event_triggers(),
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "velnor-${{ github.workflow }}-${{ github.ref }}".to_owned(),
            cancel_in_progress: "${{ github.event_name == 'pull_request' }}".to_owned(),
        },
        jobs,
    }
}

fn authority_jobs() -> BTreeMap<String, Job> {
    BTreeMap::from([
        (
            "reader".to_owned(),
            job_with(
                "Reader",
                Some(CacheMode::explicit_read()),
                None,
                vec![shell_step("Check")],
            ),
        ),
        (
            "writer".to_owned(),
            job_with(
                "Writer",
                Some(CacheMode::push_write_expression()),
                None,
                vec![shell_step("Build")],
            ),
        ),
        (
            "cleanup".to_owned(),
            job_with(
                "Cleanup",
                None,
                Some(cleanup_permissions()),
                vec![shell_step("Delete")],
            ),
        ),
    ])
}

fn names<'a>(values: &'a [&'a str]) -> BTreeSet<&'a str> {
    values.iter().copied().collect()
}

fn check_explicit_read_modes() -> Result<(), Box<dyn Error>> {
    if CacheMode::explicit_read() != CacheMode::Read {
        return Err(fail("explicit read left its read mode"));
    }
    if !CacheMode::explicit_read().is_explicit_read() {
        return Err(fail("explicit read lost its read identity"));
    }
    if CacheMode::explicit_read().grants_write() {
        return Err(fail("explicit read kept a write capability"));
    }
    if CacheMode::explicit_read().grants_unconditional_write() {
        return Err(fail("explicit read kept an unconditional write"));
    }
    for mode in [
        CacheMode::Write,
        CacheMode::WriteOnly,
        CacheMode::push_write_expression(),
    ] {
        if !mode.grants_write() {
            return Err(fail("trusted writer lost its write capability"));
        }
        if mode.is_explicit_read() {
            return Err(fail("trusted writer claimed an explicit read"));
        }
    }
    if !CacheMode::Write.grants_unconditional_write()
        || !CacheMode::WriteOnly.grants_unconditional_write()
        || CacheMode::push_write_expression().grants_unconditional_write()
    {
        return Err(fail("unconditional write lost its boundary"));
    }
    if CacheMode::parse("read")? != CacheMode::explicit_read() {
        return Err(fail("read mode lost its parse round trip"));
    }
    Ok(())
}

fn check_writer_queue_shape() -> Result<(), Box<dyn Error>> {
    let queue =
        Concurrency::writer_queue("velnor-cache-writer-${{ github.repository }}".to_owned());
    if !queue.is_writer_queue() {
        return Err(fail("writer queue lost its queue identity"));
    }
    if queue.cancel_in_progress != WRITER_QUEUE_CANCEL_IN_PROGRESS {
        return Err(fail("writer queue kept a cancelling policy"));
    }
    queue.validate_writer_queue()?;
    let retained =
        Concurrency::writer_queue("velnor-cache-writer-${{ github.repository }}".to_owned());
    if retained != queue {
        return Err(fail("writer queue drifted between renders"));
    }
    Ok(())
}

fn check_cleanup_render() -> Result<(), Box<dyn Error>> {
    let rendered = render_cache_cleanup_yml("ubuntu-24.04", "0.1.55")?;
    if rendered.path != CACHE_CLEANUP_PATH {
        return Err(fail("cleanup render left its workflow path"));
    }
    for marker in [
        "permissions: {}",
        "cancel-in-progress: false",
        "actions: write",
        "gh cache delete",
    ] {
        if !rendered.bytes.contains(marker) {
            return Err(fail(&format!(
                "cleanup render lost its authority: {marker}"
            )));
        }
    }
    if rendered.bytes.contains("actions/checkout") {
        return Err(fail("cleanup render kept an application checkout"));
    }
    Ok(())
}

#[test]
fn gc1_t31_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_explicit_read_modes()?;
    check_writer_queue_shape()?;
    check_cleanup_render()?;
    let ir = event_workflow(authority_jobs());
    ir.validate()?;
    ir.validate_event_authority(
        &names(&["reader"]),
        &names(&["writer"]),
        &names(&["cleanup"]),
    )?;
    let again = event_workflow(authority_jobs());
    again.validate()?;
    again.validate_event_authority(
        &names(&["reader"]),
        &names(&["writer"]),
        &names(&["cleanup"]),
    )?;
    Ok(())
}

#[path = "gc1_t31_reject.rs"]
mod gc1_t31_reject;
