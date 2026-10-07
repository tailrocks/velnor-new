use super::*;
use velnor_actions_contract_workflow::StepKind;

#[test]
fn provider_restore_composite_binds_exact_key_and_owned_path() {
    let key = tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", "stacks/vpc")
        .expect("key builds");
    let path = tofu_provider_cache_path("stacks/vpc").expect("path builds");
    assert_eq!(
        TOFU_PROVIDER_CACHE_BASE_EXPR,
        velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDER_CACHE_BASE_EXPR,
        "one base across crates"
    );
    assert!(
        path.starts_with(&format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/stacks-vpc-")),
        "{path}"
    );
    let step = tofu_providers_restore_step(&key, &path).expect("restore builds");
    assert_eq!(step.name, "Restore Tofu providers");
    assert!(step.condition.is_none(), "restores carry no gate");
    let StepKind::Action { uses, with, .. } = &step.kind else {
        panic!("restore must be an action step");
    };
    assert_eq!(
        uses,
        velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDER_ADMISSION_USES
    );
    assert_eq!(
        with.get("cache-key").map(String::as_str),
        Some(key.as_str()),
        "the composite receives the configured key once"
    );
    assert_eq!(
        with.get("cache-path").map(String::as_str),
        Some(path.as_str()),
        "the composite receives only the owned plugin-cache path"
    );
    let script = velnor_actions_workflow_cache::tofu_cache::TOFU_PROVIDER_ADMISSION_SCRIPT;
    for expected in [
        "[ \"$TOFU_CACHE_HIT\" = true ]",
        "[ \"$TOFU_MATCHED_KEY\" = \"$TOFU_EXPECTED_KEY\" ]",
        "\"$TOFU_PROVIDER_CACHE_PATH\" = \"$d\"",
        "rm -rf \"$d\"",
        "mkdir -m 700 \"$d\"",
    ] {
        assert!(
            script.contains(expected),
            "script lacks {expected:?}: {script}"
        );
    }
    assert!(!script.contains("TF_DATA_DIR"), "data dir is untouched");
}

#[test]
fn provider_key_shape_binds_target_tofu_root_and_lock() {
    let key = tofu_providers_cache_key("x86_64-unknown-linux-gnu", "1.13.1", "")
        .expect("provider key builds");
    assert!(
        key.starts_with("velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-root-"),
        "{key}"
    );
    assert!(
        key.ends_with("${{hashFiles('.terraform.lock.hcl')}}"),
        "{key}"
    );
    assert!(
        key.len() <= velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES,
        "{key}"
    );
}
