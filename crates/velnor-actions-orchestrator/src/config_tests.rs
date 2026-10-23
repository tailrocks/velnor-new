//! Pull-request cache policy parsing across both supported config schemas.

use std::error::Error;

use super::{CONFIG_REL, PartialConfig, load_config, load_config_from_text};
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
fn exact_text_loader_matches_file_defaults_and_rejects_invalid_schema() -> Result<(), Box<dyn Error>>
{
    let source = "schema = 1\n[workflow]\nname = \"CI\"\n";
    let root = tempfile::tempdir()?;
    let config_dir = root.path().join(".velnor");
    std::fs::create_dir(&config_dir)?;
    std::fs::write(config_dir.join("config.toml"), source)?;

    let from_file = load_config(root.path())?;
    let from_text = load_config_from_text(source)?;
    assert_eq!(
        serde_json::to_value(from_text)?,
        serde_json::to_value(from_file)?
    );
    assert!(load_config_from_text("schema = 99\n").is_err());
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
fn dependency_seed_timeout_materializes_independently() -> Result<(), Box<dyn Error>> {
    let default = parse_config("schema = 1\n[workflow]\ncrate_timeout_minutes = 10\n")?;
    assert_eq!(default.workflow.crate_timeout_minutes, 10);
    assert_eq!(default.workflow.dependency_seed_timeout_minutes, 30);

    let explicit = parse_config(
        "schema = 1\n[workflow]\ncrate_timeout_minutes = 10\ndependency_seed_timeout_minutes = 45\n",
    )?;
    assert_eq!(explicit.workflow.crate_timeout_minutes, 10);
    assert_eq!(explicit.workflow.dependency_seed_timeout_minutes, 45);
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

#[test]
fn plan_timeout_defaults_to_five_and_follows_explicit_values() -> Result<(), Box<dyn Error>> {
    let default = parse_config("schema = 1\n[workflow]\nname = \"CI\"\n")?;
    assert_eq!(default.workflow.plan_timeout_minutes, 5);
    for minutes in [1_u16, 15, 360] {
        let config = parse_config(&format!(
            "schema = 1\n[workflow]\nname = \"CI\"\nplan_timeout_minutes = {minutes}\n"
        ))?;
        assert_eq!(config.workflow.plan_timeout_minutes, minutes);
    }
    for minutes in [0_u16, 361] {
        let error = parse_config(&format!(
            "schema = 1\n[workflow]\nname = \"CI\"\nplan_timeout_minutes = {minutes}\n"
        ))
        .expect_err("out-of-range planning timeout must fail");
        assert!(
            error.to_string().contains("workflow.plan_timeout_minutes"),
            "{error}"
        );
    }
    Ok(())
}

#[test]
fn private_sources_parse_from_toml_and_reject_unknown_keys() -> Result<(), Box<dyn Error>> {
    let header = "schema = 1\n[workflow]\nname = \"CI\"\n[private_sources]\nowner = \"ChainArgos\"\napp_id_variable = \"VELNOR_DEPS_APP_ID\"\napp_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\nnpm_scopes = [\"@chainargos\"]\n";
    let config = parse_config(&format!(
        "{header}[[private_sources.release_assets]]\nid = \"explorer-schema\"\nrepository = \"ChainArgos/blockchain-explorer\"\ntag = \"explorer-schema-v1.4.0\"\nasset = \"explorer-schema-1.4.0.tar.gz\"\nextract_to = \"explorer-schema\"\n"
    ))?;
    let private = config.private_sources.expect("declared");
    assert_eq!(private.owner, "ChainArgos");
    assert_eq!(private.release_assets.len(), 1);
    assert!(
        parse_config("schema = 1\n[workflow]\nname = \"CI\"\n")?
            .private_sources
            .is_none()
    );
    let unknown =
        parse_config(&format!("{header}token = \"x\"\n")).expect_err("unknown key must fail");
    assert!(unknown.to_string().contains("token"), "{unknown}");
    let escape = parse_config(&format!(
        "{header}[[private_sources.release_assets]]\nid = \"a\"\nrepository = \"ChainArgos/x\"\ntag = \"t\"\nasset = \"a.tgz\"\nextract_to = \"../up\"\n"
    ))
    .expect_err("escaping extract_to must fail");
    assert!(escape.to_string().contains("extract_to"), "{escape}");
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
