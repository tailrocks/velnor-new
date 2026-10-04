//! Pure immutable Python closure emission for the source-bound APT launcher.

use std::collections::BTreeMap;
use velnor_actions_contract::{ContractError, canonical_json_str, marker_for_version};

const LOADER: &str = include_str!("delivery_apt_loader.py");

/// Emit the complete entry body; no checkout companion grants authority.
pub(super) fn standalone_body(version: &str) -> Result<String, ContractError> {
    let marker = canonical_json_str(&marker_for_version(version)?)?;
    let sources = canonical_json_str(&source_map())?;
    if LOADER.matches("__APT_COMPILED_SOURCES__").count() != 1
        || LOADER.matches("__APT_COMPILED_MARKER__").count() != 1
    {
        return Err(ContractError::identity(
            "apt_source",
            "invalid_compiled_loader",
        ));
    }
    let body = LOADER
        .replacen("__APT_COMPILED_MARKER__", &marker, 1)
        .replacen("__APT_COMPILED_SOURCES__", &sources, 1);
    Ok(body)
}

fn source_map() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("delivery_apt_core", include_str!("delivery_apt_core.py")),
        (
            "delivery_apt_verify",
            include_str!("delivery_apt_verify.py"),
        ),
        (
            "delivery_apt_stage_feed",
            include_str!("delivery_apt_stage_feed.py"),
        ),
        (
            "delivery_apt_stage_publish",
            include_str!("delivery_apt_stage_publish.py"),
        ),
        ("delivery_apt_stage", include_str!("delivery_apt_stage.py")),
        ("delivery_apt_entry", include_str!("delivery_apt_entry.py")),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immutable_entry_binds_complete_module_closure() {
        let source = standalone_body("0.1.0").expect("complete immutable source");
        assert_eq!(source, standalone_body("0.1.0").expect("same source"));
        assert!(
            source.contains(
                &canonical_json_str(&marker_for_version("0.1.0").expect("marker"))
                    .expect("quoted marker")
            )
        );
        assert!(!source.contains("__APT_COMPILED_SOURCES__"));
        assert!(!source.contains("__APT_COMPILED_MARKER__"));
        for (name, body) in source_map() {
            assert!(source.contains(&canonical_json_str(&name).expect("module name")));
            assert!(source.contains(&canonical_json_str(&body).expect("module source")));
        }
        assert!(source.len() <= 262_144);
    }

    #[test]
    fn invalid_version_cannot_emit_immutable_entry() {
        assert!(standalone_body("bad\nmarker").is_err());
    }
}
