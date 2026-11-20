// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use velnor_model::{CacheKey, CacheRoot, Host, JobId, RepoPath, ToolsAnchor};
use velnor_render::render_key_with_mise;
use velnor_rust::{CacheEntry, budget_plan};

const DAY: i64 = 86_400;
const NOW: i64 = 20 * DAY;

fn entry(id: u64, family: &str, age: i64) -> CacheEntry {
    let key = match family {
        "tgt" => target_key(id, "rust-test", "Cargo.toml"),
        "src" => resolve_cache_key(
            &CacheKey::Sources {
                host: Host,
                lane: cache_root("Cargo.toml"),
            },
            id,
        ),
        "tools" => resolve_cache_key(
            &CacheKey::Tools {
                host: Host,
                anchor: ToolsAnchor::Unanchored,
            },
            id,
        ),
        "dlt" => resolve_cache_key(
            &CacheKey::Delta {
                job: JobId::new("rust-test").unwrap_or_else(|e| unreachable!("{e}")),
                root: cache_root("Cargo.toml"),
                host: Host,
            },
            id,
        ),
        _ => family.to_owned(),
    };
    CacheEntry {
        id,
        key,
        reference: "refs/pull/1/merge".to_owned(),
        size_in_bytes: 10,
        created_at: i64::try_from(id).unwrap_or(0),
        last_accessed_at: NOW - age,
    }
}

fn delta(id: u64, age: i64) -> CacheEntry {
    entry(id, "dlt", age)
}
fn base(id: u64, family: &str) -> CacheEntry {
    entry(id, family, 10 * DAY)
}
fn plan(entries: &[CacheEntry], budget: u64) -> Vec<u64> {
    budget_plan(entries, &BTreeSet::from([1]), NOW, budget)
}

#[test]
fn budget_rules_preserve_newest_bases_and_delete_in_priority_order() {
    let mut closed = delta(1, 0);
    closed.reference = "refs/pull/2/merge".to_owned();
    let rows = [
        (vec![closed.clone()], 100, vec![1]),
        (vec![delta(2, 3 * DAY)], 0, vec![2]),
        (
            vec![base(3, "tgt"), base(4, "tgt"), base(5, "tgt")],
            20,
            vec![3],
        ),
        (
            vec![base(3, "src"), base(4, "src"), base(5, "src")],
            20,
            vec![3],
        ),
        (vec![base(6, "tools"), base(7, "tools")], 10, vec![6]),
        (vec![delta(8, 0)], 0, vec![8]),
        (
            vec![base(9, "tgt"), base(10, "src"), base(11, "tools")],
            0,
            vec![],
        ),
    ];
    for (entries, budget, expected) in rows {
        assert_eq!(plan(&entries, budget), expected, "{entries:?}");
    }
    let mixed = vec![
        closed,
        delta(2, 3 * DAY),
        base(3, "tgt"),
        base(4, "tgt"),
        base(5, "tgt"),
        base(6, "tools"),
        base(7, "tools"),
        delta(8, 0),
    ];
    assert_eq!(plan(&mixed, 0), [1, 2, 3, 6, 8]);
    assert_eq!(plan(&mixed, 60), [1, 2]);
    assert_eq!(plan(&mixed, 80), [1]);
}

#[test]
fn ages_are_inclusive_and_only_pr_merge_scopes_are_delta_candidates() {
    for age in [3 * DAY - 1, 3 * DAY, 3 * DAY + 1] {
        let mut stale = delta(2, age);
        let bases = [base(3, "tgt"), base(4, "tgt"), base(5, "tgt")];
        assert_eq!(
            plan(
                &[
                    bases[0].clone(),
                    bases[1].clone(),
                    bases[2].clone(),
                    stale.clone()
                ],
                30
            ),
            if age < 3 * DAY { vec![3] } else { vec![2] }
        );
        for reference in ["refs/heads/main", "refs/pull/2/head", "refs/tags/v1"] {
            stale.reference = reference.to_owned();
            assert_eq!(plan(&[stale.clone()], 0), Vec::<u64>::new());
        }
    }
    for age in [7 * DAY - 1, 7 * DAY, 7 * DAY + 1] {
        let mut old = base(1, "tools");
        old.last_accessed_at = NOW - age;
        assert_eq!(
            plan(&[old, base(2, "tools")], 0),
            if age < 7 * DAY { vec![] } else { vec![1] }
        );
    }
    assert_eq!(plan(&[delta(1, 0)], 10), Vec::<u64>::new());
}

fn anchored_tools(id: u64, age: i64) -> CacheEntry {
    CacheEntry {
        key: format!(
            "velnor-tools-Linux-X64-mise2026.11.10-{}-{id:064x}",
            "a".repeat(64)
        ),
        ..entry(id, "other", age)
    }
}

#[test]
fn tools_entries_named_by_their_anchor_are_pruned_as_one_group() {
    let old = anchored_tools(1, 8 * DAY);
    let newest = anchored_tools(2, 8 * DAY);
    assert_eq!(plan(&[old, newest], 0), vec![1]);
    assert_eq!(
        plan(&[anchored_tools(1, DAY), anchored_tools(2, DAY)], 0),
        Vec::<u64>::new()
    );
}

#[test]
fn groups_unknown_keys_ties_order_and_byte_overflow_keep_their_contract() {
    let mut entries = Vec::new();
    for family in ["tgt", "src", "tools"] {
        for id in 1..=3 {
            let mut e = base(id + entries.len() as u64, family);
            e.id += 100;
            entries.push(e);
        }
    }
    let mut isolated = base(200, "tgt");
    for key in [
        target_key(200, "rust-other", "Cargo.toml"),
        target_key(200, "rust-test", "other/Cargo.toml"),
        target_key(200, "rust-test", "Cargo.toml").replace("Linux-X64", "macOS-ARM64"),
    ] {
        isolated.key = key;
        entries.push(isolated.clone());
    }
    isolated.key = base(201, "tgt")
        .key
        .replace(&"b".repeat(16), &"d".repeat(16));
    isolated.id = 201;
    entries.push(isolated);
    let deleted = plan(&entries, 0);
    assert_eq!(deleted.len(), 6);
    assert!(deleted.iter().all(|id| *id < 200));
    let mut malformed = base(300, "tgt");
    malformed.key.push_str("-bad");
    malformed.size_in_bytes = u64::MAX;
    assert_eq!(plan(&[malformed, delta(400, 0)], u64::MAX), [400]);
    let mut ties = vec![delta(1, 0), delta(2, 0), delta(3, 0)];
    for entry in &mut ties {
        entry.created_at = 0;
    }
    assert_eq!(plan(&ties, 10), [1, 2]);
    ties.reverse();
    assert_eq!(plan(&ties, 10), [1, 2]);
    assert_eq!(plan(&[], 0), Vec::<u64>::new());
    assert_eq!(plan(&[delta(1, 0), delta(2, 0)], 0), [1, 2]);
    for malformed in [
        "velnor-dlt-rust-root-Linux-X64-bad-12345678-1",
        "velnor-src-Linux-X64-root-bad",
        "velnor-tools-Linux-X64-mise1-bad",
        "unowned",
    ] {
        assert_eq!(plan(&[entry(1, malformed, 0)], 0), Vec::<u64>::new());
    }
}

#[test]
fn pull_request_bases_cannot_displace_the_newest_default_branch_bases() {
    for family in ["tgt", "src", "tools"] {
        let mut entries = Vec::new();
        for (scope, reference) in ["refs/heads/main", "refs/pull/1/merge", "refs/pull/2/merge"]
            .iter()
            .enumerate()
        {
            for version in 1..=3 {
                let id = u64::try_from(scope * 3 + version).unwrap_or(0);
                let mut entry = base(id, family);
                entry.reference = (*reference).to_owned();
                entries.push(entry);
            }
        }
        let deleted = plan(&entries, 0);
        for newest in [3, 6, 9] {
            assert!(!deleted.contains(&newest), "{family}: {deleted:?}");
        }
        assert_eq!(
            deleted,
            if family == "tools" {
                vec![1, 2, 4, 5, 7, 8]
            } else {
                vec![1, 4, 7]
            },
            "{family}"
        );
    }
}

fn target_key(id: u64, job: &str, manifest: &str) -> String {
    let key = CacheKey::Target {
        job: JobId::new(job).unwrap_or_else(|e| unreachable!("{e}")),
        root: cache_root(manifest),
        host: Host,
    };
    resolve_cache_key(&key, id)
}

fn cache_root(manifest: &str) -> CacheRoot {
    CacheRoot::new(
        &RepoPath::new(manifest).unwrap_or_else(|e| unreachable!("{e}")),
        RepoPath::from_static("target"),
    )
}

fn resolve_cache_key(key: &CacheKey, id: u64) -> String {
    let rendered = render_key_with_mise(key, &format!("2026.1.{id}")).key;
    let mut resolved = String::new();
    let mut rest = rendered.as_str();
    while let Some((literal, expression)) = rest.split_once("${{ ") {
        resolved.push_str(literal);
        let (expression, remaining) = expression
            .split_once(" }}")
            .unwrap_or_else(|| unreachable!());
        let value = match expression {
            "runner.os" => "Linux".to_owned(),
            "runner.arch" => "X64".to_owned(),
            "github.sha" => "c".repeat(40),
            "github.run_id" => id.to_string(),
            "steps.prepare.outputs.compat" => "b".repeat(16),
            "steps.restored.outputs.base8" | "steps.delta_base_root.outputs.base8" => {
                "12345678".to_owned()
            }
            "github.event.pull_request.number" => "1".to_owned(),
            expression
                if expression.starts_with("hashFiles(")
                    || expression == "steps.sources_locks.outputs.hash" =>
            {
                format!("{id:064x}")
            }
            _ => unreachable!("{expression}"),
        };
        resolved.push_str(&value);
        rest = remaining;
    }
    resolved.push_str(rest);
    resolved
}

#[test]
fn renderer_target_keys_keep_two_versions_for_every_job_and_root() {
    for (job, manifest) in [
        ("rust", "Cargo.toml"),
        ("rust-test", "crates/a/Cargo.toml"),
        ("rust-test", "tools/b/Cargo.toml"),
    ] {
        let entries: Vec<_> = (1..=3)
            .map(|id| {
                let mut entry = base(id, "tgt");
                entry.key = target_key(id, job, manifest);
                entry
            })
            .collect();
        assert_eq!(plan(&entries, 20), [1], "{job}: {manifest}");
    }
    let obsolete: Vec<_> = (1..=3)
        .map(|id| {
            let mut entry = base(id, "tgt");
            entry.key = format!(
                "velnor-tgt-rust-test-root-Linux-X64-{}-{}-{}-{id}",
                "b".repeat(16),
                "c".repeat(64),
                "d".repeat(40)
            );
            entry
        })
        .collect();
    assert_eq!(plan(&obsolete, 20), Vec::<u64>::new());
}

#[test]
fn retired_per_root_sources_entries_are_deleted_within_any_budget() {
    let shared = |id: u64| entry(id, "src", 10 * DAY);
    let retired = |id: u64| {
        let mut old = entry(id, "src", 10 * DAY);
        old.key = old.key.replace("all.roots", "root");
        old
    };
    assert_ne!(shared(1).key, retired(1).key);
    let entries = [
        shared(1),
        shared(2),
        shared(3),
        retired(11),
        retired(12),
        retired(13),
    ];
    assert_eq!(plan(&entries, 0), [11, 12, 13, 1]);
    assert_eq!(plan(&entries, 1_000), [11, 12, 13]);
}

fn with_compat(id: u64, compat: char) -> CacheEntry {
    let mut entry = base(id, "tgt");
    entry.key = entry
        .key
        .replace(&"b".repeat(16), &compat.to_string().repeat(16));
    entry
}

#[test]
fn a_target_entry_of_an_older_compatibility_family_is_deleted_whatever_the_budget() {
    let entries = [
        with_compat(1, 'a'),
        with_compat(2, 'a'),
        with_compat(3, 'b'),
        with_compat(4, 'b'),
    ];
    assert_eq!(plan(&entries, u64::MAX), [1, 2]);
    assert_eq!(plan(&entries, 0), [1, 2]);
    assert_eq!(
        plan(&[with_compat(2, 'a'), with_compat(1, 'b')], u64::MAX),
        [1]
    );
    let mut other_job = with_compat(5, 'a');
    other_job.key =
        target_key(5, "rust-lint", "Cargo.toml").replace(&"b".repeat(16), &"a".repeat(16));
    assert_eq!(
        plan(&[other_job, with_compat(3, 'b')], u64::MAX),
        Vec::<u64>::new()
    );
}

#[test]
fn a_sources_lock_restored_within_a_day_survives_even_when_two_newer_locks_exist() {
    let entries = [base(1, "src"), base(2, "src"), base(3, "src")];
    assert_eq!(plan(&entries, 0), [1]);
    for (idle, expected) in [(DAY - 1, vec![]), (DAY, vec![1]), (2 * DAY, vec![1])] {
        let mut restored = entries.clone();
        restored[0].last_accessed_at = NOW - idle;
        assert_eq!(plan(&restored, 0), expected, "idle {idle}");
    }
}
