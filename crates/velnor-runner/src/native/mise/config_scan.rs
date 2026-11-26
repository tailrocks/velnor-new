// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::exec::ExecError;

const CONFIG: &str = "mise.toml";
const TOOL_VERSIONS: &str = ".tool-versions";
const IDIOMATIC: &str = "idiomatic_version_file_enable_tools";
const REDEFINING_VARS: [&str; 3] = [
    "MISE_OVERRIDE_CONFIG_FILENAMES",
    "MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES",
    "MISE_IDIOMATIC_VERSION_FILE_ENABLE_TOOLS",
];
const FILE_VARS: [&str; 3] = [
    "MISE_CONFIG_FILE",
    "MISE_GLOBAL_CONFIG_FILE",
    "MISE_SYSTEM_CONFIG_FILE",
];
const FAMILIES: [(&str, &str); 11] = [
    ("", "mise"),
    ("", ".mise"),
    ("", ".miserc"),
    (".config", "mise"),
    (".config", "miserc"),
    (".config/mise", "config"),
    (".config/mise", "mise"),
    (".mise", "config"),
    (".mise", "mise"),
    ("mise", "config"),
    ("mise", "mise"),
];
const CONF_D: [&str; 3] = [".config/mise/conf.d", ".mise/conf.d", "mise/conf.d"];

type Env = BTreeMap<String, String>;

fn lookup(env: &Env, name: &str) -> Option<String> {
    env.get(name)
        .cloned()
        .or_else(|| std::env::var(name).ok())
        .filter(|value| !value.is_empty())
}

fn is_toml(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
}

fn is_family(name: &str, stem: &str) -> bool {
    name.starts_with(&format!("{stem}.")) && is_toml(name)
}

async fn exists(path: &Path) -> bool {
    tokio::fs::symlink_metadata(path).await.is_ok()
}

async fn entries(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(mut reader) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut listed = Vec::new();
    while let Ok(Some(entry)) = reader.next_entry().await {
        listed.push((
            entry.file_name().to_string_lossy().into_owned(),
            entry.path(),
        ));
    }
    listed
}

async fn fragments(conf_d: &Path, found: &mut BTreeSet<PathBuf>) {
    for (name, path) in entries(conf_d).await {
        if is_toml(&name) {
            found.insert(path);
            continue;
        }
        for (inner, inner_path) in entries(&path).await {
            if is_family(&inner, "mise") {
                found.insert(inner_path);
            }
        }
    }
}

async fn project_files(dir: &Path, found: &mut BTreeSet<PathBuf>) {
    if exists(&dir.join(TOOL_VERSIONS)).await {
        found.insert(dir.join(TOOL_VERSIONS));
    }
    for (sub, stem) in FAMILIES {
        let parent = dir.join(sub);
        for (name, path) in entries(&parent).await {
            if is_family(&name, stem) {
                found.insert(path);
            }
        }
    }
    for sub in CONF_D {
        fragments(&dir.join(sub), found).await;
    }
}

async fn user_dir_files(dir: &Path, found: &mut BTreeSet<PathBuf>) {
    for (name, path) in entries(dir).await {
        if is_toml(&name) {
            found.insert(path);
        }
    }
    fragments(&dir.join("conf.d"), found).await;
}

async fn canonical(path: &Path) -> PathBuf {
    tokio::fs::canonicalize(path)
        .await
        .unwrap_or_else(|_| path.to_path_buf())
}

async fn ceilings(env: &Env, workspace: &Path) -> BTreeSet<PathBuf> {
    let mut forms = BTreeSet::new();
    let listed = lookup(env, "MISE_CEILING_PATHS").unwrap_or_default();
    for path in std::env::split_paths(&listed).filter(|path| !path.as_os_str().is_empty()) {
        let path = workspace.join(path);
        forms.insert(canonical(&path).await);
        forms.insert(path);
    }
    forms
}

async fn require_no_idiomatic_files(workspace: &Path) -> Result<(), ExecError> {
    let Ok(text) = tokio::fs::read_to_string(workspace.join(CONFIG)).await else {
        return Ok(());
    };
    let Ok(config) = text.parse::<toml::Table>() else {
        return Ok(());
    };
    let enabled = config
        .get("settings")
        .and_then(toml::Value::as_table)
        .and_then(|settings| settings.get(IDIOMATIC))
        .is_some_and(|value| value.as_array().is_none_or(|tools| !tools.is_empty()));
    if enabled {
        return Err(ExecError::Unsupported(format!(
            "mise policy: {CONFIG} sets {IDIOMATIC}, so mise would load idiomatic version files, which velnor-runner does not support"
        )));
    }
    Ok(())
}

pub(super) async fn require_workspace_config_only(
    env: &Env,
    workspace: &Path,
) -> Result<(), ExecError> {
    for name in REDEFINING_VARS {
        if lookup(env, name).is_some() {
            return Err(ExecError::Unsupported(format!(
                "mise policy: {name} changes which files mise loads, which velnor-runner does not support"
            )));
        }
    }
    let root = canonical(workspace).await;
    let expected = root.join(CONFIG);
    require_no_idiomatic_files(&root).await?;
    let home = lookup(env, "HOME").ok_or_else(|| {
        ExecError::Unsupported(
            "mise policy: HOME is not set, so mise's global config cannot be located".to_owned(),
        )
    })?;
    let mut found = BTreeSet::new();
    let stops = ceilings(env, &root).await;
    for dir in root.ancestors().take_while(|dir| !stops.contains(*dir)) {
        project_files(dir, &mut found).await;
    }
    let path_var = |name: &str| lookup(env, name).map(|value| root.join(value));
    let global = path_var("MISE_CONFIG_DIR").unwrap_or_else(|| {
        path_var("XDG_CONFIG_HOME")
            .unwrap_or_else(|| Path::new(&home).join(".config"))
            .join("mise")
    });
    let system = path_var("MISE_SYSTEM_CONFIG_DIR")
        .or_else(|| path_var("MISE_SYSTEM_DIR"))
        .unwrap_or_else(|| PathBuf::from("/etc/mise"));
    user_dir_files(&global, &mut found).await;
    user_dir_files(&system, &mut found).await;
    let mut named: Vec<PathBuf> = FILE_VARS.iter().filter_map(|name| path_var(name)).collect();
    named.push(Path::new(&home).join(TOOL_VERSIONS));
    for path in named {
        if exists(&path).await {
            found.insert(path);
        }
    }
    found.remove(&expected);
    let mut listed = BTreeSet::new();
    for path in &found {
        let resolved = canonical(path).await;
        if resolved != expected {
            listed.insert(resolved.display().to_string());
        }
    }
    if listed.is_empty() {
        return Ok(());
    }
    Err(ExecError::Unsupported(format!(
        "mise policy: mise would load [{}] but only {} is supported by velnor-runner",
        listed.into_iter().collect::<Vec<_>>().join(", "),
        expected.display()
    )))
}
