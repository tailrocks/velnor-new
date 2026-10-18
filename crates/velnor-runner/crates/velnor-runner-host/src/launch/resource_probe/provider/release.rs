//! Release signature, immutable asset set, and workflow provenance checks.

use std::collections::HashMap;
use std::ffi::OsString;

use bollard::models::ContainerConfig;
use serde_json::Value;

use super::gh::{self, Workspace};
use super::{
    OIDC_ISSUER, ProviderFailure, SIGNER_WORKFLOW, SOURCE_REF, SOURCE_REPOSITORY, hex, valid_sha,
};

mod attestation;
mod manifest;
mod metadata;
mod worker;

pub(super) use manifest::ResourceManifest;
pub(super) use worker::prepare_worker_releases;

const PROBE_ARCHIVE: &str = "velnor-resource-probe-linux-amd64.tar";
const RUNNER_ARCHIVE: &str = "velnor-runner-linux-amd64.tar";
const DIND_ARCHIVE: &str = "velnor-dind-linux-amd64.tar";
const MANIFEST_FILE: &str = "RESOURCE_PROBE_MANIFEST.json";
const CHECKSUMS_FILE: &str = "SHA256SUMS";
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
const MAX_CHECKSUMS_BYTES: u64 = 4 * 1024;
const RELEASE_SIGNER: &str = "https://dotcom.releases.github.com";

const ASSETS: [&str; 5] = [
    "velnor-runner-linux-amd64.tar",
    "velnor-dind-linux-amd64.tar",
    PROBE_ARCHIVE,
    MANIFEST_FILE,
    CHECKSUMS_FILE,
];

const CHECKSUM_ASSETS: [&str; 4] = [
    "velnor-runner-linux-amd64.tar",
    "velnor-dind-linux-amd64.tar",
    PROBE_ARCHIVE,
    MANIFEST_FILE,
];

pub(super) struct PreparedRelease {
    pub(super) load_archive: Vec<u8>,
    pub(super) source_revision: String,
    pub(super) archive_sha256: String,
    pub(super) archive_index_digest: String,
    pub(super) manifest_digest: String,
    pub(super) manifest_size: u64,
    pub(super) config_digest: String,
    pub(super) config: ContainerConfig,
    pub(super) load_archive_sha256: String,
    pub(super) load_archive_index_digest: String,
    pub(super) image_reference: String,
}

pub(super) struct PreparedWorkerReleases {
    _workspace: Workspace,
    pub(super) runner: PreparedWorkerArtifact,
    pub(super) dind: PreparedWorkerArtifact,
}

pub(super) struct PreparedWorkerArtifact {
    pub(super) image: super::archive::worker::WorkerImage,
    pub(super) original: super::archive::worker::ParsedWorkerArchive,
    pub(super) load: super::archive::worker::DerivedLoadArchive,
}

struct VerifiedDownloads {
    archive: Vec<u8>,
    manifest: ResourceManifest,
    hashes: HashMap<&'static str, String>,
}

pub(super) async fn prepare(
    pat: &str,
    source_revision: &str,
) -> Result<PreparedRelease, ProviderFailure> {
    if !valid_sha(source_revision) {
        return Err(ProviderFailure::TrustedSourcePinInvalid);
    }
    let tag = format!("runner-{source_revision}");
    let workspace = Workspace::create()?;
    gh::assert_version(&workspace, pat).await?;
    let assets = verify_release(&workspace, pat, &tag, source_revision).await?;
    download_selected_assets(
        &workspace,
        pat,
        &tag,
        &assets,
        &[PROBE_ARCHIVE, MANIFEST_FILE, CHECKSUMS_FILE],
    )
    .await?;
    let downloads = verify_downloads(&workspace, &assets, source_revision)?;
    verify_asset_attestations(&workspace, pat, &tag, source_revision, &downloads).await?;
    let parsed = super::archive::verify(&downloads.archive, &downloads.manifest, source_revision)?;
    Ok(PreparedRelease {
        load_archive: parsed.load_archive,
        source_revision: source_revision.to_owned(),
        archive_sha256: parsed.archive_sha256,
        archive_index_digest: parsed.archive_index_digest,
        manifest_digest: parsed.manifest_digest,
        manifest_size: parsed.manifest_size,
        config_digest: parsed.config_digest,
        config: parsed.config,
        load_archive_sha256: parsed.load_archive_sha256,
        load_archive_index_digest: parsed.load_archive_index_digest,
        image_reference: parsed.image_reference,
    })
}

async fn verify_release(
    workspace: &Workspace,
    pat: &str,
    tag: &str,
    source_revision: &str,
) -> Result<metadata::HashMapAssets, ProviderFailure> {
    let release = command_json(
        workspace,
        pat,
        [
            "release",
            "view",
            tag,
            "--repo",
            SOURCE_REPOSITORY,
            "--json",
            "tagName,targetCommitish,isDraft,isImmutable,isPrerelease,assets",
        ],
    )
    .await?;
    let assets = metadata::release_metadata(&release, tag, source_revision)?;
    let signature = command_json(
        workspace,
        pat,
        [
            "release",
            "verify",
            tag,
            "--repo",
            SOURCE_REPOSITORY,
            "--format",
            "json",
        ],
    )
    .await?;
    metadata::verify_release_signature(&signature, tag, source_revision, &assets)?;
    verify_tag_target(workspace, pat, tag, source_revision).await?;
    Ok(assets)
}

async fn download_selected_assets(
    workspace: &Workspace,
    pat: &str,
    tag: &str,
    assets: &metadata::HashMapAssets,
    selected: &[&str],
) -> Result<(), ProviderFailure> {
    for name in selected {
        let metadata = assets.get(*name).ok_or(ProviderFailure::ReleaseAssets)?;
        let digest = metadata
            .digest
            .strip_prefix("sha256:")
            .ok_or(ProviderFailure::ReleaseAssets)?;
        let arguments = [
            "release",
            "download",
            tag,
            "--repo",
            SOURCE_REPOSITORY,
            "--pattern",
            name,
            "--output",
            "-",
        ]
        .into_iter()
        .map(OsString::from);
        gh::download_to_file(
            workspace,
            pat,
            arguments,
            &workspace.assets().join(name),
            metadata.size,
            max_asset_bytes(name)?,
            digest,
        )
        .await?;
    }
    Ok(())
}

fn max_asset_bytes(name: &str) -> Result<u64, ProviderFailure> {
    match name {
        RUNNER_ARCHIVE | DIND_ARCHIVE => Ok(super::archive::worker::MAX_WORKER_ARCHIVE_BYTES),
        PROBE_ARCHIVE => Ok(MAX_ARCHIVE_BYTES),
        MANIFEST_FILE => Ok(MAX_MANIFEST_BYTES),
        CHECKSUMS_FILE => Ok(MAX_CHECKSUMS_BYTES),
        _ => Err(ProviderFailure::ReleaseAssets),
    }
}

fn verify_downloads(
    workspace: &Workspace,
    assets: &metadata::HashMapAssets,
    source_revision: &str,
) -> Result<VerifiedDownloads, ProviderFailure> {
    let archive_path = workspace.assets().join(PROBE_ARCHIVE);
    let manifest_path = workspace.assets().join(MANIFEST_FILE);
    let checksums_path = workspace.assets().join(CHECKSUMS_FILE);
    metadata::ensure_exact_downloads(
        workspace.assets(),
        &[PROBE_ARCHIVE, MANIFEST_FILE, CHECKSUMS_FILE],
    )?;
    let archive = metadata::read_bounded(&archive_path, MAX_ARCHIVE_BYTES)?;
    let manifest_bytes = metadata::read_bounded(&manifest_path, MAX_MANIFEST_BYTES)?;
    let checksum_bytes = metadata::read_bounded(&checksums_path, MAX_CHECKSUMS_BYTES)?;
    let manifest = manifest::parse_manifest(&manifest_bytes, source_revision)?;
    let checksums = metadata::parse_checksums(&checksum_bytes)?;
    let hashes = metadata::asset_hashes(&archive, &manifest_bytes, &checksum_bytes);
    metadata::verify_asset_digests(assets, &checksums, &hashes)?;
    Ok(VerifiedDownloads {
        archive,
        manifest,
        hashes,
    })
}

async fn verify_asset_attestations(
    workspace: &Workspace,
    pat: &str,
    tag: &str,
    source_revision: &str,
    downloads: &VerifiedDownloads,
) -> Result<(), ProviderFailure> {
    for name in [PROBE_ARCHIVE, MANIFEST_FILE, CHECKSUMS_FILE] {
        verify_asset_attestation(
            workspace,
            pat,
            tag,
            source_revision,
            name,
            downloads
                .hashes
                .get(name)
                .ok_or(ProviderFailure::Attestation)?,
        )
        .await?;
    }
    Ok(())
}

async fn verify_worker_asset_attestations(
    workspace: &Workspace,
    pat: &str,
    tag: &str,
    source_revision: &str,
    assets: &metadata::HashMapAssets,
    runner: &PreparedWorkerArtifact,
    dind: &PreparedWorkerArtifact,
) -> Result<(), ProviderFailure> {
    for (name, digest) in [
        (RUNNER_ARCHIVE, runner.original.archive_sha256.as_str()),
        (DIND_ARCHIVE, dind.original.archive_sha256.as_str()),
        (
            CHECKSUMS_FILE,
            assets
                .get(CHECKSUMS_FILE)
                .and_then(|metadata| metadata.digest.strip_prefix("sha256:"))
                .ok_or(ProviderFailure::Attestation)?,
        ),
    ] {
        verify_asset_attestation(workspace, pat, tag, source_revision, name, digest).await?;
    }
    Ok(())
}

async fn verify_asset_attestation(
    workspace: &Workspace,
    pat: &str,
    tag: &str,
    source_revision: &str,
    name: &str,
    digest: &str,
) -> Result<(), ProviderFailure> {
    let path = workspace.assets().join(name);
    gh::run(
        workspace,
        pat,
        [
            OsString::from("release"),
            OsString::from("verify-asset"),
            OsString::from(tag),
            path.as_os_str().to_owned(),
            OsString::from("--repo"),
            OsString::from(SOURCE_REPOSITORY),
        ],
    )
    .await?;
    let attestation = command_json(
        workspace,
        pat,
        [
            "attestation",
            "verify",
            path.to_str().ok_or(ProviderFailure::GhOutput)?,
            "--repo",
            SOURCE_REPOSITORY,
            "--source-ref",
            SOURCE_REF,
            "--source-digest",
            source_revision,
            "--signer-workflow",
            &format!("{SOURCE_REPOSITORY}/{SIGNER_WORKFLOW}"),
            "--signer-digest",
            source_revision,
            "--cert-oidc-issuer",
            OIDC_ISSUER,
            "--format",
            "json",
        ],
    )
    .await?;
    attestation::verify_asset_attestation(&attestation, name, digest, source_revision)
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(bytes))
}

async fn command_json<const N: usize>(
    workspace: &Workspace,
    pat: &str,
    args: [&str; N],
) -> Result<Value, ProviderFailure> {
    let output = gh::run(workspace, pat, args.into_iter().map(OsString::from)).await?;
    serde_json::from_slice(&output).map_err(|_| ProviderFailure::GhOutput)
}

async fn verify_tag_target(
    workspace: &Workspace,
    pat: &str,
    tag: &str,
    source_revision: &str,
) -> Result<(), ProviderFailure> {
    let ref_name = format!("repos/{SOURCE_REPOSITORY}/git/ref/tags/{tag}");
    let mut reference = command_json(workspace, pat, ["api", &ref_name]).await?;
    let mut object_type = reference
        .pointer("/object/type")
        .and_then(Value::as_str)
        .ok_or(ProviderFailure::ReleaseTarget)?;
    let mut object_sha = reference
        .pointer("/object/sha")
        .and_then(Value::as_str)
        .ok_or(ProviderFailure::ReleaseTarget)?;
    if object_type == "tag" {
        let tag_api = format!("repos/{SOURCE_REPOSITORY}/git/tags/{object_sha}");
        reference = command_json(workspace, pat, ["api", &tag_api]).await?;
        object_type = reference
            .pointer("/object/type")
            .and_then(Value::as_str)
            .ok_or(ProviderFailure::ReleaseTarget)?;
        object_sha = reference
            .pointer("/object/sha")
            .and_then(Value::as_str)
            .ok_or(ProviderFailure::ReleaseTarget)?;
    }
    if object_type == "commit" && object_sha == source_revision {
        Ok(())
    } else {
        Err(ProviderFailure::ReleaseTarget)
    }
}
