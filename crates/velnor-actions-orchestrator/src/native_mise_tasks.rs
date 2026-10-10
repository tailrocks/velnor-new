//! Exact task-graph and task-tool projection for compile-free Mise jobs.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::is_valid_mise_task_name;

use crate::native_tool_input::NativeMiseTask;

const MAX_TASK_CLOSURE: usize = 128;

pub(crate) fn parse_task(value: &toml::Value) -> NativeMiseTask {
    let Some(table) = value.as_table() else {
        return NativeMiseTask {
            valid_shape: false,
            ..NativeMiseTask::default()
        };
    };
    let mut unsupported_fields = table
        .keys()
        .filter(|key| {
            !matches!(
                key.as_str(),
                "run" | "description" | "usage" | "depends" | "tools"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    for key in ["description", "usage"] {
        if table.get(key).is_some_and(|value| value.as_str().is_none()) {
            unsupported_fields.push(format!("{key}.shape"));
        }
    }
    let dependencies = string_list(table.get("depends"), &mut unsupported_fields, "depends");
    let task_tools = string_map(table.get("tools"), &mut unsupported_fields, "tools");
    NativeMiseTask {
        run_field_present: table.contains_key("run"),
        run_commands: parse_task_run(table.get("run")),
        dependencies,
        task_tools,
        unsupported_fields,
        valid_shape: true,
    }
}

pub(crate) fn selected_task_tools(
    config: &crate::native_tool_input::NativeMiseConfig,
    task_name: &str,
) -> Result<BTreeMap<String, String>, &'static str> {
    selected_task_tools_with_run_policy(config, task_name, true)
}

/// Collect tools in a build task's bounded graph, allowing dependency-only
/// tasks while still requiring every leaf to perform a command.
pub(crate) fn selected_build_task_tools(
    config: &crate::native_tool_input::NativeMiseConfig,
    task_name: &str,
) -> Result<BTreeMap<String, String>, &'static str> {
    selected_task_tools_with_run_policy(config, task_name, false)
}

fn selected_task_tools_with_run_policy(
    config: &crate::native_tool_input::NativeMiseConfig,
    task_name: &str,
    require_inline_run: bool,
) -> Result<BTreeMap<String, String>, &'static str> {
    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    let mut tools = BTreeMap::new();
    let mut count = 0;
    visit(
        config,
        task_name,
        &mut active,
        &mut complete,
        &mut tools,
        &mut count,
        require_inline_run,
    )?;
    Ok(tools)
}

fn visit(
    config: &crate::native_tool_input::NativeMiseConfig,
    name: &str,
    active: &mut BTreeSet<String>,
    complete: &mut BTreeSet<String>,
    tools: &mut BTreeMap<String, String>,
    count: &mut usize,
    require_inline_run: bool,
) -> Result<(), &'static str> {
    if complete.contains(name) {
        return Ok(());
    }
    if !is_valid_mise_task_name(name) || !active.insert(name.to_owned()) {
        return Err("verification_task_graph");
    }
    *count += 1;
    if *count > MAX_TASK_CLOSURE {
        return Err("verification_task_graph_bound");
    }
    let task = config.tasks.get(name).ok_or("verification_task_missing")?;
    if !task.valid_shape || !task.unsupported_fields.is_empty() {
        return Err("verification_task_shape");
    }
    if require_inline_run && !task.run_field_present {
        return Err("verification_task_inline_run_required");
    }
    if task.run_field_present && task.run_commands.is_none() {
        return Err("verification_task_run_shape");
    }
    for (key, version) in &task.task_tools {
        if !safe_tool_key(key) || !safe_version(version) {
            return Err("verification_task_tool_shape");
        }
        if tools
            .insert(key.clone(), version.clone())
            .is_some_and(|old| old != *version)
        {
            return Err("verification_task_tool_conflict");
        }
    }
    let mut children = BTreeSet::new();
    for dependency in &task.dependencies {
        if !is_valid_mise_task_name(dependency) {
            return Err("verification_task_dependency");
        }
        children.insert(dependency.clone());
    }
    for command in task.run_commands.iter().flatten() {
        for line in command.lines() {
            let line = line.trim();
            if !line.contains("mise") {
                continue;
            }
            let words = line.split_whitespace().collect::<Vec<_>>();
            if words.len() != 3
                || words[0] != "mise"
                || words[1] != "run"
                || !is_valid_mise_task_name(words[2])
            {
                return Err("verification_task_nested_call");
            }
            children.insert(words[2].to_owned());
        }
    }
    if !require_inline_run && !task.run_field_present && children.is_empty() {
        return Err("build_task_task_empty");
    }
    for child in children {
        visit(
            config,
            &child,
            active,
            complete,
            tools,
            count,
            require_inline_run,
        )?;
    }
    active.remove(name);
    complete.insert(name.to_owned());
    Ok(())
}

fn string_list(
    value: Option<&toml::Value>,
    unsupported: &mut Vec<String>,
    name: &str,
) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        unsupported.push(format!("{name}.shape"));
        return Vec::new();
    };
    items
        .iter()
        .map(toml::Value::as_str)
        .collect::<Option<Vec<_>>>()
        .map_or_else(
            || {
                unsupported.push(format!("{name}.shape"));
                Vec::new()
            },
            |items| items.into_iter().map(ToOwned::to_owned).collect(),
        )
}

fn string_map(
    value: Option<&toml::Value>,
    unsupported: &mut Vec<String>,
    name: &str,
) -> BTreeMap<String, String> {
    let Some(value) = value else {
        return BTreeMap::new();
    };
    let Some(items) = value.as_table() else {
        unsupported.push(format!("{name}.shape"));
        return BTreeMap::new();
    };
    items
        .iter()
        .map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
        .collect::<Option<BTreeMap<_, _>>>()
        .unwrap_or_else(|| {
            unsupported.push(format!("{name}.shape"));
            BTreeMap::new()
        })
}

fn parse_task_run(value: Option<&toml::Value>) -> Option<Vec<String>> {
    match value? {
        toml::Value::String(run) if !run.trim().is_empty() => Some(vec![run.clone()]),
        toml::Value::Array(items)
            if !items.is_empty()
                && items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|run| !run.trim().is_empty())) =>
        {
            items
                .iter()
                .map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        }
        _ => None,
    }
}

fn safe_tool_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 200
        && key.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b':' | b'/' | b'.' | b'_' | b'-' | b'+' | b'@')
        })
}

pub(crate) fn safe_task_tool_version(version: &str) -> bool {
    safe_version(version)
}

fn safe_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'_' | b'-'))
        && !matches!(version, "latest" | "system" | "ref")
}

#[cfg(test)]
mod tests {
    use super::selected_build_task_tools;
    use crate::native_tool_input::{NativeMiseConfig, NativeToolSource, native_mise_source};

    fn config(source: &str) -> NativeMiseConfig {
        let value = toml::from_str(source).expect("Mise task config");
        let input = native_mise_source("native/mise.toml", source.as_bytes(), &value)
            .expect("typed Mise source");
        match input.source {
            NativeToolSource::MiseConfig(config) => config,
            NativeToolSource::RustToolchain | NativeToolSource::MiseLock(_) => {
                panic!("wrong source kind")
            }
        }
    }

    #[test]
    fn build_task_tools_include_dependency_only_task_nodes() {
        let config = config(
            r#"
[tasks.ci]
depends = ["lint"]

[tasks.lint]
run = "swiftlint lint --strict"
tools = { swiftlint = "0.65.1" }
"#,
        );

        assert_eq!(
            selected_build_task_tools(&config, "ci").expect("bounded task graph"),
            [("swiftlint".to_owned(), "0.65.1".to_owned())].into()
        );
    }

    #[test]
    fn build_task_graph_rejects_empty_dependency_nodes() {
        let config = config(
            r#"
[tasks.ci]
depends = ["empty"]

[tasks.empty]
"#,
        );

        assert!(selected_build_task_tools(&config, "ci").is_err());
    }
}
