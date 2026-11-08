// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]

mod model;
pub use model::*;

mod attested_grammar;
mod build_source;
mod candidate;
mod inventory;
mod oci_configuration;
mod oci_grammar;
mod provenance;
mod runner_publication;
mod runner_qualification;
pub mod strict_json;

pub use attested_grammar::{
    MeasuredEnvelopeDigests, attestation_descriptors, attested_index_manifests,
    split_attested_roles, validate_attestation_reference, verify_envelope_graph,
};
pub use build_source::{ExpectedBuildSource, admits_build_source};
pub use candidate::{ImageCandidateArchive, ImageCandidateOci, ImageCandidateWorkflow};
pub use inventory::{
    RUNNER_ABSENCES_PATH, RUNNER_INVENTORY_PATH, parse_runner_publication_source_absences,
    parse_runner_publication_source_inputs, runner_publication_source_absences,
    runner_publication_source_inputs,
};
pub use oci_configuration::{
    OciImageConfiguration, admits_image_configuration, has_image_kind, has_image_revision,
    has_probe_profile, project_oci_configuration,
};
pub use oci_grammar::{
    OciDescriptor, OciGrammarError, is_oci_layer_media_type, parse_oci_descriptor,
    parse_oci_manifest, reject_unknown_oci_fields,
};
pub use provenance::{
    ProvenanceEnvelopeError, UntrustedProvenance, encode_provenance_envelope,
    parse_provenance_envelope, validate_provenance_envelope_limits,
};
pub use runner_publication::{
    RUNNER_IMAGE_FAMILY_ARTIFACT_NAMES, RUNNER_IMAGE_PUBLICATION_SCHEMA, RunnerImageNamedContext,
    RunnerImagePublicationLock, RunnerImagePublicationProvenance, RunnerImagePublicationRecord,
    RunnerImagePublicationSource, parse_runner_image_publication_record,
    validate_runner_image_publication_record,
};
pub use runner_qualification::{
    RUNNER_QUALIFICATION_SCHEMA, RunnerImageQualification, RunnerImageQualificationImage,
    RunnerImageQualificationImages, parse_runner_image_qualification,
    validate_runner_image_qualification,
};

fn is_exact_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !(part.is_empty() || part.len() > 1 && part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Runner,
    Dind,
    ResourceProbe,
}

impl ImageKind {
    #[must_use]
    pub const fn repository(self) -> &'static str {
        match self {
            Self::Runner => "ghcr.io/tailrocks/velnor-runner",
            Self::Dind => "ghcr.io/tailrocks/velnor-dind",
            Self::ResourceProbe => "ghcr.io/tailrocks/velnor-resource-probe",
        }
    }
}
