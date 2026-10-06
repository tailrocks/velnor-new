use super::super::super::GuestResourceSample;
use super::super::identity::trusted_image_id;
use super::super::{GuestSampleFailure, GuestSampleStatus, ProbeImage};
use super::fake::{RESOLVED_IMAGE, image};

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
    let snapshot = super::super::GuestSampleSnapshot::completed(
        Err(GuestSampleFailure::ProbeOutput),
        Instant::now(),
    );
    assert_eq!(
        snapshot.status,
        GuestSampleStatus::Unavailable(GuestSampleFailure::ProbeOutput)
    );
    assert_eq!(snapshot.sample, GuestResourceSample::default());
}
use std::time::Instant;
