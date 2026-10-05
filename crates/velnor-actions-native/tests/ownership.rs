//! Conservative source boundary gate, not a Rust parser or helper interpreter.
//! Checks owned source placement, visible references and literal helper closures.

use std::path::{Path, PathBuf};

#[path = "ownership_ast.rs"]
mod rust_ast;

const DOMAINS: &[&str] = &[
    "apt", "homebrew", "java", "node", "oci", "reuse", "ruby", "shell", "swift",
];

fn files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(std::io::Error::other(format!(
                "source_symlink:{}",
                path.display()
            )));
        }
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "__pycache__") {
                continue;
            }
            found.extend(files(&path)?);
        } else {
            found.push(path);
        }
    }
    Ok(found)
}

fn boundary_problem(owner: &str, text: &str) -> Option<String> {
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    for domain in DOMAINS.iter().filter(|domain| **domain != owner) {
        if compact.contains(&format!("{domain}::"))
            || compact.contains(&format!("::{domain};"))
            || compact.contains(&format!("::{domain}as"))
            || compact.contains(&format!("use{domain}as"))
            || [
                format!("{{{domain}}}"),
                format!("{{{domain},"),
                format!(",{domain}}}"),
                format!(",{domain},"),
                format!("{{{domain}as"),
                format!(",{domain}as"),
            ]
            .iter()
            .any(|pattern| compact.contains(pattern))
            || compact.contains(&format!("/{domain}/"))
            || compact.contains(&format!("from{domain}import"))
            || compact.contains(&format!("import{domain}."))
        {
            return Some(format!("sibling_domain:{domain}"));
        }
    }
    for token in [
        "velnor_actions_mise",
        "velnor_actions_workflow_renderer",
        "velnor_actions_rust",
        "velnor_actions_tofu",
        "velnor_actions_orchestrator",
        "std::process",
        "::{process",
        ",processas",
        "Command::new",
        "include!",
        "macro_rules!",
    ] {
        if compact.contains(token) {
            return Some(format!("forbidden_owner_or_execution:{token}"));
        }
    }
    if let Some(problem) = source_include_problem(&compact) {
        return Some(problem);
    }
    None
}

fn source_include_problem(text: &str) -> Option<String> {
    for selector in ["include_str!", "include_bytes!", "#[path="] {
        let mut rest = text;
        while let Some((_, tail)) = rest.split_once(selector) {
            let literal = if selector == "#[path=" {
                tail
            } else {
                let Some(literal) = tail.strip_prefix(['(', '{', '[']) else {
                    return Some("malformed_source_include".to_owned());
                };
                literal
            };
            let Some(literal) = literal.strip_prefix('"') else {
                return Some("dynamic_or_raw_source_include".to_owned());
            };
            let Some((path, after)) = literal.split_once('"') else {
                return Some("unterminated_source_include".to_owned());
            };
            if path.is_empty()
                || path.starts_with('/')
                || path.contains('\\')
                || path.split('/').any(|part| matches!(part, ".." | "." | ""))
                || !after.starts_with([')', '}', ']'])
            {
                return Some("cross_owner_or_computed_source_include".to_owned());
            }
            rest = after;
        }
    }
    None
}

fn helper_problem(owner: &str, text: &str) -> Option<String> {
    for (domain, prefix) in [
        ("apt", "delivery_apt_"),
        ("swift", "desktop_native"),
        ("oci", "oci_digest"),
        ("java", "workloads_cache_gradle"),
        ("node", "workloads_cache_npm"),
        ("node", "workloads_cache_bun"),
        ("homebrew", "package_update_"),
    ] {
        if owner != domain && text.contains(prefix) {
            return Some(format!("foreign_helper_family:{domain}"));
        }
    }
    for token in [
        "__import__(",
        "importlib.import_module(",
        "importlib.util.",
        "\"cargo\"",
        "'cargo'",
        "\"rustup\"",
        "'rustup'",
        "\"mise\"",
        "'mise'",
    ] {
        if text.contains(token) {
            return Some(format!("dynamic_or_foreign_helper:{token}"));
        }
    }
    None
}

fn test_source(path: &Path, paths: &[PathBuf]) -> std::io::Result<bool> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(false);
    };
    if !(name == "tests.rs" || name.ends_with("_tests.rs")) {
        return Ok(false);
    }
    for parent in paths
        .iter()
        .filter(|parent| parent.extension().is_some_and(|ext| ext == "rs"))
    {
        if parent == path || parent.parent() != path.parent() {
            continue;
        }
        let text = std::fs::read_to_string(parent)?;
        let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
        let explicit = format!("#[cfg(test)]#[path=\"{name}\"]modtests;");
        let ordinary = "#[cfg(test)]modtests;";
        let declaration = if name == "tests.rs" && compact.contains(ordinary) {
            ordinary
        } else {
            explicit.as_str()
        };
        if compact.contains(declaration) {
            let production = compact.replace(declaration, "");
            if production.contains(&format!("\"{name}\"")) || production.contains("modtests;") {
                return Err(std::io::Error::other(
                    "test_source_also_referenced_by_product",
                ));
            }
            return Ok(true);
        }
    }
    Ok(false)
}

fn python_test_source(path: &Path, paths: &[PathBuf]) -> std::io::Result<bool> {
    let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
        return Ok(false);
    };
    if path.extension().is_none_or(|ext| ext != "py") || !stem.ends_with("_test") {
        return Ok(false);
    }
    let reference = format!("\"{stem}\"");
    for rust in paths
        .iter()
        .filter(|rust| rust.extension().is_some_and(|ext| ext == "rs"))
    {
        if test_source(rust, paths)? && std::fs::read_to_string(rust)?.contains(&reference) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn root_problem(text: &str) -> Option<String> {
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
    {
        let compact: String = line.chars().filter(|ch| !ch.is_whitespace()).collect();
        if compact == "modsupport;"
            || compact == "pubusesupport::{OwnedSupportFile,SupportBundle};"
            || DOMAINS
                .iter()
                .any(|domain| compact == format!("pubmod{domain};"))
        {
            continue;
        }
        return Some(format!("root_must_only_declare_owners:{line}"));
    }
    None
}

#[test]
fn native_domains_have_no_sibling_owner_or_tool_dependencies()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&root)? {
        let path = entry?.path();
        assert!(!std::fs::symlink_metadata(&path)?.file_type().is_symlink());
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid_source_name")?;
        assert!(
            DOMAINS.contains(&name) || matches!(name, "lib.rs" | "support.rs"),
            "unowned_source:{}",
            path.display()
        );
    }
    assert_eq!(
        root_problem(&std::fs::read_to_string(root.join("lib.rs"))?),
        None
    );
    let support = root.join("support.rs");
    if support.exists() {
        assert_eq!(
            boundary_problem("support", &std::fs::read_to_string(support)?),
            None
        );
    }
    for domain in DOMAINS {
        let dir = root.join(domain);
        if !dir.exists() {
            continue;
        }
        let paths = files(&dir)?;
        for path in &paths {
            if test_source(path, &paths)? || python_test_source(path, &paths)? {
                continue;
            }
            let text = std::fs::read_to_string(path)?;
            if path.extension().is_some_and(|ext| ext == "rs") {
                assert_eq!(rust_ast::problem(domain, &text), None, "{}", path.display());
            } else {
                assert_eq!(boundary_problem(domain, &text), None, "{}", path.display());
            }
            if path
                .extension()
                .is_some_and(|ext| ext == "py" || ext == "sh")
            {
                assert_eq!(helper_problem(domain, &text), None, "{}", path.display());
            }
        }
    }
    Ok(())
}

#[test]
fn qualified_sibling_reference_is_rejected() {
    assert!(boundary_problem("apt", "crate :: swift :: producer()").is_some());
}

#[test]
fn sibling_import_is_rejected() {
    assert!(boundary_problem("apt", "use super::oci::{Policy};").is_some());
}

#[test]
fn sibling_reexport_is_rejected() {
    assert!(boundary_problem("swift", "pub use crate::node::Profile;").is_some());
}

#[test]
fn unbraced_sibling_module_alias_is_rejected() {
    for text in [
        "use crate::ruby as r; r::syntax(paths);",
        "pub use super::ruby as r;",
        "use crate::ruby;",
    ] {
        assert!(boundary_problem("shell", text).is_some(), "{text}");
    }
}

#[test]
fn source_path_escape_is_rejected() {
    assert!(boundary_problem("apt", "#[path = \"../oci/policy.rs\"] mod policy;").is_some());
}

#[test]
fn helper_include_escape_is_rejected() {
    assert!(boundary_problem("apt", "include_str!(\"../oci/entry.py\")").is_some());
}

#[test]
fn semantic_macro_include_is_rejected() {
    assert!(boundary_problem("apt", "include!(\"policy.rs\")").is_some());
}

#[test]
fn mise_owner_dependency_is_rejected() {
    assert!(boundary_problem("java", "velnor_actions_mise::catalog::JAVA_VERSION").is_some());
}

#[test]
fn renderer_owner_dependency_is_rejected() {
    assert!(boundary_problem("oci", "velnor_actions_workflow_renderer::Yaml").is_some());
}

#[test]
fn sibling_python_helper_import_is_rejected() {
    assert!(boundary_problem("apt", "from swift import native_build").is_some());
}

#[test]
fn own_relative_helper_include_is_accepted() {
    assert_eq!(boundary_problem("apt", "include_str!(\"core.py\")"), None);
}

#[test]
fn contract_reference_is_accepted() {
    assert_eq!(
        boundary_problem("apt", "use velnor_actions_contract::ContractError;"),
        None
    );
}

#[test]
fn brace_aliases_macro_definitions_and_computed_includes_are_rejected() {
    for text in [
        "use crate::{swift as s};",
        "pub use crate::{swift};",
        "use crate::{ruby, swift as s};",
        "macro_rules! bridge { () => { foreign() } }",
        "include_str! { concat!(\"../\", \"oci/core.py\") }",
        "include_bytes!(\"core.py\".to_owned())",
        "#[path =\n\"../oci/core.rs\"] mod core;",
        "use std::{process as p};",
    ] {
        assert!(boundary_problem("apt", text).is_some(), "{text}");
    }
}

#[test]
fn root_dispatchers_and_foreign_helper_execution_are_rejected() {
    assert!(root_problem("pub fn dispatch() {} ").is_some());
    assert!(root_problem("pub use swift::producer;").is_some());
    for text in [
        "from delivery_apt_core import build",
        "subprocess.run(['cargo', 'build'])",
        "importlib.import_module(owner)",
    ] {
        assert!(helper_problem("swift", text).is_some(), "{text}");
    }
}

#[test]
fn test_file_requires_an_actual_cfg_test_declaration() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::temp_dir().join(format!("native-boundary-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let parent = directory.join("mod.rs");
    let test = directory.join("tests.rs");
    let paths = vec![parent.clone(), test.clone()];
    std::fs::write(&parent, "mod tests;")?;
    assert!(!test_source(&test, &paths)?);
    std::fs::write(&parent, "#[cfg(test)] mod tests;")?;
    assert!(test_source(&test, &paths)?);
    std::fs::write(&parent, "#[cfg(test)] mod tests; mod tests;")?;
    assert!(test_source(&test, &paths).is_err());
    std::fs::remove_dir_all(&directory)?;
    Ok(())
}
