use super::super::super::github::sha256;
use super::super::fixture::{record, request};
use super::super::{ArtifactPage, ArtifactRecord};
use super::*;
use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::time::Duration;

struct SignerApi {
    artifact: RefCell<Option<ArtifactRecord>>,
    bytes: Vec<u8>,
    authentications: Cell<usize>,
    downloads: Cell<usize>,
    fail_after_download: bool,
}
impl SignerApi {
    fn new() -> Self {
        let request = request();
        let mut artifact = record();
        artifact.name = release_aggregate_artifact_name(&request.plan, &request.context)
            .expect("aggregate name");
        Self {
            artifact: RefCell::new(Some(artifact)),
            bytes: b"closed ZIP bytes".to_vec(),
            authentications: Cell::new(0),
            downloads: Cell::new(0),
            fail_after_download: false,
        }
    }
}
impl ArtifactReadApi for SignerApi {
    fn authenticate(
        &self,
        _: &PackageReleasePlan,
        _: &ReleaseProductionContext,
        role: ReleaseConsumer,
    ) -> Result<(), OrchestratorError> {
        assert_eq!(role, ReleaseConsumer::Signer);
        self.authentications.set(self.authentications.get() + 1);
        if self.fail_after_download && self.downloads.get() > 0 {
            return Err(internal("changed_attempt"));
        }
        Ok(())
    }
    fn page(&self, run: u64, page: usize) -> Result<ArtifactPage, OrchestratorError> {
        assert_eq!((run, page), (202, 1));
        Ok(ArtifactPage {
            total_count: 1,
            artifacts: vec![self.artifact.borrow_mut().take().expect("single inventory")],
        })
    }
    fn jobs(&self, _: u64, _: u32) -> Result<Vec<super::super::JobRecord>, OrchestratorError> {
        Ok(Vec::new())
    }
    fn download(
        &self,
        id: u64,
        size: u64,
        output: &mut std::fs::File,
        _: CheckDeadline,
    ) -> Result<(), OrchestratorError> {
        assert_eq!((id, size), (303, 16));
        self.downloads.set(self.downloads.get() + 1);
        output
            .write_all(&self.bytes)
            .map_err(|_| internal("fixture_write"))
    }
}
fn digest() -> String {
    sha256(b"closed ZIP bytes")
}
fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(2)).expect("deadline")
}
struct TestAggregate {
    bytes: Vec<u8>,
}
fn invoke(api: &SignerApi, id: u64, digest: &str) -> Result<TestAggregate, OrchestratorError> {
    let request = request();
    let root = tempfile::tempdir().expect("archive directory");
    let mut actual = acquire(
        api,
        &request.plan,
        &request.context,
        id,
        digest,
        &root.path().canonicalize().expect("canonical"),
        deadline(),
    )?;
    let mut bytes = Vec::new();
    actual
        .archive
        .reader()?
        .read_to_end(&mut bytes)
        .map_err(|_| internal("fixture_read"))?;
    actual.archive.verify(deadline())?;
    Ok(TestAggregate { bytes })
}

#[test]
fn signer_acquires_aggregate_by_id_with_full_context_without_assembly_request() {
    let api = SignerApi::new();
    let actual = invoke(&api, 303, &digest()).expect("authenticated aggregate");
    assert_eq!(sha256(&actual.bytes), digest());
    assert_eq!(actual.bytes.len(), 16);
    assert_eq!(actual.bytes, b"closed ZIP bytes");
    assert_eq!(api.authentications.get(), 2);
    assert_eq!(api.downloads.get(), 1);
}

#[test]
fn aggregate_locator_digest_old_attempt_and_wrong_metadata_fail_before_download() {
    for mutate in [
        |a: &mut ArtifactRecord| a.name = a.name.replace("-a2", "-a1"),
        |a: &mut ArtifactRecord| a.name = record().name,
        |a: &mut ArtifactRecord| a.expired = true,
        |a: &mut ArtifactRecord| a.digest = None,
        |a: &mut ArtifactRecord| a.digest = Some(format!("sha256:{}", "b".repeat(64))),
        |a: &mut ArtifactRecord| a.workflow_run.as_mut().expect("run").head_repository_id = 102,
        |a: &mut ArtifactRecord| a.size_in_bytes = ZipProfile::PreviewAggregate.compressed() + 1,
    ] {
        let api = SignerApi::new();
        mutate(api.artifact.borrow_mut().as_mut().expect("record"));
        assert!(invoke(&api, 303, &digest()).is_err());
        assert_eq!(api.downloads.get(), 0);
    }
    for (id, digest) in [
        (0, digest()),
        (303, "B".repeat(64)),
        (303, "b".repeat(63)),
        (304, digest()),
    ] {
        let api = SignerApi::new();
        assert!(invoke(&api, id, &digest).is_err());
        assert_eq!(api.downloads.get(), 0);
    }
}

#[test]
fn aggregate_corruption_or_final_current_attempt_change_returns_no_admitted_bytes() {
    let mut corrupt = SignerApi::new();
    corrupt.bytes = b"different bytes!".to_vec();
    assert!(invoke(&corrupt, 303, &digest()).is_err());
    let mut stale = SignerApi::new();
    stale.fail_after_download = true;
    assert!(invoke(&stale, 303, &digest()).is_err());
    assert_eq!(stale.authentications.get(), 2);
}
