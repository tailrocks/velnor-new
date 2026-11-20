// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Deserialize;
use velnor_model::{
    CargoCommand, FilterRef, LaneFeatures, LaneJob, LaneKeysWritten, PackageName, RepoPath,
    RustCiConfig, RustRoot, TestLane,
};

use crate::config::{CONFIG_PATH, TIMEOUT_RANGE};
use crate::config_test_groups::ranged;
use crate::error::{DetectError, Reason};
use crate::harness::uses_nextest;

const FORBIDDEN_CHARACTERS: [char; 8] = ['"', '$', '`', '\\', '|', '&', ';', '<'];
const FORBIDDEN_WORDS: [&str; 5] = ["if", "for", "while", "case", "coproc"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTestLane {
    root: String,
    #[serde(default)]
    features: Vec<String>,
    all_features: Option<bool>,
    no_default_features: Option<bool>,
    filter: Option<String>,
    #[serde(default)]
    lint: bool,
    #[serde(default)]
    doc: bool,
    #[serde(default)]
    packages: Vec<String>,
    job: Option<String>,
    timeout_minutes: Option<u16>,
}

pub(crate) fn parse(raw: Vec<RawTestLane>) -> Result<Vec<TestLane>, Reason> {
    raw.into_iter().map(lane).collect()
}

fn lane(raw: RawTestLane) -> Result<TestLane, Reason> {
    let all = raw.all_features.unwrap_or(false);
    let no_default = raw.no_default_features.unwrap_or(false);
    let written = LaneKeysWritten {
        job: raw.job.is_some(),
        feature_flags: raw.all_features.is_some() || raw.no_default_features.is_some(),
    };
    if all && !raw.features.is_empty() {
        return Err(Reason::TestLaneFeatureConflict {
            first: "all_features = true",
            second: "features",
        });
    }
    if all && no_default {
        return Err(Reason::TestLaneFeatureConflict {
            first: "all_features = true",
            second: "no_default_features = true",
        });
    }
    let root = match raw.root.as_str() {
        "." => None,
        directory => Some(RepoPath::new(directory.trim_end_matches('/')).map_err(|_| {
            Reason::InvalidValue {
                key: "test_lane.root",
                value: raw.root.clone(),
            }
        })?),
    };
    let mut seen = BTreeSet::new();
    let no_features = raw.features.is_empty() && !all && !no_default;
    let bad_feature = no_features
        || raw
            .features
            .iter()
            .any(|feature| !feature_name(feature) || !seen.insert(feature));
    if bad_feature {
        return Err(Reason::InvalidValue {
            key: "test_lane.features",
            value: raw.features.join(","),
        });
    }
    if let Some(filter) = raw.filter.as_deref().filter(|filter| !filterset(filter)) {
        return Err(Reason::InvalidValue {
            key: "test_lane.filter",
            value: filter.to_owned(),
        });
    }
    let packages = lane_packages(&raw)?;
    let timeout_minutes = ranged(
        "test_lane.timeout_minutes",
        raw.timeout_minutes,
        &TIMEOUT_RANGE,
    )?;
    let job = match raw.job.as_deref() {
        None | Some("inline") => LaneJob::Inline,
        Some("parallel") => LaneJob::Parallel,
        Some(other) => {
            return Err(Reason::InvalidValue {
                key: "test_lane.job",
                value: other.to_owned(),
            });
        }
    };
    Ok(TestLane {
        root,
        features: LaneFeatures {
            names: raw.features,
            all,
            no_default,
        },
        filter: raw.filter,
        lint: raw.lint,
        doc: raw.doc,
        packages,
        job,
        written,
        timeout_minutes,
    })
}

/// The packages a filtered lane takes its tests from. The plan narrows a pull
/// request to the changed packages, so without them the generator cannot tell
/// a lane with nothing to run from a lane whose filter is broken.
fn lane_packages(raw: &RawTestLane) -> Result<Vec<PackageName>, Reason> {
    let invalid = || Reason::InvalidValue {
        key: "test_lane.packages",
        value: raw.packages.join(","),
    };
    let mut seen = BTreeSet::new();
    if (raw.filter.is_some() && raw.packages.is_empty())
        || !raw.packages.iter().all(|name| seen.insert(name))
    {
        return Err(invalid());
    }
    raw.packages
        .iter()
        .map(|name| PackageName::new(name.clone()).map_err(|_| invalid()))
        .collect()
}

fn feature_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-+./".contains(c))
}

fn filterset(filter: &str) -> bool {
    !filter.trim().is_empty()
        && balanced(filter)
        && !filter
            .chars()
            .any(|c| c.is_control() || FORBIDDEN_CHARACTERS.contains(&c))
        && !filter
            .split_whitespace()
            .any(|word| FORBIDDEN_WORDS.contains(&word))
}

/// An unbalanced `)` would close the `(...)` the filter is wrapped in and let
/// the rest of it widen the selection past the plan's filter.
fn balanced(filter: &str) -> bool {
    let mut depth = 0_usize;
    for c in filter.chars() {
        match c {
            '(' => depth += 1,
            ')' => match depth.checked_sub(1) {
                Some(rest) => depth = rest,
                None => return false,
            },
            _ => {}
        }
    }
    depth == 0
}

pub(crate) fn commands(
    config: &RustCiConfig,
    root: &RustRoot,
    direct: &[String],
) -> Vec<CargoCommand> {
    let mut linted = BTreeSet::new();
    let mut documented = BTreeSet::new();
    lanes_of(config, root, LaneJob::Inline)
        .flat_map(|lane| lane_commands(lane, root, direct, &mut linted, &mut documented))
        .collect()
}

pub(crate) struct ParallelLane {
    pub(crate) features: LaneFeatures,
    pub(crate) timeout_minutes: Option<u16>,
    pub(crate) commands: Vec<CargoCommand>,
}

pub(crate) fn parallel(
    config: &RustCiConfig,
    root: &RustRoot,
    direct: &[String],
) -> Vec<ParallelLane> {
    lanes_of(config, root, LaneJob::Parallel)
        .map(|lane| ParallelLane {
            features: lane.features.clone(),
            timeout_minutes: lane.timeout_minutes,
            commands: lane_commands(
                lane,
                root,
                direct,
                &mut BTreeSet::new(),
                &mut BTreeSet::new(),
            ),
        })
        .collect()
}

pub(crate) fn has_parallel(config: &RustCiConfig) -> bool {
    config
        .test_lanes
        .iter()
        .any(|lane| lane.job == LaneJob::Parallel)
}

fn lanes_of<'a>(
    config: &'a RustCiConfig,
    root: &'a RustRoot,
    job: LaneJob,
) -> impl Iterator<Item = &'a TestLane> {
    config
        .test_lanes
        .iter()
        .filter(move |lane| lane.job == job && lane.root == root.manifest.parent())
}

fn lane_commands(
    lane: &TestLane,
    root: &RustRoot,
    direct: &[String],
    linted: &mut BTreeSet<LaneFeatures>,
    documented: &mut BTreeSet<LaneFeatures>,
) -> Vec<CargoCommand> {
    let features = || lane.features.clone();
    let mut commands = vec![CargoCommand::NextestFeatures {
        features: features(),
        packages: lane.packages.clone(),
        filter: match &lane.filter {
            Some(expr) => FilterRef::Expr {
                expr: expr.clone(),
                except: direct.to_vec(),
            },
            None if direct.is_empty() => FilterRef::All,
            None => FilterRef::AllExcept(direct.to_vec()),
        },
        ci_profile: root.nextest_ci_profile,
        reports: lane.job == LaneJob::Parallel,
    }];
    if let Some(expr) = &lane.filter {
        commands.push(CargoCommand::LaneGuard {
            features: features(),
            packages: lane.packages.clone(),
            filter: expr.clone(),
            except: direct.to_vec(),
        });
    }
    let normalized = lane.features.normalized();
    if lane.lint && linted.insert(normalized.clone()) {
        commands.push(CargoCommand::ClippyFeatures {
            features: features(),
        });
    }
    if lane.doc && documented.insert(normalized) {
        commands.push(CargoCommand::DocFeatures {
            features: features(),
        });
    }
    commands
}

pub(crate) fn check(config: &RustCiConfig, rust_roots: &[RustRoot]) -> Result<(), DetectError> {
    for lane in &config.test_lanes {
        let name = lane
            .root
            .as_ref()
            .map_or_else(|| ".".to_owned(), ToString::to_string);
        let reason = match rust_roots
            .iter()
            .find(|root| root.manifest.parent() == lane.root)
        {
            None => Reason::UnknownTestLaneRoot(name),
            Some(root) if !root.members.iter().any(|m| uses_nextest(config, m)) => {
                Reason::TestLaneWithoutNextest(name)
            }
            Some(root) => {
                let unknown = lane
                    .packages
                    .iter()
                    .find(|package| !root.members.iter().any(|member| member.name == **package));
                match unknown {
                    Some(package) => Reason::UnknownTestLanePackage(package.to_string()),
                    None => continue,
                }
            }
        };
        return Err(DetectError::invalid(&CONFIG_PATH, reason));
    }
    Ok(())
}
