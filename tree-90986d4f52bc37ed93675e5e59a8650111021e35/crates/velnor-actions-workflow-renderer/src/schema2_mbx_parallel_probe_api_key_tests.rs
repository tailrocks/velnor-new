use std::fs;

use super::{
    NEW_SCOPE_HASH, OTHER_SHA, SHARED_SCOPE_HASH, SOURCE_SHA, fixture_data, prepare_api_fixture,
    run_api_script,
};

#[test]
fn exact_run_attempt_and_sha_are_required_for_each_cold_primary() {
    for mutation in [
        KeyMutation::StaleSeed,
        KeyMutation::StaleWriter,
        KeyMutation::WrongSeedSha,
        KeyMutation::WrongWriterSha,
        KeyMutation::NearMatchSharedScopeHash,
        KeyMutation::MalformedSeedScope,
    ] {
        let root = fixture_data::TestDirectory::new();
        let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, true);
        mutate_fixture_receipt(&runner_temp, mutation);
        let output = run_api_script(&root.0, &runner_temp, &jobs_path);
        assert!(
            !output.status.success(),
            "API script accepted key mutation {mutation:?}"
        );
    }
}

#[derive(Debug, Clone, Copy)]
enum KeyMutation {
    StaleSeed,
    StaleWriter,
    WrongSeedSha,
    WrongWriterSha,
    NearMatchSharedScopeHash,
    MalformedSeedScope,
}

fn mutate_fixture_receipt(runner_temp: &std::path::Path, mutation: KeyMutation) {
    let input = runner_temp.join("mbx-parallel-input");
    let shared_prefix = super::cache_prefix(SHARED_SCOPE_HASH, "123", "2");
    let shared_key = format!("{shared_prefix}{SOURCE_SHA}");
    let new_prefix = super::cache_prefix(NEW_SCOPE_HASH, "123", "2");
    let new_key = format!("{new_prefix}{SOURCE_SHA}");
    match mutation {
        KeyMutation::StaleSeed => {
            let stale_prefix = super::cache_prefix(SHARED_SCOPE_HASH, "122", "2");
            let stale_key = format!("{stale_prefix}{SOURCE_SHA}");
            let receipt = input.join("seed/cache-receipt.json");
            replace_receipt_field(&receipt, "cache_prefix", &shared_prefix, &stale_prefix);
            replace_receipt_field(&receipt, "primary_key", &shared_key, &stale_key);
            replace_receipt_field(&receipt, "derived_primary_key", &shared_key, &stale_key);
            replace_receipt_field(&receipt, "restore_primary_key", &shared_key, &stale_key);
        }
        KeyMutation::StaleWriter => {
            let stale_prefix = super::cache_prefix(NEW_SCOPE_HASH, "122", "2");
            let stale_key = format!("{stale_prefix}{SOURCE_SHA}");
            let receipt = input.join("new-key-writer/cache-receipt.json");
            replace_receipt_field(&receipt, "cache_prefix", &new_prefix, &stale_prefix);
            replace_receipt_field(&receipt, "primary_key", &new_key, &stale_key);
            replace_receipt_field(&receipt, "derived_primary_key", &new_key, &stale_key);
            replace_receipt_field(&receipt, "restore_primary_key", &new_key, &stale_key);
        }
        KeyMutation::WrongSeedSha => {
            let wrong_key = format!("{shared_prefix}{OTHER_SHA}");
            let receipt = input.join("seed/cache-receipt.json");
            replace_receipt_field(&receipt, "primary_key", &shared_key, &wrong_key);
            replace_receipt_field(&receipt, "derived_primary_key", &shared_key, &wrong_key);
            replace_receipt_field(&receipt, "restore_primary_key", &shared_key, &wrong_key);
        }
        KeyMutation::WrongWriterSha => {
            let wrong_key = format!("{new_prefix}{OTHER_SHA}");
            let receipt = input.join("new-key-writer/cache-receipt.json");
            replace_receipt_field(&receipt, "primary_key", &new_key, &wrong_key);
            replace_receipt_field(&receipt, "derived_primary_key", &new_key, &wrong_key);
            replace_receipt_field(&receipt, "restore_primary_key", &new_key, &wrong_key);
        }
        KeyMutation::NearMatchSharedScopeHash => {
            let near_hash = format!("{}3", &SHARED_SCOPE_HASH[..SHARED_SCOPE_HASH.len() - 1]);
            let near_prefix = super::cache_prefix(&near_hash, "123", "2");
            let near_key = format!("{near_prefix}{SOURCE_SHA}");
            let receipt = input.join("seed/cache-receipt.json");
            replace_receipt_field(&receipt, "cache_prefix", &shared_prefix, &near_prefix);
            replace_receipt_field(&receipt, "primary_key", &shared_key, &near_key);
            replace_receipt_field(&receipt, "derived_primary_key", &shared_key, &near_key);
            replace_receipt_field(&receipt, "restore_primary_key", &shared_key, &near_key);
        }
        KeyMutation::MalformedSeedScope => replace_receipt_field(
            &input.join("seed/cache-receipt.json"),
            "scope",
            "qualification-mbx-v1/parallel/shared",
            "qualification-mbx-v1/parallel/shared-near",
        ),
    }
}

fn replace_receipt_field(path: &std::path::Path, field: &str, old: &str, new: &str) {
    let contents = fs::read_to_string(path).expect("read receipt fixture for mutation");
    let old_field = format!("\"{field}\":\"{old}\"");
    let new_field = format!("\"{field}\":\"{new}\"");
    assert_eq!(contents.matches(&old_field).count(), 1);
    fs::write(path, contents.replace(&old_field, &new_field))
        .expect("write mutated receipt fixture");
}
