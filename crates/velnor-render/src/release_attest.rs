// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::BinaryPlan;

use crate::pinned_actions::ATTEST_BUILD_PROVENANCE;
use crate::release::HOSTED_RUNNER;
use crate::release_assets::{assets, bind_version, read_version, word};
use crate::release_binaries::{
    DIST, PUBLISH_TIMEOUT_MINUTES, SUMS_FILE, artifact, behind_the_gate, download,
};
use crate::release_context::package_tag;
use crate::release_steps::{install_tool, permissions, variables};
use crate::rules::{RenderedJob, RenderedStep, Value};

const GH_VERSION: &str = "2.104.0";

/// The `sums-<package>` step that attests every archive and `SHA256SUMS`.
pub(crate) fn attest(binary: &BinaryPlan) -> RenderedStep {
    provenance(format!("Attest the build provenance of {}", binary.package))
}

pub(crate) fn provenance(name: String) -> RenderedStep {
    RenderedStep {
        with: vec![("subject-path".to_owned(), Value::Text(format!("{DIST}/*")))],
        ..ATTEST_BUILD_PROVENANCE.step(name)
    }
}

/// The `verify-<package>` job: the pinned `gh` runs with a token that reads
/// attestations and nothing else, and `publish-<package>` needs it.
pub(crate) fn verify(binary: &BinaryPlan) -> RenderedJob {
    let mut steps: Vec<RenderedStep> = binary
        .platforms
        .iter()
        .map(|platform| download(artifact(binary, *platform)))
        .collect();
    steps.push(download(format!("{}-sums", binary.package)));
    steps.extend(read_version(binary, &package_tag(binary)));
    steps.extend(verifications(binary));
    bind_version(&mut steps);
    RenderedJob {
        permissions: permissions(&[("attestations", "read")]),
        runs_on: HOSTED_RUNNER.to_owned(),
        timeout_minutes: PUBLISH_TIMEOUT_MINUTES,
        steps,
        ..behind_the_gate(
            format!("verify-{}", binary.package),
            binary,
            vec![format!("sums-{}", binary.package)],
        )
    }
}

/// The steps that verify every asset before it is published.
///
/// Each path goes through `word`, so a versioned asset name keeps its
/// `${VERSION}` reference inside double quotes (see #1599).
pub(crate) fn verifications(binary: &BinaryPlan) -> Vec<RenderedStep> {
    verification_steps(
        assets(binary)
            .into_iter()
            .chain([SUMS_FILE.to_owned()])
            .map(|name| {
                let path = word(binary, &format!("{DIST}/{name}"));
                (name, path)
            })
            .collect(),
    )
}

pub(crate) fn verification_steps(paths: Vec<(String, String)>) -> Vec<RenderedStep> {
    let mut steps = vec![install_tool(&format!("gh@{GH_VERSION}"))];
    steps.extend(
        paths
            .into_iter()
            .map(|(name, path)| verification_step(&name, &path, None)),
    );
    steps
}

pub(crate) fn verification_step(name: &str, path: &str, condition: Option<String>) -> RenderedStep {
    RenderedStep {
        name: format!("Verify the build provenance of {name}"),
        condition,
        env: variables(&[("GH_TOKEN", "${{ github.token }}")]),
        run: Some(format!(
            "mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh attestation verify {path} --repo \"$GITHUB_REPOSITORY\" --source-digest \"$GITHUB_SHA\" --source-ref \"$GITHUB_REF\" --signer-workflow \"$GITHUB_REPOSITORY/.github/workflows/release.yml\" --signer-digest \"$GITHUB_SHA\" --deny-self-hosted-runners"
        )),
        ..RenderedStep::default()
    }
}
