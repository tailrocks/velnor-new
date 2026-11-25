// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy)]
pub(in super::super) enum Which<'a> {
    Host,
    Target(Option<&'a str>),
    Unresolved,
}

pub(in super::super) enum Configured {
    Absent,
    Triple(String),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) struct Platform {
    triple: String,
    cfgs: Vec<(String, Option<String>)>,
}

impl Platform {
    pub(in super::super) fn new(triple: &str, cfg_lines: &str) -> Self {
        Self {
            triple: triple.to_owned(),
            cfgs: cfg_lines.lines().filter_map(split_cfg).collect(),
        }
    }

    pub(in super::super) fn aimed(
        repository: &Path,
        location: &Path,
        which: Which<'_>,
    ) -> Option<Self> {
        match which {
            Which::Host => Self::probe(location, None),
            Which::Unresolved => None,
            Which::Target(Some(target)) => Self::probe(location, Some(target)),
            Which::Target(None) => match configured_target(repository, location) {
                Configured::Absent => Self::probe(location, None),
                Configured::Triple(triple) => Self::probe(location, Some(&triple)),
                Configured::Unknown => None,
            },
        }
    }

    fn probe(directory: &Path, target: Option<&str>) -> Option<Self> {
        let rustc = |arguments: &[&str]| {
            let output = Command::new("rustc")
                .args(arguments)
                .current_dir(directory)
                .output()
                .ok()?;
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
        };
        let triple = match target {
            Some(target)
                if Path::new(target)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json")) =>
            {
                return None;
            }
            Some(target) => target.to_owned(),
            None => rustc(&["-vV"])?
                .lines()
                .find_map(|line| line.strip_prefix("host: "))?
                .to_owned(),
        };
        let cfgs = match target {
            Some(target) => rustc(&["--print", "cfg", "--target", target])?,
            None => rustc(&["--print", "cfg"])?,
        };
        Some(Self::new(&triple, &cfgs))
    }

    pub(in super::super) fn active(&self, predicate: &str) -> Option<bool> {
        match predicate
            .strip_prefix("cfg(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            Some(expression) => {
                let mut parser = Parser {
                    rest: expression,
                    platform: self,
                };
                let value = parser.expression()?;
                parser.rest.trim().is_empty().then_some(value)
            }
            None => Some(self.triple == predicate),
        }
    }
}

fn split_cfg(line: &str) -> Option<(String, Option<String>)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    Some(match line.split_once('=') {
        Some((key, value)) => (
            key.trim().to_owned(),
            Some(value.trim().trim_matches('"').to_owned()),
        ),
        None => (line.to_owned(), None),
    })
}

struct Parser<'a> {
    rest: &'a str,
    platform: &'a Platform,
}

impl<'a> Parser<'a> {
    fn expression(&mut self) -> Option<bool> {
        let name = self.name()?;
        self.rest = self.rest.trim_start();
        if self.rest.starts_with('(') {
            self.rest = &self.rest[1..];
            let mut values = Vec::new();
            loop {
                self.rest = self.rest.trim_start();
                if let Some(rest) = self.rest.strip_prefix(')') {
                    self.rest = rest;
                    break;
                }
                values.push(self.expression()?);
                self.rest = self.rest.trim_start();
                if let Some(rest) = self.rest.strip_prefix(',') {
                    self.rest = rest;
                }
            }
            return match name {
                "all" => Some(values.iter().all(|value| *value)),
                "any" => Some(values.iter().any(|value| *value)),
                "not" if values.len() == 1 => Some(!values[0]),
                _ => None,
            };
        }
        if let Some(rest) = self.rest.strip_prefix('=') {
            let rest = rest.trim_start().strip_prefix('"')?;
            let (value, rest) = rest.split_once('"')?;
            self.rest = rest;
            return Some(
                self.platform
                    .cfgs
                    .iter()
                    .any(|(key, found)| key == name && found.as_deref() == Some(value)),
            );
        }
        Some(
            self.platform
                .cfgs
                .iter()
                .any(|(key, found)| key == name && found.is_none()),
        )
    }

    fn name(&mut self) -> Option<&'a str> {
        self.rest = self.rest.trim_start();
        let end = self
            .rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(self.rest.len());
        let (name, rest) = self.rest.split_at(end);
        self.rest = rest;
        (!name.is_empty()).then_some(name)
    }
}

pub(super) fn command_target(words: &[&str]) -> Configured {
    let mut found = Configured::Absent;
    for (index, word) in words.iter().enumerate() {
        let value = word.strip_prefix("--config=").or_else(|| {
            (*word == "--config")
                .then(|| words.get(index + 1).copied())
                .flatten()
        });
        let Some(value) = value else { continue };
        let Some((key, text)) = value.split_once('=') else {
            found = Configured::Unknown;
            continue;
        };
        match key.trim() {
            "build.target" => {
                found = match format!("target = {text}").parse::<toml::Table>() {
                    Ok(table) => match table.get("target") {
                        Some(toml::Value::String(triple)) => Configured::Triple(triple.clone()),
                        _ => Configured::Unknown,
                    },
                    Err(_) => Configured::Unknown,
                };
            }
            "build" => found = Configured::Unknown,
            _ => {}
        }
    }
    found
}

fn configured_target(repository: &Path, location: &Path) -> Configured {
    if let Some(triple) = std::env::var("CARGO_BUILD_TARGET")
        .ok()
        .filter(|t| !t.is_empty())
    {
        return Configured::Triple(triple);
    }
    let home = std::env::var_os("CARGO_HOME").map(std::path::PathBuf::from);
    let directories = location
        .ancestors()
        .take_while(|directory| directory.starts_with(repository))
        .map(|directory| directory.join(".cargo"))
        .chain(home);
    for directory in directories {
        for name in ["config", "config.toml"] {
            let Ok(text) = std::fs::read_to_string(directory.join(name)) else {
                continue;
            };
            let Ok(table) = text.parse::<toml::Table>() else {
                return Configured::Unknown;
            };
            match table.get("build").and_then(|build| build.get("target")) {
                None => {}
                Some(toml::Value::String(triple)) => return Configured::Triple(triple.clone()),
                Some(_) => return Configured::Unknown,
            }
        }
    }
    Configured::Absent
}
