// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    OrchestratorError, ProjectedSourceTools, SourceSessionId, SupervisedSourceExecution, internal,
};

impl ProjectedSourceTools {
    pub(crate) fn connect(
        self,
        socket: std::os::unix::net::UnixStream,
        boot_deadline: u64,
    ) -> Result<SupervisedSourceExecution, OrchestratorError> {
        #[cfg(not(test))]
        {
            self.connect_inner(socket, boot_deadline)
        }
        #[cfg(test)]
        {
            self.connect_inner(socket, boot_deadline, || Ok(()))
        }
    }

    fn connect_inner(
        mut self,
        socket: std::os::unix::net::UnixStream,
        boot_deadline: u64,
        #[cfg(test)] after_chmod: impl FnOnce() -> Result<(), &'static str>,
    ) -> Result<SupervisedSourceExecution, OrchestratorError> {
        use crate::release::crate_release_source_wire::{SourceOperation, SourceWire};
        let invalid = || internal("crate_release_source_channel_invalid");
        self.sealed.deadline.remaining().map_err(|_| invalid())?;
        self.sealed.tools.revalidate_view(&self.sealed.view)?;
        let initial = self.context(false)?;
        let bound = self.context(true)?;
        let root = self.sealed.view.path().join("source");
        #[cfg(not(test))]
        self.sealed.view.expose().map_err(|_| invalid())?;
        #[cfg(test)]
        self.sealed
            .view
            .expose_with_hook(after_chmod)
            .map_err(|_| invalid())?;
        let mut wire = SourceWire::new(socket, boot_deadline).map_err(|_| invalid())?;
        wire.request(SourceOperation::Init, Some(&initial))
            .map_err(|_| invalid())?;
        wire.request(SourceOperation::Metadata, None)
            .map_err(|_| invalid())?;
        wire.request(SourceOperation::Bind, Some(&bound))
            .map_err(|_| invalid())?;
        Ok(SupervisedSourceExecution {
            session: SourceSessionId(std::sync::Arc::new(())),
            boot_deadline,
            projected: self,
            wire: std::sync::Mutex::new(wire),
            root,
        })
    }

    #[cfg(test)]
    pub(crate) fn connect_with_exposure_hook(
        self,
        socket: std::os::unix::net::UnixStream,
        boot_deadline: u64,
        after_chmod: impl FnOnce() -> Result<(), &'static str>,
    ) -> Result<SupervisedSourceExecution, OrchestratorError> {
        self.connect_inner(socket, boot_deadline, after_chmod)
    }

    pub(crate) fn context(&self, bound: bool) -> Result<Vec<u8>, OrchestratorError> {
        let invalid = || internal("crate_release_source_context_invalid");
        let receipt = self.sealed.source.receipt().data();
        let authority = self.sealed.source.receipt().authority();
        let request = &authority.candidate().request;
        let intent = velnor_actions_contract::canonical_json_bytes(&receipt.intent)
            .map_err(|_| invalid())?;
        let packages = receipt
            .packages
            .iter()
            .map(|package| {
                let features = receipt
                    .intent
                    .publish_features
                    .get(&package.source.name)
                    .ok_or_else(invalid)?;
                Ok(serde_json::json!({
                    "name": package.source.name,
                    "version": if bound { Some(&package.source.version) } else { None },
                    "manifest": if bound { Some(&package.source.manifest) } else { None },
                    "features": features,
                "upload_config_sha256": crate::planner::generator::sha256_hex(
                    crate::crate_release_controller::single_package_config(&receipt.intent, &package.source.name)?.as_bytes()
                ),
                }))
            })
            .collect::<Result<Vec<_>, OrchestratorError>>()?;
        if packages.is_empty() {
            return Err(invalid());
        }
        serde_json::to_vec(&serde_json::json!({
            "identity": {
                "repository": request.repository,
                "repository_id": authority.repository_id(),
                "owner_id": authority.owner_id(),
                "run_id": request.run_id,
                "run_attempt": request.run_attempt,
                "workflow_sha": request.workflow_sha,
                "intent_sha256": crate::planner::generator::sha256_hex(&intent),
            },
            "source_sha": request.source_sha,
            "authentication": receipt.intent.authentication,
            "rust_version": receipt.intent.rust_version,
            "manifest": receipt.intent.manifest_path,
            "source_files": self.sealed.source.source().files(),
            "packages": packages,
            "tools": {
                "cargo": self.sealed.tools.cargo_sha256(),
                "rustc": self.sealed.tools.rustc_sha256(),
                "rustdoc": self.sealed.tools.rustdoc_sha256(),
                "semver": self.semver_sha256,
                "release_plz": self.release_plz_sha256,
                "git": self.git_sha256,
                "git_http": self.git_http_sha256,
                "cc": self.compiler_sha256,
            },
        }))
        .map_err(|_| invalid())
    }
}
