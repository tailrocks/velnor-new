//! Opaque tool selectors gain cache-key authority only from compiled invocations.

use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, Job, SourceBoundHelper, SourceBoundOperation, Step,
};

const TARGET: &str = "x86_64-unknown-linux-gnu";
const MISE_VERSION: &str = "2026.9.18";
const OPAQUE_SELECTOR: &str = "https://example.invalid/tool?raw=%2F%2f&channel=stable";

fn job(steps: Vec<Step>) -> Job {
    serde_json::from_value(serde_json::json!({
        "display_name": "Opaque selector key",
        "runs_on": "ubuntu-24.04",
        "timeout_minutes": 10,
        "needs": [],
        "steps": steps
    }))
    .expect("job fixture")
}

fn helper(args: Vec<String>, selectors: Vec<String>) -> (Step, CompiledSourceHelper) {
    let source =
        velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source fixture");
    let operation = SourceBoundOperation::MiseToolPrepare;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    let invocation = HelperInvocation::compiled(descriptor, args, selectors).expect("invocation");
    let record = CompiledSourceHelper::compiled(invocation, source)
        .expect("compiled helper")
        .with_environment(BTreeMap::new());
    let step =
        crate::source_helper::source_helper_step("Qualified opaque tool", &record, BTreeMap::new())
            .expect("helper step");
    (step, record)
}

#[test]
fn naked_and_shell_opaque_urls_cannot_become_selectors() {
    assert!(
        super::mise_cache_key_for_tools(TARGET, MISE_VERSION, &[OPAQUE_SELECTOR.to_owned()])
            .is_err()
    );
    for argv in [
        vec!["printf".to_owned(), OPAQUE_SELECTOR.to_owned()],
        vec![
            "mise".to_owned(),
            "install".to_owned(),
            OPAQUE_SELECTOR.to_owned(),
        ],
    ] {
        let step = crate::steps::ambient_shell_step("Opaque shell text", argv, BTreeMap::new())
            .expect("shell fixture");
        assert!(
            super::mise_cache_key_for_job(TARGET, MISE_VERSION, &job(vec![step]), &[]).is_err()
        );
    }
}

#[test]
fn only_installed_selector_membership_grants_opaque_authority() {
    let (selected, record) = helper(
        vec!["tool-prepare".to_owned()],
        vec![OPAQUE_SELECTOR.to_owned()],
    );
    let key = super::mise_cache_key_for_job(
        TARGET,
        MISE_VERSION,
        &job(vec![selected]),
        std::slice::from_ref(&record),
    )
    .expect("compiled selector key");
    let expected = format!(
        "mise-v3-{TARGET}-{MISE_VERSION}-{}",
        super::tools_digest(&[OPAQUE_SELECTOR.to_owned()])
    );
    assert_eq!(key, expected);

    let (args_only, args_record) = helper(vec![OPAQUE_SELECTOR.to_owned()], Vec::new());
    assert!(
        super::mise_cache_key_for_job(
            TARGET,
            MISE_VERSION,
            &job(vec![args_only]),
            std::slice::from_ref(&args_record),
        )
        .is_err()
    );
    let (unregistered, _) = helper(
        vec!["tool-prepare".to_owned()],
        vec![OPAQUE_SELECTOR.to_owned()],
    );
    assert!(
        super::mise_cache_key_for_job(TARGET, MISE_VERSION, &job(vec![unregistered]), &[]).is_err()
    );
}
