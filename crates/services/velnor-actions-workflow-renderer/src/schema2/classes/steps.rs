//! Step maps for qualification classes.

use velnor_actions_workflow_tree::yaml::Yaml;

/// A `run` step with an id, so the job can publish its output.
pub(super) fn run_id(name: &str, id: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

/// A `run` step that sets `shell`.
pub(super) fn shell_step(name: &str, shell: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str(shell)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

/// A `run` step with environment bindings.
pub(super) fn run_env(name: &str, env: &[(&str, &str)], run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("env".to_owned(), mapping(env)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

/// A `uses` step with string inputs.
pub(super) fn uses_with(name: &str, uses: &str, with: &[(&str, &str)]) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(uses)),
        ("with".to_owned(), mapping(with)),
    ])
}

/// A string mapping, in the given order.
pub(super) fn mapping(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}
