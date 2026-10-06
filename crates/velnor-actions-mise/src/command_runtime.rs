//! Compiled runtime-domain binding for isolated Mise commands.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::runtime_paths::{MISE_DATA_DIR_ENV, RuntimePaths};

use super::{EnvPolicy, IsolatedCommand};

impl IsolatedCommand {
    /// Override the working directory for this invocation.
    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// Bind the compiled Mise data root for a generator-owned runtime domain.
    ///
    /// The caller supplies [`RuntimePaths`], never a path string. This is the
    /// only command-level route for the planning root; ordinary declared env
    /// inputs remain subject to the reserved-key checks in [`Self::with_env`].
    #[must_use]
    pub fn with_runtime_paths(mut self, paths: RuntimePaths) -> Self {
        self.extra_env.retain(|(key, _)| key != MISE_DATA_DIR_ENV);
        let (key, value) = paths.mise_data_env();
        self.extra_env
            .push((OsString::from(key), OsString::from(value)));
        self.runtime_paths = Some(paths);
        self
    }

    /// Full child env over a parent snapshot (pure [`Self::run`] contract).
    #[must_use]
    pub fn spawn_env(&self, parent: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
        let mut additions = self.resolved_env(parent).unwrap_or_else(|| self.full_env());
        if super::git::is_discovery_git(self) {
            additions.extend(
                super::git::DISCOVERY_DIAGNOSTIC_ENV
                    .map(|(key, value)| (OsString::from(key), OsString::from(value))),
            );
        }
        let parent: Vec<_> = parent
            .iter()
            .filter(|(key, _)| {
                !super::git::is_discovery_git(self)
                    || (!key.to_string_lossy().starts_with("GIT_")
                        && !super::is_git_loader_key(&key.to_string_lossy()))
            })
            .cloned()
            .collect();
        self.policy.child_env(&parent, &additions)
    }

    /// Resolve typed runtime roots against a concrete parent snapshot.
    pub(super) fn resolved_env(
        &self,
        parent: &[(OsString, OsString)],
    ) -> Option<Vec<(OsString, OsString)>> {
        let Some(paths) = self.runtime_paths else {
            return Some(self.full_env());
        };
        let runner_temp = parent
            .iter()
            .find(|(key, value)| key == "RUNNER_TEMP" && !value.is_empty())
            .map(|(_, value)| std::path::PathBuf::from(value))?;
        let concrete = paths.concrete_mise_data_dir(&runner_temp);
        Some(
            self.full_env()
                .into_iter()
                .map(|(key, value)| {
                    if key == MISE_DATA_DIR_ENV {
                        (key, concrete.as_os_str().to_owned())
                    } else {
                        (key, value)
                    }
                })
                .collect(),
        )
    }

    /// Reassign the typed env policy (crate-internal constructors only).
    ///
    /// The only caller is the pinned-exec constructor, which selects
    /// [`EnvPolicy::Baseline`] for the `gh` baseline-lookup tool; every
    /// other command keeps the policy its own constructor assigned.
    pub(crate) fn with_policy(mut self, policy: EnvPolicy) -> Self {
        self.policy = policy;
        self
    }
}
