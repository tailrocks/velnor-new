//! V1 reusable-callee contract and identity regressions.

use serde_json::{Value, json};
use velnor_actions_contract_workflow::workflow::permissions::PermissionLevel;
use velnor_actions_contract_workflow::workflow::reusable_callee::{
    REUSABLE_CALLEE_EVENT, REUSABLE_CALLEE_INPUT_TYPE, REUSABLE_CALLEE_INPUTS,
    REUSABLE_CALLER_PERMISSIONS, ReusableCalleeContract, ReusableCalleeContractError,
    ReusableCalleeIdentity, ReusableCalleeIdentityError, ReusableCalleePolicy,
    ReusableCalleePolicyError, exact_workflow_path, schema_value,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn policy() -> ReusableCalleePolicy {
    ReusableCalleePolicy {
        caller_repository: "tailrocks/example-caller".to_owned(),
        caller_workflow_path: ".github/workflows/release.yml".to_owned(),
        caller_branch: "main".to_owned(),
        callee_repository: "tailrocks/example-callee".to_owned(),
    }
}

fn identity<'a>() -> ReusableCalleeIdentity<'a> {
    ReusableCalleeIdentity {
        caller_repository: "tailrocks/example-caller",
        event_name: "push",
        caller_ref: "refs/heads/main",
        caller_workflow_ref: "tailrocks/example-caller/.github/workflows/release.yml@refs/heads/main",
        caller_workflow_sha: SHA,
        source_sha: SHA,
        callee_repository: "tailrocks/example-callee",
    }
}

fn schema() -> Value {
    Value::Object(schema_value())
}

#[test]
fn exact_contract_has_six_required_string_mappings() {
    let got = schema();
    assert_eq!(
        ReusableCalleeContract::from_schema(&got),
        Ok(ReusableCalleeContract)
    );
    assert_eq!(got["inputs"].as_object().map(serde_json::Map::len), Some(6));
    for input in REUSABLE_CALLEE_INPUTS {
        assert_eq!(
            got["inputs"][input.name]["type"],
            Value::String(REUSABLE_CALLEE_INPUT_TYPE.to_owned())
        );
        assert_eq!(got["inputs"][input.name]["required"], Value::Bool(true));
        assert!(got["inputs"][input.name].get("default").is_none());
    }
    let expressions: [&str; 6] = [
        "github.repository",
        "github.event_name",
        "github.ref",
        "github.sha",
        "github.run_id",
        "github.run_attempt",
    ];
    for (input, expression) in REUSABLE_CALLEE_INPUTS.iter().zip(expressions) {
        assert_eq!(input.expression, expression);
    }
}

#[test]
fn contract_rejects_schema_changes_and_defaults() {
    let mut missing = schema();
    missing["inputs"]
        .as_object_mut()
        .and_then(|inputs| inputs.remove("run_id"));
    assert_eq!(
        ReusableCalleeContract::from_schema(&missing),
        Err(ReusableCalleeContractError::MissingInput("run_id"))
    );
    for (name, change) in [
        ("repository", json!({"type": "number", "required": true})),
        ("event_name", json!({"type": "string", "required": false})),
        (
            "ref",
            json!({"type": "string", "required": true, "default": ""}),
        ),
        ("sha", json!({"type": "string"})),
        ("run_id", json!({"type": "integer", "required": true})),
        (
            "run_attempt",
            json!({"type": "string", "required": true, "default": null}),
        ),
    ] {
        let mut bad = schema();
        if let Some(inputs) = bad["inputs"].as_object_mut() {
            inputs.insert(name.to_owned(), change);
        }
        assert_eq!(
            ReusableCalleeContract::from_schema(&bad),
            Err(ReusableCalleeContractError::InvalidInput(name_static(name)))
        );
    }
    let mut unknown = schema();
    if let Some(inputs) = unknown["inputs"].as_object_mut() {
        inputs.insert(
            "observer_id".to_owned(),
            json!({"type": "string", "required": true}),
        );
    }
    assert_eq!(
        ReusableCalleeContract::from_schema(&unknown),
        Err(ReusableCalleeContractError::UnknownInput(
            "observer_id".to_owned(),
        ))
    );
    let mut root = schema();
    if let Some(root_object) = root.as_object_mut() {
        root_object.insert("observer".to_owned(), Value::Bool(true));
    }
    assert_eq!(
        ReusableCalleeContract::from_schema(&root),
        Err(ReusableCalleeContractError::Root)
    );
    assert_eq!(
        ReusableCalleeContract::from_schema(&Value::Array(Vec::new())),
        Err(ReusableCalleeContractError::Root)
    );
}

#[test]
fn contract_rejects_every_omitted_input() {
    for input in REUSABLE_CALLEE_INPUTS {
        let mut bad = schema();
        let missing = bad["inputs"]
            .as_object_mut()
            .and_then(|inputs| inputs.remove(input.name));
        assert!(missing.is_some());
        assert_eq!(
            ReusableCalleeContract::from_schema(&bad),
            Err(ReusableCalleeContractError::MissingInput(input.name))
        );
    }
}

#[test]
fn contract_rejects_false_required_for_every_input() {
    for input in REUSABLE_CALLEE_INPUTS {
        let mut bad = schema();
        bad["inputs"][input.name]["required"] = Value::Bool(false);
        assert_eq!(
            ReusableCalleeContract::from_schema(&bad),
            Err(ReusableCalleeContractError::InvalidInput(input.name))
        );
    }
}

#[test]
fn caller_permissions_are_exact_closed_map() {
    let expected = [
        ("actions", PermissionLevel::Read),
        ("contents", PermissionLevel::Write),
        ("pull-requests", PermissionLevel::None),
        ("id-token", PermissionLevel::None),
    ];
    assert_eq!(REUSABLE_CALLER_PERMISSIONS, expected);
    assert_eq!(REUSABLE_CALLER_PERMISSIONS.len(), 4);
    for (index, (scope, _)) in REUSABLE_CALLER_PERMISSIONS.iter().enumerate() {
        assert!(
            REUSABLE_CALLER_PERMISSIONS[index + 1..]
                .iter()
                .all(|(other_scope, _)| other_scope != scope)
        );
    }
}

#[test]
fn caller_permissions_reject_scope_or_level_mutations() {
    for index in 0..REUSABLE_CALLER_PERMISSIONS.len() {
        let mut bad_scope = REUSABLE_CALLER_PERMISSIONS;
        bad_scope[index].0 = "observer";
        assert_ne!(bad_scope, REUSABLE_CALLER_PERMISSIONS);

        let levels = [
            PermissionLevel::Read,
            PermissionLevel::Write,
            PermissionLevel::None,
        ];
        for level in levels {
            let mut bad_level = REUSABLE_CALLER_PERMISSIONS;
            if bad_level[index].1 == level {
                continue;
            }
            bad_level[index].1 = level;
            assert_ne!(bad_level, REUSABLE_CALLER_PERMISSIONS);
        }
    }
}

fn name_static(name: &str) -> &'static str {
    for input in REUSABLE_CALLEE_INPUTS {
        if input.name == name {
            return input.name;
        }
    }
    REUSABLE_CALLEE_INPUTS[0].name
}

#[test]
fn identity_accepts_only_exact_dynamic_push_context() {
    assert!(exact_workflow_path(".github/workflows/release.yml"));
    assert_eq!(
        policy().caller_workflow_ref().as_deref(),
        Ok("tailrocks/example-caller/.github/workflows/release.yml@refs/heads/main")
    );
    assert_eq!(policy().validate_identity(&identity()), Ok(()));
    assert_eq!(REUSABLE_CALLEE_EVENT, "push");
}

#[test]
fn workflow_path_accepts_only_the_exact_trusted_shape() {
    for path in [
        ".github/workflows/release.yml",
        ".github/workflows/release.yaml",
        ".github/workflows/ci.yml",
    ] {
        assert!(exact_workflow_path(path), "{path:?}");
    }
}

#[test]
fn workflow_path_rejects_every_untrusted_segment_class() {
    for path in [
        "",
        ".",
        "..",
        ".github",
        ".github/workflows",
        ".github/workflows/",
        ".github/workflows/release.yml/",
        ".github//workflows/release.yml",
        ".github/workflows//release.yml",
        ".github/workflows/nested/release.yml",
        ".github/workflows/release.yml/nested.yml",
        ".github/workflows/./release.yml",
        ".github/workflows/../release.yml",
        ".github/workflows/releases/../release.yml",
        ".github/workflows/.release.yml",
        ".github/workflows/..yml",
        ".github/workflows/.yaml",
        ".github/workflows/release.txt",
        ".github/workflows/release",
        ".github/workflows/release.YML",
        ".github/workflows/release'.yml",
        ".github/workflows/release\".yml",
        ".github/workflows/${{",
        ".github/workflows/}}",
        ".github/workflows/${{ inputs.file }}.yml",
        ".github/workflows/release${{ inputs.file }}.yml",
        ".github/workflows/release;.yml",
        ".github/workflows/release\\.yml",
        ".github\\workflows\\release.yml",
        ".github/workflows/release yml",
        ".github/workflows/rélease.yml",
        ".github/workflows/release\n.yml",
        ".github/workflows/release\t.yml",
    ] {
        assert!(!exact_workflow_path(path), "{path:?}");
    }
}

#[test]
fn identity_rejects_every_guard_mismatch() {
    for (expected, mut bad) in [
        (ReusableCalleeIdentityError::CallerRepository, identity()),
        (ReusableCalleeIdentityError::EventName, identity()),
        (ReusableCalleeIdentityError::CallerRef, identity()),
        (ReusableCalleeIdentityError::CallerWorkflowRef, identity()),
        (ReusableCalleeIdentityError::WorkflowSourceSha, identity()),
        (ReusableCalleeIdentityError::CalleeRepository, identity()),
    ] {
        match expected {
            ReusableCalleeIdentityError::CallerRepository => bad.caller_repository = "other/repo",
            ReusableCalleeIdentityError::EventName => bad.event_name = "workflow_dispatch",
            ReusableCalleeIdentityError::CallerRef => bad.caller_ref = "refs/heads/release",
            ReusableCalleeIdentityError::CallerWorkflowRef => bad.caller_workflow_ref = "other/ref",
            ReusableCalleeIdentityError::WorkflowSourceSha => bad.source_sha = "different-source",
            ReusableCalleeIdentityError::CalleeRepository => bad.callee_repository = "other/repo",
        }
        assert_eq!(policy().validate_identity(&bad), Err(expected));
    }
    let mut missing_sha = identity();
    missing_sha.caller_workflow_sha = "";
    assert_eq!(
        policy().validate_identity(&missing_sha),
        Err(ReusableCalleeIdentityError::WorkflowSourceSha)
    );
}

#[test]
fn ambiguous_policy_disables_trusted_reference() {
    assert_eq!(
        policy().caller_workflow_ref().as_deref(),
        Ok("tailrocks/example-caller/.github/workflows/release.yml@refs/heads/main")
    );
    for (expected, field) in [
        (ReusableCalleePolicyError::Repository, "repository"),
        (ReusableCalleePolicyError::WorkflowPath, "workflow_path"),
        (ReusableCalleePolicyError::Branch, "branch"),
    ] {
        let mut bad = policy();
        match field {
            "repository" => bad.caller_repository.clear(),
            "workflow_path" => bad.caller_workflow_path.clear(),
            _ => bad.caller_branch.clear(),
        }
        assert_eq!(bad.caller_workflow_ref(), Err(expected));
        assert_eq!(
            bad.validate_identity(&identity()),
            Err(match field {
                "repository" => ReusableCalleeIdentityError::CallerRepository,
                "workflow_path" => ReusableCalleeIdentityError::CallerWorkflowRef,
                _ => ReusableCalleeIdentityError::CallerRef,
            })
        );
    }
}
