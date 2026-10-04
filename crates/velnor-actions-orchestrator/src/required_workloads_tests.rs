//! Registry requirements survive missing declarations and weaker emitted universes.

use super::*;
use velnor_actions_contract::{ContractError, build_index_from_list};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture_root(
    value: &str,
) -> Result<velnor_actions_contract::config::Utf8RepoRelDir, ContractError> {
    velnor_actions_contract::config::Utf8RepoRelDir::parse(value).map_err(|problem| {
        ContractError::config(
            REGISTRY_PATH,
            "obligations[0].root",
            format!("invalid_root:{problem:?}"),
        )
    })
}

fn fixture() -> Result<
    (
        tempfile::TempDir,
        VelnorConfig,
        velnor_actions_contract::FileIndex,
    ),
    Box<dyn std::error::Error>,
> {
    let repo = tempfile::TempDir::new()?;
    std::fs::create_dir_all(repo.path().join(".velnor"))?;
    std::fs::write(repo.path().join(".velnor/config.toml"), "schema = 1\n")?;
    std::fs::write(repo.path().join("script.rb"), "puts 'typed syntax'\n")?;
    let mut config = crate::config::load_config(repo.path())?;
    config
        .stacks
        .workloads
        .push(serde_json::from_value(serde_json::json!({
            "name": "syntax", "kind": "ruby_syntax", "paths": ["script.rb"]
        }))?);
    let index = build_index_from_list(repo.path(), &["script.rb".to_owned()], &[])?;
    Ok((repo, config, index))
}

fn registry(config: &VelnorConfig) -> Result<RequiredNativeObligations, OrchestratorError> {
    Ok(RequiredNativeObligations {
        schema: 1,
        obligations: vec![RequiredNativeObligation {
            component: "syntax".to_owned(),
            operation: velnor_actions_contract::config::WorkloadKind::RubySyntax,
            root: fixture_root(".")?,
            phases: vec![RequiredNativePhase::Syntax],
            profile_digest: profile_digest(&config.stacks.workloads[0])?,
        }],
    })
}

#[test]
fn declaration_deletion_fails_even_before_empty_work() -> TestResult {
    let (repo, mut config, index) = fixture()?;
    let expected = registry(&config)?;
    std::fs::write(repo.path().join(REGISTRY_PATH), toml::to_string(&expected)?)?;
    let tasks = crate::workloads::derive(&config, &index)?;
    assert_eq!(tasks.len(), 1);
    config.stacks.workloads.clear();
    let error =
        crate::workloads::derive(&config, &index).expect_err("required declaration retained");
    assert!(error.to_string().contains("missing_declaration:syntax"));
    Ok(())
}

#[test]
fn phase_omission_identity_drift_and_recipe_drift_all_refuse() -> TestResult {
    let (_repo, mut config, index) = fixture()?;
    let expected = registry(&config)?;
    let tasks = crate::workloads::derive(&config, &index)?;
    validate_registry(&expected, &config, &tasks)?;
    assert!(validate_registry(&expected, &config, &[]).is_err());
    let mut missing_phase = tasks.clone();
    missing_phase[0].task_kind = "test".to_owned();
    assert!(validate_registry(&expected, &config, &missing_phase).is_err());
    let mut wrong_owner = tasks.clone();
    wrong_owner[0].identity.unit_path = "other".to_owned();
    assert!(validate_registry(&expected, &config, &wrong_owner).is_err());
    let mut duplicate = tasks.clone();
    duplicate.extend(tasks.clone());
    assert!(validate_registry(&expected, &config, &duplicate).is_err());
    config.stacks.workloads[0]
        .inputs
        .push("extra.txt".to_owned());
    let error = validate_registry(&expected, &config, &tasks).expect_err("full recipe bound");
    assert!(error.to_string().contains("profile_mismatch:syntax"));
    Ok(())
}

#[test]
fn schema_rejects_raw_commands_unknown_families_and_wrong_phase_pairs() -> TestResult {
    let (_repo, config, _index) = fixture()?;
    let expected = registry(&config)?;
    let value = serde_json::to_value(&expected)?;
    for (key, replacement) in [
        ("operation", serde_json::json!("rust")),
        ("operation", serde_json::json!("tofu")),
        ("phases", serde_json::json!(["arbitrary-command"])),
        ("command", serde_json::json!("echo forged")),
    ] {
        let mut invalid = value.clone();
        invalid["obligations"][0][key] = replacement;
        assert!(serde_json::from_value::<RequiredNativeObligations>(invalid).is_err());
    }
    let mut pair = expected;
    pair.obligations[0].phases = vec![RequiredNativePhase::Test];
    assert!(pair.validate(REGISTRY_PATH).is_err());
    pair.obligations[0].phases = vec![RequiredNativePhase::Syntax, RequiredNativePhase::Syntax];
    assert!(pair.validate(REGISTRY_PATH).is_err());
    pair.obligations.clear();
    assert!(pair.validate(REGISTRY_PATH).is_err());
    Ok(())
}

#[test]
fn absent_registry_does_not_claim_historical_completeness() -> TestResult {
    let (repo, config, index) = fixture()?;
    let tasks = crate::workloads::derive(&config, &index)?;
    validate(repo.path(), &config, &tasks)?;
    Ok(())
}

#[test]
fn registry_operation_and_root_are_not_weaker_recipe_substitutes() -> TestResult {
    let (_repo, config, index) = fixture()?;
    let expected = registry(&config)?;
    let tasks = crate::workloads::derive(&config, &index)?;
    let mut changed = config.clone();
    changed.stacks.workloads[0].kind = velnor_actions_contract::config::WorkloadKind::Shellcheck;
    let error = validate_registry(&expected, &changed, &tasks).expect_err("operation differs");
    assert!(error.to_string().contains("declaration_mismatch:syntax"));
    changed = config;
    changed.stacks.workloads[0].root = fixture_root("other")?;
    let error = validate_registry(&expected, &changed, &tasks).expect_err("source root differs");
    assert!(error.to_string().contains("declaration_mismatch:syntax"));
    Ok(())
}
