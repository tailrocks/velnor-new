// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use velnor_model::{RepoPath, StepKind, WorkflowPlan, sha256_hex};

use crate::base_steps::fetch_chunks;
use crate::rules::{Rule, RuleViolation};

const PREFIX: &str = "VELNOR_SOURCE_ROOTS_";
const MIN_CHARACTERS: usize = 100;
const NAME_HEX_CHARACTERS: usize = 32;

fn plain(word: &str) -> bool {
    word.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._-/+@,:=%".contains(&byte))
}

fn raw_word(manifest: &RepoPath) -> String {
    manifest
        .parent()
        .map_or_else(|| ".".to_owned(), |directory| format!("./{directory}"))
}

fn name_of(value: &str) -> String {
    let digest = sha256_hex(value.as_bytes());
    format!("{PREFIX}{}", &digest[..NAME_HEX_CHARACTERS])
}

pub(crate) fn roots_env(manifests: &[RepoPath]) -> Option<(String, String)> {
    let words: Vec<String> = manifests.iter().map(raw_word).collect();
    let value = words.join(" ");
    (fetch_chunks(manifests).len() == 1
        && words.iter().all(|word| plain(word))
        && value.chars().count() > MIN_CHARACTERS)
        .then(|| (name_of(&value), value))
}

fn collect(
    entries: impl IntoIterator<Item = (String, String)>,
) -> Result<BTreeMap<String, String>, Rule> {
    let mut roots = BTreeMap::new();
    for (name, value) in entries {
        match roots.entry(name) {
            Entry::Vacant(slot) => {
                slot.insert(value);
            }
            Entry::Occupied(slot) if *slot.get() == value => {}
            Entry::Occupied(slot) => return Err(Rule::SourceRootsEnvCollision(slot.key().clone())),
        }
    }
    Ok(roots)
}

pub(crate) fn workflow_roots(
    plan: &WorkflowPlan,
) -> Result<BTreeMap<String, String>, Vec<RuleViolation>> {
    collect(
        plan.jobs
            .iter()
            .flat_map(|job| &job.steps)
            .filter_map(|step| match &step.kind {
                StepKind::SourcesFetchAll { manifests, .. } => roots_env(manifests),
                _ => None,
            }),
    )
    .map_err(|rule| {
        vec![RuleViolation {
            job: String::new(),
            step: 0,
            rule,
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::{collect, name_of};

    fn entry(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn two_different_lists_under_one_name_are_refused() {
        let result = collect([
            entry("VELNOR_SOURCE_ROOTS_x", "./a"),
            entry("VELNOR_SOURCE_ROOTS_x", "./b"),
        ]);
        assert!(result.is_err_and(|rule| matches!(
            rule,
            crate::rules::Rule::SourceRootsEnvCollision(name) if name == "VELNOR_SOURCE_ROOTS_x"
        )));
    }

    #[test]
    fn the_same_list_twice_is_one_entry() {
        let result = collect([
            entry("VELNOR_SOURCE_ROOTS_x", "./a"),
            entry("VELNOR_SOURCE_ROOTS_x", "./a"),
        ]);
        assert_eq!(result.map(|roots| roots.len()), Ok(1));
    }

    #[test]
    fn the_name_is_a_128_bit_hash_of_the_exact_value() {
        let name = name_of("./a ./b");
        assert_eq!(name.len(), "VELNOR_SOURCE_ROOTS_".len() + 32);
        assert_ne!(name, name_of("./a ./c"));
        assert_ne!(name, name_of("./a  ./b"));
    }
}
