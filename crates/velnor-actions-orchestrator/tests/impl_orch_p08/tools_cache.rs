//! Tools-cache identity assertions for P08 workflow fixtures.

type ToolsCacheIdentity = (String, String);
type ToolsCacheIdentities = (
    std::collections::BTreeSet<ToolsCacheIdentity>,
    Vec<ToolsCacheIdentity>,
    std::collections::BTreeSet<ToolsCacheIdentity>,
    Vec<ToolsCacheIdentity>,
);

/// YAML: one push-gated tools save per restored V2 key (plus the sources
/// save), every action-owned Mise cache disabled.
pub(super) fn assert_tools_saves_push_gated_per_key(yaml: &str) {
    let (keys, saves, seed_keys, seed_saves) = tools_cache_identities(yaml);
    assert!(!keys.is_empty(), "at least one restored tools key:\n{yaml}");
    assert_one_save_per_identity(&keys, &saves, &seed_keys, yaml);
    assert!(
        seed_saves.is_empty(),
        "Dependency seeds must not save the shared tools cache.\n{yaml}"
    );
    let mbx_saves = yaml.matches("- name: Save MBX bundle").count();
    assert_eq!(
        yaml.matches("actions/cache/save@").count(),
        1 + saves.len() + mbx_saves,
        "sources plus one tools save per key plus the MBX bundle:\n{yaml}"
    );
    assert_eq!(
        yaml.matches("if: success() && github.event_name == 'push'")
            .count(),
        saves.len(),
        "tools saves stay push-gated:\n{yaml}"
    );
    assert!(
        !yaml.contains("- name: Restore Cargo sources\n        if:")
            || yaml
                .match_indices("- name: Restore Cargo sources\n        if:")
                .all(|(at, marker)| {
                    yaml[at + marker.len()..]
                        .lines()
                        .next()
                        .is_some_and(|condition| {
                            condition.trim() == "github.event_name != 'workflow_dispatch'"
                        })
                }),
        "restores carry at most the dispatch deny:\n{yaml}"
    );
    assert_eq!(
        yaml.matches("- name: Save Mise tools").count(),
        saves.len(),
        "exactly one tools saver per key:\n{yaml}"
    );
    assert!(
        !yaml.contains("cache_save: \"true\""),
        "no unconditional mise save:\n{yaml}"
    );
    assert!(
        !yaml.contains("cache_save: ${{"),
        "no promised built-in save:\n{yaml}"
    );
    let setups = yaml.matches("- name: Setup Mise").count();
    let demoted = yaml.matches("cache_save: \"false\"").count();
    assert_eq!(setups, demoted, "every setup restore-only:\n{yaml}");
}

fn assert_one_save_per_identity(
    restored: &std::collections::BTreeSet<ToolsCacheIdentity>,
    saves: &[ToolsCacheIdentity],
    seed_restored: &std::collections::BTreeSet<ToolsCacheIdentity>,
    yaml: &str,
) {
    for identity in restored {
        assert_eq!(
            saves.iter().filter(|saved| *saved == identity).count(),
            1,
            "exactly one writer for {identity:?}:\n{yaml}"
        );
    }
    for identity in seed_restored {
        let count = saves.iter().filter(|saved| *saved == identity).count();
        let expected = usize::from(restored.contains(identity));
        assert_eq!(
            count, expected,
            "A seed restore must have one shared writer only if another job restores the same identity: {identity:?}.\n{yaml}"
        );
    }
    assert!(
        saves.iter().all(|saved| restored.contains(saved)),
        "every tools writer has a restored identity:\n{yaml}"
    );
}

fn tools_cache_identities(yaml: &str) -> ToolsCacheIdentities {
    let mut restored = std::collections::BTreeSet::new();
    let mut saves = Vec::new();
    let mut seed_restored = std::collections::BTreeSet::new();
    let mut seed_saves = Vec::new();
    let mut digest = None;
    let mut identity_step = false;
    let mut cache_step = None;
    let mut job = String::new();
    for line in yaml.lines() {
        let trimmed = line.trim();
        if line.starts_with("  ") && !line.starts_with("   ") && trimmed.ends_with(':') {
            trimmed.trim_end_matches(':').clone_into(&mut job);
            digest = None;
        }
        if let Some(name) = trimmed.strip_prefix("- name: ") {
            identity_step = name.trim_matches('"') == "V2 identity";
            cache_step = match name.trim_matches('"') {
                "Restore Mise tools" => Some(false),
                "Save Mise tools" => Some(true),
                _ => None,
            };
            continue;
        }
        if identity_step && let Some(value) = trimmed.strip_prefix("d: ") {
            digest = Some(value.to_owned());
            identity_step = false;
        }
        if let Some(is_save) = cache_step
            && let Some(key) = trimmed.strip_prefix("key: ")
            && let Some(digest) = &digest
        {
            let identity = (key.trim_matches('"').to_owned(), digest.clone());
            let is_seed = job == "dependency-seed" || job.starts_with("dependency-seed-");
            if is_save && is_seed {
                seed_saves.push(identity);
            } else if is_save {
                saves.push(identity);
            } else if is_seed {
                seed_restored.insert(identity);
            } else {
                restored.insert(identity);
            }
            cache_step = None;
        }
    }
    (restored, saves, seed_restored, seed_saves)
}
