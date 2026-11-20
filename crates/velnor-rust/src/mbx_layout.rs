// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;

use toml::Value;
use velnor_model::{RepoFacts, RepoPath, ToolPins};

use crate::error::DetectError;
use crate::mise::{MISE_CONFIGS, rust_tool};
use crate::toml_file::table;

pub(crate) const MBX_TOOL: &str = "mr-boxington";
const SHIM_MODE: &str = "MBX_CARGO_SHIM_MODE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub path: RepoPath,
    pub rust_option: bool,
    pub wrapper: bool,
}

pub(crate) fn standard_wrapper(wrapper: &Value) -> bool {
    let Some(table) = wrapper.as_table() else {
        return false;
    };
    let env_ok = table.get("env").and_then(Value::as_table).is_some_and(|env| {
        env.len() == 1 && env.get(SHIM_MODE).and_then(Value::as_str) == Some("1")
    });
    table.len() == 2 && table.get("command").and_then(Value::as_str) == Some("mbx") && env_ok
}

pub(crate) fn pin_required(path: &RepoPath, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
        f,
        "generated jobs run cargo through mbx, which needs one unfiltered pin of {MBX_TOOL} in {path}; add `{MBX_TOOL} = {{ version = \"v1.30.0\", workflow = \"release.yml@refs/heads/main\" }}` under [tools] and run `mise lock`, or set `mbx = false` in the [cache] table of .velnor/rust-ci.toml to run cargo without mbx"
    )
}

impl Layout {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut missing = Vec::new();
        if self.rust_option {
            missing.push("set `mr_boxington = true` in the rust entry of [tools]");
        }
        if self.wrapper {
            missing.push(
                "add `[wrappers.cargo]` with `command = \"mbx\"` and `env = { MBX_CARGO_SHIM_MODE = \"1\" }`",
            );
        }
        write!(
            f,
            "{}: generated jobs run cargo through mbx, but this mise configuration does not route cargo there; {}, then run `mise lock`. Or set `mbx = false` in the [cache] table of .velnor/rust-ci.toml to run cargo without mbx",
            self.path,
            missing.join(" and ")
        )
    }
}

fn mbx_pin_usable(pins: &ToolPins) -> bool {
    pins.tools
        .get(MBX_TOOL)
        .is_some_and(|pin| pin.requested.len() == 1 && pin.requested[0].os.is_none())
}

pub(crate) fn check_pin(facts: &RepoFacts) -> Result<(), DetectError> {
    if facts.config.mbx.routed() && !mbx_pin_usable(&facts.tools) {
        return Err(DetectError::MbxPinRequired {
            path: facts.tools.configs.first().cloned().unwrap_or(MISE_CONFIGS[0].clone()),
        });
    }
    Ok(())
}

pub fn check_mbx(
    facts: &RepoFacts,
    files: &BTreeMap<RepoPath, Vec<u8>>,
) -> Result<(), DetectError> {
    if !facts.config.mbx.routed() {
        return Ok(());
    }
    check_pin(facts)?;
    let path = facts
        .tools
        .configs
        .first()
        .cloned()
        .unwrap_or(MISE_CONFIGS[0].clone());
    let rust_option = !rust_tool(&facts.tools)
        .and_then(|name| facts.tools.tools.get(name))
        .is_some_and(|pin| {
            pin.requested
                .iter()
                .all(|request| request.options.get("mr_boxington").map(String::as_str) == Some("true"))
        });
    let wrapper = !facts.tools.configs.iter().any(|config| {
        files
            .get(config)
            .and_then(|bytes| table(bytes).ok())
            .and_then(|parsed| parsed.get("wrappers")?.get("cargo").map(standard_wrapper))
            .unwrap_or(false)
    });
    if rust_option || wrapper {
        return Err(DetectError::MbxLayout(Layout {
            path,
            rust_option,
            wrapper,
        }));
    }
    Ok(())
}
