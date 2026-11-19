// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::BinaryPlan;

use crate::pinned_actions::ATTEST_BUILD_PROVENANCE;
use crate::release_assets::{assets, word};
use crate::release_binaries::{DIST, SUMS_FILE};
use crate::release_steps::{install_tool, variables};
use crate::rules::{RenderedStep, Value};

const GH_VERSION: &str = "2.103.3";

/// The `sums-<package>` step that attests every archive and `SHA256SUMS`.
pub(crate) fn attest(binary: &BinaryPlan) -> RenderedStep {
    RenderedStep {
        name: format!("Attest the build provenance of {}", binary.package),
        uses: Some(format!(
            "{}@{}",
            ATTEST_BUILD_PROVENANCE.action, ATTEST_BUILD_PROVENANCE.commit
        )),
        uses_comment: Some(ATTEST_BUILD_PROVENANCE.tag.to_owned()),
        with: vec![("subject-path".to_owned(), Value::Text(format!("{DIST}/*")))],
        ..RenderedStep::default()
    }
}

/// The `publish-<package>` steps that verify every asset before it is published.
///
/// Each path goes through `word`, so a versioned asset name keeps its
/// `${VERSION}` reference inside double quotes (see #1599).
pub(crate) fn verifications(binary: &BinaryPlan) -> Vec<RenderedStep> {
    let mut steps = vec![install_tool(&format!("gh@{GH_VERSION}"))];
    steps.extend(
        assets(binary)
            .into_iter()
            .chain([SUMS_FILE.to_owned()])
            .map(|name| RenderedStep {
                name: format!("Verify the build provenance of {name}"),
                env: variables(&[("GH_TOKEN", "${{ github.token }}")]),
                run: Some(format!(
                    "mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh attestation verify {} --repo \"$GITHUB_REPOSITORY\" --source-digest \"$GITHUB_SHA\" --source-ref \"$GITHUB_REF\" --signer-workflow \"$GITHUB_REPOSITORY/.github/workflows/release.yml\" --signer-digest \"$GITHUB_SHA\"",
                    word(binary, &format!("{DIST}/{name}"))
                )),
                ..RenderedStep::default()
            }),
    );
    steps
}
