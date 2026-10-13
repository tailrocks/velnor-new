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
