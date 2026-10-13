use velnor_actions_contract_workflow::{Step, StepKind};

use super::MiseToolsCacheKey;
use velnor_actions_workflow_steps::{MiseSetup, steps::validate_uses};

/// Check qualified setup pins or the legacy restore-only shape.
pub(super) fn setup_shape_ok(
    step: &Step,
    setup: &MiseSetup,
    qualified: bool,
    key: Option<&MiseToolsCacheKey>,
) -> bool {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return false;
    };
    if validate_uses(uses).is_err() {
        return false;
    }
    let base = step.condition.is_none()
        && uses == &setup.uses
        && env.is_empty()
        && with.len() == usize::from(qualified) + 6
        && with.get("install").is_some_and(|v| v == "false")
        && with.get("env").is_some_and(|v| v == "false")
        && with.contains_key("version")
        && with.contains_key("sha256");
    if !base {
        return false;
    }
    if qualified {
        qualified_cache_shape(with)
            && key.is_none_or(|expected| setup_pins_match(with, setup, expected))
    } else {
        legacy_cache_shape(with) && setup_versions_match(with, setup)
    }
}

fn qualified_cache_shape(with: &std::collections::BTreeMap<String, String>) -> bool {
    with.get("cache")
        .is_some_and(|v| v == super::MISE_CACHE_ENABLED_EXPR)
        && with.get("cache_save").is_some_and(|v| v == "false")
        && with.contains_key("cache_key")
}

fn legacy_cache_shape(with: &std::collections::BTreeMap<String, String>) -> bool {
    with.get("cache").is_some_and(|v| v == "false")
        && with.get("cache_save").is_some_and(|v| v == "false")
}

fn setup_pins_match(
    with: &std::collections::BTreeMap<String, String>,
    setup: &MiseSetup,
    key: &MiseToolsCacheKey,
) -> bool {
    with.get("cache_key")
        .is_some_and(|value| value == key.as_str())
        && setup_versions_match(with, setup)
}

fn setup_versions_match(
    with: &std::collections::BTreeMap<String, String>,
    setup: &MiseSetup,
) -> bool {
    with.get("version")
        .is_some_and(|value| value == &setup.version)
        && with
            .get("sha256")
            .is_some_and(|value| value == &setup.sha256)
}
