// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;

use velnor_actions_contract::workflow::ir::WRITER_QUEUE_CANCEL_IN_PROGRESS;
use velnor_actions_contract::{
    CacheAccess, CacheWriter, Concurrency, Job, JobTimeout, PermissionLevel, Permissions, Step,
    StepKind, Trigger, WorkflowIr,
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
    cache_access: CacheAccess,
    permissions: Option<Permissions>,
    steps: Vec<Step>,
) -> Job {
    Job {
        display_name: display.to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions,
        cache_access,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: velnor_actions_contract::JobEnv::Unset,
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
        attest: PermissionLevel::None,
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
            job_with("Reader", CacheAccess::Read, None, vec![shell_step("Check")]),
        ),
        (
            "build".to_owned(),
            job_with("Build", CacheAccess::Read, None, vec![shell_step("Build")]),
        ),
        (
            "cleanup".to_owned(),
            job_with(
                "Cleanup",
                CacheAccess::Read,
                Some(cleanup_permissions()),
                vec![shell_step("Delete")],
            ),
        ),
    ])
}

fn names<'a>(values: &'a [&'a str]) -> BTreeSet<&'a str> {
    values.iter().copied().collect()
}

fn check_access_modes() -> Result<(), Box<dyn Error>> {
    if CacheAccess::Read.mode() != "read" || !CacheAccess::Read.is_read() {
        return Err(fail("read access lost its read mode"));
    }
    for writer in [
        CacheWriter::ToolsProducer,
        CacheWriter::TofuProvidersProducer,
        CacheWriter::SourcesProducer,
    ] {
        let access = CacheAccess::Writer(writer);
        if access.mode() != "write" || access.is_read() || access.writer() != Some(writer) {
            return Err(fail("trusted writer lost its write capability"));
        }
        let gate = writer.condition();
        if !writer.admits("writer", Some(&gate)) || writer.admits("writer", None) {
            return Err(fail("writer gate lost its boundary"));
        }
        if writer.admits("writer", Some("github.event_name == 'push'")) {
            return Err(fail("writer gate admitted a bare push"));
        }
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
    let rendered = render_cache_cleanup_yml("ubuntu-26.04", "0.1.55")?;
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
    check_access_modes()?;
    check_writer_queue_shape()?;
    check_cleanup_render()?;
    let ir = event_workflow(authority_jobs());
    ir.validate()?;
    ir.validate_event_authority(
        &names(&["reader", "build"]),
        &names(&[]),
        &names(&["cleanup"]),
    )?;
    let again = event_workflow(authority_jobs());
    again.validate()?;
    again.validate_event_authority(
        &names(&["reader", "build"]),
        &names(&[]),
        &names(&["cleanup"]),
    )?;
    Ok(())
}

#[path = "gc1_t31_reject.rs"]
mod gc1_t31_reject;
