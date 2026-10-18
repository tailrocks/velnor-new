use std::time::Duration;

use crate::journal::ImageImportState;
use serde_json::Value;

use super::super::super::{ProviderFailure, import_worker_artifact};
use super::docker::{ImportDockerStub, ImportReply};
use super::support::{
    BACKPRESSURE_BYTES, Fixture, SOURCE_SHA, UPLOAD_BYTES, assert_replay_posts_nothing,
    assert_uncertain_without_receipt, engine_info, image_missing, import_reply,
    post_import_replies_with_image, pre_import_replies,
};

#[tokio::test]
async fn early_http_response_keeps_claim_uncertain_and_replay_does_not_post() -> Result<(), String>
{
    let fixture = Fixture::new("load-early-response", BACKPRESSURE_BYTES).await?;
    let mut replies = pre_import_replies();
    replies.push(import_reply(
        &fixture,
        ImportReply::Early {
            status: 200,
            body: br#"{"status":"Loaded"}"#.to_vec(),
        },
    ));
    let stub = ImportDockerStub::open(replies)?;

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
    assert_eq!(requests.len(), 6);
    assert!(!requests[5].body_complete);
    assert!(requests[5].body.len() < fixture.bytes.len());
    assert_eq!(
        requests[5].state_at_import,
        Some(ImageImportState::Requested)
    );
    assert_uncertain_without_receipt(&fixture.journal, &fixture.request).await?;
    assert_replay_posts_nothing(&fixture).await
}

#[tokio::test]
async fn truncated_upload_keeps_claim_uncertain_and_replay_does_not_post() -> Result<(), String> {
    let fixture = Fixture::new("load-truncated", BACKPRESSURE_BYTES).await?;
    let mut replies = pre_import_replies();
    replies.push(import_reply(&fixture, ImportReply::Truncated));
    let stub = ImportDockerStub::open(replies)?;

    let result = import_worker_artifact(
        &stub.docker,
        &fixture.journal,
        &fixture.selected,
        SOURCE_SHA,
        &fixture.artifact,
    )
    .await;
    let requests = stub.finish().await?;

    assert!(result.is_err());
    assert_eq!(requests.len(), 6);
    assert!(!requests[5].body_complete);
    assert_ne!(requests[5].body.len(), 0);
    assert!(requests[5].body.len() < fixture.bytes.len());
    assert_eq!(
        requests[5].state_at_import,
        Some(ImageImportState::Requested)
    );
    assert_uncertain_without_receipt(&fixture.journal, &fixture.request).await?;
    assert_replay_posts_nothing(&fixture).await
}

#[tokio::test]
async fn connection_loss_after_complete_upload_keeps_claim_uncertain() -> Result<(), String> {
    let fixture = Fixture::new("load-connection-loss", UPLOAD_BYTES).await?;
    let mut replies = pre_import_replies();
    replies.push(import_reply(&fixture, ImportReply::DisconnectAfterBody));
    let stub = ImportDockerStub::open(replies)?;

    let result = import_worker_artifact(
        &stub.docker,
        &fixture.journal,
        &fixture.selected,
        SOURCE_SHA,
        &fixture.artifact,
    )
    .await;
    let requests = stub.finish().await?;

    assert!(result.is_err());
    assert_eq!(requests.len(), 6);
    assert!(requests[5].body_complete);
    assert_eq!(requests[5].body, fixture.bytes);
    assert_eq!(
        requests[5].state_at_import,
        Some(ImageImportState::Requested)
    );
    assert_uncertain_without_receipt(&fixture.journal, &fixture.request).await?;
    assert_replay_posts_nothing(&fixture).await
}

#[tokio::test]
async fn docker_status_and_stream_errors_keep_claim_uncertain() -> Result<(), String> {
    for (label, status, body) in [
        (
            "load-http-error",
            500,
            br#"{"message":"image import rejected"}"#.to_vec(),
        ),
        (
            "load-stream-error",
            200,
            b"{\"errorDetail\":{\"message\":\"image import rejected\"}}\n".to_vec(),
        ),
    ] {
        let fixture = Fixture::new(label, UPLOAD_BYTES).await?;
        let mut replies = pre_import_replies();
        replies.push(import_reply(
            &fixture,
            ImportReply::Complete { status, body },
        ));
        let stub = ImportDockerStub::open(replies)?;

        let result = import_worker_artifact(
            &stub.docker,
            &fixture.journal,
            &fixture.selected,
            SOURCE_SHA,
            &fixture.artifact,
        )
        .await;
        let requests = stub.finish().await?;

        assert!(result.is_err(), "{label} unexpectedly produced proof");
        assert_eq!(requests.len(), 6);
        assert!(requests[5].body_complete);
        assert_eq!(
            requests[5].state_at_import,
            Some(ImageImportState::Requested)
        );
        assert_uncertain_without_receipt(&fixture.journal, &fixture.request).await?;
        assert_replay_posts_nothing(&fixture).await?;
    }
    Ok(())
}

#[tokio::test]
async fn timed_out_import_keeps_claim_uncertain_and_replay_does_not_post() -> Result<(), String> {
    let fixture = Fixture::new("load-timeout", BACKPRESSURE_BYTES).await?;
    let mut replies = pre_import_replies();
    replies.push(import_reply(&fixture, ImportReply::Hang));
    let stub = ImportDockerStub::open(replies)?;

    let result = super::super::super::import_worker_artifact_with_timeout(
        &stub.docker,
        &fixture.journal,
        &fixture.selected,
        SOURCE_SHA,
        &fixture.artifact,
        Duration::from_millis(40),
    )
    .await;
    let requests = stub.finish_observed().await?;

    assert!(matches!(result, Err(ProviderFailure::ImageLoad)));
    assert_eq!(requests.len(), 6);
    assert_eq!(
        requests[5].state_at_import,
        Some(ImageImportState::Requested)
    );
    assert_uncertain_without_receipt(&fixture.journal, &fixture.request).await?;
    assert_replay_posts_nothing(&fixture).await
}

#[tokio::test]
async fn changed_engine_after_claim_stops_before_image_load() -> Result<(), String> {
    let fixture = Fixture::new("load-engine-before", UPLOAD_BYTES).await?;
    let replies = vec![
        image_missing(),
        engine_info("selected-engine"),
        engine_info("changed-engine"),
    ];
    let stub = ImportDockerStub::open(replies)?;

    let result = import_worker_artifact(
        &stub.docker,
        &fixture.journal,
        &fixture.selected,
        SOURCE_SHA,
        &fixture.artifact,
    )
    .await;
    let requests = stub.finish().await?;

    assert!(matches!(result, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| {
        !(request.line.contains("POST ") && request.line.contains("/images/load"))
    }));
    assert_uncertain_without_receipt(&fixture.journal, &fixture.request).await?;
    assert_replay_posts_nothing(&fixture).await
}

#[tokio::test]
async fn changed_engine_after_image_load_stops_before_receipt() -> Result<(), String> {
    let fixture = Fixture::new("load-engine-after", UPLOAD_BYTES).await?;
    let mut replies = pre_import_replies();
    replies.push(import_reply(
        &fixture,
        ImportReply::Complete {
            status: 200,
            body: br#"{"status":"Loaded"}"#.to_vec(),
        },
    ));
    replies.push(engine_info("changed-engine"));
    let stub = ImportDockerStub::open(replies)?;

    let result = import_worker_artifact(
        &stub.docker,
        &fixture.journal,
        &fixture.selected,
        SOURCE_SHA,
        &fixture.artifact,
    )
    .await;
    let requests = stub.finish().await?;

    assert!(matches!(result, Err(ProviderFailure::ImageIdentity)));
    assert_eq!(requests.len(), 7);
    assert!(
        requests[5].line.contains("POST ") && requests[5].line.contains("/images/load"),
        "unexpected import request: {}",
        requests[5].line
    );
    assert!(requests[5].body_complete);
    assert!(requests[6].line.ends_with("/info HTTP/1.1"));
    assert_uncertain_without_receipt(&fixture.journal, &fixture.request).await?;
    assert_replay_posts_nothing(&fixture).await
}

#[tokio::test]
async fn streamed_image_config_rootfs_and_store_identity_are_checked() -> Result<(), String> {
    for label in ["loaded-config", "loaded-rootfs", "loaded-store"] {
        let fixture = Fixture::new(label, UPLOAD_BYTES).await?;
        let inspect = match label {
            "loaded-config" => super::super::image_inspect(&fixture.artifact, Some("wrong-user")),
            "loaded-rootfs" => super::super::image_inspect_with_layers(
                &fixture.artifact,
                &["sha256:wrong-layer".to_owned()],
                None,
            ),
            "loaded-store" => {
                let mut inspection: Value =
                    serde_json::from_str(&super::super::image_inspect(&fixture.artifact, None))
                        .map_err(|error| error.to_string())?;
                inspection["Descriptor"]["digest"] =
                    Value::String(fixture.artifact.original.manifest_digest.clone());
                serde_json::to_string(&inspection).map_err(|error| error.to_string())?
            }
            _ => return Err("unknown loaded image fixture".to_owned()),
        };
        let mut replies = pre_import_replies();
        replies.push(import_reply(
            &fixture,
            ImportReply::Complete {
                status: 200,
                body: b"{\"status\":\"Loaded\"}\n".to_vec(),
            },
        ));
        replies.extend(post_import_replies_with_image(inspect));
        let stub = ImportDockerStub::open(replies)?;

        let result = import_worker_artifact(
            &stub.docker,
            &fixture.journal,
            &fixture.selected,
            SOURCE_SHA,
            &fixture.artifact,
        )
        .await;
        let requests = stub.finish_observed().await?;

        assert!(result.is_err(), "{label} unexpectedly produced proof");
        assert!(requests[5].body_complete);
        assert_eq!(requests[5].body, fixture.bytes);
        assert_eq!(
            requests[5].state_at_import,
            Some(ImageImportState::Requested)
        );
        assert_uncertain_without_receipt(&fixture.journal, &fixture.request).await?;
        assert_replay_posts_nothing(&fixture).await?;
    }
    Ok(())
}

#[tokio::test]
async fn requested_and_uncertain_replays_send_no_docker_requests() -> Result<(), String> {
    for state in [ImageImportState::Requested, ImageImportState::Uncertain] {
        let fixture = Fixture::new("load-replay", 16).await?;
        let record = fixture
            .journal
            .prepare_image_import(fixture.request.clone())
            .await
            .map_err(|error| error.to_string())?;
        let claim = fixture
            .journal
            .claim_image_import_effect(record.id, &fixture.request)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "fresh image import claim missing".to_owned())?;
        if state == ImageImportState::Uncertain {
            fixture
                .journal
                .mark_image_import_uncertain(claim)
                .await
                .map_err(|error| error.to_string())?;
        } else {
            drop(claim);
        }
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
        let record = fixture
            .journal
            .prepare_image_import(fixture.request.clone())
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(record.state, state);
        assert!(record.receipt.is_none());
    }
    Ok(())
}
