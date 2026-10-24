//! Retained promotion has a closed default-branch selection and no publish route.
use super::*;
use velnor_actions_contract::{ExecutionMode, ScaleSetSelector, config::ValidatedImageReference};
#[test]
fn reconcile_has_only_four_bounded_jobs_and_no_release_mutation_route() {
    let rendered = render_release_tap_reconcile(&with_tap(), &hosted_policy()).expect("render");
    assert_eq!(rendered.path, ".github/workflows/preview-tap-reconcile.yml");
    let text = &rendered.bytes;
    assert!(text.contains("name: Preview tap reconciliation"), "{text}");
    assert!(
        text.contains("cron: '*/5 * * * *'") || text.contains("cron: \"*/5 * * * *\""),
        "{text}"
    );
    assert!(text.contains("workflow_dispatch:"));
    assert!(text.contains("native-preview-tap-reconcile-${{ github.repository }}"));
    assert!(!text.contains("group: native-preview-${{ github.repository }}"));
    let original = render_consumer_preview(&with_tap(), &hosted_policy()).expect("publisher");
    assert!(
        original
            .bytes
            .contains("group: native-preview-${{ github.repository }}")
    );

    assert_eq!(text.matches("timeout-minutes: 5").count(), 4);
    for id in [
        "preview-tap-selection",
        "preview-tap-prepare",
        "preview-tap-writer",
        "preview-tap-merge",
    ] {
        assert!(text.contains(&format!("  {id}:")), "{id}");
    }
    for forbidden in [
        "workflow_run:",
        "inputs:",
        "recover-package-release-v1",
        "sign-package-release-v1",
        "publish-package-release-v1",
        "execute-release-producer-v1",
        "release-signed-",
        "permission-actions",
        "actions: write",
        "attestations: write",
        "id-token: write",
        "cargo ",
    ] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
}
#[test]
fn reconcile_selection_and_updater_never_receive_tap_credentials() {
    let rendered = render_release_tap_reconcile(&with_tap(), &hosted_policy()).expect("render");
    let text = &rendered.bytes;
    let selection = job(text, "preview-tap-selection", Some("preview-tap-prepare"));
    assert!(selection.contains("select-release-tap-continuation-v1"));
    assert!(selection.contains("EXPECTED_SOURCE_SHA: ${{ github.sha }}"));
    assert!(selection.contains("steps.history.outputs.commit_count"));
    for part in [
        selection,
        job(text, "preview-tap-prepare", Some("preview-tap-writer")),
    ] {
        let reader = "${{ secrets.RELEASE_STATUS_READ_TOKEN }}";
        assert!(part.contains(reader));
        assert!(part.contains("VELNOR_RELEASE_STATUS_TOKEN:"));
        assert!(!part.replace(reader, "").contains("secrets."));
        assert!(!part.contains("permission-contents: write"));
        assert!(!part.contains("VELNOR_TAP_TOKEN"));
        assert!(!part.contains("VELNOR_TAP_MERGE_TOKEN"));
        assert!(!part.contains("VELNOR_TAP_PROTECTION_TOKEN"));
    }
    let prepare = job(text, "preview-tap-prepare", Some("preview-tap-writer"));
    assert!(prepare.contains("needs.preview-tap-selection.outputs.eligible == 'true'"));
    assert!(prepare.contains("ref: ${{ needs.preview-tap-selection.outputs.source_sha }}"));
}
fn hosted_policy() -> crate::workflow_routing::WorkflowRoutingPolicy {
    crate::workflow_routing::WorkflowRoutingPolicy::hosted_only()
}

fn scale_set_policy() -> crate::workflow_routing::WorkflowRoutingPolicy {
    crate::workflow_routing::WorkflowRoutingPolicy::configured(
        ExecutionMode::ScaleSet,
        ScaleSetSelector::try_new(
            "linux-amd64",
            &["velnor".to_owned(), "linux-amd64".to_owned()],
        )
        .expect("valid selector"),
        Some(
            ValidatedImageReference::parse(
                "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .expect("valid immutable test image"),
        ),
    )
}

#[test]
fn reconcile_routes_every_job_to_the_configured_scale_set() {
    let rendered = render_release_tap_reconcile(&with_tap(), &scale_set_policy())
        .expect("render routed reconcile workflow");
    let text = rendered.bytes;
    let count = text.matches("runs-on:").count();
    assert_eq!(count, 4);
    assert_eq!(
        text.matches("runs-on: [velnor, linux-amd64]").count(),
        count
    );
    let image = "container:\n      image: ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert_eq!(text.matches(image).count(), count);
    assert!(!text.contains("runs-on: ubuntu-26.04"));
}

#[test]
fn reconcile_native_operations_bind_original_tuple_from_selection_not_dispatch_inputs() {
    let spec = with_tap();
    let tap = spec.homebrew_tap.as_ref().expect("tap");
    let jobs = tap::jobs(&spec, tap, TapInvocationContext::Reconcile).expect("jobs");
    for (_, job) in &jobs {
        let text = crate::yaml::render_yaml(job);
        for field in [
            "commit_count",
            "ci_run_id",
            "ci_attempt",
            "original_run_id",
            "original_run_attempt",
        ] {
            assert!(
                text.contains(&format!("needs.preview-tap-selection.outputs.{field}")),
                "{text}"
            );
        }
        assert!(!text.contains("inputs."));
        assert!(!text.contains("needs.preview-eligibility"));
    }
    let current = tap::jobs(&spec, tap, TapInvocationContext::Preview).expect("original");
    assert!(
        crate::yaml::render_yaml(&current[0].1)
            .contains("needs.preview-publisher.result == 'success'")
    );
    assert!(crate::yaml::render_yaml(&current[0].1).contains("inputs.original_run_id"));
    assert!(render_release_tap_reconcile(&fixture(), &hosted_policy()).is_err());
}
