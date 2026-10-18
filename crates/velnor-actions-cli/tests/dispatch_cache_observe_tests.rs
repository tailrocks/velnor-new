use super::{
    ACTION_HIT_ENV, ACTION_MATCHED_KEY_ENV, ActionOutputs, output_body, read_action_outputs,
    workspace_path,
};
use std::collections::BTreeMap;
use std::env;
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation,
};
use velnor_actions_orchestrator::QualificationCacheBindingOutputs;

fn action_values(hit: &str, key: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (ACTION_HIT_ENV.to_owned(), hit.to_owned()),
        (ACTION_MATCHED_KEY_ENV.to_owned(), key.to_owned()),
    ])
}

fn read_action(
    values: &BTreeMap<String, String>,
    restore_requested: bool,
) -> Result<ActionOutputs, &'static str> {
    read_action_outputs(
        |name| values.get(name).cloned().ok_or(env::VarError::NotPresent),
        restore_requested,
    )
}

#[test]
fn action_outputs_accept_only_canonical_values_and_bounded_keys() {
    assert_eq!(
        read_action(&action_values("true", "exact-key"), true),
        Ok(ActionOutputs {
            hit: true,
            matched_key: Some("exact-key".to_owned()),
        })
    );
    assert_eq!(
        read_action(&BTreeMap::new(), false),
        Ok(ActionOutputs {
            hit: false,
            matched_key: None,
        })
    );
    assert_eq!(
        read_action(&BTreeMap::new(), true),
        Err("qualification_cache_action_hit_invalid")
    );
    assert_eq!(
        read_action(&action_values("TRUE", ""), true),
        Err("qualification_cache_action_hit_invalid")
    );
    assert_eq!(
        read_action(&action_values("false", "bad\nkey"), true),
        Err("qualification_cache_action_key_invalid")
    );
    assert_eq!(
        read_action(
            &action_values(
                "true",
                &"k".repeat(velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES + 1),
            ),
            true,
        ),
        Err("qualification_cache_action_key_invalid")
    );
}

#[test]
fn workspace_path_rejects_traversal_and_nonabsolute_values() {
    assert_eq!(
        workspace_path(Ok("/runner/work/project".to_owned())),
        Ok(std::path::PathBuf::from("/runner/work/project"))
    );
    assert_eq!(
        workspace_path(Ok("/runner/work/../project".to_owned())),
        Err("github_workspace_path_invalid")
    );
    assert_eq!(
        workspace_path(Ok("relative/path".to_owned())),
        Err("github_workspace_path_invalid")
    );
}

#[test]
fn output_body_records_actual_backend_object_and_approval_last() {
    let entry = QualificationCacheBackendEntry {
        id: 91,
        key: "exact-key".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        version: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
        size_bytes: 17,
    };
    let binding = QualificationCacheBindingOutputs {
        active: true,
        campaign: "campaign-1".to_owned(),
        phase: "warm".to_owned(),
        plan_id: "plan-1".to_owned(),
        run_key: "r700-a1".to_owned(),
        matrix_key: "matrix-a".to_owned(),
        layer: "tools".to_owned(),
        restore_key: Some("exact-key".to_owned()),
        expected_cache: Some(entry.clone()),
        save_key: None,
        save_if_state_changes: false,
        expected_prior_state_digest: None,
        runtime_identity_digest: Some("runtime-digest".to_owned()),
    };
    let body = output_body(
        &binding,
        &ActionOutputs {
            hit: true,
            matched_key: Some("exact-key".to_owned()),
        },
        &QualificationCacheBackendObservation::Found(entry),
    )
    .expect("output body");
    assert!(body.lines().any(|line| line == "backend_cache_id=91"));
    assert!(body.lines().any(|line| line == "backend_cache_size=17"));
    assert_eq!(body.lines().last(), Some("backend_admitted=true"));
}
