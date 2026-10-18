use serde_json::json;
use sha2::{Digest, Sha256};

use crate::journal::{ImageImportRequest, ImageImportState, Journal};
use crate::launch_harness::Scratch;

use super::super::super::super::identity::SelectedDaemon;
use super::super::super::super::release::PreparedWorkerArtifact;
use super::super::super::{ProviderFailure, import_request, import_worker_artifact};
use super::docker::{ImportDockerStub, ImportReply, Reply};

pub(super) const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const UPLOAD_BYTES: usize = 384 * 1024;
pub(super) const BACKPRESSURE_BYTES: usize = 12 * 1024 * 1024;

pub(super) struct Fixture {
    _scratch: Scratch,
    pub(super) journal: Journal,
    pub(super) selected: SelectedDaemon,
    pub(super) artifact: PreparedWorkerArtifact,
    pub(super) request: ImageImportRequest,
    pub(super) bytes: Vec<u8>,
}

impl Fixture {
    pub(super) async fn new(label: &str, byte_count: usize) -> Result<Self, String> {
        let (scratch, journal) = super::super::journal(label).await?;
        journal
            .bind_engine("selected-engine")
            .await
            .map_err(|error| error.to_string())?;
        let selected = super::super::selected_daemon();
        let mut artifact = super::super::artifact();
        let bytes = vec![0x5a; byte_count];
        let path = scratch.file().with_file_name("verified-worker-load.tar");
        std::fs::write(&path, &bytes).map_err(|error| error.to_string())?;
        artifact.load.path = path;
        artifact.load.sha256 = hex(&Sha256::digest(&bytes));
        let request = import_request(&selected, SOURCE_SHA, &artifact)
            .map_err(|error| format!("image import identity invalid: {error:?}"))?;
        Ok(Self {
            _scratch: scratch,
            journal,
            selected,
            artifact,
            request,
            bytes,
        })
    }
}

pub(super) fn pre_import_replies() -> Vec<Reply> {
    vec![
        image_missing(),
        engine_info("selected-engine"),
        engine_info("selected-engine"),
        image_missing(),
        engine_info("selected-engine"),
    ]
}

pub(super) fn post_import_replies(artifact: &PreparedWorkerArtifact) -> Vec<Reply> {
    post_import_replies_with_image(super::super::image_inspect(artifact, None))
}

pub(super) fn post_import_replies_with_image(image: String) -> Vec<Reply> {
    vec![
        engine_info("selected-engine"),
        Reply::http(200, image.into_bytes()),
        engine_info("selected-engine"),
    ]
}

pub(super) fn import_reply(fixture: &Fixture, mode: ImportReply) -> Reply {
    Reply::import(mode, &fixture.journal, &fixture.request)
}

pub(super) fn image_missing() -> Reply {
    Reply::http(404, br#"{"message":"image missing"}"#.to_vec())
}

pub(super) fn engine_info(engine_id: &str) -> Reply {
    Reply::http(
        200,
        json!({
            "ID": engine_id,
            "OSType": "linux",
            "Architecture": "aarch64",
            "DockerRootDir": "/var/lib/docker"
        })
        .to_string()
        .into_bytes(),
    )
}

pub(super) async fn assert_uncertain_without_receipt(
    journal: &Journal,
    request: &ImageImportRequest,
) -> Result<(), String> {
    let record = journal
        .prepare_image_import(request.clone())
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(record.state, ImageImportState::Uncertain);
    assert!(record.receipt.is_none());
    Ok(())
}

pub(super) async fn assert_replay_posts_nothing(fixture: &Fixture) -> Result<(), String> {
    let stub = ImportDockerStub::open(Vec::new())?;
    let result = import_worker_artifact(
        &stub.docker,
        &fixture.journal,
        &fixture.selected,
        SOURCE_SHA,
        &fixture.artifact,
    )
    .await;
    let requests = stub.finish().await?;
    assert!(matches!(result, Err(ProviderFailure::ImageLoad)));
    assert!(requests.is_empty());
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}
