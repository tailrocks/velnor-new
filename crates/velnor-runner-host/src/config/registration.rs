// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{RegistrationScope, RunnerGroup, workflow_run::VerifiedRepositoryScope};

use super::RepositoryConnection;
use super::connection::{valid_group_name, valid_repo_part};
use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistrationTarget<'a> {
    Repository { owner: &'a str, repo: &'a str },
    Organization { org: &'a str },
}

impl RegistrationTarget<'_> {
    pub(crate) const fn scope(&self) -> RegistrationScope<'_> {
        match *self {
            Self::Repository { owner, repo } => RegistrationScope::Repository { owner, repo },
            Self::Organization { org } => RegistrationScope::Organization { org },
        }
    }

    pub(crate) fn config_url(&self) -> String {
        match *self {
            Self::Repository { owner, repo } => format!("https://github.com/{owner}/{repo}"),
            Self::Organization { org } => format!("https://github.com/{org}"),
        }
    }
}

impl RepositoryConnection {
    pub fn with_organization_group(
        mut self,
        organization: &str,
        runner_group: Option<&str>,
    ) -> Result<Self, HostError> {
        self.organization = Some(organization.to_owned());
        self.runner_group = runner_group.map(str::to_owned);
        self.validate()?;
        Ok(self)
    }

    pub(crate) fn registration_target(&self) -> Result<RegistrationTarget<'_>, HostError> {
        let (owner, repo) = self.repository.split_once('/').ok_or(HostError::Config)?;
        match self.organization.as_deref() {
            None => Ok(RegistrationTarget::Repository { owner, repo }),
            Some(org) if org == owner => Ok(RegistrationTarget::Organization { org }),
            Some(_) => Err(HostError::Config),
        }
    }

    pub(crate) fn select_runner_group(&self, groups: &[RunnerGroup]) -> Result<i64, HostError> {
        if groups
            .iter()
            .enumerate()
            .any(|(index, group)| groups[..index].iter().any(|prior| prior.id == group.id))
        {
            return Err(HostError::Identity);
        }
        let mut candidates: Vec<&RunnerGroup> = match self.runner_group.as_deref() {
            Some(name) => groups.iter().filter(|group| group.name == name).collect(),
            None => groups.iter().filter(|group| group.is_default).collect(),
        };
        let (Some(group), None) = (candidates.pop(), candidates.pop()) else {
            return Err(HostError::Identity);
        };
        if group.id <= 0 {
            return Err(HostError::Identity);
        }
        Ok(group.id)
    }

    pub(crate) fn matches_scope(&self, scope: &VerifiedRepositoryScope) -> bool {
        self.repository
            .split_once('/')
            .is_some_and(|(owner, repo)| {
                scope.id() == self.repository_id
                    && scope.owner().eq_ignore_ascii_case(owner)
                    && scope.name().eq_ignore_ascii_case(repo)
            })
    }

    pub(super) fn validate_registration(&self) -> bool {
        match (&self.organization, &self.runner_group) {
            (None, None) => true,
            (Some(org), group) => {
                valid_repo_part(org)
                    && self.repository.split_once('/').map(|pair| pair.0) == Some(org.as_str())
                    && group.as_deref().is_none_or(valid_group_name)
            }
            (None, Some(_)) => false,
        }
    }
}

#[cfg(test)]
mod tests;
