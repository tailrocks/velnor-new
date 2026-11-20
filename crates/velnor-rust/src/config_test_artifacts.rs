// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Deserialize;
use velnor_model::{
    ARTIFACT_RETENTION_DEFAULT, ArtifactWhen, GroupArtifact, RustCiConfig, aliased_secret,
    secret_like,
};

use crate::config_test_groups::ranged;
use crate::error::Reason;

const NAME_MAX_LENGTH: usize = 60;
const PATHS_MAX: usize = 16;
const ARTIFACTS_MAX: usize = 8;
const RETENTION_RANGE: std::ops::RangeInclusive<u16> = 1..=90;
const RESERVED_PREFIXES: [&str; 2] = ["nextest-archive", "velnor-"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTestArtifact {
    name: String,
    paths: Vec<String>,
    when: String,
    retention_days: Option<u16>,
}

fn refused(key: &'static str, value: &str, fix: impl Into<String>) -> Reason {
    Reason::TestGroupArtifact {
        key,
        value: value.to_owned(),
        fix: fix.into(),
    }
}

pub(crate) fn parse(raw: Option<Vec<RawTestArtifact>>) -> Result<Vec<GroupArtifact>, Reason> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    if raw.is_empty() || raw.len() > ARTIFACTS_MAX {
        return Err(refused(
            "test_groups.artifacts",
            &format!("{} entries", raw.len()),
            "write 1 to 8 entries, or remove the key",
        ));
    }
    raw.into_iter().map(artifact).collect()
}

fn artifact(raw: RawTestArtifact) -> Result<GroupArtifact, Reason> {
    check_name(&raw.name)?;
    let when = match raw.when.as_str() {
        "failure" => ArtifactWhen::Failure,
        "always" => ArtifactWhen::Always,
        other => {
            return Err(refused(
                "test_groups.artifacts.when",
                other,
                "must be \"failure\" or \"always\"",
            ));
        }
    };
    if raw.paths.is_empty() || raw.paths.len() > PATHS_MAX {
        return Err(refused(
            "test_groups.artifacts.paths",
            &format!("{} paths", raw.paths.len()),
            "name 1 to 16 paths under the repository",
        ));
    }
    for path in &raw.paths {
        if let Some(fix) = path_fault(path) {
            return Err(refused("test_groups.artifacts.paths", path, fix));
        }
        if let Some(fix) = alias_fault(path) {
            return Err(refused("test_groups.artifacts.paths", path, fix));
        }
    }
    let retention = ranged(
        "test_groups.artifacts.retention_days",
        raw.retention_days,
        &RETENTION_RANGE,
    )?;
    Ok(GroupArtifact {
        name: raw.name,
        paths: raw.paths,
        when,
        retention_days: retention.map_or(ARTIFACT_RETENTION_DEFAULT, |days| {
            u8::try_from(days).unwrap_or(u8::MAX)
        }),
    })
}

fn check_name(name: &str) -> Result<(), Reason> {
    let lower = name.to_ascii_lowercase();
    if name.is_empty()
        || name.len() > NAME_MAX_LENGTH
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(refused(
            "test_groups.artifacts.name",
            name,
            "use 1 to 60 letters, digits, '.', '_' or '-'",
        ));
    }
    if RESERVED_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return Err(refused(
            "test_groups.artifacts.name",
            name,
            "does not start with nextest-archive or velnor-, which the generated jobs use",
        ));
    }
    Ok(())
}

fn path_fault(path: &str) -> Option<&'static str> {
    if path.starts_with('-') {
        return Some(
            "starts with a hyphen, which a command line reads as an option; write the path from a directory name",
        );
    }
    if path.starts_with('/') {
        return Some("is outside the workspace; name a path under the repository");
    }
    if path.contains('~') || path.contains("${{") {
        return Some("is not a repository path; write the path itself, without ~ or an expression");
    }
    if path.is_empty()
        || !path.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '*' | '+' | '@')
        })
    {
        return Some(
            "has a character other than letters, digits, . _ - / * + and @; write one plain repository path or glob per entry",
        );
    }
    let mut segments: Vec<&str> = path.split('/').collect();
    if segments.last() == Some(&"") {
        segments.pop();
    }
    if segments
        .iter()
        .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Some(
            "is outside the workspace or not normal; write a path with no '..', '.' or empty part",
        );
    }
    if segments.first().is_some_and(|first| first.contains("**")) {
        return Some(
            "starts with a ** glob, which walks the whole workspace; start the path with a directory name",
        );
    }
    if segments
        .iter()
        .any(|segment| segment.eq_ignore_ascii_case(".git"))
    {
        return Some("is under .git, which holds repository credentials; name another path");
    }
    while segments
        .last()
        .is_some_and(|segment| segment.chars().all(|c| c == '*'))
    {
        segments.pop();
    }
    let Some(last) = segments.last() else {
        return Some("selects the whole workspace; name the report files themselves");
    };
    let in_target = segments.len() >= 2 && segments[segments.len() - 2] == "target";
    if *last == "target" || (in_target && matches!(*last, "debug" | "release")) {
        return Some(
            "uploads a whole build directory, which is far too large; name the report files, such as target/nextest/ci/junit.xml",
        );
    }
    if segments.iter().any(|segment| secret_like(segment)) {
        return Some(
            "may hold a secret (a .env file, a key, a token, a password or credentials), and anyone who can read the repository can download an artifact; name another path",
        );
    }
    segments
        .iter()
        .any(|segment| segment.starts_with('.'))
        .then_some(
            "has a hidden part (one that starts with a dot), which actions/upload-artifact does not upload by default, so the artifact would come out empty; name a path without hidden parts",
        )
}

pub(crate) fn check_unique(config: &RustCiConfig) -> Result<(), Reason> {
    let mut seen = BTreeSet::new();
    for group in &config.test_groups {
        for artifact in &group.artifacts {
            let names: Vec<String> = if group.partition_count() > 1 {
                (1..=group.partition_count())
                    .map(|index| format!("{}-{index}", artifact.name))
                    .collect()
            } else {
                vec![artifact.name.clone()]
            };
            for name in names {
                if !seen.insert(name.to_ascii_lowercase()) {
                    return Err(Reason::ArtifactNameTaken(name));
                }
            }
        }
    }
    Ok(())
}

fn alias_fault(path: &str) -> Option<String> {
    path.split('/').find_map(|segment| {
        aliased_secret(segment).map(|secret| {
            format!(
                "the part {segment:?} can match {secret:?}, a name that may hold a secret, and anyone who can read the repository can download an artifact; name the report files without a wildcard that reaches a secret name"
            )
        })
    })
}
