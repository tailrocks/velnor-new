// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "image_candidate/task.rs"]
mod task;
pub use task::*;
#[path = "image_candidate/operation.rs"]
mod operation;
pub use operation::{JobImageCandidateInputs, job_image_candidate_source_script};
pub(crate) use operation::{
    is_job_image_candidate_identity, validate_job_image_candidate_operation,
};
#[path = "image_candidate/native_task.rs"]
mod native_task;
pub use native_task::NativeTaskKind;

#[path = "image_candidate/closure.rs"]
mod image_candidate_closure;
#[path = "image_candidate/record.rs"]
mod image_candidate_record;
#[path = "image_candidate/runner_publication.rs"]
mod runner_publication;
#[path = "image_candidate/runner_publication_context.rs"]
mod runner_publication_context;
#[path = "image_candidate/runner_qualification.rs"]
mod runner_qualification;

pub use image_candidate_closure::{
    IMAGE_NATIVE_CLOSURE_SCHEMA, ImageCandidateSourceClosure, native_image_candidate_inputs,
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
    RunnerImagePublicationRecord, RunnerImagePublicationSource,
    parse_runner_image_publication_record, validate_runner_image_publication_record,
};
pub use runner_publication_context::{
    RunnerPublicationDockerfileContext, runner_publication_dockerfile_context,
};

pub use runner_qualification::{
    RUNNER_QUALIFICATION_SCHEMA, RunnerImageQualification, RunnerImageQualificationImage,
    RunnerImageQualificationImages, parse_runner_image_qualification,
    validate_runner_image_qualification,
};
