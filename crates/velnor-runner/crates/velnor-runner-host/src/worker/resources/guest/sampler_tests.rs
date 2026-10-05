#[path = "sampler_tests/fake.rs"]
mod fake;

use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use super::super::GuestResourceSample;
use super::identity::{IMAGE_REFERENCE, trusted_image_id};
use super::{GuestSampleFailure, GuestSampleStatus, ProbeImage};
use fake::{
    FakeClient, REPLACEMENT_IMAGE, RESOLVED_IMAGE, fast_sampler, identity, image, owned_container,
    unavailable,
};

#[tokio::test]
async fn sample_uses_resolved_image_id_after_the_tag_moves() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.replacement_image_after_lookup = Some(image(REPLACEMENT_IMAGE));

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    let state = client.state.lock().await;
    let expected_name = super::identity::container_name(&identity);

    assert_eq!(snapshot.status, GuestSampleStatus::Available);
    assert_eq!(snapshot.sample.cpu_millicores, Some(4000));
    assert_eq!(state.image_reference.as_deref(), Some(IMAGE_REFERENCE));
    assert_eq!(state.created_image.as_deref(), Some(RESOLVED_IMAGE));
    assert_eq!(state.created_name.as_deref(), Some(expected_name.as_str()));
    assert_eq!(state.remove_count, 1);
    assert!(state.container.is_none());
    Ok(())
}

#[tokio::test]
async fn absent_or_failed_image_inspection_never_creates_a_probe() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    for fail_with_missing_image in [true, false] {
        let client = Arc::new(FakeClient::new());
        {
            let mut state = client.state.lock().await;
            if fail_with_missing_image {
                state.image = None;
            } else {
                state.image_error = true;
            }
        }
        let snapshot = fast_sampler(Arc::clone(&client), &identity)
            .sample_once()
            .await;
        let expected = if fail_with_missing_image {
            GuestSampleFailure::ProbeImage
        } else {
            GuestSampleFailure::Docker
        };
        assert!(unavailable(snapshot, expected));
        assert_eq!(client.state.lock().await.create_count, 0);
    }
    Ok(())
}

#[tokio::test]
async fn image_inspection_timeout_has_no_fallback_or_create() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.image_hangs = true;

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;

    assert!(unavailable(snapshot, GuestSampleFailure::Timeout));
    assert_eq!(client.state.lock().await.create_count, 0);
    Ok(())
}

#[tokio::test]
async fn daemon_info_timeout_stops_before_image_or_container_operations()
-> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.info_hangs = true;

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    let state = client.state.lock().await;

    assert!(unavailable(snapshot, GuestSampleFailure::Timeout));
    assert!(state.image_reference.is_none());
    assert_eq!(state.create_count, 0);
    assert_eq!(state.remove_count, 0);
    Ok(())
}

#[tokio::test]
async fn wrong_selected_engine_stops_before_image_or_container_operations()
-> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.wrong_engine = true;

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    let state = client.state.lock().await;

    assert!(unavailable(snapshot, GuestSampleFailure::EngineIdentity));
    assert!(state.image_reference.is_none());
    assert_eq!(state.create_count, 0);
    assert_eq!(state.remove_count, 0);
    Ok(())
}

#[tokio::test]
async fn create_timeout_and_lost_response_reconcile_only_owned_probe() -> Result<(), Box<dyn Error>>
{
    let identity = identity()?;
    for hang in [true, false] {
        let client = Arc::new(FakeClient::new());
        {
            let mut state = client.state.lock().await;
            state.create_hangs = hang;
            state.create_loses_response = !hang;
        }
        let snapshot = fast_sampler(Arc::clone(&client), &identity)
            .sample_once()
            .await;
        assert!(unavailable(snapshot, GuestSampleFailure::ProbeCreate));
        let state = client.state.lock().await;
        assert_eq!(state.create_count, 1);
        assert_eq!(state.remove_count, 1);
        assert!(state.container.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn wrong_owner_labels_are_never_removed_or_replaced() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    let mut foreign = owned_container(&identity);
    foreign.labels = HashMap::from([(
        "velnor.guest-resource.owner".to_owned(),
        "foreign".to_owned(),
    )]);
    client.seed_container(foreign.clone()).await;

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    let state = client.state.lock().await;

    assert!(unavailable(snapshot, GuestSampleFailure::Ownership));
    assert_eq!(state.create_count, 0);
    assert_eq!(state.remove_count, 0);
    assert_eq!(state.container.as_ref(), Some(&foreign));
    Ok(())
}

#[tokio::test]
async fn malformed_and_oversized_output_fail_closed_and_cleanup() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    for (output, reason) in [
        (vec![0xff], GuestSampleFailure::ProbeOutput),
        (vec![b'x'; 513], GuestSampleFailure::OutputLimit),
    ] {
        let client = Arc::new(FakeClient::new());
        client.state.lock().await.output = output;

        let snapshot = fast_sampler(Arc::clone(&client), &identity)
            .sample_once()
            .await;
        assert!(unavailable(snapshot, reason));
        let state = client.state.lock().await;
        assert_eq!(state.start_count, 1);
        assert_eq!(state.remove_count, 1);
        assert!(state.container.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn output_read_timeout_fails_closed_and_cleans_probe() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.output_hangs = true;

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    let state = client.state.lock().await;

    assert!(unavailable(snapshot, GuestSampleFailure::Timeout));
    assert_eq!(state.start_count, 1);
    assert_eq!(state.remove_count, 1);
    assert!(state.container.is_none());
    Ok(())
}

#[tokio::test]
async fn start_timeout_and_unconfirmed_cleanup_remain_unknown() -> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.start_hangs = true;

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    assert!(unavailable(snapshot, GuestSampleFailure::Timeout));
    assert!(client.state.lock().await.container.is_none());

    let client = Arc::new(FakeClient::new());
    client.state.lock().await.remove_keeps_container = true;
    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    assert!(unavailable(snapshot, GuestSampleFailure::Cleanup));
    assert!(client.state.lock().await.container.is_some());
    Ok(())
}

#[tokio::test]
async fn remove_timeout_is_bounded_and_unconfirmed_probe_stays_unknown()
-> Result<(), Box<dyn Error>> {
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.state.lock().await.remove_hangs = true;

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    let state = client.state.lock().await;

    assert!(unavailable(snapshot, GuestSampleFailure::Cleanup));
    assert_eq!(state.remove_count, 2);
    assert!(state.container.is_some());
    Ok(())
}

#[tokio::test]
async fn restart_cleans_prior_owned_probe_before_creating_a_new_one() -> Result<(), Box<dyn Error>>
{
    let identity = identity()?;
    let client = Arc::new(FakeClient::new());
    client.seed_container(owned_container(&identity)).await;

    let snapshot = fast_sampler(Arc::clone(&client), &identity)
        .sample_once()
        .await;
    let state = client.state.lock().await;

    assert_eq!(snapshot.status, GuestSampleStatus::Available);
    assert_eq!(state.create_count, 1);
    assert_eq!(state.remove_count, 2);
    assert!(state.container.is_none());
    Ok(())
}

#[test]
fn image_identity_requires_exact_tag_digest_and_platform() {
    assert_eq!(
        trusted_image_id(&image(RESOLVED_IMAGE)),
        Ok(RESOLVED_IMAGE.to_owned())
    );
    for invalid in [
        ProbeImage {
            id: None,
            ..image(RESOLVED_IMAGE)
        },
        image("sha256:bad"),
        ProbeImage {
            repo_tags: vec!["untrusted:tag".to_owned()],
            ..image(RESOLVED_IMAGE)
        },
        ProbeImage {
            os: Some("darwin".to_owned()),
            ..image(RESOLVED_IMAGE)
        },
        ProbeImage {
            architecture: Some("arm64".to_owned()),
            ..image(RESOLVED_IMAGE)
        },
    ] {
        assert_eq!(
            trusted_image_id(&invalid),
            Err(GuestSampleFailure::ProbeImage)
        );
    }
}

#[test]
fn unusable_samples_never_carry_capacity() {
    let snapshot = super::GuestSampleSnapshot::completed(Err(GuestSampleFailure::ProbeOutput));
    assert_eq!(
        snapshot.status,
        GuestSampleStatus::Unavailable(GuestSampleFailure::ProbeOutput)
    );
    assert_eq!(snapshot.sample, GuestResourceSample::default());
}
