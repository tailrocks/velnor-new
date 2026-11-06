// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OrchestratorError, internal};

pub(crate) struct ClosedCompletion {
    pub(super) completion: crate::crate_release_controller::FrozenCompletion,
    pub(super) closed: ClosedSourceSession,
}

pub(crate) struct ClosedSourceSession {
    pub(super) deadline: velnor_actions_mise::CheckDeadline,
    pub(super) boot_deadline: u64,
}

impl ClosedSourceSession {
    pub(crate) fn check(&self) -> Result<(), OrchestratorError> {
        let invalid = || internal("crate_release_completion_deadline");
        self.deadline.remaining().map_err(|_| invalid())?;
        let clock = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
        let now = u64::try_from(clock.tv_sec)
            .map_err(|_| invalid())?
            .checked_mul(1_000_000_000)
            .and_then(|seconds| seconds.checked_add(u64::try_from(clock.tv_nsec).ok()?))
            .ok_or_else(invalid)?;
        if now >= self.boot_deadline {
            return Err(invalid());
        }
        Ok(())
    }

    pub(crate) fn complete<T>(
        self,
        operation: impl FnOnce(&Self) -> Result<T, OrchestratorError>,
    ) -> Result<T, OrchestratorError> {
        self.check()?;
        let result = self
            .deadline
            .scope(|| operation(&self))
            .map_err(|_| internal("crate_release_completion_deadline"))?;
        self.check()?;
        result
    }
}

impl ClosedCompletion {
    pub(crate) fn complete<
        J: crate::crate_release_journal::JournalIo
            + crate::crate_release_journal::JournalOperationRefs,
        G: crate::crate_release_github::GitHubAdmissionRead,
        R: crate::crate_release_registry::CrateRegistryRead,
    >(
        self,
        journal: &J,
        github: &G,
        registry: &R,
    ) -> Result<String, OrchestratorError> {
        self.completion
            .record_closed(self.closed, journal, github, registry)
    }
}

#[derive(Clone)]
pub(crate) struct SourceSessionId(pub(super) std::sync::Arc<()>);

impl SourceSessionId {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}
