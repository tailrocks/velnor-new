// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) mod execution_publication;
pub(super) mod lease;
mod newrole;
pub(super) mod operations;
pub(crate) use operations::capacity_request_sha256;
mod phase;
pub(crate) use execution_publication::{
    ExecutionPublicationContext, ExecutionPublicationParent, ExecutionPublicationRecoveryRecord,
    ExecutionPublicationSeed, PublicationKind,
};
pub(crate) use lease::ProbeLeaseState;
pub(crate) use newrole::{HostHelperRole, ResourceProbePurpose};
pub(crate) use phase::ProbePhase;

#[derive(Debug, Clone)]
pub(crate) struct ProbeSeed {
    pub(crate) operation_id: String,

    pub(crate) instance_id: String,

    pub(crate) engine_id: String,

    pub(crate) docker_root_digest: String,

    pub(crate) source_revision: String,

    pub(crate) runtime_image_id: String,

    pub(crate) image_binding_digest: String,

    pub(crate) operation_name: String,

    pub(crate) projection_digest: String,

    pub(crate) purpose: ResourceProbePurpose,

    pub(crate) request_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedCleanupObserverProbe {
    request: velnor_resource_probe::cleanup_observer_wire::CleanupObserverRequest,
    request_json: String,
    probe: ProbeRow,
}

impl PreparedCleanupObserverProbe {
    pub(crate) fn request(
        &self,
    ) -> &velnor_resource_probe::cleanup_observer_wire::CleanupObserverRequest {
        &self.request
    }

    pub(crate) fn request_json(&self) -> &str {
        &self.request_json
    }

    pub(crate) fn probe(&self) -> &ProbeRow {
        &self.probe
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeRow {
    pub(crate) operation_id: String,

    pub(crate) instance_id: String,

    pub(crate) engine_id: String,

    pub(crate) docker_root_digest: String,

    pub(crate) source_revision: String,

    pub(crate) runtime_image_id: String,

    pub(crate) image_binding_digest: String,

    pub(crate) operation_name: String,

    pub(crate) role: String,

    pub(crate) helper_role: HostHelperRole,

    pub(crate) work: ProbeWork,

    pub(crate) projection_digest: String,

    pub(crate) phase: ProbePhase,

    pub(crate) container_id: Option<String>,

    pub(crate) lease_state: ProbeLeaseState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeWork {
    ResourceProbe {
        purpose: ResourceProbePurpose,
        request_sha256: String,
        request_context: Option<String>,
    },

    ExecutionPublication,
}

impl ProbeRow {
    pub(crate) const fn helper_role(&self) -> HostHelperRole {
        self.helper_role
    }

    pub(crate) fn work(&self) -> &ProbeWork {
        &self.work
    }
}
