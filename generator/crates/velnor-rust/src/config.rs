// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{
    CronExpr, FullRun, InputRule, OnMain, PackageName, PathGlob, RepoPath, RunnerLabel,
    RustCiConfig, Shape,
};

use crate::error::{DetectError, Reason};
use crate::toml_file::table;

pub const CONFIG_PATH: RepoPath = RepoPath::from_static(".velnor/rust-ci.toml");

const KEYS: [&str; 11] = [
    "runner",
    "timeout_minutes",
    "exclude",
    "non_inputs",
    "inputs",
    "default_ignores",
    "shape",
    "required_check",
    "default_branch",
    "on_main",
    "full_run",
];
const TIMEOUT_RANGE: std::ops::RangeInclusive<u16> = 1..=360;

#[derive(Deserialize)]
struct RawConfig {
    runner: Option<String>,
    timeout_minutes: Option<u16>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    non_inputs: Vec<String>,
    #[serde(default)]
    inputs: Vec<RawInput>,
    default_ignores: Option<bool>,
    shape: Option<String>,
    required_check: Option<String>,
    default_branch: Option<String>,
    on_main: Option<String>,
    full_run: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInput {
    glob: String,
    packages: Vec<String>,
}

pub(crate) fn parse(bytes: &[u8]) -> Result<RustCiConfig, DetectError> {
    let invalid = |reason| DetectError::invalid(&CONFIG_PATH, reason);
    let table = table(bytes).map_err(invalid)?;
    if let Some(key) = table.keys().find(|key| !KEYS.contains(&key.as_str())) {
        return Err(invalid(Reason::UnknownKey(key.clone())));
    }
    let raw: RawConfig = table
        .try_into()
        .map_err(|error: toml::de::Error| invalid(Reason::Syntax(error.message().to_owned())))?;
    let defaults = RustCiConfig::default();
    let timeout_minutes = raw.timeout_minutes.unwrap_or(defaults.timeout_minutes);
    if !TIMEOUT_RANGE.contains(&timeout_minutes) {
        return Err(invalid(Reason::OutOfRange {
            key: "timeout_minutes",
            value: timeout_minutes,
            min: *TIMEOUT_RANGE.start(),
            max: *TIMEOUT_RANGE.end(),
        }));
    }
    let (non_inputs, inputs) = rules(raw.non_inputs, raw.inputs).map_err(invalid)?;
    let shape = match raw.shape.as_deref() {
        None | Some("auto") => None,
        Some("one-job") => Some(Shape::OneJob),
        Some("lint-test") => Some(Shape::LintAndTest),
        Some("groups") => Some(Shape::BuildThenGroups),
        Some(other) => {
            return Err(invalid(Reason::InvalidValue {
                key: "shape",
                value: other.to_owned(),
            }));
        }
    };
    let required_check = match raw.required_check {
        None => defaults.required_check,
        Some(name) if !name.trim().is_empty() && !name.chars().any(char::is_control) => name,
        Some(name) => {
            return Err(invalid(Reason::InvalidValue {
                key: "required_check",
                value: name,
            }));
        }
    };
    let default_branch =
        default_branch(raw.default_branch, defaults.default_branch).map_err(invalid)?;
    let on_main = on_main(raw.on_main.as_deref(), defaults.on_main).map_err(invalid)?;
    let full_run = match raw.full_run {
        None => defaults.full_run,
        Some(value) => full_run(value).map_err(invalid)?,
    };
    Ok(RustCiConfig {
        on_main,
        full_run,
        shape,
        required_check,
        default_branch,
        non_inputs,
        inputs,
        default_ignores: raw.default_ignores.unwrap_or(defaults.default_ignores),
        runner: runner(raw.runner, defaults.runner).map_err(invalid)?,
        timeout_minutes,
        exclude: raw
            .exclude
            .into_iter()
            .map(|entry| {
                PathGlob::new(entry.trim_end_matches('/')).map_err(|_| {
                    invalid(Reason::InvalidValue {
                        key: "exclude",
                        value: entry,
                    })
                })
            })
            .collect::<Result<_, _>>()?,
    })
}

fn runner(configured: Option<String>, default: RunnerLabel) -> Result<RunnerLabel, Reason> {
    let Some(label) = configured else {
        return Ok(default);
    };
    RunnerLabel::new(label.clone())
        .ok()
        .filter(|runner| !without_helper_build(runner.as_str()))
        .ok_or(Reason::InvalidValue {
            key: "runner",
            value: label,
        })
}

fn on_main(configured: Option<&str>, default: OnMain) -> Result<OnMain, Reason> {
    match configured {
        None => Ok(default),
        Some("affected") => Ok(OnMain::Affected),
        Some("everything") => Ok(OnMain::Everything),
        Some(other) => Err(Reason::InvalidValue {
            key: "on_main",
            value: other.to_owned(),
        }),
    }
}

fn full_run(value: String) -> Result<FullRun, Reason> {
    Ok(match value.as_str() {
        "off" => FullRun::Off,
        "hourly" => FullRun::Hourly,
        "nightly" => FullRun::Nightly,
        "weekly" => FullRun::Weekly,
        _ => FullRun::Cron(
            CronExpr::new(value.clone()).map_err(|_| Reason::InvalidValue {
                key: "full_run",
                value,
            })?,
        ),
    })
}

fn without_helper_build(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    let Some(version) = label.strip_prefix("macos-") else {
        return label.starts_with("windows");
    };
    let major: u32 = version
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(u32::MAX);
    major <= 13 || version.ends_with("-large") || version.ends_with("-intel")
}

fn default_branch(configured: Option<String>, default: String) -> Result<String, Reason> {
    match configured {
        None => Ok(default),
        Some(name) if branch_name(&name) => Ok(name),
        Some(name) => Err(Reason::InvalidValue {
            key: "default_branch",
            value: name,
        }),
    }
}

fn branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['/', '-'])
        && !name.ends_with(['/', '.'])
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
}

pub(crate) fn excluded(config: &RustCiConfig, manifest: &RepoPath) -> bool {
    config.exclude.iter().any(|entry| entry.matches(manifest))
}

fn rules(
    non_inputs: Vec<String>,
    inputs: Vec<RawInput>,
) -> Result<(Vec<PathGlob>, Vec<InputRule>), Reason> {
    let non_inputs = non_inputs
        .into_iter()
        .map(|entry| rule_glob("non_inputs", entry))
        .collect::<Result<Vec<_>, _>>()?;
    let inputs = inputs
        .into_iter()
        .map(|input| {
            let glob = rule_glob("inputs", input.glob)?;
            let packages = input
                .packages
                .into_iter()
                .map(|package| {
                    PackageName::new(package.clone()).map_err(|_| Reason::InvalidValue {
                        key: "inputs.packages",
                        value: package,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if packages.is_empty() {
                return Err(Reason::InvalidValue {
                    key: "inputs.packages",
                    value: glob.to_string(),
                });
            }
            Ok(InputRule { glob, packages })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for non_input in &non_inputs {
        if let Some(input) = inputs.iter().find(|input| overlaps(non_input, &input.glob)) {
            return Err(Reason::OverlappingRules {
                non_input: non_input.clone(),
                input: input.glob.clone(),
            });
        }
    }
    Ok((non_inputs, inputs))
}

const ANY_SEGMENT: &str = "*";
const REST: &str = "**";

fn rule_glob(key: &'static str, entry: String) -> Result<PathGlob, Reason> {
    let invalid = |value: String| Reason::InvalidValue { key, value };
    let (segments, _) = shape(&entry);
    let grammatical = segments
        .iter()
        .all(|segment| *segment == ANY_SEGMENT || !segment.contains('*'));
    if !grammatical {
        return Err(invalid(entry));
    }
    PathGlob::new(entry.clone()).map_err(|_| invalid(entry))
}

fn shape(glob: &str) -> (Vec<&str>, bool) {
    let mut segments: Vec<&str> = glob.split('/').collect();
    let open = segments.len() > 1 && segments.last() == Some(&REST);
    if open {
        segments.pop();
    }
    (segments, open)
}

pub(crate) fn rule_matches(glob: &PathGlob, path: &RepoPath) -> bool {
    let (pattern, open) = shape(glob.as_str());
    let segments: Vec<&str> = path.as_str().split('/').collect();
    let length_fits = if open {
        segments.len() > pattern.len()
    } else {
        segments.len() == pattern.len()
    };
    length_fits
        && pattern
            .iter()
            .zip(&segments)
            .all(|(want, got)| want == got || *want == ANY_SEGMENT)
}

fn overlaps(left: &PathGlob, right: &PathGlob) -> bool {
    let (left, left_open) = shape(left.as_str());
    let (right, right_open) = shape(right.as_str());
    let agree = left
        .iter()
        .zip(&right)
        .all(|(a, b)| a == b || *a == ANY_SEGMENT || *b == ANY_SEGMENT);
    agree
        && match (left_open, right_open) {
            (true, true) => true,
            (true, false) => right.len() > left.len(),
            (false, true) => left.len() > right.len(),
            (false, false) => left.len() == right.len(),
        }
}
