//! Qualification of manifest-only conservative dependency inventories.

/// Reject resolution overrides that `metadata --no-deps` cannot represent.
///
/// Explicit path dependencies, workspace inheritance, and every dependency
/// kind are represented by Cargo metadata. Registry patches/replacements
/// need a resolved graph; callers broaden instead of dropping their effects.
///
/// # Errors
///
/// Returns parse errors or `manifest_graph_requires_resolution` for overrides.
pub fn qualify_manifest_graph(text: &str) -> Result<(), String> {
    let document: toml::Table = toml::from_str(text).map_err(|error| error.to_string())?;
    if document.contains_key("patch") || document.contains_key("replace") {
        return Err("manifest_graph_requires_resolution".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::qualify_manifest_graph;

    #[test]
    fn resolution_overrides_are_unknown() {
        for table in ["patch.crates-io", "replace"] {
            let text = format!("[{table}]\nshared = {{ path = '../shared' }}\n");
            assert_eq!(
                qualify_manifest_graph(&text),
                Err("manifest_graph_requires_resolution".to_owned())
            );
        }
        assert!(
            qualify_manifest_graph("[workspace.dependencies]\nshared = { path = 'shared' }\n")
                .is_ok()
        );
    }
}
