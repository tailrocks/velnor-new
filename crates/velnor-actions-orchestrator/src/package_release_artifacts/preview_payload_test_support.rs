//! Minimal synthetic leaf bytes. No archive member is executed or extracted.

use super::super::{VerifiedPayload, verify_payload_output};
use crate::OrchestratorError;
use flate2::write::GzEncoder;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    preview_production::{
        RELEASE_PRODUCTION_SCHEMA_V1, ReleasePayloadPair, ReleaseProductionContext,
        ReleaseProductionRequest, ReleaseProductionStage, release_plan_sha256,
    },
};
use velnor_actions_mise::CheckDeadline;

pub(super) const ARCHIVE: &str = "cli-x86_64-unknown-linux-gnu.tar.gz";
pub(super) const SBOM: &str = "cli-x86_64-unknown-linux-gnu.tar.gz.sbom.json";
pub(super) const MANIFEST: &str = "package-output.json";

pub(super) struct Fixture {
    pub root: tempfile::TempDir,
    pub producer: PathBuf,
    pub staging: PathBuf,
    pub request: ReleaseProductionRequest,
}

pub(super) fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("fixture");
    let producer = root.path().join("producer");
    fs::create_dir(&producer).expect("producer");
    let plan = PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: vec![PackageReleasePlanProduct {
            id: "cli".to_owned(),
            package: "jackin".to_owned(),
            bins: vec!["cli".to_owned()],
            targets: vec![
                "aarch64-apple-darwin".to_owned(),
                "x86_64-unknown-linux-gnu".to_owned(),
            ],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: Vec::new(),
    };
    let request = ReleaseProductionRequest {
        policy_sha256: "d".repeat(64),
        schema: RELEASE_PRODUCTION_SCHEMA_V1.to_owned(),
        plan_sha256: release_plan_sha256(&plan).expect("plan digest"),
        plan,
        context: ReleaseProductionContext {
            repository_id: 10,
            head_repository_id: 10,
            source_sha: "a".repeat(40),
            source_ref: "refs/heads/main".to_owned(),
            workflow_authority_sha: "a".repeat(40),
            run_id: 20,
            run_attempt: 1,
            ci_run_id: 19,
            ci_run_attempt: 1,
            commit_count: 7,
            version: "1.2.3-preview.7+aaaaaaa".to_owned(),
        },
        stage: ReleaseProductionStage::Payload {
            pair: ReleasePayloadPair {
                product_id: "cli".to_owned(),
                target: "x86_64-unknown-linux-gnu".to_owned(),
            },
        },
    };
    let fixture = Fixture {
        staging: root.path().join("staged"),
        root,
        producer,
        request,
    };
    write_archive(&fixture.producer.join(ARCHIVE), "cli", 62);
    let sbom = json!({"bomFormat":"CycloneDX", "metadata":{"component":{
        "type":"file", "name":ARCHIVE,
        "hashes":[{"alg":"SHA-256","content":hash_file(&fixture.producer.join(ARCHIVE))}]
    }}});
    fs::write(
        fixture.producer.join(SBOM),
        serde_json::to_vec(&sbom).expect("SBOM"),
    )
    .expect("write SBOM");
    write_manifest(&fixture);
    fixture
}

pub(super) fn write_archive(path: &Path, member: &str, machine: u16) {
    let gzip = GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(gzip);
    let mut bytes = vec![0u8; 64];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&machine.to_le_bytes());
    let mut header = tar::Header::new_gnu();
    header.set_path(member).expect("member");
    header.set_mode(0o755);
    header.set_size(bytes.len() as u64);
    header.set_cksum();
    tar.append(&header, bytes.as_slice()).expect("append");
    fs::write(path, tar.into_inner().expect("tar").finish().expect("gzip")).expect("archive");
}

pub(super) fn write_manifest(fixture: &Fixture) {
    let context = &fixture.request.context;
    let manifest = json!({
        "schema":"velnor.package-payload-output.v1", "source_commit":context.source_sha,
        "source_ref":context.source_ref, "version":context.version,
        "payloads":[{"product_id":"cli", "target":"x86_64-unknown-linux-gnu",
            "format":"tar-gz", "name":ARCHIVE, "sha256":hash_file(&fixture.producer.join(ARCHIVE))}],
        "companions":[], "sboms":[{"payload_name":ARCHIVE, "name":SBOM,
            "sha256":hash_file(&fixture.producer.join(SBOM))}]
    });
    fs::write(
        fixture.producer.join(MANIFEST),
        serde_json::to_vec(&manifest).expect("manifest"),
    )
    .expect("write manifest");
}

pub(super) fn mutate_manifest(fixture: &Fixture, mutate: impl FnOnce(&mut serde_json::Value)) {
    let path = fixture.producer.join(MANIFEST);
    let mut value =
        serde_json::from_slice(&fs::read(&path).expect("read manifest")).expect("parse manifest");
    mutate(&mut value);
    fs::write(path, serde_json::to_vec(&value).expect("mutated JSON")).expect("mutate manifest");
}

pub(super) fn hash_file(path: &Path) -> String {
    use sha2::{Digest as _, Sha256};
    super::super::inventory::hex_lower(&Sha256::digest(fs::read(path).expect("hash fixture")))
}

pub(super) fn verify(fixture: &Fixture) -> Result<VerifiedPayload, OrchestratorError> {
    verify_payload_output(
        &fixture.producer,
        MANIFEST,
        &fixture.staging,
        &fixture.request,
        CheckDeadline::after(Duration::from_secs(10)).expect("deadline"),
    )
}
