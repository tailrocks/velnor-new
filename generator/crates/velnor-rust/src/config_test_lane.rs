// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Deserialize;
use velnor_model::{CargoCommand, FilterRef, RepoPath, RustCiConfig, RustRoot, TestLane};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};
use crate::harness::uses_nextest;

const FORBIDDEN_CHARACTERS: [char; 8] = ['"', '$', '`', '\\', '|', '&', ';', '<'];
const FORBIDDEN_WORDS: [&str; 5] = ["if", "for", "while", "case", "coproc"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTestLane {
    root: String,
    features: Vec<String>,
    filter: Option<String>,
}

pub(crate) fn parse(raw: Vec<RawTestLane>) -> Result<Vec<TestLane>, Reason> {
    raw.into_iter().map(lane).collect()
}

fn lane(raw: RawTestLane) -> Result<TestLane, Reason> {
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
    let bad_feature = raw.features.is_empty()
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
    Ok(TestLane {
        root,
        features: raw.features,
        filter: raw.filter,
    })
}

fn feature_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-+./".contains(c))
}

fn filterset(filter: &str) -> bool {
    !filter.trim().is_empty()
        && !filter
            .chars()
            .any(|c| c.is_control() || FORBIDDEN_CHARACTERS.contains(&c))
        && !filter
            .split_whitespace()
            .any(|word| FORBIDDEN_WORDS.contains(&word))
}

pub(crate) fn commands(
    config: &RustCiConfig,
    root: &RustRoot,
    direct: &[String],
) -> Vec<CargoCommand> {
    config
        .test_lanes
        .iter()
        .filter(|lane| lane.root == root.manifest.parent())
        .map(|lane| CargoCommand::NextestFeatures {
            features: lane.features.clone(),
            filter: match &lane.filter {
                Some(expr) => FilterRef::Expr {
                    expr: expr.clone(),
                    except: direct.to_vec(),
                },
                None if direct.is_empty() => FilterRef::All,
                None => FilterRef::AllExcept(direct.to_vec()),
            },
            ci_profile: root.nextest_ci_profile,
        })
        .collect()
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
            Some(_) => continue,
        };
        return Err(DetectError::invalid(&CONFIG_PATH, reason));
    }
    Ok(())
}
