// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use crate::error::HostError;

use super::{docker, family, image_identity, probe_image, receipt};

fn valid_socket(value: &str) -> bool {
    let path = value.strip_prefix("unix://").unwrap_or(value);
    path.starts_with('/') && path.len() > 1 && !path.chars().any(char::is_control)
}

pub(in super::super) async fn qualify_archives(
    request: &super::super::RunnerArchiveQualification<'_>,
) -> Result<(), HostError> {
    use velnor_runner_image_manifest::ImageKind;

    if !valid_socket(request.docker_socket)
        || std::env::var("GITHUB_REPOSITORY").ok().as_deref() != Some("tailrocks/velnor-new")
        || std::env::var("GITHUB_RUN_ID")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            != Some(request.qualifier_run_id)
        || std::env::var("GITHUB_RUN_ATTEMPT")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            != Some(request.qualifier_run_attempt)
    {
        return Err(HostError::Identity);
    }
    let end = Instant::now() + Duration::from_secs(3600);
    let family = Box::pin(family::Family::admit(
        request.root,
        request.source_sha,
        [
            family::CandidateInput {
                archive: request.runner_archive,
                envelope: request.runner_envelope,
                record: Some(request.runner_record),
                kind: ImageKind::Runner,
            },
            family::CandidateInput {
                archive: request.dind_archive,
                envelope: request.dind_envelope,
                record: Some(request.dind_record),
                kind: ImageKind::Dind,
            },
            family::CandidateInput {
                archive: request.probe_archive,
                envelope: request.probe_envelope,
                record: Some(request.probe_record),
                kind: ImageKind::ResourceProbe,
            },
        ],
        request.contexts,
        end,
    ))
    .await?;
    let result = match make_proof(request, &family) {
        Ok(proof) => Box::pin(execute_family(request.docker_socket, &family, end))
            .await
            .map(|execution| (proof, execution)),
        Err(error) => Err(error),
    };
    let cleanup = family.cleanup();
    let ((proof, execution), source) = match (result, cleanup) {
        (Ok(execution), Ok(source)) => (execution, source),
        (Err(error), Ok(_)) | (_, Err(error)) => return Err(error),
    };
    receipt::publish(request.output, &source, &proof, end).await?;
    report_qualification(
        &execution.qualification,
        &proof.images.runner.oci.platform_manifest_digest,
        &proof.images.dind.oci.platform_manifest_digest,
        request.source_sha,
    );
    execution.daemon.report();
    Ok(())
}

pub(in super::super) async fn check_candidate(
    request: &super::super::RunnerCandidateCheck<'_>,
) -> Result<(), HostError> {
    use velnor_runner_image_manifest::ImageKind;

    if !valid_socket(request.docker_socket) {
        return Err(HostError::Identity);
    }
    let end = Instant::now() + Duration::from_secs(3600);
    let family = Box::pin(family::Family::admit(
        request.root,
        request.source_sha,
        [
            family::CandidateInput {
                archive: request.runner_archive,
                envelope: request.runner_envelope,
                record: None,
                kind: ImageKind::Runner,
            },
            family::CandidateInput {
                archive: request.dind_archive,
                envelope: request.dind_envelope,
                record: None,
                kind: ImageKind::Dind,
            },
            family::CandidateInput {
                archive: request.probe_archive,
                envelope: request.probe_envelope,
                record: None,
                kind: ImageKind::ResourceProbe,
            },
        ],
        request.contexts,
        end,
    ))
    .await?;
    let runner = family
        .runner
        .inspection
        .oci
        .platform_manifest_digest
        .clone();
    let dind = family.dind.inspection.oci.platform_manifest_digest.clone();
    let result = Box::pin(execute_family(request.docker_socket, &family, end)).await;
    let cleanup = family.cleanup();
    match (result, cleanup) {
        (Ok(execution), Ok(_)) => {
            report_qualification(&execution.qualification, &runner, &dind, request.source_sha);
            execution.daemon.report();
            Ok(())
        }
        (Err(error), Ok(_)) | (_, Err(error)) => Err(error),
    }
}

struct Execution {
    qualification: docker::Qualification,
    daemon: image_identity::DaemonEvidence,
}

async fn execute_family(
    docker_socket: &str,
    family: &family::Family,
    end: Instant,
) -> Result<Execution, HostError> {
    let docker = crate::docker_client::connect_unix(docker_socket)?;
    let daemon = image_identity::daemon(&docker).await?;
    family.load_all(&docker, end).await?;
    let runner = &family.runner.inspection.oci.platform_manifest_digest;
    let dind = &family.dind.inspection.oci.platform_manifest_digest;
    let probe = &family.probe.inspection.oci.platform_manifest_digest;
    let qualification = docker::qualify(
        &docker,
        &family.source,
        runner,
        dind,
        [
            image_identity::source_healthcheck(&family.runner.inspection.configuration)?,
            image_identity::source_healthcheck(&family.dind.inspection.configuration)?,
        ],
        end,
    )
    .await?;
    probe_image::qualify(
        &docker,
        probe,
        &daemon.root,
        image_identity::source_healthcheck(&family.probe.inspection.configuration)?,
        end,
    )
    .await?;
    for candidate in [&family.runner, &family.dind, &family.probe] {
        image_identity::inspect(&docker, &candidate.inspection).await?;
    }
    if image_identity::daemon(&docker).await? != daemon {
        return Err(HostError::Identity);
    }
    family.revalidate(end).await?;
    Ok(Execution {
        qualification,
        daemon,
    })
}

fn make_proof(
    request: &super::super::RunnerArchiveQualification<'_>,
    family: &family::Family,
) -> Result<velnor_runner_image_manifest::RunnerImageQualification, HostError> {
    use velnor_runner_image_manifest::{
        RUNNER_QUALIFICATION_SCHEMA, RunnerImageQualification, RunnerImageQualificationImage,
        RunnerImageQualificationImages,
    };
    let record = family.runner.record.as_ref().ok_or(HostError::Identity)?;
    if record.workflow.run_id != request.producer_run_id
        || record.workflow.run_attempt != u64::from(request.producer_run_attempt)
    {
        return Err(HostError::Identity);
    }
    let image =
        |candidate: &family::Candidate| -> Result<RunnerImageQualificationImage, HostError> {
            let record = candidate.record.as_ref().ok_or(HostError::Identity)?;
            Ok(RunnerImageQualificationImage {
                record_sha256: candidate.record_sha256.clone().ok_or(HostError::Identity)?,
                archive: record.archive.clone(),
                oci: record.oci.clone(),
                provenance: record.provenance.clone(),
            })
        };
    let proof = RunnerImageQualification {
        schema: RUNNER_QUALIFICATION_SCHEMA.to_owned(),
        source_commit: record.source.commit.clone(),
        source_ref: record.source.source_ref.clone(),
        runner_lock_sha256: record.lock.sha256.clone(),
        producer_run_id: request.producer_run_id,
        producer_run_attempt: request.producer_run_attempt,
        qualifier_run_id: request.qualifier_run_id,
        qualifier_run_attempt: request.qualifier_run_attempt,
        qualifier_job_id: request.qualifier_job_id,
        images: RunnerImageQualificationImages {
            runner: image(&family.runner)?,
            dind: image(&family.dind)?,
            resource_probe: image(&family.probe)?,
        },
    };
    velnor_runner_image_manifest::validate_runner_image_qualification(&proof)
        .map_err(|_| HostError::Identity)?;
    Ok(proof)
}

fn report_qualification(
    qualification: &docker::Qualification,
    runner: &str,
    dind: &str,
    source_sha: &str,
) {
    println!(
        "runner_listener_version={} runner_docker_version={} dind_docker_version={} dind_daemon_version={} runner_buildx_version={} runner_compose_version={} runner_node_version={}",
        qualification.versions.runner_listener,
        qualification.versions.runner_docker,
        qualification.versions.dind_docker,
        qualification.versions.dind_daemon,
        qualification.versions.runner_buildx,
        qualification.versions.runner_compose,
        qualification.versions.runner_node,
    );
    println!(
        "outer_docker_version={} outer_docker_id={} inner_docker_id={}",
        qualification.outer.version, qualification.outer.id, qualification.inner_id
    );
    println!(
        "qualified_runner_image_id={runner} qualified_dind_image_id={dind} source_revision={source_sha}"
    );
}
