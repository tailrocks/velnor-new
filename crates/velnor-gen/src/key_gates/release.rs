// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Every key `.velnor/release.toml` accepts, with the first `velnor-ci` that parses it.

use super::{ConfigFile::Release, KeyGate, gate};
use crate::HELPER_MINIMUM;

pub(super) const RELEASE: &[KeyGate] = &[
    gate(Release, "after", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.98 via 29d96c9839
    gate(Release, "attest", (0, 1, 98)),
    gate(Release, "binaries", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "binaries.asset_name", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.91 via 6e2760b0b1
    gate(Release, "binaries.attest", (0, 1, 91)),
    gate(Release, "binaries.bin", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "binaries.bins", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "binaries.bins.bin", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "binaries.bins.package", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.92 via 3cd39fe1cf
    gate(Release, "binaries.checksum_files", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.92 via 3cd39fe1cf
    gate(Release, "binaries.commit_file", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.88 via 6ae7e88a95
    gate(Release, "binaries.format", (0, 1, 88)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "binaries.include", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.93 via b7eb11f0d9
    gate(Release, "binaries.member_digests", (0, 1, 93)),
    // helper-gate: first in tools-v0.1.109 (not tagged yet when this gate was written)
    gate(Release, "binaries.optional_platforms", (0, 1, 109)),
    gate(Release, "binaries.package", HELPER_MINIMUM),
    gate(Release, "binaries.platforms", HELPER_MINIMUM),
    // helper-gate: first in tools-v0.1.86 via 6700a81907
    gate(Release, "binaries.profile", (0, 1, 86)),
    // helper-gate: first in tools-v0.1.92 via 3cd39fe1cf
    gate(Release, "binaries.smoke", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.92 via 3cd39fe1cf
    gate(Release, "binaries.target", (0, 1, 92)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "checks", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "checks.artifacts", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "checks.before", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "checks.name", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "checks.needs", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "checks.permissions", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "checks.run", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "checks.runs_on", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.90 via 995c8bed14
    gate(Release, "checks.timeout_minutes", (0, 1, 90)),
    // helper-gate: first in tools-v0.1.98 via 29d96c9839
    gate(Release, "checksums_file", (0, 1, 98)),
    // helper-gate: first in tools-v0.1.98 via 29d96c9839
    gate(Release, "commit_file", (0, 1, 98)),
    // helper-gate: first in tools-v0.1.99 via 5abfdcc6d8
    gate(Release, "family_required_checks", (0, 1, 99)),
    // helper-gate: first in tools-v0.1.85 via bd00dd340b
    gate(Release, "family_tag", (0, 1, 85)),
    // helper-gate: first in tools-v0.1.98 via 29d96c9839
    gate(Release, "family_trigger", (0, 1, 98)),
    gate(Release, "pr_only_checks", HELPER_MINIMUM),
    gate(Release, "roots", HELPER_MINIMUM),
];
