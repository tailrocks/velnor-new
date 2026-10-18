//! Exact YAML parser dependencies for the workflow renderer.

use std::error::Error;

use crate::impl_repo_policy::{MEMBERS, manifest, p11_toml};

const OWNER: &str = "crates/velnor-actions-workflow-renderer";
const PARSERS: [&str; 2] = ["saphyr", "saphyr-parser"];

/// Accept only the renderer's exact parser versions without default features.
pub(crate) fn reviewed_workflow_yaml_dependency(dir: &str, key: &str, line: &str) -> bool {
    if dir != OWNER || !PARSERS.contains(&key) {
        return false;
    }
    let Some((name, value)) = line.split_once('=') else {
        return false;
    };
    if name.trim() != key {
        return false;
    }
    let fields = p11_toml::inline_pairs(value);
    fields.len() == 2
        && fields
            .iter()
            .any(|(name, value)| name == "version" && value == "=0.2.2")
        && fields
            .iter()
            .any(|(name, value)| name == "default-features" && value == "false")
}

#[test]
fn workflow_yaml_policy_rejects_other_owners_versions_and_options() {
    for parser in PARSERS {
        let line = format!("{parser} = {{ version = \"=0.2.2\", default-features = false }}");
        assert!(reviewed_workflow_yaml_dependency(OWNER, parser, &line));
        for owner in [
            "crates/velnor-actions-cli",
            "crates/velnor-actions-orchestrator",
        ] {
            assert!(!reviewed_workflow_yaml_dependency(owner, parser, &line));
        }
        for altered in [
            line.replace("=0.2.2", "0.2.2"),
            line.replace("=0.2.2", "=0.2.1"),
            line.replace("false", "true"),
            line.replace(", default-features = false", ""),
            line.replace(" }", ", features = [\"debug_prints\"] }"),
            line.replace(" }", ", optional = true }"),
            line.replace(" }", ", package = \"other\" }"),
            line.replace(" }", ", path = \"../other\" }"),
            line.replace(" }", ", git = \"https://example.invalid/other\" }"),
            line.replace(" }", ", version = \"=0.2.2\" }"),
        ] {
            assert!(!reviewed_workflow_yaml_dependency(OWNER, parser, &altered));
        }
        assert!(!reviewed_workflow_yaml_dependency(OWNER, "other", &line));
    }
}

fn dependency_sections_are_exact(dir: &str, text: &str) -> Result<bool, Box<dyn Error>> {
    let doc = p11_toml::parse(text)?;
    for section in doc.sections {
        if PARSERS
            .iter()
            .any(|parser| section.name.ends_with(&format!(".{parser}")))
        {
            return Ok(false);
        }
        for (key, value) in section.pairs {
            let alias = p11_toml::inline_pairs(&value)
                .iter()
                .any(|(name, value)| name == "package" && PARSERS.contains(&value.as_str()));
            if alias {
                return Ok(false);
            }
            if PARSERS.contains(&key.as_str())
                && (section.name != "dependencies"
                    || !reviewed_workflow_yaml_dependency(dir, &key, &format!("{key} = {value}")))
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[test]
fn workflow_yaml_dependencies_stay_in_the_renderer_normal_dependencies()
-> Result<(), Box<dyn Error>> {
    for (dir, _) in MEMBERS {
        assert!(
            dependency_sections_are_exact(dir, &manifest(dir)?)?,
            "{dir}"
        );
    }
    for section in [
        "dev-dependencies",
        "build-dependencies",
        "target.'cfg(unix)'.dependencies",
        "dependencies.saphyr",
    ] {
        let text =
            format!("[{section}]\nsaphyr = {{ version = \"=0.2.2\", default-features = false }}\n");
        assert!(!dependency_sections_are_exact(OWNER, &text)?);
    }
    for text in [
        "[dependencies.saphyr]\nversion = \"=0.2.2\"\ndefault-features = false\n",
        "[dependencies]\nserde = { package = \"saphyr\", version = \"=0.2.2\", default-features = false }\n",
    ] {
        assert!(!dependency_sections_are_exact(OWNER, text)?);
    }
    Ok(())
}
