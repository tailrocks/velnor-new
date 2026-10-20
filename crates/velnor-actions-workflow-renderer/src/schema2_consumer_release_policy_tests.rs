//! Public generation policy cannot infer mutable publication authority from URLs.
use super::*;

fn hosted_policy() -> crate::workflow_routing::WorkflowRoutingPolicy {
    crate::workflow_routing::WorkflowRoutingPolicy::hosted_only()
}
#[test]
fn preview_requires_explicit_existing_release_authority() {
    let mut spec = fixture();
    spec.preview = None;
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
    let mut spec = fixture();
    spec.preview.as_mut().expect("preview").release_id = 0;
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
    let mut spec = fixture();
    spec.preview
        .as_mut()
        .expect("preview")
        .bootstrap_snapshot_sha256 = Some("A".repeat(64));
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
    let mut spec = fixture();
    spec.preview
        .as_mut()
        .expect("preview")
        .bootstrap_snapshot_sha256 = Some("b".repeat(64));
    let step = steps::operation(
        &spec,
        "publish-package-release-v1",
        true,
        TapInvocationContext::Preview,
    )
    .expect("fixed rolling policy");
    let Yaml::Map(fields) = step else {
        panic!("operation map");
    };
    let Some((_, Yaml::Map(env))) = fields.iter().find(|(key, _)| key == "env") else {
        panic!("operation environment");
    };
    assert!(env.contains(&("VELNOR_PREVIEW_RELEASE_ID".to_owned(), Yaml::str("123"))));
    assert!(env.contains(&(
        "VELNOR_PREVIEW_BOOTSTRAP_SNAPSHOT_SHA256".to_owned(),
        Yaml::str("b".repeat(64))
    )));
    let yaml = render_consumer_preview(&spec, &hosted_policy())
        .expect("fixed rolling workflow")
        .bytes;
    assert!(
        !job(&yaml, "preview-publisher", Some("preview-recovery")).contains("actions/checkout")
    );
}

#[test]
fn preview_rejects_unbounded_producer_limits_before_rendering() {
    for minutes in [0, 21, 30, 360, 361, u16::MAX] {
        let mut spec = fixture();
        spec.producer_timeout_minutes = minutes;
        assert!(
            render_consumer_preview(&spec, &hosted_policy()).is_err(),
            "{minutes}"
        );
    }
}

#[test]
fn twenty_minute_producer_jobs_leave_control_signer_publication_and_updater_at_five() {
    let mut spec = with_tap();
    spec.producer_timeout_minutes = 20;
    let pairs = spec.policy.payload_pairs().expect("full plan").len();
    let rendered =
        render_consumer_preview(&spec, &hosted_policy()).expect("explicit twenty-minute producer");
    assert_eq!(
        rendered.bytes.matches("timeout-minutes: 20").count(),
        pairs + 1,
        "only each payload and the assembly job use the producer budget"
    );
    assert!(rendered.bytes.contains("timeout-minutes: 5"));
    for minutes in [1, 5, 6, 10, 15, 20] {
        spec.producer_timeout_minutes = minutes;
        assert!(
            render_consumer_preview(&spec, &hosted_policy()).is_ok(),
            "{minutes}"
        );
    }
    spec.homebrew_tap
        .as_mut()
        .expect("tap")
        .policy
        .updater
        .timeout_minutes = 6;
    assert!(
        render_consumer_preview(&spec, &hosted_policy()).is_err(),
        "producer authorization does not widen updater"
    );
}
