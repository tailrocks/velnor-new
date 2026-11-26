// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    ScaleSetByName, ScaleSetDelete, ScaleSetDeleteStatus, ScaleSetFound, delete_runner_scale_set,
    get_runner_scale_set, list_runner_groups, list_scale_set_runners, remove_runner,
};

use super::census::wait_until_unserved;
use crate::config::RepositoryConnection;
use crate::error::{HostError, RustRunnerFailure};
use crate::host_controller::connection_pat;
use crate::launch_blocking::ListenFault;
use crate::listen::map_listen;
use crate::scale_set::{EnsureError, map_session, open_binding_admin};

pub fn detach_rust_runner_connection(
    state: &Path,
    binding: &RepositoryConnection,
    timeout: Duration,
) -> Result<bool, HostError> {
    let deadline = Instant::now() + timeout;
    if !wait_until_unserved(state, binding, timeout) {
        return Ok(false);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| HostError::Lock)?;
    let load_deadline =
        tokio::time::Instant::now() + deadline.saturating_duration_since(Instant::now());
    let secret = runtime.block_on(binding.credential_ref.load_until(load_deadline))?;
    let pat = connection_pat(&secret)?;
    delete_scale_set(pat, binding).map_err(|fault| match fault {
        ListenFault::Host(host) => host,
        ListenFault::Ensure(_) => HostError::RustRunner(RustRunnerFailure::GitHubRefused),
    })?;
    Ok(true)
}

fn delete_scale_set(pat: &str, binding: &RepositoryConnection) -> Result<(), ListenFault> {
    let (mut transport, connection) = open_binding_admin(pat, binding)?;
    let token = connection.expose_token().to_owned();
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
    let ScaleSetFound::Found(view) = found else {
        return Ok(());
    };
    let registered = list_scale_set_runners(&mut transport, view.id, &token).map_err(map_listen)?;
    let mut refused = false;
    for runner in &registered {
        refused |= remove_runner(&mut transport, runner.id, &token).is_err();
    }
    if refused {
        return Err(HostError::RustRunner(RustRunnerFailure::RunnerRemovalRefused).into());
    }
    let call = ScaleSetDelete {
        scale_set_id: view.id,
        admin_token: &token,
    };
    match delete_runner_scale_set(&mut transport, &call) {
        Ok(ScaleSetDeleteStatus::Deleted | ScaleSetDeleteStatus::NotFound) => Ok(()),
        Ok(ScaleSetDeleteStatus::Refused(_)) => {
            Err(HostError::RustRunner(RustRunnerFailure::ScaleSetDeleteRefused).into())
        }
        Err(_) => Err(HostError::RustRunner(RustRunnerFailure::ScaleSetDeleteUncertain).into()),
    }
}
