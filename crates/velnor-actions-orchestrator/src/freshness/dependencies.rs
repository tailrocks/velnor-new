use std::fs;
use std::path::Path;

use toml::Value as TomlValue;

use super::context::{FreshnessContext, py_repr};
use super::dependency_graph::Declaration;

const CHECK: &str = "lock-staleness";

pub(super) fn check_effective_identity(ctx: &mut FreshnessContext) {
    super::nested_workspaces::check_all(ctx);
}

pub(super) fn read_manifest(ctx: &mut FreshnessContext, path: &Path) -> Option<TomlValue> {
    let subject = path.strip_prefix(&ctx.root).map_or_else(
        |_| path.display().to_string(),
        |path| path.display().to_string(),
    );
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            ctx.fail_row(CHECK, &subject, &format!("unreadable ({error})"));
            return None;
        }
    };
    match toml::from_str::<TomlValue>(&text) {
        Ok(document) => Some(document),
        Err(error) => {
            ctx.fail_row(CHECK, &subject, &format!("unreadable ({error})"));
            None
        }
    }
}

pub(super) fn walk_dep_tables(document: &TomlValue) -> Vec<(String, String, TomlValue)> {
    let mut found = Vec::new();
    collect_dep_tables(document, "", &mut found);
    found
}

fn collect_dep_tables(
    node: &TomlValue,
    prefix: &str,
    found: &mut Vec<(String, String, TomlValue)>,
) {
    let Some(table) = node.as_table() else {
        return;
    };
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(dependencies) = table.get(section).and_then(TomlValue::as_table) {
            let scope = format!("{prefix}{section}");
            for (alias, spec) in dependencies {
                found.push((scope.clone(), alias.clone(), spec.clone()));
            }
        }
    }
    if let Some(targets) = table.get("target").and_then(TomlValue::as_table) {
        for (target, value) in targets {
            collect_dep_tables(value, &format!("{prefix}target.{target}."), found);
        }
    }
}

pub(super) fn declared_dependency(
    ctx: &mut FreshnessContext,
    crate_name: &str,
    scope: &str,
    alias: &str,
    spec: &TomlValue,
    inherited: &TomlValue,
) -> Option<Declaration> {
    let subject = format!("{crate_name}:{scope}:{alias}");
    if let Some(requirement) = spec.as_str() {
        return Some(Declaration {
            subject,
            real: alias.to_owned(),
            requirement: TomlValue::String(requirement.to_owned()),
        });
    }
    let Some(spec_table) = spec.as_table() else {
        ctx.fail_row(
            CHECK,
            &subject,
            &format!("malformed spec {}", toml_repr(spec)),
        );
        return None;
    };
    if let Some(git) = spec_table.get("git").filter(|value| toml_truthy(value)) {
        ctx.fail_row(
            CHECK,
            &subject,
            &format!("git dependency forbidden ({})", toml_repr(git)),
        );
        return None;
    }
    let real = spec_table
        .get("package")
        .and_then(TomlValue::as_str)
        .unwrap_or(alias)
        .to_owned();
    if let Some(requirement) = spec_table.get("version") {
        return Some(Declaration {
            subject,
            real,
            requirement: requirement.clone(),
        });
    }
    if spec_table.get("workspace").and_then(TomlValue::as_bool) == Some(true) {
        return workspace_dependency(ctx, subject, alias, real, inherited);
    }
    if spec_table.contains_key("path") {
        ctx.pass_row(CHECK, &subject, "path-only, no registry identity");
        return None;
    }
    ctx.fail_row(
        CHECK,
        &subject,
        &format!("no version, workspace, or path in {}", toml_repr(spec)),
    );
    None
}

fn workspace_dependency(
    ctx: &mut FreshnessContext,
    subject: String,
    alias: &str,
    mut real: String,
    inherited: &TomlValue,
) -> Option<Declaration> {
    let inherited_table = inherited.as_table();
    let base = inherited_table.and_then(|table| table.get(&real).or_else(|| table.get(alias)));
    if let Some(package) = base
        .and_then(TomlValue::as_table)
        .and_then(|table| table.get("package"))
        .and_then(TomlValue::as_str)
    {
        package.clone_into(&mut real);
    }
    let requirement = match base {
        Some(TomlValue::String(value)) => Some(TomlValue::String(value.clone())),
        Some(TomlValue::Table(table)) => table.get("version").cloned(),
        _ => None,
    };
    let Some(requirement) = requirement else {
        ctx.fail_row(
            CHECK,
            &subject,
            &format!(
                "workspace inheritance unresolvable: [workspace.dependencies] lacks {}",
                py_repr(Some(&serde_json::json!(real)))
            ),
        );
        return None;
    };
    Some(Declaration {
        subject,
        real,
        requirement,
    })
}

fn toml_truthy(value: &TomlValue) -> bool {
    match value {
        TomlValue::String(value) => !value.is_empty(),
        TomlValue::Integer(value) => *value != 0,
        TomlValue::Float(value) => *value != 0.0,
        TomlValue::Boolean(value) => *value,
        TomlValue::Datetime(_) => true,
        TomlValue::Array(value) => !value.is_empty(),
        TomlValue::Table(value) => !value.is_empty(),
    }
}

fn toml_repr(value: &TomlValue) -> String {
    serde_json::to_value(value).map_or_else(|_| value.to_string(), |value| py_repr(Some(&value)))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn fixture(lock_version: &str, edge_version: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("fixture directory");
        let member = root.path().join("crates/demo");
        fs::create_dir_all(&member).expect("member directory");
        fs::write(
            root.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/demo\"]\n\
             [workspace.dependencies]\nserde_json = \"=1.0.100\"\n",
        )
        .expect("workspace manifest");
        fs::write(
            member.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\
             edition = \"2024\"\n\n[dependencies]\nserde_json = { workspace = true }\n",
        )
        .expect("member manifest");
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n\
             dependencies = [\"serde_json {edge_version} (registry+https://github.com/rust-lang/crates.io-index)\"]\n\n\
             [[package]]\nname = \"serde_json\"\nversion = \"{lock_version}\"\n\
             source = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
        );
        fs::write(root.path().join("Cargo.lock"), lock).expect("lockfile");
        root
    }

    fn check(root: &Path) -> FreshnessContext {
        let mut ctx = FreshnessContext::new(root, false, false);
        check_effective_identity(&mut ctx);
        ctx
    }

    #[test]
    fn inherited_exact_pin_and_complete_lock_graph_pass() {
        let root = fixture("1.0.100", "1.0.100");
        let ctx = check(root.path());
        assert!(ctx.failures.is_empty(), "{:?}", ctx.failures);
    }

    #[test]
    fn declared_version_missing_from_lock_fails() {
        let root = fixture("1.0.101", "1.0.101");
        let ctx = check(root.path());
        assert!(
            ctx.failures
                .iter()
                .any(|failure| failure.contains("has no locked identity")),
            "{:?}",
            ctx.failures
        );
    }
}
