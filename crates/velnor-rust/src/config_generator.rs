// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{LintKind, RepoPath, RustCiConfig, RustRoot};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};

const GENERATOR_PACKAGE: &str = "velnor-gen";

/// The manifest of the one Cargo root whose workspace holds the `velnor-gen` package. A
/// `from_source` check builds the generator with this manifest, so it works in any root.
pub(crate) fn generator_root(rust_roots: &[RustRoot]) -> Result<RepoPath, Reason> {
    let holders: Vec<&RustRoot> = rust_roots
        .iter()
        .filter(|root| {
            root.members
                .iter()
                .any(|member| member.name.as_str() == GENERATOR_PACKAGE)
        })
        .collect();
    match holders.as_slice() {
        [only] => Ok(only.manifest.clone()),
        _ => Err(Reason::GeneratorPackage {
            found: holders.len(),
        }),
    }
}

pub(crate) fn check(config: &RustCiConfig, rust_roots: &[RustRoot]) -> Result<(), DetectError> {
    let wanted = config
        .lint
        .iter()
        .any(|lint| matches!(lint.kind, LintKind::GeneratedCheck { from_source: true }));
    if !wanted {
        return Ok(());
    }
    generator_root(rust_roots)
        .map(drop)
        .map_err(|reason| DetectError::invalid(&CONFIG_PATH, reason))
}
