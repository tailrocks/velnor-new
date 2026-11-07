// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Ordering, SemverEvidence};

impl SemverEvidence {
    pub(crate) fn valid_for(&self, version: &str) -> bool {
        if !self.is_final() {
            return false;
        }
        match self {
            Self::Checked { baseline, .. } => semver::Version::parse(baseline)
                .ok()
                .zip(semver::Version::parse(version).ok())
                .is_some_and(|(baseline, target)| {
                    baseline.cmp_precedence(&target) == Ordering::Less
                }),
            Self::NoBaseline { .. } => semver::Version::parse(version).is_ok(),
            Self::Deferred { .. }
            | Self::PriorNoBaselinePending { .. }
            | Self::MetadataOnly { .. } => false,
        }
    }

    pub(crate) fn is_final(&self) -> bool {
        let Some(expected) =
            velnor_actions_mise::catalog::qualified_cargo_semver_checks("ubuntu-26.04")
        else {
            return false;
        };
        match self {
            Self::Checked {
                baseline,
                baseline_checksum,
                checker_sha256,
            } => {
                semver::Version::parse(baseline).is_ok()
                    && baseline_checksum.len() == 64
                    && baseline_checksum
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && checker_sha256 == expected.executable_sha256
            }
            Self::NoBaseline { checker_sha256 } => checker_sha256 == expected.executable_sha256,
            Self::Deferred { .. }
            | Self::PriorNoBaselinePending { .. }
            | Self::MetadataOnly { .. } => false,
        }
    }
}
