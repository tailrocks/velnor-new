// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::BinaryPlan;

use crate::release::HOSTED_RUNNER;
use crate::release_assets::{assets, bind_version, read_version, word};
use crate::release_binaries::{
    DIST, GH_VERSION, PUBLISH_TIMEOUT_MINUTES, SUMS_FILE, artifact, behind_the_gate, download,
};
use crate::release_steps::{install_tool, permissions, variables};
use crate::rules::{RenderedJob, RenderedStep};

fn verifications(binary: &BinaryPlan) -> Vec<RenderedStep> {
    assets(binary)
        .into_iter()
        .chain([SUMS_FILE.to_owned()])
        .map(|name| RenderedStep {
            name: format!("Verify the build provenance of {name}"),
            env: variables(&[("GH_TOKEN", "${{ github.token }}")]),
            run: Some(format!(
                "mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh attestation verify {} --repo \"$GITHUB_REPOSITORY\" --source-digest \"$GITHUB_SHA\" --source-ref \"$GITHUB_REF\" --signer-workflow \"$GITHUB_REPOSITORY/.github/workflows/release.yml\" --signer-digest \"$GITHUB_SHA\" --deny-self-hosted-runners",
                word(binary, &format!("{DIST}/{name}"))
            )),
            ..RenderedStep::default()
        })
        .collect()
}

pub(crate) fn verify(binary: &BinaryPlan) -> RenderedJob {
    let mut steps: Vec<RenderedStep> = binary
        .platforms
        .iter()
        .map(|platform| download(artifact(binary, *platform)))
        .collect();
    steps.push(download(format!("{}-sums", binary.package)));
    steps.extend(read_version(binary));
    steps.push(install_tool(&format!("gh@{GH_VERSION}")));
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
