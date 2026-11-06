// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::baseline::stage_baseline;
use super::{OrchestratorError, Path, ReleaseGit, SupervisedSourceExecution, internal};

impl crate::CrateSourceExecution for SupervisedSourceExecution {
    fn historical_intent_reader(
        &self,
    ) -> Result<crate::crate_release_workspace::HistoricalIntentReader<'_>, OrchestratorError> {
        let invalid = || internal("crate_release_history_custody_required");
        let receipt = self.projected.sealed.source.receipt().data();
        let mut wire = self.wire.lock().map_err(|_| invalid())?;
        if self
            .validate(&self.root, &receipt.intent, &self.projected.sealed.git)
            .is_err()
            || wire.remaining().is_err()
        {
            wire.poison();
            return Err(invalid());
        }
        Ok(crate::crate_release_workspace::HistoricalIntentReader { execution: self })
    }

    fn verify_toolchain_separation(
        &self,
        root: &Path,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        git: &ReleaseGit,
    ) -> Result<(), OrchestratorError> {
        let invalid = || internal("crate_release_source_context_invalid");
        let mut wire = self.wire.lock().map_err(|_| invalid())?;
        if self.validate(root, intent, git).is_err()
            || self
                .projected
                .sealed
                .tools
                .verify_source_separation(&self.projected.sealed.view)
                .is_err()
            || wire.remaining().is_err()
        {
            wire.poison();
            return Err(invalid());
        }
        Ok(())
    }

    fn metadata(
        &self,
        root: &Path,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        git: &ReleaseGit,
    ) -> Result<String, OrchestratorError> {
        use crate::crate_release_source_wire::SourceOperation;
        let invalid = || internal("crate_release_source_channel_invalid");
        let mut wire = self.wire.lock().map_err(|_| invalid())?;
        if self.validate(root, intent, git).is_err() {
            wire.poison();
            return Err(invalid());
        }
        let bytes = wire
            .request(SourceOperation::Metadata, None)
            .map_err(|_| invalid())?;
        if self.validate(root, intent, git).is_err() || wire.remaining().is_err() {
            wire.poison();
            return Err(invalid());
        }
        if let Ok(value) = String::from_utf8(bytes) {
            Ok(value)
        } else {
            wire.poison();
            Err(invalid())
        }
    }

    fn package(
        &self,
        request: &crate::CratePackageSource<'_>,
    ) -> Result<Vec<u8>, OrchestratorError> {
        use crate::crate_release_source_wire::SourceOperation;
        let invalid = || internal("crate_release_source_channel_invalid");
        let mut wire = self.wire.lock().map_err(|_| invalid())?;
        if self
            .validate(request.source, request.intent, request.git)
            .is_err()
        {
            wire.poison();
            return Err(invalid());
        }
        let receipt = self.projected.sealed.source.receipt().data();
        if request.expected_sha != receipt.github.request.source_sha {
            return Err(invalid());
        }
        let index = receipt
            .packages
            .iter()
            .position(|package| package.source == *request.package)
            .ok_or_else(invalid)?;
        let bytes = wire
            .request(
                SourceOperation::Package {
                    index,
                    verify: request.verify,
                },
                None,
            )
            .map_err(|_| invalid())?;
        if self
            .validate(request.source, request.intent, request.git)
            .is_err()
            || wire.remaining().is_err()
        {
            wire.poison();
            return Err(invalid());
        }
        Ok(bytes)
    }

    fn check_semver(
        &self,
        request: &crate::CrateSemverSource<'_>,
    ) -> Result<(), OrchestratorError> {
        use crate::crate_release_source_wire::SourceOperation;
        let invalid = || internal("crate_release_source_semver_invalid");
        let receipt = self.projected.sealed.source.receipt().data();
        let mut wire = self.wire.lock().map_err(|_| invalid())?;
        if self
            .validate(request.source, &receipt.intent, request.git)
            .is_err()
        {
            wire.poison();
            return Err(invalid());
        }
        if request.rust != receipt.intent.rust_version
            || request.executable != self.projected.checker_original
        {
            return Err(invalid());
        }
        let index = receipt
            .packages
            .iter()
            .position(|package| {
                package.source.name == request.package
                    && package.source.manifest == request.manifest
            })
            .ok_or_else(invalid)?;
        let selected = &receipt.packages[index].source;
        if receipt
            .intent
            .publish_features
            .get(request.package)
            .map(Vec::as_slice)
            != Some(request.features)
        {
            return Err(invalid());
        }
        let input = crate::crate_release_semver::ProjectionInput {
            archive: request.baseline_archive,
            checksum: request.baseline_checksum,
            package: request.package,
            baseline_version: request.baseline_version,
            source_sha: &receipt.github.request.source_sha,
            target_version: &selected.version,
            manifest: request.manifest,
            features: request.features,
        };
        let (context, files) = crate::crate_release_semver::supervised_projection(&input)?;
        let encoded = serde_json::to_vec(&context).map_err(|_| invalid())?;
        stage_baseline(
            self.projected.sealed.view.path(),
            request.baseline_checksum,
            &files,
            self.projected.sealed.deadline,
        )?;
        wire.request(SourceOperation::Semver { index }, Some(&encoded))
            .map_err(|_| invalid())?;
        if self
            .validate(request.source, &receipt.intent, request.git)
            .is_err()
            || wire.remaining().is_err()
        {
            wire.poison();
            return Err(invalid());
        }
        Ok(())
    }
}
