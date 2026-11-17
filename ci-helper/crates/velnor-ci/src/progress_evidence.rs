// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::cargo_artifacts::{CargoArtifact, CargoProgress, ProgressKind};
use crate::cargo_package::PackageIdentity;
use crate::telemetry::Invocation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    Compiled,
    Fresh,
    Unavailable,
}

#[must_use]
pub fn corroborate(invocation: &Invocation, progress: &CargoProgress) -> Evidence {
    if invocation.artifacts.is_empty() || progress.kind == ProgressKind::Downloaded {
        return Evidence::Compiled;
    }
    let mut matching = invocation
        .artifacts
        .iter()
        .filter(|artifact| matches(artifact, progress))
        .peekable();
    if matching.peek().is_none() {
        Evidence::Unavailable
    } else if matching.any(|artifact| !artifact.fresh) {
        Evidence::Compiled
    } else {
        Evidence::Fresh
    }
}

fn matches(artifact: &CargoArtifact, progress: &CargoProgress) -> bool {
    let Some(package) = PackageIdentity::parse(&artifact.package_id) else {
        return false;
    };
    let located = progress.location.as_deref().is_none_or(|location| {
        Path::new(&artifact.manifest_path).parent() == Some(Path::new(location))
            || package
                .source
                .is_some_and(|source| git_location(source, location))
    });
    if !located {
        return false;
    }
    package.name == progress.package && package.version == progress.version
}

fn git_location(source: &str, location: &str) -> bool {
    let Some(source) = source.strip_prefix("git+") else {
        return false;
    };
    let Some((repository, revision)) = location.rsplit_once('#') else {
        return false;
    };
    if source != repository {
        return false;
    }
    let expected = source
        .split_once('?')
        .and_then(|(_, query)| query.split('&').find_map(|part| part.strip_prefix("rev=")))
        .filter(|value| value.len() >= 7 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    expected.is_none_or(|expected| !revision.is_empty() && expected.starts_with(revision))
}

#[must_use]
pub fn retain(
    invocation: &Invocation,
    progress: Vec<CargoProgress>,
    diagnostics: &mut Vec<String>,
    verification_errors: &mut Vec<String>,
) -> Vec<CargoProgress> {
    progress.into_iter().filter(|event| {
                match crate::progress_evidence::corroborate(invocation, event) {
                    crate::progress_evidence::Evidence::Compiled => true,
                    crate::progress_evidence::Evidence::Fresh => {
                        diagnostics.push(format!("{} v{}: progress has only fresh artifact evidence; no new compilation attributed", event.package, event.version));
                        false
                    }
                    crate::progress_evidence::Evidence::Unavailable => {
                        verification_errors.push(format!("{} v{}: progress has no matching artifact evidence", event.package, event.version));
                        false
                    }
                }
            }).collect()
}
