// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    ScaleSetByName, ScaleSetDelete, ScaleSetDeleteStatus, ScaleSetFound, Transport,
    delete_runner_scale_set, get_runner_scale_set, list_runner_groups, list_scale_set_runners,
    remove_runner,
};

use super::census::wait_until_unserved;
use super::controller::owned::{self, Owned, Scope, Transfer};
use crate::config::{ConnectionId, RepositoryConnection};
use crate::controller_ipc::CreatedSetDeletion;
use crate::daemon_lock::DaemonLock;
use crate::error::{HostError, RustRunnerFailure};
use crate::journal::ScaleSetDeleteTarget;
use crate::launch_blocking::ListenFault;
use crate::listen::map_listen;
use crate::scale_set::delete::{Observation, absence, observe};
use crate::scale_set::{EnsureError, Recording, map_session, open_binding_admin_until};

const LOCK_RETRY: Duration = Duration::from_millis(50);

pub fn detach_rust_runner_connection(
    state: &Path,
    binding: &RepositoryConnection,
    pat: &str,
    timeout: Duration,
) -> Result<CreatedSetDeletion, HostError> {
    let deadline = Instant::now() + timeout;
    if !wait_until_unserved(state, binding, timeout) {
        return Ok(CreatedSetDeletion::TimedOut);
    }
    let Some(_config_lock) = lock_config(state, deadline) else {
        return Ok(CreatedSetDeletion::TimedOut);
    };
    if let Some(claimant) = claimant(state, binding)? {
        if claimant != binding.connection_id
            && owned::transfer(state, &binding.connection_id, &claimant)? == Transfer::Conflict
        {
            eprintln!(
                "rust-runner disconnect held: the connection that replaced {} already holds another ownership record",
                binding.connection_id.as_str()
            );
            return Ok(CreatedSetDeletion::Uncertain);
        }
        return Ok(CreatedSetDeletion::NotOwned);
    }
    match remove_scale_set(state, binding, pat, deadline) {
        Ok(deletion) => Ok(deletion),
        Err(_) if Instant::now() >= deadline => Ok(CreatedSetDeletion::TimedOut),
        Err(ListenFault::Host(host)) => Err(host),
        Err(ListenFault::Ensure(_)) => Err(HostError::RustRunner(RustRunnerFailure::GitHubRefused)),
    }
}

fn lock_config(state: &Path, deadline: Instant) -> Option<DaemonLock> {
    loop {
        if let Ok(lock) = DaemonLock::try_acquire(&state.join("config.lock")) {
            return Some(lock);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(LOCK_RETRY);
    }
}

fn claimant(
    state: &Path,
    binding: &RepositoryConnection,
) -> Result<Option<ConnectionId>, HostError> {
    let (config, _) = crate::readiness_scan::controller_config(state).ok_or(HostError::Config)?;
    let scope = Scope::of(binding);
    Ok(config
        .connections()
        .iter()
        .find(|other| {
            other.connection_id == binding.connection_id
                || (other.scale_set.name() == binding.scale_set.name() && Scope::of(other) == scope)
        })
        .map(|other| other.connection_id.clone()))
}

fn remove_scale_set(
    state: &Path,
    binding: &RepositoryConnection,
    pat: &str,
    deadline: Instant,
) -> Result<CreatedSetDeletion, ListenFault> {
    let (mut transport, connection) = open_binding_admin_until(pat, binding, Some(deadline))?;
    let token = connection.expose_token().to_owned();
    let Some(record) = owned::read(state, &binding.connection_id)? else {
        let groups = list_runner_groups(&mut transport, &token)
            .map_err(|error| map_session(error, &transport))?;
        let group = binding
            .select_runner_group(&groups)
            .map_err(|_| EnsureError::Rejected)?;
        let found = get_runner_scale_set(
            &mut transport,
            &ScaleSetByName {
                runner_group_id: group,
                name: binding.scale_set.name(),
                expected_labels: binding.scale_set.labels(),
                admin_token: &token,
            },
        )
        .map_err(|error| map_session(error, &transport))?;
        return Ok(match found {
            ScaleSetFound::Found(_) => CreatedSetDeletion::NotOwned,
            ScaleSetFound::NotFound => CreatedSetDeletion::Deleted,
        });
    };
    if record.scope != Scope::of(binding) {
        return Ok(CreatedSetDeletion::NotOwned);
    }
    if record.server != connection.expose_url() {
        eprintln!(
            "rust-runner disconnect held: the ownership record names another Actions service"
        );
        return Ok(CreatedSetDeletion::Uncertain);
    }
    let Some(target) = target_of(&mut transport, &token, binding, &record)? else {
        owned::forget(state, &binding.connection_id)?;
        return Ok(CreatedSetDeletion::Deleted);
    };
    let deletion = match observe(&mut transport, &token, &target) {
        Observation::Absent(_) => CreatedSetDeletion::Deleted,
        Observation::Unknown => CreatedSetDeletion::Uncertain,
        Observation::Present => delete_owned(&mut transport, &token, &target)?,
    };
    if deletion == CreatedSetDeletion::Deleted {
        owned::forget(state, &binding.connection_id)?;
    }
    Ok(deletion)
}

fn target_of(
    transport: &mut Recording,
    token: &str,
    binding: &RepositoryConnection,
    record: &Owned,
) -> Result<Option<ScaleSetDeleteTarget>, ListenFault> {
    let scale_set_id = if let Some(id) = record.scale_set_id {
        id
    } else {
        let by_name = ScaleSetByName {
            runner_group_id: record.runner_group_id,
            name: binding.scale_set.name(),
            expected_labels: binding.scale_set.labels(),
            admin_token: token,
        };
        match get_runner_scale_set(transport, &by_name)
            .map_err(|error| map_session(error, transport))?
        {
            ScaleSetFound::Found(view) => view.id,
            ScaleSetFound::NotFound => return Ok(None),
        }
    };
    Ok(Some(ScaleSetDeleteTarget {
        scale_set_id,
        name: binding.scale_set.name().to_owned(),
        labels: binding.scale_set.labels().to_vec(),
        runner_group_id: record.runner_group_id,
        state: None,
        unverified: false,
    }))
}

fn delete_owned<T: Transport + ?Sized>(
    transport: &mut T,
    token: &str,
    target: &ScaleSetDeleteTarget,
) -> Result<CreatedSetDeletion, ListenFault> {
    let registered =
        list_scale_set_runners(transport, target.scale_set_id, token).map_err(map_listen)?;
    let mut refused = false;
    for runner in &registered {
        refused |= remove_runner(transport, runner.id, token).is_err();
    }
    if refused {
        return Err(HostError::RustRunner(RustRunnerFailure::RunnerRemovalRefused).into());
    }
    let call = ScaleSetDelete {
        scale_set_id: target.scale_set_id,
        admin_token: token,
    };
    Ok(match delete_runner_scale_set(transport, &call) {
        Ok(ScaleSetDeleteStatus::Deleted) => CreatedSetDeletion::Deleted,
        Ok(ScaleSetDeleteStatus::NotFound) if absence(transport, token, target).is_some() => {
            CreatedSetDeletion::Deleted
        }
        Ok(ScaleSetDeleteStatus::Refused(status)) => CreatedSetDeletion::Refused(status),
        Ok(ScaleSetDeleteStatus::NotFound) | Err(_) => CreatedSetDeletion::Uncertain,
    })
}
