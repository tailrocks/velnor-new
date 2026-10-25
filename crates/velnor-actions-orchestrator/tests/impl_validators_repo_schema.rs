//! Live repository config sample schema coverage.

use super::{TestResult, repo_sample_text};

#[test]
fn repo_config_sample_covers_schema_keys() -> TestResult {
    let sample = repo_sample_text()?;
    let table: toml::Table = toml::from_str(&sample)?;
    for key in table.keys() {
        assert!(
            [
                "schema",
                "workflow",
                "resources",
                "test_sharding",
                "stacks",
                "discovery",
                "actions",
                "execution",
                "checks",
                "qualified_tools",
                "dco",
            ]
            .contains(&key.as_str()),
            "sample key outside schema: {key}"
        );
    }
    let section_keys = |section: &str| -> Vec<String> {
        table
            .get(section)
            .and_then(toml::Value::as_table)
            .map(|values| values.keys().cloned().collect())
            .unwrap_or_default()
    };
    for key in section_keys("workflow") {
        assert!(
            [
                "name",
                "policy",
                "default_branch",
                "generator_validation",
                "max_parallel_jobs",
                "crate_timeout_minutes",
                "plan_timeout_minutes",
                "pull_request_cache_policy",
                "runner_label",
                "lint",
            ]
            .contains(&key.as_str()),
            "workflow key outside schema: {key}"
        );
    }
    for key in section_keys("discovery") {
        assert_eq!(key, "exclude", "discovery key outside schema: {key}");
    }
    for key in section_keys("execution") {
        assert!(
            [
                "default_profile",
                "mode",
                "hosted_profile",
                "scale_set_profile",
                "profiles",
                "parity",
                "overrides",
                "workflows",
            ]
            .contains(&key.as_str()),
            "execution key outside schema: {key}"
        );
    }
    assert_sample_mentions(&sample);
    Ok(())
}

/// Tokens the live schema-2 sample must still spell.
fn assert_sample_mentions(sample: &str) {
    for token in [
        "schema",
        "[workflow]",
        "name",
        "policy",
        "default_branch",
        "generator_validation",
        "max_parallel_jobs",
        "pull_request_cache_policy",
        "[resources]",
        "compiler_process_budget",
        "test_process_budget",
        "[test_sharding]",
        "default_shards",
        "by_manifest",
        "[stacks]",
        "ignore",
        "[stacks.rust]",
        "configurations",
        "[discovery]",
        "exclude",
        "[actions.overrides]",
        "[execution]",
        "default_profile",
        "hosted_profile",
        "scale_set_profile",
        "workflows",
    ] {
        assert!(
            sample.contains(token),
            "schema key missing from sample: {token}"
        );
    }
}
