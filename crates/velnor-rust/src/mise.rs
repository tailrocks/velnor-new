// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;
use velnor_model::{MbxRemote, MisePlatform, RepoFacts, RepoPath, ToolPin, ToolPins, ToolRequest};

use crate::error::{DetectError, Reason};
use crate::harness::uses_nextest;
use crate::mise_lock::{is_rustup_tool, locked_tools};
use crate::mise_options::option_value;
use crate::mise_template::render_versions;
use crate::toml_file::table;

pub static MISE_CONFIGS: [RepoPath; 2] = [
    RepoPath::from_static("mise.toml"),
    RepoPath::from_static(".mise.toml"),
];
pub const MISE_LOCK: RepoPath = RepoPath::from_static("mise.lock");
const RUST_TOOLS: [&str; 2] = ["rust", "core:rust"];
const NEXTEST_SELECTORS: [&str; 2] = ["cargo-nextest", "nextest-rs/nextest"];

type Versions = BTreeMap<String, Vec<ToolRequest>>;

fn plain(version: String) -> ToolRequest {
    ToolRequest {
        version,
        ..ToolRequest::default()
    }
}

#[must_use]
pub fn mise_files() -> Vec<RepoPath> {
    MISE_CONFIGS.iter().cloned().chain([MISE_LOCK]).collect()
}

pub fn tool_pins(files: &BTreeMap<RepoPath, Vec<u8>>) -> Result<ToolPins, DetectError> {
    let present: Vec<(&RepoPath, &Vec<u8>)> = MISE_CONFIGS
        .iter()
        .filter_map(|path| files.get(path).map(|bytes| (path, bytes)))
        .collect();
    if present.is_empty() {
        return Err(DetectError::MiseRequired {
            path: MISE_CONFIGS[0].clone(),
        });
    }
    let mut requested = Versions::new();
    for (config, bytes) in &present {
        requested.extend(versions(bytes).map_err(|reason| DetectError::invalid(config, reason))?);
        validate_config(config, bytes)?;
    }
    let mut locked = match files.get(&MISE_LOCK) {
        Some(lock) => {
            locked_tools(lock).map_err(|reason| DetectError::invalid(&MISE_LOCK, reason))?
        }
        None => BTreeMap::new(),
    };
    Ok(ToolPins {
        locked_file: files.contains_key(&MISE_LOCK),
        configs: present
            .iter()
            .map(|(config, _)| (*config).clone())
            .collect(),
        tools: requested
            .into_iter()
            .map(|(name, requested)| {
                let locked = locked.remove(&name).unwrap_or_default();
                (name, ToolPin { requested, locked })
            })
            .collect(),
    })
}

fn validate_config(config: &RepoPath, bytes: &[u8]) -> Result<(), DetectError> {
    let parsed = table(bytes).map_err(|reason| DetectError::invalid(config, reason))?;
    let unknown = parsed.keys().find(|key| {
        ![
            "tools",
            "settings",
            "min_version",
            "tasks",
            "task_templates",
            "vars",
            "wrappers",
        ]
        .contains(&key.as_str())
    });
    let settings = parsed.get("settings").and_then(Value::as_table);
    let unknown_setting = settings.and_then(|settings| {
        settings.keys().find(|key| {
            !["cargo", "idiomatic_version_file_enable_tools", "lockfile"].contains(&key.as_str())
        })
    });
    let unknown_cargo_setting = settings
        .and_then(|settings| settings.get("cargo"))
        .and_then(Value::as_table)
        .and_then(|cargo| cargo.keys().find(|key| key.as_str() != "binstall"));
    let vars_directive = parsed
        .get("vars")
        .and_then(Value::as_table)
        .and_then(|vars| vars.keys().find(|key| key.starts_with('_')));
    let postinstall = parsed
        .get("tools")
        .and_then(Value::as_table)
        .is_some_and(|tools| tools.values().any(runs_a_command));
    let templates = parsed
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "tasks" | "task_templates"))
        .any(|(key, value)| {
            if ["tools", "vars"].contains(&key.as_str()) {
                runs_exec(value)
            } else {
                has_template(value)
            }
        });
    if let Some(name) = unknown
        .or(unknown_setting)
        .or(unknown_cargo_setting)
        .or(vars_directive)
        .map(String::as_str)
        .or(postinstall.then_some("tools.postinstall"))
    {
        return Err(DetectError::invalid(
            config,
            Reason::MiseOverridesFlags(name.to_owned()),
        ));
    }
    if templates {
        return Err(DetectError::invalid(config, Reason::MiseTemplate));
    }
    Ok(())
}

pub(crate) fn nextest_tool(pins: &ToolPins) -> Option<&str> {
    pins.tools.keys().map(String::as_str).find(|name| {
        NEXTEST_SELECTORS.iter().any(|selector| {
            name.strip_suffix(selector)
                .is_some_and(|rest| rest.is_empty() || rest.ends_with([':', '/']))
        })
    })
}

pub(crate) const MISE_CONFIG: RepoPath = RepoPath::from_static("mise.toml");
const MBX_TOOL: &str = "mr-boxington";
// First released in tools-v0.1.108 (not tagged yet when this gate was written).
pub(crate) const MBX_PIN_HELPER: (u64, u64, u64) = (0, 1, 108);

#[must_use]
pub fn exact_release(text: &str) -> Option<&str> {
    let bare = text.strip_prefix('v').unwrap_or(text);
    let parts: Vec<&str> = bare.split('.').collect();
    let [major, minor, patch] = parts.as_slice() else {
        return None;
    };
    let number = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    (number(major) && number(minor) && number(patch)).then_some(bare)
}

pub fn check_tools(facts: &RepoFacts) -> Result<(), DetectError> {
    if !facts.tools.locked_file {
        return Err(DetectError::MiseLockRequired { path: MISE_LOCK });
    }
    if let Some((file, mise)) = toolchain_conflict(facts) {
        return Err(DetectError::ToolchainConflict { file, mise });
    }
    if let Some(channel) = facts
        .toolchain
        .as_ref()
        .and_then(|file| file.channel.clone())
        && rust_tool(&facts.tools).is_none()
    {
        return Err(DetectError::RustPinRequired {
            channel,
            path: facts.tools.configs.first().cloned().unwrap_or(MISE_CONFIG),
        });
    }
    let rust_pin = rust_tool(&facts.tools).and_then(|name| facts.tools.tools.get(name));
    let rust_pins = rust_pin.map_or(1, |pin| pin.requested.len());
    let selectors = RUST_TOOLS
        .iter()
        .filter(|name| facts.tools.tools.contains_key(**name))
        .count();
    if rust_pins != 1
        || selectors > 1
        || rust_pin.is_some_and(|pin| pin.requested.iter().any(|request| request.os.is_some()))
    {
        return Err(DetectError::invalid(
            facts.tools.configs.first().unwrap_or(&MISE_CONFIG),
            Reason::InvalidValue {
                key: "tools.rust",
                value: "one version, without os filters or alternatives".to_owned(),
            },
        ));
    }
    check_prebuilt(facts)?;
    check_mbx_pin(facts)?;
    let tested = facts
        .rust_roots
        .iter()
        .flat_map(|root| &root.members)
        .any(|member| uses_nextest(&facts.config, member));
    if tested && nextest_tool(&facts.tools).is_none() {
        return Err(DetectError::NextestRequired {
            path: facts.tools.configs.first().cloned().unwrap_or(MISE_CONFIG),
        });
    }
    Ok(())
}

fn check_mbx_pin(facts: &RepoFacts) -> Result<(), DetectError> {
    let reads_pin = facts.config.mbx_remote == MbxRemote::GithubActions
        && facts
            .helper
            .as_ref()
            .is_some_and(|helper| helper.version().at_least(MBX_PIN_HELPER));
    if !reads_pin {
        return Ok(());
    }
    let exact = match facts
        .tools
        .tools
        .get(MBX_TOOL)
        .map(|pin| &pin.requested[..])
    {
        Some([request]) => request.os.is_none() && exact_release(&request.version).is_some(),
        _ => false,
    };
    if exact {
        return Ok(());
    }
    Err(DetectError::invalid(
        facts.tools.configs.first().unwrap_or(&MISE_CONFIG),
        Reason::InvalidValue {
            key: "tools.mr-boxington",
            value: "one exact X.Y.Z release, because the mbx action reads it from mise.toml"
                .to_owned(),
        },
    ))
}

fn check_prebuilt(facts: &RepoFacts) -> Result<(), DetectError> {
    let Some(platform) = MisePlatform::runner(facts.config.runner.as_str()) else {
        return Ok(());
    };
    let key = platform.lock_key();
    for (name, pin) in &facts.tools.tools {
        if is_rustup_tool(name) {
            continue;
        }
        let rows = pin.locked_on(platform).map_err(|value| {
            DetectError::invalid(
                &MISE_LOCK,
                Reason::InvalidValue {
                    key: "tools",
                    value: format!("{name}: {value}"),
                },
            )
        })?;
        let needed = pin
            .requested
            .iter()
            .any(|request| request.applies_on(platform));
        if needed && (rows.is_empty() || rows.iter().any(|row| !row.prebuilt.contains(&key))) {
            return Err(DetectError::ToolNotPrebuilt {
                tool: name.clone(),
                platform: key,
            });
        }
    }
    Ok(())
}

fn toolchain_conflict(facts: &RepoFacts) -> Option<(String, String)> {
    let toolchain = facts.toolchain.as_ref()?;
    let channel = toolchain
        .channel
        .clone()
        .or_else(|| toolchain.path.as_ref().map(|path| format!("path {path}")))?;
    let pinned = facts
        .tools
        .tools
        .get(rust_tool(&facts.tools)?)?
        .requested
        .first()?;
    (pinned.version != channel).then(|| (channel, pinned.version.clone()))
}

pub(crate) fn rust_tool(pins: &ToolPins) -> Option<&str> {
    RUST_TOOLS
        .into_iter()
        .find(|name| pins.tools.contains_key(*name))
}

fn has_template(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("{{") || text.contains("{%"),
        Value::Array(items) => items.iter().any(has_template),
        Value::Table(items) => items.values().any(has_template),
        _ => false,
    }
}

fn runs_exec(value: &Value) -> bool {
    match value {
        Value::String(text) => has_template(value) && text.contains("exec("),
        Value::Array(items) => items.iter().any(runs_exec),
        Value::Table(items) => items.values().any(runs_exec),
        _ => false,
    }
}

fn runs_a_command(value: &Value) -> bool {
    match value {
        Value::Table(entry) => entry.contains_key("postinstall"),
        Value::Array(items) => items.iter().any(runs_a_command),
        _ => false,
    }
}

fn versions(bytes: &[u8]) -> Result<Versions, Reason> {
    let parsed = table(bytes)?;
    let mut versions = match parsed.get("tools") {
        None => Ok(Versions::new()),
        Some(Value::Table(tools)) => tools
            .iter()
            .map(|(name, value)| {
                version_list(value, name)
                    .map(|list| (name.clone(), list))
                    .ok_or_else(|| Reason::InvalidValue {
                        key: "tools",
                        value: name.clone(),
                    })
            })
            .collect(),
        Some(other) => Err(Reason::InvalidValue {
            key: "tools",
            value: other.to_string(),
        }),
    }?;
    render_versions(&mut versions, &parsed, bytes)?;
    Ok(versions)
}

fn os_names(entry: &toml::Table) -> Option<Vec<String>> {
    match entry.get("os")? {
        Value::String(name) => Some(vec![name.clone()]),
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect(),
        ),
        _ => None,
    }
}

fn version_list(value: &Value, name: &str) -> Option<Vec<ToolRequest>> {
    let rust = RUST_TOOLS.contains(&name);
    match value {
        Value::String(version) => Some(vec![plain(version.clone())]),
        Value::Array(items) => items
            .iter()
            .map(|value| version_list(value, name))
            .collect::<Option<Vec<_>>>()
            .map(|lists| lists.concat()),
        Value::Table(entry) => {
            let mut list = entry
                .get("version")
                .map_or_else(|| source_request(entry), |value| version_list(value, name))?;
            if let Some(names) = os_names(entry) {
                for request in &mut list {
                    request.os.get_or_insert_with(|| names.clone());
                }
            }
            let options: BTreeMap<_, _> = entry
                .iter()
                .filter(|(key, _)| {
                    !([
                        "version",
                        "os",
                        "ref",
                        "path",
                        "prefix",
                        "depends",
                        "install_env",
                        "lazy",
                        "lazy_bins",
                        "auto_update",
                    ]
                    .contains(&key.as_str())
                        || (rust && key.as_str() == "mr_boxington"))
                })
                .filter_map(|(key, value)| {
                    option_value(name, key, value, rust).map(|value| (key.clone(), value))
                })
                .collect();
            for request in &mut list {
                request.options.extend(options.clone());
            }
            Some(list)
        }
        _ => None,
    }
}

fn source_request(entry: &toml::Table) -> Option<Vec<ToolRequest>> {
    ["ref", "path", "prefix"].iter().find_map(|key| {
        entry
            .get(*key)
            .and_then(Value::as_str)
            .map(|value| vec![plain(format!("{key}:{value}"))])
    })
}
