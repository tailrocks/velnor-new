fn tools_cache_identities(yaml: &str) -> ToolsCacheIdentities {
    let mut restored = std::collections::BTreeSet::new();
    let mut saves = Vec::new();
    let mut digest = None;
    let mut identity_step = false;
    let mut cache_step = None;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if line.starts_with("  ") && !line.starts_with("   ") && trimmed.ends_with(':') {
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
            if is_save {
                saves.push(identity);
            } else {
                restored.insert(identity);
            }
            cache_step = None;
        }
    }
    (restored, saves)
}
