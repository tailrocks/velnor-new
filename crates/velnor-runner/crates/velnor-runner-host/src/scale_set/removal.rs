// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{AdminConnection, Transport, remove_runner};

use super::support::Recording;
use super::{EnsureError, binding_owns_set, list_runner_ids, open_binding_admin};
use crate::config::RepositoryConnection;

/// Deregisters a removed connection's runner on GitHub and says whether it is gone.
pub(crate) trait RunnerProver {
    /// `Ok(true)`: the runner was deregistered and GitHub no longer lists it. `Ok(false)`: the
    /// connection does not own that scale set, so nothing was touched. `Err`: GitHub did not
    /// confirm absence; the caller records no proof.
    fn prove_absent(&mut self, scale_set_id: i64, runner_id: i64) -> Result<bool, EnsureError>;
}

/// Deregisters `runner_id` and re-lists its scale set. The delete result is not trusted on its own
/// (a 404 or a lost reply may still mean it is gone); only a complete listing without the runner
/// counts as proof, and a listing that cannot be read or still names it is `Uncertain`.
pub(crate) fn deregister_and_confirm<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    scale_set_id: i64,
    runner_id: i64,
) -> Result<(), EnsureError> {
    let _ = remove_runner(transport, runner_id, admin_token);
    let listed = list_runner_ids(transport, admin_token, scale_set_id)?;
    if listed.contains(&runner_id.to_string()) {
        return Err(EnsureError::Uncertain);
    }
    Ok(())
}

/// The production prover: one admin session for one removed connection's repository.
pub(crate) struct RemovedRunnerProver {
    binding: RepositoryConnection,
    transport: Recording,
    admin: AdminConnection,
}

impl RemovedRunnerProver {
    pub(crate) fn open(pat: &str, binding: &RepositoryConnection) -> Result<Self, EnsureError> {
        let (transport, admin) = open_binding_admin(pat, binding)?;
        Ok(Self {
            binding: binding.clone(),
            transport,
            admin,
        })
    }
}

impl RunnerProver for RemovedRunnerProver {
    fn prove_absent(&mut self, scale_set_id: i64, runner_id: i64) -> Result<bool, EnsureError> {
        let token = self.admin.expose_token();
        if !binding_owns_set(&mut self.transport, token, &self.binding, scale_set_id)? {
            return Ok(false);
        }
        deregister_and_confirm(&mut self.transport, token, scale_set_id, runner_id).map(|()| true)
    }
}

#[cfg(test)]
#[path = "removal/tests.rs"]
mod tests;
