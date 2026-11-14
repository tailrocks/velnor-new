// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use futures_util::StreamExt;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

use rustix::fs::{self as rfs, Mode, OFlags};
use sha2::{Digest, Sha256};
use velnor_runner_image_manifest::{
    ExpectedBuildSource, ImageKind, MeasuredEnvelopeDigests, RunnerImagePublicationRecord,
    admits_build_source, parse_provenance_envelope, parse_runner_image_publication_record,
    runner_publication_source_inputs, verify_envelope_graph,
};

use super::source::{self, SourceSnapshot};
use crate::error::HostError;
use crate::launch::{ProjectedCandidate, inspect_projected_candidate};

mod contexts;
mod snapshot;
mod source_validation;
use source_validation::{
    base_image, role_name, sha256_hex, snapshot_path, validate_held_file, validate_records,
};

use contexts::Contexts;
use snapshot::{SnapshotDirectory, create_bounded_snapshot, create_snapshot};

const RECORD_LIMIT: usize = 1024 * 1024;
const ENVELOPE_LIMIT: usize = 8 * 1024 * 1024;
const SOURCE_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct CandidateInput<'a> {
    pub archive: &'a Path,
    pub envelope: &'a Path,
    pub record: Option<&'a Path>,
    pub kind: ImageKind,
}

pub(super) struct Candidate {
    pub file: std::fs::File,
    pub inspection: ProjectedCandidate,
    pub record: Option<RunnerImagePublicationRecord>,
    pub record_sha256: Option<String>,
    kind: ImageKind,
    path: PathBuf,
    envelope: HeldBytes,
    record_file: Option<HeldBytes>,
}

struct HeldBytes {
    file: std::fs::File,
    path: PathBuf,
    bytes: Vec<u8>,
}

pub(super) struct Family {
    directory: SnapshotDirectory,
    contexts: Contexts,
    pub source: SourceSnapshot,
    pub runner: Candidate,
    pub dind: Candidate,
    pub probe: Candidate,
}

impl Family {
    pub(super) async fn admit(
        root: &Path,
        source_sha: &str,
        inputs: [CandidateInput<'_>; 3],
        contexts_root: &Path,
        end: Instant,
    ) -> Result<Self, HostError> {
        let source =
            super::deadline(source::load(root, source_sha), super::remaining(end)?).await??;
        let path = tempfile::Builder::new()
            .prefix("velnor-family-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .map_err(|_| HostError::Path)?;
        let handle = std::fs::File::from(
            rfs::open(
                path.path(),
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| HostError::Path)?,
        );
        let mut directory = SnapshotDirectory {
            path: Some(path),
            handle,
            created: Vec::new(),
        };
        directory.check()?;
        let contexts = Contexts::capture(contexts_root, &mut directory, end)?;
        let [runner, dind, probe] = inputs;
        if runner.kind != ImageKind::Runner
            || dind.kind != ImageKind::Dind
            || probe.kind != ImageKind::ResourceProbe
        {
            return Err(HostError::Identity);
        }
        let runner = Candidate::admit(&mut directory, runner, &source, &contexts, end)?;
        let dind = Candidate::admit(&mut directory, dind, &source, &contexts, end)?;
        let probe = Candidate::admit(&mut directory, probe, &source, &contexts, end)?;
        validate_records(&source, [&runner.record, &dind.record, &probe.record], end)?;
        contexts.revalidate(end)?;
        super::deadline(source::revalidate(&source), super::remaining(end)?).await??;
        Ok(Self {
            directory,
            contexts,
            source,
            runner,
            dind,
            probe,
        })
    }

    pub(super) async fn load_all(
        &self,
        docker: &bollard::Docker,
        end: Instant,
    ) -> Result<(), HostError> {
        for candidate in [&self.runner, &self.dind, &self.probe] {
            self.revalidate(end).await?;
            candidate
                .load(docker, &self.source, &self.contexts, end)
                .await?;
            super::image_identity::inspect(docker, &candidate.inspection).await?;
        }
        Ok(())
    }

    pub(super) async fn revalidate(&self, end: Instant) -> Result<(), HostError> {
        super::deadline(source::revalidate(&self.source), super::remaining(end)?).await??;
        self.directory.check()?;
        self.contexts.revalidate(end)?;
        for candidate in [&self.runner, &self.dind, &self.probe] {
            candidate.revalidate(&self.source, &self.contexts, end)?;
        }
        validate_records(
            &self.source,
            [&self.runner.record, &self.dind.record, &self.probe.record],
            end,
        )
    }

    pub(super) fn cleanup(self) -> Result<SourceSnapshot, HostError> {
        let Self {
            directory,
            contexts,
            source,
            runner,
            dind,
            probe,
        } = self;
        drop((contexts, runner, dind, probe));
        directory.close()?;
        Ok(source)
    }
}

impl Candidate {
    fn admit(
        directory: &mut SnapshotDirectory,
        input: CandidateInput<'_>,
        source: &SourceSnapshot,
        contexts: &Contexts,
        end: Instant,
    ) -> Result<Self, HostError> {
        let name = role_name(input.kind);
        let path = snapshot_path(directory, name)?;
        let file = create_snapshot(input.archive, directory, name, end)?;
        let inspection = inspect_projected_candidate(&file, input.kind, &source.source_sha, end)?;
        let envelope = hold_bytes(
            directory,
            input.envelope,
            &format!("{name}-envelope"),
            ENVELOPE_LIMIT,
            end,
        )?;
        let record_file = input
            .record
            .map(|path| {
                hold_bytes(
                    directory,
                    path,
                    &format!("{name}-record"),
                    RECORD_LIMIT,
                    end,
                )
            })
            .transpose()?;
        let record = record_file
            .as_ref()
            .map(|held| {
                parse_runner_image_publication_record(&held.bytes).map_err(|_| HostError::Identity)
            })
            .transpose()?;
        let record_sha256 = record_file.as_ref().map(|held| sha256_hex(&held.bytes));
        let candidate = Self {
            file,
            inspection,
            record,
            record_sha256,
            kind: input.kind,
            path,
            envelope,
            record_file,
        };
        candidate.validate(input.kind, source, contexts, end)?;
        Ok(candidate)
    }

    fn revalidate(
        &self,
        source: &SourceSnapshot,
        contexts: &Contexts,
        end: Instant,
    ) -> Result<(), HostError> {
        self.validate(self.kind, source, contexts, end)
    }

    async fn load(
        &self,
        docker: &bollard::Docker,
        source: &SourceSnapshot,
        contexts: &Contexts,
        end: Instant,
    ) -> Result<(), HostError> {
        self.validate(self.kind, source, contexts, end)?;
        let mut file = self.file.try_clone().map_err(|_| HostError::Path)?;
        file.seek(SeekFrom::Start(0)).map_err(|_| HostError::Path)?;
        let file = tokio::fs::File::from_std(file);
        let sent = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&sent);
        let input =
            futures_util::stream::try_unfold((file, counter), |(mut file, counter)| async move {
                let mut bytes = vec![0_u8; 64 * 1024];
                let count = file.read(&mut bytes).await?;
                if count == 0 {
                    return Ok(None);
                }
                bytes.truncate(count);
                counter.fetch_add(count as u64, Ordering::Relaxed);
                Ok::<_, std::io::Error>(Some((bytes::Bytes::from(bytes), (file, counter))))
            });
        let limit = Duration::from_mins(15).min(super::remaining(end)?);
        super::deadline(
            async {
                let options = bollard::query_parameters::ImportImageOptionsBuilder::new()
                    .quiet(true)
                    .build();
                let mut output = docker.import_image_stream(options, input, None);
                let mut count = 0_usize;
                let mut size = 0_usize;
                while let Some(item) = output.next().await {
                    let item = item.map_err(|_| HostError::Docker)?;
                    count = count.saturating_add(1);
                    size = size
                        .saturating_add(item.stream.as_ref().map_or(0, String::len))
                        .saturating_add(item.status.as_ref().map_or(0, String::len))
                        .saturating_add(item.id.as_ref().map_or(0, String::len));
                    if count > 2048 || size > 1024 * 1024 || item.error_detail.is_some() {
                        return Err(HostError::Docker);
                    }
                }
                Ok::<(), HostError>(())
            },
            limit,
        )
        .await??;
        if sent.load(Ordering::Relaxed) != self.inspection.archive.size_bytes {
            return Err(HostError::Docker);
        }
        self.validate(self.kind, source, contexts, end)
    }

    fn validate(
        &self,
        kind: ImageKind,
        source: &SourceSnapshot,
        contexts: &Contexts,
        end: Instant,
    ) -> Result<(), HostError> {
        validate_held_file(&self.file, &self.path)?;
        self.envelope.revalidate(ENVELOPE_LIMIT)?;
        if let Some(record) = &self.record_file {
            record.revalidate(RECORD_LIMIT)?;
        }
        let inspection = inspect_projected_candidate(&self.file, kind, &source.source_sha, end)?;
        if inspection.archive != self.inspection.archive
            || inspection.oci != self.inspection.oci
            || inspection.configuration != self.inspection.configuration
        {
            return Err(HostError::Identity);
        }
        let envelope =
            parse_provenance_envelope(&self.envelope.bytes).map_err(|_| HostError::Identity)?;
        let nested = sha256_hex(&envelope.buildkit_index);
        let manifest = sha256_hex(&envelope.attestation_manifest);
        let config = sha256_hex(&envelope.attestation_config);
        let statement = sha256_hex(&envelope.statement);
        let platform = verify_envelope_graph(
            &envelope,
            &MeasuredEnvelopeDigests {
                buildkit_index_sha256: &nested,
                attestation_manifest_sha256: &manifest,
                attestation_config_sha256: &config,
                statement_sha256: &statement,
            },
        )
        .map_err(|_| HostError::Identity)?;
        if format!("sha256:{}", platform.digest) != inspection.oci.platform_manifest_digest
            || platform.size != inspection.oci.platform_manifest_size_bytes
        {
            return Err(HostError::Identity);
        }
        let named = contexts.expected(kind == ImageKind::Runner);
        let named_paths = named
            .iter()
            .map(|(key, value)| (key.clone(), value.path.clone()))
            .collect();
        let base = base_image(&source.lock_bytes)?;
        let expected = ExpectedBuildSource {
            role: kind,
            source_commit: &source.source_sha,
            platform_digest: &inspection.oci.platform_manifest_digest,
            base_image: (kind != ImageKind::ResourceProbe).then_some(base.as_str()),
            named_context_paths: &named_paths,
        };
        if !admits_build_source(&envelope.statement, &expected) {
            return Err(HostError::Identity);
        }
        if let Some(record) = &self.record
            && (record.image != role_name(kind)
                || record.archive != inspection.archive
                || record.oci != inspection.oci
                || record.provenance.named_contexts != named
                || record.provenance.envelope.sha256 != sha256_hex(&self.envelope.bytes)
                || record.provenance.envelope.size_bytes != self.envelope.bytes.len() as u64
                || record.provenance.original_archive.sha256 != envelope.original_archive_sha256
                || record.provenance.original_archive.size_bytes
                    != envelope.original_archive_size_bytes
                || record.provenance.original_index.sha256 != sha256_hex(&envelope.original_index)
                || record.provenance.original_index.size_bytes
                    != envelope.original_index.len() as u64)
        {
            return Err(HostError::Identity);
        }
        if Instant::now() >= end {
            return Err(HostError::DockerTimeout);
        }
        Ok(())
    }
}

fn hold_bytes(
    directory: &mut SnapshotDirectory,
    source: &Path,
    name: &str,
    limit: usize,
    end: Instant,
) -> Result<HeldBytes, HostError> {
    let path = snapshot_path(directory, name)?;
    let file = create_bounded_snapshot(source, directory, name, end, limit as u64)?;
    let bytes = super::super::files::read_bounded_regular(&path, limit)?;
    let held = HeldBytes { file, path, bytes };
    held.revalidate(limit)?;
    Ok(held)
}

impl HeldBytes {
    fn revalidate(&self, limit: usize) -> Result<(), HostError> {
        validate_held_file(&self.file, &self.path)?;
        let mut held = self.file.try_clone().map_err(|_| HostError::Path)?;
        held.seek(SeekFrom::Start(0)).map_err(|_| HostError::Path)?;
        let mut bytes = Vec::new();
        held.take(limit.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| HostError::Path)?;
        validate_held_file(&self.file, &self.path)?;
        if bytes != self.bytes {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}
