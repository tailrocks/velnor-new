// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fmt::Write;

use velnor_model::{RepoFacts, RustRoot, Timings};

use crate::config_test_groups::{DEFAULT_NAME, NAME_MAX_LENGTH};

const GROUP_SHARE_DIVISOR: u64 = 3;
/// A group name must be an identifier: it cannot be empty or start with a digit.
const FALLBACK_SLUG: &str = "binary";

#[must_use]
pub fn suggest_groups(facts: &RepoFacts) -> String {
    let several = facts.rust_roots.len() > 1;
    let blocks: Vec<String> = facts
        .rust_roots
        .iter()
        .filter_map(|root| Some((root, facts.timings.get(&root.manifest)?)))
        .flat_map(|(root, timings)| root_blocks(facts, root, timings, several))
        .collect();
    blocks.join("\n")
}

fn root_blocks(
    facts: &RepoFacts,
    root: &RustRoot,
    timings: &Timings,
    several: bool,
) -> Vec<String> {
    let total: u64 = timings.test_binary_seconds.values().sum();
    let mut slow: Vec<(&str, u64)> = timings
        .test_binary_seconds
        .iter()
        .filter(|(_, seconds)| **seconds * GROUP_SHARE_DIVISOR > total)
        .map(|(id, seconds)| (id.as_str(), *seconds))
        .collect();
    slow.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let own = || {
        facts
            .config
            .test_groups
            .iter()
            .filter(|group| group.belongs_to(root, facts.rust_roots.len()))
    };
    let mut taken: BTreeSet<String> = own()
        .map(|group| group.name.as_str().to_ascii_lowercase())
        .collect();
    taken.insert(DEFAULT_NAME.to_owned());
    slow.into_iter()
        .filter_map(|(id, seconds)| {
            let filter = format!("binary_id(={id})");
            if own().any(|group| group.filter == filter) {
                return None;
            }
            let name = unused(&slug(id), &taken);
            taken.insert(name.to_ascii_lowercase());
            let mut block = format!(
                "# {id} takes {seconds} of {total} s of the tests in {}\n[[test_groups]]\nname = \"{name}\"\nfilter = \"{filter}\"\n",
                root.manifest
            );
            if several {
                let folder = root
                    .manifest
                    .parent()
                    .map_or_else(|| ".".to_owned(), |folder| folder.to_string());
                writeln!(block, "root = \"{folder}\"").ok();
            }
            Some(block)
        })
        .collect()
}

fn slug(id: &str) -> String {
    let mut slug = String::new();
    for c in id.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() || slug.starts_with(|c: char| c.is_ascii_digit()) {
        format!("{FALLBACK_SLUG}-{slug}").trim_end_matches('-').to_owned()
    } else {
        slug.to_owned()
    }
}

/// Names compare without regard to case, as `test_groups.name` validation does.
fn unused(slug: &str, taken: &BTreeSet<String>) -> String {
    let fits = |text: &str| text.chars().take(NAME_MAX_LENGTH).collect::<String>();
    let base = fits(slug);
    if !taken.contains(&base.to_ascii_lowercase()) {
        return base;
    }
    (2_u32..=u32::MAX)
        .map(|number| {
            let suffix = format!("-{number}");
            let room = NAME_MAX_LENGTH - suffix.len();
            format!("{}{suffix}", slug.chars().take(room).collect::<String>())
        })
        .find(|name| !taken.contains(&name.to_ascii_lowercase()))
        .unwrap_or_default()
}
