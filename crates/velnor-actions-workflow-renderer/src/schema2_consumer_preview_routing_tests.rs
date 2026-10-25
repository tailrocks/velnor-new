use super::*;

#[test]
fn scale_set_routes_every_preview_and_tap_job_and_keeps_optional_image_policy() {
    let spec = with_tap();
    let hosted = render_consumer_preview(&spec, &hosted_policy()).expect("hosted preview");
    let policy = scale_set_policy(Some(fixture_image()));
    let scale_set = render_consumer_preview(&spec, &policy).expect("Scale Set preview");
    let job_count = scale_set.bytes.matches("runs-on:").count();
    assert_ne!(job_count, 0);
    assert_eq!(
        scale_set
            .bytes
            .matches("runs-on: [velnor, linux-amd64]")
            .count(),
        job_count
    );
    let image = "container:\n      image: ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert_eq!(scale_set.bytes.matches(image).count(), job_count);
    assert!(!hosted.bytes.contains(image));
    assert!(
        scale_set
            .bytes
            .contains("VELNOR_RELEASE_REQUIRE_LINUX_AMD64: \"true\"")
    );
    crate::workflow_routing::validate_generated_workflow_calls(
        std::slice::from_ref(&scale_set),
        &policy,
    )
    .expect("parsed output validates every routed preview and tap job");
}

#[test]
fn scale_set_without_global_image_stays_containerless_and_routes_native_preview_jobs() {
    let policy = scale_set_policy(None);
    let routed =
        render_consumer_preview(&fixture(), &policy).expect("selector-only Scale Set preview");
    let job_count = routed.bytes.matches("runs-on:").count();
    assert_ne!(job_count, 0);
    assert_eq!(
        routed
            .bytes
            .matches("runs-on: [velnor, linux-amd64]")
            .count(),
        job_count
    );
    assert!(!routed.bytes.contains("container:"));
    crate::workflow_routing::validate_generated_workflow_calls(
        std::slice::from_ref(&routed),
        &policy,
    )
    .expect("containerless selector-only output remains valid");
}
