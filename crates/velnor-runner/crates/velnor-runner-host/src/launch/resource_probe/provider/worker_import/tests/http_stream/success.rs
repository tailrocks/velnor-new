use crate::journal::ImageImportState;

use super::super::super::import_worker_artifact;
use super::docker::{ImportDockerStub, ImportReply};
use super::support::{
    Fixture, SOURCE_SHA, UPLOAD_BYTES, import_reply, post_import_replies, pre_import_replies,
};

#[tokio::test]
async fn production_load_stream_completes_only_after_full_upload_and_inspection()
-> Result<(), String> {
    let fixture = Fixture::new("load-success", UPLOAD_BYTES).await?;
    let mut replies = pre_import_replies();
    replies.push(import_reply(
        &fixture,
        ImportReply::Complete {
            status: 200,
            body: b"{\"status\":\"Loaded\"}\n".to_vec(),
        },
    ));
    replies.extend(post_import_replies(&fixture.artifact));
    let stub = ImportDockerStub::open(replies)?;

    let proof = import_worker_artifact(
        &stub.docker,
        &fixture.journal,
        &fixture.selected,
        SOURCE_SHA,
        &fixture.artifact,
    )
    .await
    .map_err(|error| format!("production image import failed: {error:?}"))?;
    let requests = stub.finish().await?;

    assert_eq!(requests.len(), 9);
    assert!(
        requests[5].line.contains("POST ") && requests[5].line.contains("/images/load"),
        "unexpected import request: {}",
        requests[5].line
    );
    assert_eq!(requests[5].body, fixture.bytes);
    assert!(requests[5].body_complete);
    assert_eq!(
        requests[5].state_at_import,
        Some(ImageImportState::Requested)
    );
    assert_eq!(
        proof.runtime_id(),
        fixture.artifact.original.image_index_digest
    );
    let record = fixture
        .journal
        .prepare_image_import(fixture.request.clone())
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(record.state, ImageImportState::Completed);
    assert!(record.receipt.is_some());
    Ok(())
}
