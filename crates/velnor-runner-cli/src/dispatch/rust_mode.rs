// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use velnor_runner_host::{
    ConnectionId, HostError, RepositoryConnection, RustRunnerFailure, RustRunnerLauncher,
    detach_rust_runner_connection, load_connection_pat, run_rust_runner_doctor_probe,
    wait_for_rust_runner_connection_admitted, wait_for_rust_runner_connection_drain,
    wait_for_rust_runner_drain,
};

use super::DRAIN_WAIT_BUDGET;
use super::connect::{ConnectError, ConnectRequest};
use super::connection_state::connection_drain_marker;
use super::disconnect::{Detach, disconnect_with_detach};
use super::drain::drain_with;
use super::in_rust_runner_mode;
use super::marker::marker_token;
use velnor_runner_host::CreatedSetDeletion;

const DOCTOR_BUDGET: Duration = Duration::from_secs(240);

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

pub(super) fn drain(state: &Path) -> ExitCode {
    drain_with(state, true, wait_for_rust_runner_drain)
}

pub(super) fn disconnect(state: &Path, connection_id: &str, wait: bool) -> ExitCode {
    let deadline = Instant::now() + DRAIN_WAIT_BUDGET;
    disconnect_with_detach(
        state,
        connection_id,
        wait,
        move |state, connection_id, _| {
            wait_for_rust_runner_connection_drain(state, connection_id, remaining(deadline))
        },
        move |state, binding, _| detach(state, binding, deadline),
        |_, _, _| Ok(None),
    )
}

fn detach(
    state: &Path,
    binding: &RepositoryConnection,
    deadline: Instant,
) -> Result<Detach, HostError> {
    let pat = load_connection_pat(binding, remaining(deadline))?;
    Ok(
        match detach_rust_runner_connection(state, binding, &pat, remaining(deadline))? {
            CreatedSetDeletion::TimedOut => Detach::TimedOut,
            other => Detach::Set(other),
        },
    )
}

pub(super) fn doctor(state: &Path) -> ExitCode {
    let launcher = RustRunnerLauncher::from_environment();
    match run_rust_runner_doctor_probe(state, &launcher, DOCTOR_BUDGET) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("doctor probe failed ({}): {error}", error.code());
            ExitCode::from(1)
        }
    }
}

pub(super) fn reactivate(state: &Path, binding: &RepositoryConnection) -> Result<bool, HostError> {
    if in_rust_runner_mode(state) {
        return Ok(true);
    }
    crate::readiness_probe::reactivate_connection_until(
        state,
        binding,
        Instant::now() + DRAIN_WAIT_BUDGET,
    )
}

pub(super) struct Reconnect {
    connection_id: ConnectionId,
}

impl Reconnect {
    pub(super) fn begin(request: &ConnectRequest<'_>) -> Option<Self> {
        let connection_id = ConnectionId::parse(request.connection_id).ok()?;
        let drained = matches!(
            marker_token(&connection_drain_marker(request.state, &connection_id)),
            Ok(Some(_))
        );
        (in_rust_runner_mode(request.state) && drained).then_some(Self { connection_id })
    }

    pub(super) fn admitted(self, state: &Path) -> Result<(), ConnectError> {
        self.admitted_within(state, DRAIN_WAIT_BUDGET)
    }

    pub(super) fn admitted_within(
        self,
        state: &Path,
        budget: Duration,
    ) -> Result<(), ConnectError> {
        let admitted = wait_for_rust_runner_connection_admitted(state, &self.connection_id, budget)
            .map_err(ConnectError::Lifecycle)?;
        if admitted {
            Ok(())
        } else {
            Err(ConnectError::Lifecycle(HostError::RustRunner(
                RustRunnerFailure::ReconnectNotAdmitted,
            )))
        }
    }
}
