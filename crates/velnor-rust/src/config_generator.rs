// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{LintKind, RepoPath, RustCiConfig, RustRoot, TargetKind};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};

const GENERATOR_PACKAGE: &str = "velnor-gen";

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
        [only] => {
            let bins = only
                .members
                .iter()
                .filter(|member| member.name.as_str() == GENERATOR_PACKAGE)
                .flat_map(|member| &member.targets)
                .filter(|target| {
                    target.kinds.contains(&TargetKind::Bin) && !target.requires_features
                })
                .count();
            if bins == 1 {
                Ok(only.manifest.clone())
            } else {
                Err(Reason::GeneratorBinary { found: bins })
            }
        }
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
