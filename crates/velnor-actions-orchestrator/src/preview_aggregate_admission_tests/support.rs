use super::super::super::leaf_manifest::{PreviewLeafManifest, encode};
use super::*;
use crate::package_release_signer::PackageReleaseSigningIdentity;
use std::{io::Write, path::PathBuf, time::Duration};
use velnor_actions_contract::{
    PackageReleasePlan, PackageReleasePlanProduct,
    config::RustBinaryReleaseCompanion,
    package_release::PackageReleasePayloadFormat,
    preview_production::{
        PayloadArtifactRef, ReleasePayloadPair, ReleaseProductionContext, ReleaseProductionRequest,
        ReleaseProductionStage, release_plan_sha256,
    },
};

pub(super) struct Fixture {
    _temp: tempfile::TempDir,
    pub root: PathBuf,
    pub proof: PreviewAggregateManifest,
    pub(super) files: Vec<(String, Vec<u8>)>,
}

impl Fixture {
    pub(super) fn new(channel: PackageReleaseChannel) -> Self {
        let temp = tempfile::tempdir().expect("private runner");
        let root = temp.path().canonicalize().expect("canonical runner");
        let plan = plan(channel);
        let preview = channel == PackageReleaseChannel::Preview;
        let context = ReleaseProductionContext {
            repository_id: 1,
            head_repository_id: 1,
            source_sha: "a".repeat(40),
            source_ref: "refs/heads/main".into(),
            workflow_authority_sha: "a".repeat(40),
            run_id: 3,
            run_attempt: 1,
            ci_run_id: 2,
            ci_run_attempt: 1,
            commit_count: u64::from(preview) * 7,
            version: if preview {
                "1.2.3-preview.7+aaaaaaa"
            } else {
                "1.2.3"
            }
            .into(),
        };
        let pair = ReleasePayloadPair {
            product_id: "cli".into(),
            target: "x86_64-unknown-linux-gnu".into(),
        };
        let request = ReleaseProductionRequest {
            schema: velnor_actions_contract::preview_production::RELEASE_PRODUCTION_SCHEMA_V1
                .into(),
            policy_sha256: "d".repeat(64),
            plan_sha256: release_plan_sha256(&plan).expect("plan"),
            plan,
            context,
            stage: ReleaseProductionStage::Assemble {
                inputs: vec![PayloadArtifactRef {
                    pair: pair.clone(),
                    artifact_id: 100,
                    sha256: "b".repeat(64),
                }],
            },
        };
        let files = files();
        let assets = files
            .iter()
            .map(|(name, bytes)| handoff::AssetProof {
                name: name.clone(),
                sha256: handoff::sha256(bytes),
                size: bytes.len() as u64,
            })
            .collect::<Vec<_>>();
        let mut leaf = request.clone();
        leaf.stage = ReleaseProductionStage::Payload { pair };
        let proof = PreviewAggregateManifest {
            schema: "velnor.release-aggregate.v1".into(),
            producer: producer(&request),
            request,
            leaves: vec![PreviewLeafManifest {
                schema: "velnor.release-payload.v1".into(),
                producer: producer(&leaf),
                request: leaf,
                assets: assets
                    .iter()
                    .filter(|asset| asset.name != "LICENSE")
                    .cloned()
                    .collect(),
            }],
            assets,
        };
        Self {
            _temp: temp,
            root,
            proof,
            files,
        }
    }

    pub(super) fn deadline() -> CheckDeadline {
        CheckDeadline::after(Duration::from_secs(30)).expect("deadline")
    }

    pub(super) fn zip(&self, extra: bool) -> PathBuf {
        let path = self.root.join("aggregate.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).expect("ZIP"));
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        let mut files = self.files.clone();
        files.push((
            "package-output.json".into(),
            manifest(&self.proof.request, &self.files),
        ));
        files.push((
            handoff::AGGREGATE_MANIFEST.into(),
            encode(&self.proof).expect("proof"),
        ));
        if extra {
            files.push(("extra".into(), b"extra".to_vec()));
        }
        for (name, bytes) in files {
            zip.start_file(name, options).expect("ZIP entry");
            zip.write_all(&bytes).expect("ZIP bytes");
        }
        zip.finish().expect("ZIP close");
        path
    }

    pub(super) fn extract(
        &self,
    ) -> Result<(PathBuf, PreviewAggregateManifest, Vec<u8>), OrchestratorError> {
        extract_aggregate(
            &mut fs::File::open(self.zip(false)).expect("open ZIP"),
            &self.root,
            &self.proof.request.policy_sha256,
            &self.proof.request.plan,
            &self.proof.request.context,
            Self::deadline(),
        )
    }

    pub(super) fn identity(&self) -> PackageReleaseSigningIdentity {
        let r = &self.proof.request;
        PackageReleaseSigningIdentity {
            repository: r.plan.repository.clone(),
            repository_id: 1,
            source_sha: r.context.source_sha.clone(),
            source_ref: r.context.source_ref.clone(),
            default_branch: r.plan.default_branch.clone(),
            event: r.plan.channel.event(),
            workflow_path: r.plan.channel.workflow_path().into(),
            workflow_ref: format!(
                "{}/{}@refs/heads/main",
                r.plan.repository,
                r.plan.channel.workflow_path()
            ),
            workflow_sha: r.context.source_sha.clone(),
            run_id: 3,
            run_attempt: 1,
            ci_run_id: 2,
            ci_run_attempt: 1,
            commit_count: (r.plan.channel == PackageReleaseChannel::Preview).then_some(7),
        }
    }
}

fn producer(request: &ReleaseProductionRequest) -> crate::preview_producer::PreviewProducerProof {
    crate::preview_producer::PreviewProducerProof {
        source_sha: request.context.source_sha.clone(),
        source_ref: request.context.source_ref.clone(),
        version: request.context.version.clone(),
        commit_count: request.context.commit_count,
        task: "release".into(),
        task_exit_code: 0,
        duration_ms: 1,
        output_directory: "dist".into(),
        output_manifest: "dist/package-output.json".into(),
        qualification_digest: "fixture-not-qualification".into(),
        system_tools: vec![],
        qualified_tools: vec![],
    }
}

fn files() -> Vec<(String, Vec<u8>)> {
    let mut elf = vec![0_u8; 64];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4..7].copy_from_slice(&[2, 1, 1]);
    elf[16..18].copy_from_slice(&3_u16.to_le_bytes());
    elf[18..20].copy_from_slice(&62_u16.to_le_bytes());
    let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(gzip);
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o755);
    header.set_size(elf.len() as u64);
    header.set_cksum();
    tar.append_data(&mut header, "cli", elf.as_slice())
        .expect("ELF archive");
    let archive = tar.into_inner().expect("TAR").finish().expect("gzip");
    let name = "cli-x86_64-unknown-linux-gnu.tar.gz";
    let sbom = serde_json::json!({ "bomFormat": "CycloneDX", "specVersion": "1.6", "version": 1,
        "metadata": {"component": {"type": "file", "name": name,
            "hashes": [{"alg": "SHA-256", "content": handoff::sha256(&archive)}]}}, "components": [] });
    vec![
        ("LICENSE".into(), b"license bytes".to_vec()),
        (name.into(), archive),
        (
            format!("{name}.sbom.json"),
            serde_json::to_vec(&sbom).expect("SBOM"),
        ),
    ]
}

fn manifest(request: &ReleaseProductionRequest, files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let name = &files[1].0;
    serde_json::to_vec(&serde_json::json!({
        "schema": "velnor.package-output.v1", "source_commit": request.context.source_sha,
        "source_ref": request.context.source_ref, "version": request.context.version,
        "payloads": [{"product_id": "cli", "target": "x86_64-unknown-linux-gnu",
            "format": "tar-gz", "name": name, "sha256": handoff::sha256(&files[1].1)}],
        "companions": [{"id": "license", "path": "metadata/license.txt", "name": "LICENSE",
            "sha256": handoff::sha256(&files[0].1)}],
        "sboms": [{"payload_name": name, "name": files[2].0, "sha256": handoff::sha256(&files[2].1)}]
    })).expect("source package output")
}

fn plan(channel: PackageReleaseChannel) -> PackageReleasePlan {
    PackageReleasePlan {
        schema: velnor_actions_contract::PACKAGE_RELEASE_PLAN_SCHEMA_V1.into(),
        channel,
        repository: "jackin-project/jackin".into(),
        default_branch: "main".into(),
        base_version: "1.2.3".into(),
        products: vec![PackageReleasePlanProduct {
            id: "cli".into(),
            package: "cli".into(),
            bins: vec!["cli".into()],
            targets: vec!["x86_64-unknown-linux-gnu".into()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: vec![RustBinaryReleaseCompanion {
            id: "license".into(),
            path: "metadata/license.txt".into(),
            asset_name: "LICENSE".into(),
        }],
    }
}
