//! Cargo workspace discovery and locked dependency identity checks.

mod lock;
mod workspace;

use std::fs;
use std::path::Path;

use toml::Value;

use crate::context::FreshnessContext;

const DEP_SECTIONS: &[&str] = &["dependencies", "dev-dependencies", "build-dependencies"];
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

#[derive(Debug, Clone)]
pub(super) struct DeclaredDependency {
    pub(super) subject: String,
    pub(super) name: String,
    pub(super) requirement: String,
}

/// Discover every workspace and compare its declarations with its lock graph.
pub(crate) fn check_effective_identity(ctx: &mut FreshnessContext) {
    ctx.workspace_roots = workspace::discover_workspaces(ctx);
    ctx.locked.clear();
    ctx.member_names.clear();
    for (relative, document) in ctx.workspace_roots.clone() {
        let locked = lock::check_workspace(ctx, &relative, &document);
        ctx.locked.extend(locked.clone());
        for entry in locked {
            if entry.get("source").and_then(Value::as_str).is_none()
                && let Some(name) = entry.get("name").and_then(Value::as_str)
            {
                ctx.member_names.insert(name.to_owned());
            }
        }
    }
}

pub(super) fn read_toml_row(ctx: &mut FreshnessContext, path: &Path, check: &str) -> Option<Value> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        ctx.root.join(path)
    };
    let relative = path_text(absolute.strip_prefix(&ctx.root).unwrap_or(&absolute));
    match fs::read_to_string(&absolute)
        .map_err(|error| error.to_string())
        .and_then(|text| toml::from_str(&text).map_err(|error| error.to_string()))
    {
        Ok(document) => Some(document),
        Err(error) => {
            ctx.fail_row(check, &relative, &format!("unreadable ({error})"));
            None
        }
    }
}

pub(super) fn load_manifest(ctx: &mut FreshnessContext, relative: &str) -> Option<Value> {
    read_toml_row(ctx, Path::new(relative), "lock-staleness")
}

pub(super) fn manifest_label(relative: &str) -> String {
    if relative.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{relative}/Cargo.toml")
    }
}

pub(super) fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
