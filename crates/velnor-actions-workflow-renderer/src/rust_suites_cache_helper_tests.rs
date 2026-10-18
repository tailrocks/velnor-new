use std::collections::BTreeMap;

use velnor_actions_contract::RustSuiteTrigger;

use super::{TestResult, minimal_suite, sample_spec};
use crate::workflow_routing::WorkflowRoutingPolicy;

pub(super) fn verified_acquire_step() -> velnor_actions_contract::Step {
    let helper = format!("{}0.1.0", crate::steps::STAGED_BINARY_PREFIX);
    crate::steps::acquire_velnor_step(
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl -fsSL \"$VELNOR_ASSET_URL\" -o {helper} && echo \"$VELNOR_ASSET_SHA256  {helper}\" | sha256sum -c - && chmod +x {helper}"
            ),
        ],
        &BTreeMap::from([
            (
                crate::steps::ASSET_URL_ENV.to_owned(),
                "https://example.invalid/velnor".to_owned(),
            ),
            (crate::steps::ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
            (crate::steps::RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
        ]),
    )
    .expect("digest-verified helper step")
}

#[test]
fn standalone_suite_without_verified_helper_keeps_setup_and_omits_cache() -> TestResult {
    let mut suite = minimal_suite("nightly", vec![RustSuiteTrigger::Schedule]);
    suite.schedule = vec!["0 4 * * *".to_owned()];
    let spec = sample_spec(vec![suite]);
    let files = super::render_suites_workflows(&spec)?;
    let yaml = &files[0].bytes;
    assert!(yaml.contains("uses: jdx/mise-action@"));
    assert!(!yaml.contains("uses: actions/cache/restore@"));
    assert!(!yaml.contains("name: Save Mise tools"));
    Ok(())
}

#[test]
fn scale_set_routes_standalone_scheduled_linux_suites() -> TestResult {
    let mut suite = minimal_suite("nightly", vec![RustSuiteTrigger::Schedule]);
    suite.schedule = vec!["0 4 * * *".to_owned()];
    let spec = sample_spec(vec![suite]);
    let policy = WorkflowRoutingPolicy::ScaleSet(
        crate::schema2::Schema2WorkflowRequest::canonical_scale_set()?,
    );
    let helper_steps = BTreeMap::from([("ubuntu-24.04".to_owned(), verified_acquire_step())]);
    let files =
        super::render_suites_workflows_with_routing_and_helpers(&spec, &policy, &helper_steps)?;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, ".github/workflows/suites.yml");
    assert!(
        files[0]
            .bytes
            .contains("runs-on: [velnor, ubuntu-26.04-scale-set]")
    );
    assert!(!files[0].bytes.contains("needs.plan."));
    assert!(
        files[0]
            .bytes
            .contains("if: github.event_name != 'workflow_dispatch'")
    );
    assert!(!files[0].bytes.contains("runs-on: ubuntu-24.04"));
    Ok(())
}
