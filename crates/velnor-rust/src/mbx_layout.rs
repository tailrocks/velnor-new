// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;

use toml::Value;
use velnor_model::{MBX_TOOL, MbxDeclaration, RepoFacts, RepoPath, ToolPins, mbx_declaration};

use crate::error::DetectError;
use crate::mise::{MISE_CONFIGS, rust_tool};
use crate::toml_file::table;

const SHIM_MODE: &str = "MBX_CARGO_SHIM_MODE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrapper {
    Accepted,
    Missing,
    WithoutShim,
    ExtraKeys,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub path: RepoPath,
    pub rust: bool,
    pub rust_option: bool,
    pub wrapper: Wrapper,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    pub key: &'static str,
    pub first: RepoPath,
    pub second: RepoPath,
}

impl Split {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { key, first, second } = self;
        write!(
            f,
            "{second}: `{key}` is set in {second}, but {first} already carries mbx settings; mise resolves several config files by its own precedence, which the generator does not replicate, so it cannot tell which entry decides whether cargo runs through mbx. Keep every mbx setting (the rust tool with `mr_boxington = true`, the `mr-boxington` pin and `[wrappers.cargo]`) in one file, or set `mbx = false` in the [cache] table of .velnor/rust-ci.toml to run cargo without mbx"
        )
    }
}

impl Layout {
    #[must_use]
    pub fn wrapper_refused(&self) -> bool {
        self.wrapper != Wrapper::Accepted
    }

    #[must_use]
    pub fn shim_missing(&self) -> bool {
        self.wrapper == Wrapper::WithoutShim
    }

    #[must_use]
    pub fn wrapper_has_extras(&self) -> bool {
        self.wrapper == Wrapper::ExtraKeys
    }
}

pub(crate) fn standard_wrapper(wrapper: &Value) -> bool {
    let Some(table) = wrapper.as_table() else {
        return false;
    };
    let env_ok = table
        .get("env")
        .and_then(Value::as_table)
        .is_some_and(|env| {
            env.len() == 1 && env.get(SHIM_MODE).and_then(Value::as_str) == Some("1")
        });
    table.len() == 2 && table.get("command").and_then(Value::as_str) == Some("mbx") && env_ok
}

fn shim_env(table: &toml::Table) -> Option<&str> {
    table
        .get("env")
        .and_then(Value::as_table)
        .and_then(|env| env.get(SHIM_MODE))
        .and_then(Value::as_str)
}

fn mbx_without_shim(wrapper: &Value) -> bool {
    wrapper.as_table().is_some_and(|table| {
        table.get("command").and_then(Value::as_str) == Some("mbx") && shim_env(table) != Some("1")
    })
}

fn mbx_with_extras(wrapper: &Value) -> bool {
    wrapper.as_table().is_some_and(|table| {
        table.get("command").and_then(Value::as_str) == Some("mbx")
            && shim_env(table) == Some("1")
            && !standard_wrapper(wrapper)
    })
}

pub(crate) fn pin_required(path: &RepoPath, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
        f,
        "generated jobs run cargo through mbx, which needs one unfiltered pin of {MBX_TOOL} in {path}, spelled exactly `{MBX_TOOL}` (an `aqua:` or `github:` spelling is not recognised); add `{MBX_TOOL} = {{ version = \"v1.30.0\", workflow = \"release.yml@refs/heads/main\" }}` under [tools] and run `mise lock`, or set `mbx = false` in the [cache] table of .velnor/rust-ci.toml to run cargo without mbx"
    )
}

impl Layout {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = format!(
            "{}: generated jobs run cargo through mbx, but this mise configuration does not route cargo there. ",
            self.path
        );
        if self.shim_missing() {
            text.push_str(
                "The `[wrappers.cargo]` entry runs `mbx` without `env = { MBX_CARGO_SHIM_MODE = \"1\" }`, and without that env `cargo -V` and `cargo --list` print mbx's own output instead of cargo's. Either add that env to the wrapper, or delete `[wrappers.cargo]` and use the native layout below. ",
            );
        }
        if self.wrapper_has_extras() {
            text.push_str(
                "The `[wrappers.cargo]` entry runs `mbx` with the shim env but carries more than `command` and that one env variable (extra keys such as `args`, or more `env` entries), and only the exact two-key entry is accepted. Remove the extras, or delete `[wrappers.cargo]` and use the native layout below. ",
            );
        }
        if self.rust {
            text.push_str(
                "Native layout: set `mr_boxington = true` on the rust entry of [tools], keep one unfiltered pin of `mr-boxington` under [tools], and add no `[wrappers.cargo]`. The explicit layout is also accepted: the same option and pin plus `[wrappers.cargo]` with `command = \"mbx\"` and `env = { MBX_CARGO_SHIM_MODE = \"1\" }`. ",
            );
        } else {
            text.push_str(
                "This repository pins no rust tool, so the explicit layout applies: keep one unfiltered pin of `mr-boxington` under [tools] and add `[wrappers.cargo]` with `command = \"mbx\"` and `env = { MBX_CARGO_SHIM_MODE = \"1\" }`. ",
            );
        }
        text.push_str(
            "Then run `mise lock`. Or set `mbx = false` in the [cache] table of .velnor/rust-ci.toml to run cargo without mbx",
        );
        f.write_str(&text)
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
            path: facts
                .tools
                .configs
                .first()
                .cloned()
                .unwrap_or(MISE_CONFIGS[0].clone()),
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
    let (paths, parsed): (Vec<&RepoPath>, Vec<toml::Table>) = facts
        .tools
        .configs
        .iter()
        .filter_map(|config| {
            files
                .get(config)
                .and_then(|bytes| table(bytes).ok())
                .map(|parsed| (config, parsed))
        })
        .unzip();
    if let MbxDeclaration::Conflicting(conflict) = mbx_declaration(&parsed) {
        return Err(DetectError::MbxConfigSplit(Split {
            key: conflict.key,
            first: paths[conflict.first].clone(),
            second: paths[conflict.second].clone(),
        }));
    }
    let rust = rust_tool(&facts.tools);
    let rust_option = rust.is_some_and(|name| {
        !parsed.iter().any(|config| {
            config
                .get("tools")
                .and_then(|tools| tools.get(name))
                .and_then(|entry| entry.get("mr_boxington"))
                .and_then(Value::as_bool)
                == Some(true)
        })
    });
    let wrappers: Vec<&Value> = parsed
        .iter()
        .filter_map(|config| config.get("wrappers").and_then(|w| w.get("cargo")))
        .collect();
    let routed = if wrappers.is_empty() {
        rust.is_some()
    } else {
        wrappers.iter().any(|wrapper| standard_wrapper(wrapper))
    };
    let wrapper = if routed {
        Wrapper::Accepted
    } else if wrappers.iter().any(|w| mbx_without_shim(w)) {
        Wrapper::WithoutShim
    } else if wrappers.iter().any(|w| mbx_with_extras(w)) {
        Wrapper::ExtraKeys
    } else if wrappers.is_empty() {
        Wrapper::Missing
    } else {
        Wrapper::Other
    };
    if rust_option || wrapper != Wrapper::Accepted {
        return Err(DetectError::MbxLayout(Layout {
            path,
            rust: rust.is_some(),
            rust_option,
            wrapper,
        }));
    }
    Ok(())
}
