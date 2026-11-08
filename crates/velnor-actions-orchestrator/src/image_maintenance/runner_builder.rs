// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::runner_family_admission::{ExpectedBuildSource, ImageKind, verify_family_provenance};
use super::{
    BuildRunnerImagesRequest, RunnerImageBuildExecution, RunnerImageBuildPurpose,
    runner_builder_output::{Output, OutputParent},
    runner_buildx::Buildx,
    runner_contexts::{ContextStage, FILES},
    runner_source_batch, source, validate,
};
use crate::{OrchestratorError, internal::internal};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::File, os::unix::fs::PermissionsExt, path::Path};
use velnor_actions_contract::{
    ImageCandidateArchive, ImageCandidateOci, ImageCandidateWorkflow,
    RUNNER_IMAGE_PUBLICATION_SCHEMA, RunnerImagePublicationLock, RunnerImagePublicationProvenance,
    RunnerImagePublicationRecord, RunnerImagePublicationSource, runner_publication_source_inputs,
    validate_runner_image_publication_record,
};
use velnor_actions_mise::ImageCandidateCommand;
use velnor_archive_guard::owned_archive_guard::oci_candidate::OciCandidateEvidence;

struct Inputs {
    root: std::path::PathBuf,
    source: RunnerImagePublicationSource,
    lock: RunnerImagePublicationLock,
    lock_bytes: Vec<u8>,
    workflow: Option<ImageCandidateWorkflow>,
}

const ROLES: [(ImageKind, &str, &str, &str, &str); 3] = [
    (
        ImageKind::Runner,
        "runner",
        "velnor-runner-linux-amd64.tar",
        "runner-image-publication.json",
        "runner-image-provenance.json",
    ),
    (
        ImageKind::Dind,
        "dind",
        "velnor-dind-linux-amd64.tar",
        "dind-image-publication.json",
        "dind-image-provenance.json",
    ),
    (
        ImageKind::ResourceProbe,
        "resource_probe",
        "velnor-resource-probe-linux-amd64.tar",
        "resource-probe-image-publication.json",
        "resource-probe-image-provenance.json",
    ),
];

pub(super) fn build(request: &BuildRunnerImagesRequest) -> Result<(), OrchestratorError> {
    build_with_execution(request, &ImageCandidateCommand)
}

pub(super) fn build_with_execution(
    request: &BuildRunnerImagesRequest,
    execution: &dyn RunnerImageBuildExecution,
) -> Result<(), OrchestratorError> {
    let root = source::checked_root(&request.root)?;
    let destination = OutputParent::open(&request.output_dir, &root)?;
    let inputs = prepare(request, root)?;
    let buildx = Buildx::load(
        &inputs.root,
        &request.runner_temp,
        request.deadline,
        execution,
    )?;
    let mut output = Output::new(destination, if inputs.workflow.is_some() { 13 } else { 10 })?;
    let contexts =
        match ContextStage::capture(&inputs.root, &request.contexts_dir, request.deadline) {
            Ok(contexts) => contexts,
            Err(error) => {
                output.cleanup()?;
                return Err(error);
            }
        };
    let result = build_all(request, &inputs, &buildx, &contexts, &mut output, execution);
    let cleanup = contexts.close();
    if let Err(error) = result {
        output.cleanup()?;
        cleanup?;
        return Err(error);
    }
    if let Err(error) = cleanup {
        output.cleanup()?;
        return Err(error);
    }
    output.publish()
}

fn prepare(
    request: &BuildRunnerImagesRequest,
    root: std::path::PathBuf,
) -> Result<Inputs, OrchestratorError> {
    source::verify_selected_source(&root, &request.source_commit, request.deadline)?;
    let lock_bytes = source::read_source_file(
        &root,
        &request.source_commit,
        ".velnor/runner.lock",
        1024 * 1024,
        request.deadline,
    )?;
    let lock = validate::validate_runner_lock(&lock_bytes)?;
    let workflow = if request.purpose == RunnerImageBuildPurpose::Publication {
        if source::env("GITHUB_SHA")? != request.source_commit {
            return Err(internal("runner_build_source"));
        }
        let (workflow, source_ref) = validate::workflow_identity()?;
        if workflow.event != "push" || source_ref != "refs/heads/main" {
            return Err(internal("runner_build_publication_authority"));
        }
        Some(workflow)
    } else {
        None
    };
    let source = RunnerImagePublicationSource {
        commit: request.source_commit.clone(),
        source_ref: "refs/heads/main".to_owned(),
        inputs: source_inputs(&root, &request.source_commit, request.deadline)?,
    };
    Ok(Inputs {
        root,
        source,
        lock,
        lock_bytes,
        workflow,
    })
}

fn build_all(
    request: &BuildRunnerImagesRequest,
    inputs: &Inputs,
    buildx: &Buildx,
    contexts: &ContextStage,
    output: &mut Output,
    execution: &dyn RunnerImageBuildExecution,
) -> Result<(), OrchestratorError> {
    let raw = tempfile::Builder::new()
        .prefix("velnor-runner-build-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(&request.runner_temp)
        .map_err(|_| internal("runner_build_scratch"))?;
    let result = (|| {
        for (kind, role, archive, record, envelope) in ROLES {
            contexts.revalidate(request.deadline)?;
            buildx.revalidate()?;
            let original = raw.path().join(archive);
            let command = ImageCandidateCommand::build_family_oci(
                &inputs.root,
                &original,
                &buildx.config,
                &request.source_commit,
                role,
            )
            .map_err(|_| internal("runner_build_command"))?;
            execution
                .run(&command, request.deadline)
                .map_err(|_| internal("runner_build_failed"))?;
            contexts.revalidate(request.deadline)?;
            buildx.revalidate()?;
            project(
                request,
                inputs,
                contexts,
                output,
                &original,
                (kind, role, archive, record, envelope),
            )?;
        }
        copy_contexts(request, contexts, output)?;
        contexts.revalidate(request.deadline)?;
        source::verify_selected_source(&inputs.root, &request.source_commit, request.deadline)?;
        if source_inputs(&inputs.root, &request.source_commit, request.deadline)?
            != inputs.source.inputs
        {
            return Err(internal("runner_build_source_changed"));
        }
        Ok(())
    })();
    let cleanup = raw
        .close()
        .map_err(|_| internal("runner_build_scratch_cleanup"));
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (_, Err(error)) => Err(error),
    }
}

fn project(
    request: &BuildRunnerImagesRequest,
    inputs: &Inputs,
    contexts: &ContextStage,
    output: &mut Output,
    original: &Path,
    role: (ImageKind, &str, &str, &str, &str),
) -> Result<(), OrchestratorError> {
    let (kind, name, archive, record_name, envelope_name) = role;
    let original = File::from(
        rustix::fs::open(
            original,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| internal("runner_build_archive"))?,
    );
    let mut candidate = output.create(archive)?;
    let projection =
        velnor_archive_guard::owned_archive_guard::oci_candidate::project_attested_oci_file(
            &original,
            &mut candidate,
        )
        .map_err(|_| internal("runner_build_projection"))?;
    let envelope =
        velnor_archive_guard::owned_archive_guard::oci_candidate::encode_provenance_envelope(
            &projection.provenance,
        )
        .map_err(|_| internal("runner_build_envelope"))?;
    let named_contexts = contexts.records(name)?;
    let paths = named_contexts
        .iter()
        .map(|(name, value)| (name.clone(), value.path.clone()))
        .collect::<BTreeMap<_, _>>();
    let lock: toml::Value = toml::from_str(
        std::str::from_utf8(&inputs.lock_bytes).map_err(|_| internal("runner_build_lock"))?,
    )
    .map_err(|_| internal("runner_build_lock"))?;
    let base = if kind == ImageKind::ResourceProbe {
        None
    } else {
        Some(
            lock.get("base_image")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| internal("runner_build_lock_base"))?,
        )
    };
    let expected = ExpectedBuildSource {
        role: kind,
        source_commit: &request.source_commit,
        platform_digest: &projection.candidate.evidence.platform_manifest_digest,
        base_image: base,
        named_context_paths: &paths,
    };
    let verified = verify_family_provenance(&candidate, &envelope, &expected)
        .ok_or_else(|| internal("runner_build_provenance"))?;
    output.write(envelope_name, &envelope)?;
    write_record(
        inputs,
        (name, record_name),
        &projection,
        &envelope,
        named_contexts,
        output,
    )?;
    if !verified.revalidate(&candidate) {
        return Err(internal("runner_build_candidate_changed"));
    }
    Ok(())
}

fn write_record(
    inputs: &Inputs,
    role: (&str, &str),
    projection: &velnor_archive_guard::owned_archive_guard::oci_candidate::AttestedProjection,
    envelope: &[u8],
    named_contexts: BTreeMap<String, velnor_actions_contract::RunnerImageNamedContext>,
    output: &mut Output,
) -> Result<(), OrchestratorError> {
    let Some(workflow) = &inputs.workflow else {
        return Ok(());
    };
    let (name, record_name) = role;
    let (archive, oci) = map_evidence(&projection.candidate.evidence);
    let provenance = RunnerImagePublicationProvenance {
        envelope: ImageCandidateArchive {
            sha256: source::hex(&Sha256::digest(envelope)),
            size_bytes: envelope.len() as u64,
        },
        original_archive: ImageCandidateArchive {
            sha256: projection.provenance.original_archive_sha256.clone(),
            size_bytes: projection.provenance.original_archive_size_bytes,
        },
        original_index: ImageCandidateArchive {
            sha256: source::hex(&Sha256::digest(&projection.provenance.original_index)),
            size_bytes: projection.provenance.original_index.len() as u64,
        },
        named_contexts,
    };
    let record = RunnerImagePublicationRecord {
        schema: RUNNER_IMAGE_PUBLICATION_SCHEMA.to_owned(),
        image: name.to_owned(),
        source: inputs.source.clone(),
        workflow: workflow.clone(),
        platform: "linux/amd64".to_owned(),
        archive,
        oci,
        provenance,
        lock: inputs.lock.clone(),
    };
    validate_runner_image_publication_record(&record)
        .map_err(|_| internal("runner_build_record"))?;
    let mut bytes = serde_json::to_vec(&record).map_err(|_| internal("runner_build_record"))?;
    bytes.push(b'\n');
    output.write(record_name, &bytes)?;
    Ok(())
}

fn copy_contexts(
    request: &BuildRunnerImagesRequest,
    contexts: &ContextStage,
    output: &mut Output,
) -> Result<(), OrchestratorError> {
    use std::io::Write;
    use std::os::unix::fs::FileExt;
    let records = contexts.records("runner")?;
    for (name, _) in FILES {
        let record = records
            .get(name)
            .ok_or_else(|| internal("runner_build_context"))?;
        let input = contexts.file(name)?;
        let mut destination = output.create(name)?;
        let mut buffer = vec![0; 64 * 1024];
        let mut offset = 0;
        let mut sha = Sha256::new();
        loop {
            request
                .deadline
                .remaining()
                .map_err(|_| internal("runner_build_deadline"))?;
            let count = input
                .read_at(&mut buffer, offset)
                .map_err(|_| internal("runner_build_context_read"))?;
            if count == 0 {
                break;
            }
            offset = offset
                .checked_add(count as u64)
                .filter(|n| *n <= record.size_bytes)
                .ok_or_else(|| internal("runner_build_context_changed"))?;
            destination
                .write_all(&buffer[..count])
                .map_err(|_| internal("runner_build_context_write"))?;
            sha.update(&buffer[..count]);
        }
        if offset != record.size_bytes || source::hex(&sha.finalize()) != record.sha256 {
            return Err(internal("runner_build_context_changed"));
        }
        destination
            .sync_all()
            .map_err(|_| internal("runner_build_context_write"))?;
    }
    Ok(())
}

fn source_inputs(
    root: &Path,
    commit: &str,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let paths = runner_publication_source_inputs()
        .map_err(|_| internal("runner_build_source_inventory"))?;
    runner_source_batch::read(root, commit, paths, deadline)
}

fn map_evidence(evidence: &OciCandidateEvidence) -> (ImageCandidateArchive, ImageCandidateOci) {
    (
        ImageCandidateArchive {
            sha256: evidence.archive_sha256.clone(),
            size_bytes: evidence.archive_size_bytes,
        },
        ImageCandidateOci {
            archive_index_digest: evidence.archive_index_digest.clone(),
            archive_index_size_bytes: evidence.archive_index_size_bytes,
            platform_manifest_digest: evidence.platform_manifest_digest.clone(),
            platform_manifest_size_bytes: evidence.platform_manifest_size_bytes,
            image_configuration_digest: evidence.image_configuration_digest.clone(),
            image_configuration_size_bytes: evidence.image_configuration_size_bytes,
            layer_digests: evidence.layer_digests.clone(),
        },
    )
}
