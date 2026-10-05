//! Root package identities for consumer and audited repository fixtures.

use serde::Deserialize;
use velnor_actions_contract::WorkflowPolicy;

#[derive(Default, Deserialize)]
struct FixtureConfig {
    #[serde(default)]
    workflow: FixtureWorkflow,
}

#[derive(Default, Deserialize)]
struct FixtureWorkflow {
    policy: Option<WorkflowPolicy>,
}

/// Repository fixtures use the registered contract suite's tool profile.
/// Unparseable config or invalid policy keeps a generic package; `prepare`
/// validates the original config and owns the expected failure.
pub(crate) fn root_manifest(config: &str) -> String {
    let package = match toml::from_str::<FixtureConfig>(config) {
        Ok(config) if config.workflow.policy == Some(WorkflowPolicy::VelnorRepositoryV1) => {
            "velnor-actions-contract"
        }
        Ok(_) | Err(_) => "demo",
    };
    format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")
}

#[cfg(test)]
mod tests {
    use super::root_manifest;

    #[test]
    fn repository_fixture_uses_an_audited_suite_identity() {
        let manifest = root_manifest("schema = 1\n[workflow]\npolicy = \"velnor-repository-v1\"\n");
        assert!(manifest.contains("name = \"velnor-actions-contract\""));
    }

    #[test]
    fn consumer_fixture_keeps_its_generic_identity() {
        assert!(root_manifest("schema = 1\n").contains("name = \"demo\""));
    }

    #[test]
    fn invalid_config_fixture_preserves_prepare_failure_scope() {
        assert!(root_manifest("not = [valid").contains("name = \"demo\""));
    }
}
