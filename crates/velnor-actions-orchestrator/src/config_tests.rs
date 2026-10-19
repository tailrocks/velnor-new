//! Pull-request cache policy parsing across both supported config schemas.

use std::error::Error;

use super::{CONFIG_REL, PartialConfig};
use velnor_actions_contract::{PullRequestCachePolicy, VelnorConfig};

#[test]
fn pull_request_cache_policy_defaults_and_opts_in_for_both_schemas() -> Result<(), Box<dyn Error>> {
    for schema in [VelnorConfig::SCHEMA, VelnorConfig::SCHEMA_V2] {
        let default = parse_config(&source(schema, None))?;
        assert_eq!(
            default.workflow.pull_request_cache_policy,
            PullRequestCachePolicy::ReadOnly,
            "schema {schema} default"
        );

        let opted_in = parse_config(&source(schema, Some("same-repository-scoped")))?;
        assert_eq!(
            opted_in.workflow.pull_request_cache_policy,
            PullRequestCachePolicy::SameRepositoryScoped,
            "schema {schema} opt-in"
        );
    }
    Ok(())
}

#[test]
fn pull_request_cache_policy_rejects_invalid_values_for_both_schemas() {
    for schema in [VelnorConfig::SCHEMA, VelnorConfig::SCHEMA_V2] {
        assert!(
            parse_config(&source(schema, Some("same-repo"))).is_err(),
            "schema {schema} accepted an invalid policy"
        );
    }
}

fn parse_config(source: &str) -> Result<VelnorConfig, Box<dyn Error>> {
    let partial: PartialConfig = toml::from_str(source)?;
    let config = partial.materialize()?;
    config.validate(CONFIG_REL)?;
    Ok(config)
}

#[test]
fn mise_quiet_defaults_on_and_follows_explicit_values() -> Result<(), Box<dyn Error>> {
    let default = parse_config(&mise_quiet_source(None))?;
    assert!(
        default.workflow.mise_quiet,
        "absent mise_quiet stays loud-silent"
    );

    for (wire, expected) in [("false", false), ("true", true)] {
        let config = parse_config(&mise_quiet_source(Some(wire)))?;
        assert_eq!(config.workflow.mise_quiet, expected, "mise_quiet = {wire}");
    }
    Ok(())
}

#[test]
fn mise_quiet_rejects_non_boolean_with_structured_config_error() {
    for wire in ["\"yes\"", "1"] {
        let err = toml::from_str::<PartialConfig>(&mise_quiet_source(Some(wire)))
            .map_err(|err| {
                velnor_actions_contract::ContractError::map_decode_error(
                    CONFIG_REL,
                    &err.to_string(),
                )
            })
            .expect_err("non-boolean mise_quiet must fail");
        let velnor_actions_contract::ContractError::Config {
            key_path, problem, ..
        } = err
        else {
            panic!("expected a structured config error, got {err:?}");
        };
        assert_eq!(key_path, "document");
        assert!(problem.contains("invalid type"), "{problem}");
        assert!(problem.contains("mise_quiet"), "{problem}");
    }
}

#[test]
fn crate_timeout_defaults_to_five_and_follows_explicit_values() -> Result<(), Box<dyn Error>> {
    let default = parse_config("schema = 1\n[workflow]\nname = \"CI\"\n")?;
    assert_eq!(default.workflow.crate_timeout_minutes, 5);
    for minutes in [1_u16, 30, 360] {
        let config = parse_config(&format!(
            "schema = 1\n[workflow]\nname = \"CI\"\ncrate_timeout_minutes = {minutes}\n"
        ))?;
        assert_eq!(config.workflow.crate_timeout_minutes, minutes);
    }
    for minutes in [0_u16, 361] {
        let error = parse_config(&format!(
            "schema = 1\n[workflow]\nname = \"CI\"\ncrate_timeout_minutes = {minutes}\n"
        ))
        .expect_err("out-of-range crate timeout must fail");
        assert!(
            error.to_string().contains("workflow.crate_timeout_minutes"),
            "{error}"
        );
    }
    Ok(())
}

fn mise_quiet_source(mise_quiet: Option<&str>) -> String {
    let value = mise_quiet.map_or_else(String::new, |value| format!("mise_quiet = {value}\n"));
    format!("schema = 1\n[workflow]\nname = \"CI\"\n{value}")
}

fn source(schema: u32, pull_request_cache_policy: Option<&str>) -> String {
    let policy = pull_request_cache_policy.map_or_else(String::new, |policy| {
        format!("pull_request_cache_policy = \"{policy}\"\n")
    });
    let execution = if schema == VelnorConfig::SCHEMA_V2 {
        SCHEMA2_EXECUTION
    } else {
        ""
    };
    format!("schema = {schema}\n[workflow]\nname = \"CI\"\n{policy}{execution}")
}

const SCHEMA2_EXECUTION: &str = r#"
[execution]
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"

[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"

[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["ubuntu-26.04-scale-set", "velnor"]
platform = "linux/amd64"
"#;
