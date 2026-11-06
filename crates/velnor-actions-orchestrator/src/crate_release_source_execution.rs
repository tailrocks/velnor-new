// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::tools::ProjectedSourceTools;
use super::{OrchestratorError, Path, PathBuf, ReleaseGit, ReleaseGitExecutable, internal};

pub(crate) struct SupervisedSourceExecution {
    projected: ProjectedSourceTools,
    wire: std::sync::Mutex<crate::release::crate_release_source_wire::SourceWire>,
    root: PathBuf,
    session: SourceSessionId,
    boot_deadline: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct SupervisedUpload<'a> {
    pub(crate) root: &'a Path,
    pub(crate) intent: &'a velnor_actions_contract::config::CrateReleaseIntent,
    pub(crate) git: &'a ReleaseGit,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) sequence: u32,
    pub(crate) release_plz_sha256: &'a str,
}

impl SupervisedSourceExecution {
    pub(crate) fn session_identity(&self) -> SourceSessionId {
        self.session.clone()
    }
    pub(crate) fn upload(&self, request: &SupervisedUpload<'_>) -> Result<(), OrchestratorError> {
        let SupervisedUpload {
            root,
            intent,
            git,
            package,
            version,
            sequence,
            release_plz_sha256,
        } = *request;
        let invalid = || internal("crate_release_upload_unverified");
        let mut wire = self.wire.lock().map_err(|_| invalid())?;
        if self.validate(root, intent, git).is_err()
            || release_plz_sha256 != self.projected.release_plz_sha256
        {
            wire.poison();
            return Err(invalid());
        }
        let receipt = self.projected.sealed.source.receipt().data();
        let index = receipt
            .packages
            .iter()
            .position(|item| item.source.name == package && item.source.version == version)
            .ok_or_else(invalid)?;
        let features = intent.publish_features.get(package).ok_or_else(invalid)?;
        let config = crate::crate_release_controller::single_package_config(intent, package)?;
        let intent_bytes =
            velnor_actions_contract::canonical_json_bytes(intent).map_err(|_| invalid())?;
        let context = serde_json::to_vec(&serde_json::json!({
            "source_sha": receipt.github.request.source_sha,
            "intent_sha256": crate::planner::generator::sha256_hex(&intent_bytes),
            "package": package,
            "version": version,
            "features": features,
            "config": config,
            "release_plz_sha256": release_plz_sha256,
            "sequence": sequence,
        }))
        .map_err(|_| invalid())?;
        wire.request(
            crate::release::crate_release_source_wire::SourceOperation::Upload { index },
            Some(&context),
        )
        .map_err(|_| invalid())?;
        if self.validate(root, intent, git).is_err() || wire.remaining().is_err() {
            wire.poison();
            return Err(invalid());
        }
        Ok(())
    }

    pub(crate) fn credential_preflight(
        &self,
        root: &Path,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        git: &ReleaseGit,
    ) -> Result<(), OrchestratorError> {
        let invalid = || internal("crate_release_upload_credential_missing");
        let mut wire = self.wire.lock().map_err(|_| invalid())?;
        if self.validate(root, intent, git).is_err() {
            wire.poison();
            return Err(invalid());
        }
        wire.request(
            crate::release::crate_release_source_wire::SourceOperation::CredentialPreflight,
            None,
        )
        .map_err(|_| invalid())?;
        if self.validate(root, intent, git).is_err() || wire.remaining().is_err() {
            wire.poison();
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(target_arch = "x86_64")]
    pub(crate) fn finish(
        self,
        completion: crate::crate_release_controller::FrozenCompletion,
    ) -> Result<ClosedCompletion, OrchestratorError> {
        let invalid = || internal("crate_release_source_channel_invalid");
        self.projected
            .sealed
            .deadline
            .remaining()
            .map_err(|_| invalid())?;
        if !completion.is_bound_to(&self.session)
            || completion.admission() != self.projected.sealed.source.receipt().data()
        {
            return Err(invalid());
        }
        let mut wire = self.wire.into_inner().map_err(|_| invalid())?;
        if self.projected.sealed.revalidate_source().is_err() {
            wire.poison();
            return Err(invalid());
        }
        wire.request(
            crate::release::crate_release_source_wire::SourceOperation::Close,
            None,
        )
        .map_err(|_| invalid())?;
        if self.projected.sealed.revalidate_source().is_err() || wire.remaining().is_err() {
            wire.poison();
            return Err(invalid());
        }
        Ok(ClosedCompletion {
            completion,
            closed: ClosedSourceSession {
                deadline: self.projected.sealed.deadline,
                boot_deadline: self.boot_deadline,
            },
        })
    }

    fn validate(
        &self,
        root: &Path,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        git: &ReleaseGit,
    ) -> Result<(), OrchestratorError> {
        let invalid = || internal("crate_release_source_context_invalid");
        self.projected
            .sealed
            .deadline
            .remaining()
            .map_err(|_| invalid())?;
        self.projected.sealed.revalidate_source()?;
        if root != self.root
            || intent != &self.projected.sealed.source.receipt().data().intent
            || git.verified().is_none()
            || git.executable() != self.projected.sealed.git.executable()
        {
            return Err(invalid());
        }
        git.revalidate().map_err(|_| invalid())?;
        Ok(())
    }
}

impl crate::release::crate_release_workspace::HistoricalIntentReader<'_> {
    pub(crate) fn verify_prior(
        &self,
        prior: &crate::crate_release_journal::JournalReceipt,
        request: &crate::crate_release_github::GitHubAdmissionRequest,
        github: &dyn crate::crate_release_github::GitHubAdmissionRead,
    ) -> Result<velnor_actions_contract::config::CrateReleaseIntent, OrchestratorError> {
        let invalid = || internal("crate_release_history_custody_required");
        let execution = self.execution;
        let receipt = execution.projected.sealed.source.receipt().data();
        let mut wire = execution.wire.lock().map_err(|_| invalid())?;
        if execution
            .validate(
                &execution.root,
                &receipt.intent,
                &execution.projected.sealed.git,
            )
            .is_err()
            || wire.remaining().is_err()
            || request != &receipt.github.request
        {
            wire.poison();
            return Err(invalid());
        }
        let deadline = velnor_actions_mise::CheckDeadline::after(
            wire.remaining_duration().map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
        let checked = DeadlineGithub {
            github,
            wire: &wire,
        };
        let result = crate::release::crate_release_historical_intent::verify_prior_bound(
            &execution.root,
            prior,
            request,
            &checked,
            &execution.projected.sealed.git,
            deadline,
            &wire,
        );
        if execution
            .validate(
                &execution.root,
                &receipt.intent,
                &execution.projected.sealed.git,
            )
            .is_err()
            || wire.remaining().is_err()
            || result.is_err()
        {
            wire.poison();
            return Err(invalid());
        }
        result
    }

    pub(crate) fn verify_transition(
        &self,
        prior: &crate::crate_release_journal::JournalReceipt,
        current: &crate::crate_release_journal::JournalReceipt,
        request: &crate::crate_release_github::GitHubAdmissionRequest,
        github: &dyn crate::crate_release_github::GitHubAdmissionRead,
    ) -> Result<(), OrchestratorError> {
        let invalid = || internal("crate_release_history_custody_required");
        let execution = self.execution;
        let receipt = execution.projected.sealed.source.receipt().data();
        let mut wire = execution.wire.lock().map_err(|_| invalid())?;
        if execution
            .validate(
                &execution.root,
                &receipt.intent,
                &execution.projected.sealed.git,
            )
            .is_err()
            || wire.remaining().is_err()
            || request != &receipt.github.request
        {
            wire.poison();
            return Err(invalid());
        }
        let deadline = velnor_actions_mise::CheckDeadline::after(
            wire.remaining_duration().map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
        let checked = DeadlineGithub {
            github,
            wire: &wire,
        };
        let result = crate::release::crate_release_historical_intent::verify_runner_transition(
            &execution.root,
            crate::release::crate_release_historical_intent::TransitionReceipts {
                prior,
                current,
                current_intent: &receipt.intent,
            },
            request,
            &checked,
            &execution.projected.sealed.git,
            deadline,
            &wire,
        );
        if result.is_err()
            || execution
                .validate(
                    &execution.root,
                    &receipt.intent,
                    &execution.projected.sealed.git,
                )
                .is_err()
            || wire.remaining().is_err()
        {
            wire.poison();
            return Err(invalid());
        }
        Ok(())
    }
}

struct DeadlineGithub<'a> {
    github: &'a dyn crate::crate_release_github::GitHubAdmissionRead,
    wire: &'a crate::release::crate_release_source_wire::SourceWire,
}

impl crate::crate_release_github::GitHubAdmissionRead for DeadlineGithub<'_> {
    fn get(&self, route: &str) -> Result<serde_json::Value, OrchestratorError> {
        let invalid = || internal("crate_release_history_deadline");
        self.wire.remaining().map_err(|_| invalid())?;
        let value = self.github.get(route)?;
        self.wire.remaining().map_err(|_| invalid())?;
        Ok(value)
    }

    fn get_artifact_page(
        &self,
        route: &str,
        limit: u64,
    ) -> Result<(serde_json::Value, u64), OrchestratorError> {
        let invalid = || internal("crate_release_history_deadline");
        self.wire.remaining().map_err(|_| invalid())?;
        let value = self.github.get_artifact_page(route, limit)?;
        self.wire.remaining().map_err(|_| invalid())?;
        Ok(value)
    }

    fn get_optional(&self, route: &str) -> Result<Option<serde_json::Value>, OrchestratorError> {
        let invalid = || internal("crate_release_history_deadline");
        self.wire.remaining().map_err(|_| invalid())?;
        let value = self.github.get_optional(route)?;
        self.wire.remaining().map_err(|_| invalid())?;
        Ok(value)
    }
}

#[path = "crate_release_source_adapter.rs"]
mod adapter;
#[path = "crate_release_source_completion.rs"]
mod completion;
#[path = "crate_release_source_connect.rs"]
mod connect;
pub(crate) use completion::{ClosedCompletion, ClosedSourceSession, SourceSessionId};
