//! Preview tap job isolation and privileged credential regressions.

use super::*;

fn with_custom_tap() -> ConsumerReleaseSpec {
    use velnor_actions_contract::config::{
        CheckExecutor, CheckPlatform, CheckRunner, MiseTaskSource, TapUpdaterBinding,
    };
    let mut spec = with_tap();
    let tap = spec.homebrew_tap.as_mut().expect("tap");
    tap.formula_spec = None;
    tap.policy.updater = Some(TapUpdaterBinding {
        task: "tap:update".to_owned(),
        source: MiseTaskSource {
            mise_config: "mise.toml".to_owned(),
            working_directory: ".".to_owned(),
        },
        runner: CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec![
            "mise.toml".to_owned(),
            "Formula/scanner-preview.rb".to_owned(),
        ],
        tools: vec!["ruby".to_owned()],
        timeout_minutes: 4,
    });
    spec
}

#[test]
fn tap_jobs_separate_unprivileged_preparation_and_scoped_app_authority() {
    let rendered = render_consumer_preview(&with_tap(), &hosted_policy()).expect("tap workflow");
    let yaml = &rendered.bytes;
    let prepare = job(yaml, "preview-tap-prepare", Some("preview-tap-writer"));
    assert!(prepare.contains("actions/checkout"));
    assert!(!prepare.contains("environment:"));
    assert!(!prepare.contains("create-github-app-token"));
    assert!(!prepare.contains("${{ secrets.TAP_APP_PRIVATE_KEY }}"));
    assert!(!prepare.contains("\n      contents: write\n"));
    assert!(prepare.contains("needs.preview-recovery.result == 'success'"));
    assert!(prepare.contains("needs.preview-publisher.result == 'success'"));
    for (id, next) in [
        ("preview-tap-writer", Some("preview-tap-merge")),
        ("preview-tap-merge", None),
    ] {
        let privileged = job(yaml, id, next);
        assert!(!privileged.contains("actions/checkout"));
        assert!(!privileged.contains("id-token:"));
        assert!(!privileged.contains("\n      contents: write\n"));
        assert!(privileged.contains("environment: tap-update"));
        assert!(privileged.contains("owner: example"));
        assert!(privileged.contains("repositories: homebrew-tap"));
        assert!(privileged.contains("skip-token-revoke: false"));
        assert!(privileged.contains("${{ secrets.RELEASE_STATUS_READ_TOKEN }}"));
        assert!(privileged.contains("VELNOR_RELEASE_ORIGINAL_RUN_ID:"));
    }
    let writer = job(yaml, "preview-tap-writer", Some("preview-tap-merge"));
    assert!(!writer.contains("permission-administration"));
    assert!(writer.contains("VELNOR_TAP_TOKEN:"));
    assert!(!writer.contains("VELNOR_TAP_MERGE_TOKEN:"));
    let merge = job(yaml, "preview-tap-merge", None);
    assert!(merge.contains("permission-administration: read"));
    assert!(!merge.contains("permission-checks: read"));
    let tap_token = merge
        .split("id: tap-token")
        .nth(1)
        .expect("tap token")
        .split("id: tap-protection-token")
        .next()
        .expect("tap token boundary");
    assert!(!tap_token.contains("permission-checks: read"));
    assert!(merge.contains("VELNOR_TAP_PROTECTION_TOKEN"));
    assert!(!writer.contains("VELNOR_TAP_PROTECTION_"));
    assert!(!writer.contains("${{ secrets.TAP_PROTECTION_APP_PRIVATE_KEY }}"));
    assert!(!prepare.contains("VELNOR_TAP_PROTECTION_"));
    assert!(!prepare.contains("${{ secrets.TAP_PROTECTION_APP_PRIVATE_KEY }}"));
    let observer = merge
        .split("id: tap-protection-token")
        .nth(1)
        .expect("observer mint");
    assert!(observer.contains("permission-administration: read"));
    assert!(merge.contains("VELNOR_TAP_MERGE_TOKEN:"));
    assert!(!merge.contains("VELNOR_TAP_TOKEN:"));
    assert!(merge.contains("VELNOR_TAP_COMPLETION_APP_ID:"));
    assert!(merge.contains("VELNOR_TAP_REVIEWER_APP_ID_0:"));
}

#[test]
fn credential_references_cannot_become_privileged_environment_keys() {
    let mut spec = with_tap();
    spec.homebrew_tap
        .as_mut()
        .expect("tap")
        .policy
        .review_completion
        .app_id_variable = "PATH".to_owned();
    let yaml = render_consumer_preview(&spec, &hosted_policy())
        .expect("literal variable reference")
        .bytes;
    let merge = job(&yaml, "preview-tap-merge", None);
    assert!(merge.contains("VELNOR_TAP_COMPLETION_APP_ID: ${{ vars.PATH }}"));
    assert!(!merge.contains("\n          PATH:"));
}

#[test]
fn tap_updater_rejects_self_hosted_execution_without_native_provisioning() {
    let mut spec = with_custom_tap();
    let runner = &mut spec
        .homebrew_tap
        .as_mut()
        .expect("tap")
        .policy
        .updater
        .as_mut()
        .expect("custom updater")
        .runner;
    runner.executor = velnor_actions_contract::config::CheckExecutor::EphemeralSelfHosted;
    runner.label = "tap-updater".to_owned();
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
}

#[test]
fn custom_tap_preparation_keeps_its_declared_runner_timeout_and_no_native_spec() {
    let yaml = render_consumer_preview(&with_custom_tap(), &hosted_policy())
        .expect("custom preparation")
        .bytes;
    let prepare = job(&yaml, "preview-tap-prepare", Some("preview-tap-writer"));
    assert!(prepare.contains("runs-on: ubuntu-24.04"));
    assert!(prepare.contains("timeout-minutes: 4"));
    assert!(prepare.contains("prepare-homebrew-tap-v1"));
    assert!(!prepare.contains("VELNOR_HOMEBREW_FORMULA_SPEC"));
}

#[test]
fn native_tap_preparation_has_fixed_hosted_execution() {
    let yaml = render_consumer_preview(&with_tap(), &hosted_policy())
        .expect("native preparation")
        .bytes;
    let prepare = job(&yaml, "preview-tap-prepare", Some("preview-tap-writer"));
    assert!(prepare.contains("runs-on: ubuntu-26.04"));
    assert!(prepare.contains("prepare-homebrew-tap-v1"));
    assert!(prepare.contains("VELNOR_HOMEBREW_FORMULA_SPEC"));
    assert!(!prepare.contains("package:update-preview"));
}
