// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "image_candidate/closure.rs"]
mod image_candidate_closure;
#[path = "image_candidate/record.rs"]
mod image_candidate_record;
#[path = "image_candidate/runner_publication.rs"]
mod runner_publication;

pub use image_candidate_closure::{
    IMAGE_NATIVE_CLOSURE_SCHEMA, ImageCandidateSourceClosure, NATIVE_IMAGE_CANDIDATE_INPUTS,
    parse_image_candidate_source_closure, serialize_image_candidate_source_closure,
    validate_image_candidate_source_closure,
};
pub use image_candidate_record::{
    IMAGE_CANDIDATE_MAX_RECORD_BYTES, IMAGE_CANDIDATE_SCHEMA, IMAGE_CANDIDATE_SOURCE_INPUTS,
    ImageCandidateArchive, ImageCandidateBuild, ImageCandidateOci, ImageCandidateRecord,
    ImageCandidateSource, ImageCandidateWorkflow, parse_image_candidate_record,
    serialize_image_candidate_record, validate_image_candidate_record,
};
pub use runner_publication::{
    RUNNER_IMAGE_PUBLICATION_SCHEMA, RUNNER_PUBLICATION_SOURCE_INPUTS, RunnerImagePublicationLock,
    RunnerImagePublicationQualification, RunnerImagePublicationRecord,
    RunnerImagePublicationReferencedWorkflow, RunnerImagePublicationSource,
    parse_runner_image_publication_record, validate_runner_image_publication_record,
};
