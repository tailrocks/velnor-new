//! Cache-save and restore assertions shared by P08 renderer fixtures.

use std::collections::BTreeSet;

/// YAML: one push-gated tools save per restored V2 key plus the sources save.
pub(super) fn assert_tools_saves_push_gated_per_key(yaml: &str) {
    let keys = restored_tools_keys(yaml);
    assert!(!keys.is_empty(), "at least one restored tools key:\n{yaml}");
    assert_save_counts(yaml, keys.len());
    assert_save_steps_are_push_gated(yaml, keys.len());
    assert_restore_steps_are_dispatch_denied(yaml);
    assert_tools_savers_use_restored_keys(yaml, &keys);
    assert_mise_setup_is_restore_only(yaml);
}

fn restored_tools_keys(yaml: &str) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let mut in_tools_restore = false;
    for line in yaml.lines() {
        if let Some(name) = line.trim().strip_prefix("- name: ") {
            in_tools_restore = name.trim_matches('"') == "Restore Mise tools";
        }
        if in_tools_restore && let Some(key) = line.trim().strip_prefix("key: ") {
            keys.insert(key.trim_matches('"').to_owned());
            in_tools_restore = false;
        }
    }
    keys
}

fn assert_save_counts(yaml: &str, tools_keys: usize) {
    let mbx_saves = yaml.matches("- name: Save MBX single bundle").count();
    assert_eq!(
        yaml.matches("actions/cache/save@").count(),
        1 + tools_keys + mbx_saves,
        "sources plus one tools save per key plus the MBX bundle:\n{yaml}"
    );
    assert_eq!(
        yaml.matches("- name: Save Mise tools").count(),
        tools_keys,
        "exactly one tools saver per key:\n{yaml}"
    );
}

fn assert_save_steps_are_push_gated(yaml: &str, tools_keys: usize) {
    let mbx_saves = yaml.matches("- name: Save MBX single bundle").count();
    let mbx_exports = yaml.matches("- name: Export MBX single bundle").count();
    assert_eq!(
        count_steps_with_gate(
            yaml,
            &["Save Cargo sources"],
            &["github.event_name == 'push'"]
        ),
        1,
        "the sources save uses the canonical push-only gate:\n{yaml}"
    );
    assert_eq!(
        count_steps_with_gate(
            yaml,
            &["Save Mise tools"],
            &[
                "github.event_name == 'push'",
                "github.ref == format('refs/heads/{0}', github.event.repository.default_branch)",
                "github.ref_protected == true",
                "steps.v2.outputs.enabled == 'true'",
            ],
        ),
        tools_keys,
        "tools saves are limited to protected default-branch pushes with qualified identity:\n{yaml}"
    );
    assert_eq!(
        count_steps_with_gate(
            yaml,
            &["Export MBX single bundle", "Save MBX single bundle"],
            &[
                "runner.environment != 'github-hosted'",
                "github.event_name == 'push'",
                "github.event_name != 'workflow_dispatch'",
            ],
        ),
        mbx_saves + mbx_exports,
        "MBX bundle export and save stay Scale Set and push-gated:\n{yaml}"
    );
}

fn assert_restore_steps_are_dispatch_denied(yaml: &str) {
    let restore_names = [
        "Restore Cargo sources",
        "Restore Mise tools",
        "Restore MBX objects",
        "Restore MBX single bundle",
        "Restore Tofu providers",
    ];
    let restore_count = restore_names
        .iter()
        .map(|name| yaml.matches(&format!("- name: {name}")).count())
        .sum::<usize>();
    assert!(
        restore_count > 0,
        "fixture emits cache restore steps:\n{yaml}"
    );
    assert_eq!(
        count_steps_with_gate(
            yaml,
            &restore_names,
            &["github.event_name != 'workflow_dispatch'"],
        ),
        restore_count,
        "cache restores stay disabled during dispatch until admission:\n{yaml}"
    );
}

fn assert_tools_savers_use_restored_keys(yaml: &str, keys: &BTreeSet<String>) {
    for line in yaml.lines() {
        if let Some(key) = line.trim().strip_prefix("key: ")
            && key.starts_with("mise-tools-v2-")
        {
            assert!(
                keys.contains(key.trim_matches('"')),
                "tools save archives a restored key:\n{yaml}"
            );
        }
    }
    assert!(
        !yaml.contains("cache_save: \"true\""),
        "no unconditional mise save:\n{yaml}"
    );
    assert!(
        !yaml.contains("cache_save: ${{"),
        "no promised built-in save:\n{yaml}"
    );
}

fn assert_mise_setup_is_restore_only(yaml: &str) {
    let setups = yaml.matches("- name: Setup Mise").count();
    let demoted = yaml.matches("cache_save: \"false\"").count();
    assert_eq!(setups, demoted, "every setup restore-only:\n{yaml}");
}

fn count_steps_with_gate(yaml: &str, names: &[&str], required: &[&str]) -> usize {
    let mut current = None;
    let mut count = 0;
    for line in yaml.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("- name: ") {
            current = Some(name.trim_matches('"'));
        } else if let Some(condition) = line.strip_prefix("if: ")
            && current.is_some_and(|name| names.contains(&name))
            && required.iter().all(|part| condition.contains(part))
        {
            count += 1;
        }
    }
    count
}
