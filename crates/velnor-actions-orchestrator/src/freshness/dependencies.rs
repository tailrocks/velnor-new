use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use super::context::{FreshnessContext, py_repr};
use super::dependency_graph::{self, Declaration};

const CHECK: &str = "lock-staleness";

pub(super) fn check_effective_identity(ctx: &mut FreshnessContext) {
    let inherited = read_workspace(ctx);
    ctx.locked = read_lock(ctx);
    let (Some(inherited), Some(locked)) = (inherited, ctx.locked.clone()) else {
        return;
    };
    let (manifests, declared) = read_manifests(ctx, &inherited);
    let members = ctx.member_names.clone();
    dependency_graph::check(ctx, &locked, &declared, &members);
    check_lock_mtime(ctx, &manifests);
}

fn read_workspace(ctx: &mut FreshnessContext) -> Option<TomlValue> {
    let path = ctx.path("Cargo.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            ctx.fail_row(
                CHECK,
                "workspace Cargo.toml",
                &format!("unreadable ({error})"),
            );
            return None;
        }
    };
    let document = match toml::from_str::<TomlValue>(&text) {
        Ok(document) => document,
        Err(error) => {
            ctx.fail_row(
                CHECK,
                "workspace Cargo.toml",
                &format!("unreadable ({error})"),
            );
            return None;
        }
    };
    Some(
        document
            .get("workspace")
            .and_then(TomlValue::as_table)
            .and_then(|workspace| workspace.get("dependencies"))
            .cloned()
            .unwrap_or_else(|| TomlValue::Table(toml::map::Map::new())),
    )
}

fn read_lock(ctx: &mut FreshnessContext) -> Option<Vec<JsonValue>> {
    let path = ctx.path("Cargo.lock");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            ctx.fail_row(CHECK, "Cargo.lock", &format!("unreadable ({error})"));
            return None;
        }
    };
    let document = match toml::from_str::<TomlValue>(&text) {
        Ok(document) => document,
        Err(error) => {
            ctx.fail_row(CHECK, "Cargo.lock", &format!("unreadable ({error})"));
            return None;
        }
    };
    let packages = document
        .get("package")
        .and_then(TomlValue::as_array)
        .cloned()
        .unwrap_or_default();
    packages
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_or_else(
            |error| {
                ctx.fail_row(CHECK, "Cargo.lock", &format!("unreadable ({error})"));
                None
            },
            Some,
        )
}

fn read_manifests(
    ctx: &mut FreshnessContext,
    inherited: &TomlValue,
) -> (Vec<PathBuf>, Vec<Declaration>) {
    let manifests = manifest_paths(ctx);
    if manifests.is_empty() {
        ctx.fail_row(CHECK, "crates/*/Cargo.toml", "no members found");
    }
    let mut members = BTreeSet::new();
    let mut declared = Vec::new();
    for manifest in &manifests {
        let Some(document) = read_manifest(ctx, manifest) else {
            continue;
        };
        let Some(package_name) = document
            .get("package")
            .and_then(TomlValue::as_table)
            .and_then(|package| package.get("name"))
            .and_then(TomlValue::as_str)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        members.insert(package_name.to_owned());
        for (scope, alias, spec) in walk_dep_tables(&document) {
            if let Some(item) =
                declared_dependency(ctx, package_name, &scope, &alias, &spec, inherited)
            {
                declared.push(item);
            }
        }
    }
    ctx.member_names = members;
    (manifests, declared)
}

fn manifest_paths(ctx: &FreshnessContext) -> Vec<PathBuf> {
    let crates = ctx.path("crates");
    let mut manifests = fs::read_dir(crates)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("Cargo.toml"))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    manifests.sort();
    manifests
}

fn read_manifest(ctx: &mut FreshnessContext, path: &Path) -> Option<TomlValue> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            ctx.fail_row(
                CHECK,
                &path.display().to_string(),
                &format!("unreadable ({error})"),
            );
            return None;
        }
    };
    match toml::from_str::<TomlValue>(&text) {
        Ok(document) => Some(document),
        Err(error) => {
            ctx.fail_row(
                CHECK,
                &path.display().to_string(),
                &format!("unreadable ({error})"),
            );
            None
        }
    }
}

fn walk_dep_tables(document: &TomlValue) -> Vec<(String, String, TomlValue)> {
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

fn declared_dependency(
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

fn check_lock_mtime(ctx: &mut FreshnessContext, manifests: &[PathBuf]) {
    let lock_time = fs::metadata(ctx.path("Cargo.lock")).and_then(|metadata| metadata.modified());
    let manifest_times = manifests
        .iter()
        .map(|path| fs::metadata(path).and_then(|metadata| metadata.modified()))
        .collect::<Result<Vec<_>, _>>();
    match (lock_time, manifest_times) {
        (Ok(lock_time), Ok(times)) if !times.is_empty() => {
            let newest_manifest = times.into_iter().max().unwrap_or(lock_time);
            ctx.info_row(
                "lock-mtime",
                "Cargo.lock",
                &format!("lock_is_newest={}", lock_time >= newest_manifest),
            );
        }
        (Err(error), _) | (_, Err(error)) => {
            ctx.info_row(
                "lock-mtime",
                "Cargo.lock",
                &format!("mtime unreadable ({error})"),
            );
        }
        (_, Ok(_)) => ctx.info_row(
            "lock-mtime",
            "Cargo.lock",
            "mtime unreadable (no manifests)",
        ),
    }
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
