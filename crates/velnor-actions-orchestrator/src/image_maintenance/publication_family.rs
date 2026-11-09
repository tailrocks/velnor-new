// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, fs::File, path::Path};

use super::super::runner_family_admission::{
    ExpectedBuildSource, ImageKind, StructurallyVerifiedProvenance, verify_family_provenance,
};
use sha2::{Digest, Sha256};
use velnor_actions_contract::runner_image_manifest::BuildSource;
use velnor_archive_guard::owned_archive_guard::oci_candidate::{
    OciCandidateEvidence, parse_provenance_envelope,
};

use crate::{OrchestratorError, internal::internal};

use super::{ImagePublicationProfile, PublishImageRequest, authority};

pub(super) struct BoundCandidate {
    file: File,
    verified: StructurallyVerifiedProvenance,
    source: BuildSource,
}

impl BoundCandidate {
    pub(super) fn source(&self) -> &BuildSource {
        &self.source
    }
    pub(super) fn revalidate(&self) -> Result<(), OrchestratorError> {
        if !self.verified.revalidate(&self.file) {
            return Err(internal("image_publication_family_changed"));
        }
        Ok(())
    }

    pub(super) fn evidence(&self) -> &OciCandidateEvidence {
        &self.verified.candidate().evidence
    }

    pub(super) fn materialize(&self, destination: &Path) -> Result<(), OrchestratorError> {
        if !self.verified.revalidate(&self.file) {
            return Err(internal("image_publication_family_changed"));
        }
        let evidence = velnor_archive_guard::owned_archive_guard::oci_candidate::materialize_oci_candidate_file(
            &self.file,
            destination,
            self.evidence(),
        )
        .map_err(|_| internal("image_publication_family_materialize"))?;
        if &evidence != self.evidence() || !self.verified.revalidate(&self.file) {
            return Err(internal("image_publication_family_changed"));
        }
        Ok(())
    }
}

pub(super) fn load(
    request: &PublishImageRequest,
    candidate: &authority::CandidateRecord,
    sources: &BTreeMap<String, Vec<u8>>,
) -> Result<Option<BoundCandidate>, OrchestratorError> {
    let Some(provenance) = &candidate.provenance else {
        return if request.profile == ImagePublicationProfile::Job {
            Ok(None)
        } else {
            Err(internal("image_publication_provenance_missing"))
        };
    };
    let parent = request
        .record_path
        .parent()
        .ok_or_else(|| internal("image_publication_family_path"))?;
    let envelope = super::super::validate::read_limited(
        &parent.join(envelope_name(request.profile)?),
        8 * 1024 * 1024,
    )?;
    if authority::hex(&Sha256::digest(&envelope)) != provenance.envelope.sha256
        || u64::try_from(envelope.len()).ok() != Some(provenance.envelope.size_bytes)
    {
        return Err(internal("image_publication_envelope_identity"));
    }
    let parsed = parse_provenance_envelope(&envelope)
        .map_err(|_| internal("image_publication_envelope_invalid"))?;
    if parsed.original_archive_sha256 != provenance.original_archive.sha256
        || parsed.original_archive_size_bytes != provenance.original_archive.size_bytes
        || authority::hex(&Sha256::digest(&parsed.original_index))
            != provenance.original_index.sha256
        || u64::try_from(parsed.original_index.len()).ok()
            != Some(provenance.original_index.size_bytes)
    {
        return Err(internal("image_publication_envelope_origin"));
    }
    let mut paths = BTreeMap::new();
    for (name, context) in &provenance.named_contexts {
        let (digest, size) = authority::hash_regular_file(
            &parent.join(name),
            1024 * 1024 * 1024,
            Some(request.deadline),
        )?;
        if digest != context.sha256 || size != context.size_bytes {
            return Err(internal("image_publication_context_identity"));
        }
        paths.insert(name.clone(), context.path.clone());
    }
    let context = match request.profile {
        ImagePublicationProfile::Runner => "images/runner/ubuntu-26.04",
        ImagePublicationProfile::Dind => "images/dind",
        ImagePublicationProfile::ResourceProbe => "images/resource-probe",
        ImagePublicationProfile::Job => return Err(internal("image_publication_family_role")),
    };
    let source = BuildSource {
        metadata_sha256: authority::hex(&Sha256::digest(&parsed.statement)),
        build_type:
            "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md"
                .to_owned(),
        source_repository: "https://github.com/tailrocks/velnor-new".to_owned(),
        source_commit: candidate.source_commit.clone(),
        context_path: context.to_owned(),
        dockerfile_path: format!("{context}/Dockerfile"),
        builder_platform: "linux/amd64".to_owned(),
        named_contexts: provenance
            .named_contexts
            .iter()
            .map(|(name, value)| (name.clone(), value.sha256.clone()))
            .collect(),
    };
    bind_candidate(request, candidate, sources, &envelope, &paths, source).map(Some)
}

fn bind_candidate(
    request: &PublishImageRequest,
    candidate: &authority::CandidateRecord,
    sources: &BTreeMap<String, Vec<u8>>,
    envelope: &[u8],
    paths: &BTreeMap<String, String>,
    source: BuildSource,
) -> Result<BoundCandidate, OrchestratorError> {
    let role = match request.profile {
        ImagePublicationProfile::Runner => ImageKind::Runner,
        ImagePublicationProfile::Dind => ImageKind::Dind,
        ImagePublicationProfile::ResourceProbe => ImageKind::ResourceProbe,
        ImagePublicationProfile::Job => return Err(internal("image_publication_family_role")),
    };
    let lock = sources
        .get(".velnor/runner.lock")
        .ok_or_else(|| internal("image_publication_lock_missing"))?;
    let lock: toml::Value = toml::from_str(
        std::str::from_utf8(lock).map_err(|_| internal("image_publication_lock_utf8"))?,
    )
    .map_err(|_| internal("image_publication_lock_parse"))?;
    let base_image = if role == ImageKind::ResourceProbe {
        None
    } else {
        Some(
            lock.get("base_image")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| internal("image_publication_lock_base"))?,
        )
    };
    let file = File::from(
        rustix::fs::open(
            &request.archive_path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| internal("image_publication_family_open"))?,
    );
    let expected = ExpectedBuildSource {
        role,
        source_commit: &candidate.source_commit,
        platform_digest: &candidate.oci.platform_manifest_digest,
        base_image,
        named_context_paths: paths,
    };
    let verified = verify_family_provenance(&file, envelope, &expected)
        .ok_or_else(|| internal("image_publication_family_provenance"))?;
    let bound = BoundCandidate {
        file,
        verified,
        source,
    };
    if !super::authority::oci::same_oci_evidence(
        bound.evidence(),
        &candidate.archive,
        &candidate.oci,
    ) {
        return Err(internal("image_publication_family_oci"));
    }
    Ok(bound)
}

pub(super) fn envelope_name(
    profile: ImagePublicationProfile,
) -> Result<&'static str, OrchestratorError> {
    match profile {
        ImagePublicationProfile::Runner => Ok("runner-image-provenance.json"),
        ImagePublicationProfile::Dind => Ok("dind-image-provenance.json"),
        ImagePublicationProfile::ResourceProbe => Ok("resource-probe-image-provenance.json"),
        ImagePublicationProfile::Job => Err(internal("image_publication_family_role")),
    }
}
