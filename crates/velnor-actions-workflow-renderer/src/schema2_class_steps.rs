// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::yaml::Yaml;

pub(super) fn run_id(name: &str, id: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

pub(super) fn shell_step(name: &str, shell: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str(shell)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

pub(super) fn run_env(name: &str, env: &[(&str, &str)], run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("env".to_owned(), mapping(env)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

pub(super) fn uses_with(name: &str, uses: &str, with: &[(&str, &str)]) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(uses)),
        ("with".to_owned(), mapping(with)),
    ])
}

pub(super) fn mapping(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}
