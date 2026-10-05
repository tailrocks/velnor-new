//! Single-writer and push-gated cache-save assertions for P08 workflows.

use super::*;

#[test]
fn c11_cache_saves_push_only_prs_and_forks_read_only() -> TestResult {
    use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
    // IR: exactly one save step (plan writer), carrying the push-only gate;
    // every other plan step (restores, fetch, obligations) stays ungated.
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), demo_lock("demo"))?;
    with_mbx(root)?;
    let prep = prepare(root)?;
    let plan = prep
        .workflow
        .ir
        .jobs
        .get("plan")
        .ok_or_else(|| std::io::Error::other("missing plan"))?;
    let mut saves = 0;
    for step in &plan.steps {
        if step.name == "Save Cargo sources" {
            saves += 1;
            assert_eq!(
                step.condition.as_deref(),
                Some(CACHE_SAVE_CONDITION),
                "save must be push-gated"
            );
        } else {
            assert!(
                step.condition.is_none(),
                "only the save step is gated: {}",
                step.name
            );
        }
    }
    assert_eq!(saves, 1, "single plan writer");
    let yaml = yaml_for(true)?;
    let save_at = yaml.find("- name: Save Cargo sources").ok_or("save step")?;
    assert!(
        yaml[save_at..].starts_with(
            "- name: Save Cargo sources\n        if: success() && github.event_name == 'push'",
        ),
        "save renders push-only if:\n{yaml}"
    );
    assert_tools_saves_push_gated_per_key(&yaml);
    Ok(())
}

/// YAML: one push-gated tools save per restored key; action cache remains off.
fn assert_tools_saves_push_gated_per_key(yaml: &str) {
    let mut keys = std::collections::BTreeSet::new();
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
    assert!(!keys.is_empty(), "at least one restored tools key:\n{yaml}");
    let mbx_saves = yaml.matches("- name: Save MBX single bundle").count();
    let mbx_exports = yaml.matches("- name: Export MBX single bundle").count();
    assert_eq!(
        yaml.matches("actions/cache/save@").count(),
        1 + keys.len() + mbx_saves,
        "sources plus one tools save per key plus the MBX bundle:\n{yaml}"
    );
    assert_eq!(
        yaml.matches("if: success() && github.event_name == 'push'")
            .count(),
        1 + keys.len(),
        "cargo and tools saves stay push-gated:\n{yaml}"
    );
    assert_eq!(
        yaml.matches(
            "if: runner.environment != 'github-hosted' && success() && github.event_name == 'push'"
        )
        .count(),
        mbx_saves + mbx_exports,
        "MBX export and save stay on Scale Set and push-gated:\n{yaml}"
    );
    assert!(
        !yaml.contains("- name: Restore Cargo sources\n        if:"),
        "restores stay unconditional:\n{yaml}"
    );
    assert_eq!(
        yaml.matches("- name: Save Mise tools").count(),
        keys.len(),
        "one tools saver per key:\n{yaml}"
    );
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
        "no unconditional save"
    );
    assert!(
        !yaml.contains("cache_save: ${{"),
        "no promised built-in save"
    );
    let setups = yaml.matches("- name: Setup Mise").count();
    let demoted = yaml.matches("cache_save: \"false\"").count();
    assert_eq!(setups, demoted, "every setup restore-only:\n{yaml}");
}
