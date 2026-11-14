// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::fixed_python_authority::{self, FixedSource};

use super::{ProductReleasePins, RenderError, Yaml, generator_release, run};

const FILES: [FixedSource<'static>; 5] = [
    FixedSource {
        path: "runner_image_authority_identity.py",
        source: include_str!("../runner_image_authority_identity.py"),
    },
    FixedSource {
        path: "runner_image_authority_candidate.py",
        source: include_str!("../runner_image_authority_candidate.py"),
    },
    FixedSource {
        path: "runner_image_authority_custody.py",
        source: include_str!("../runner_image_authority_custody.py"),
    },
    FixedSource {
        path: "runner_image_authority_publish.py",
        source: include_str!("../runner_image_authority_publish.py"),
    },
    FixedSource {
        path: "runner_image_authority_driver.py",
        source: include_str!("../runner_image_authority_driver.py"),
    },
];

pub(super) fn materialize_steps() -> Vec<Yaml> {
    let mut files = vec![
        FixedSource {
            path: "job_image_authority_identity.py",
            source: include_str!("../job_image_authority_identity.py"),
        },
        FixedSource {
            path: "job_image_authority_record.py",
            source: include_str!("../job_image_authority_record.py"),
        },
        FixedSource {
            path: "job_image_authority_oci.py",
            source: include_str!("../job_image_authority_oci.py"),
        },
        FixedSource {
            path: "job_image_authority_tar.py",
            source: include_str!("../job_image_authority_tar.py"),
        },
        FixedSource {
            path: "job_image_authority_registry.py",
            source: include_str!("../runner_shared_job_image_authority_registry.py"),
        },
        FixedSource {
            path: "job_image_authority_publish.py",
            source: include_str!("../runner_shared_job_image_authority_publish.py"),
        },
        FixedSource {
            path: "job_image_authority_transport.py",
            source: include_str!("../runner_shared_job_image_authority_transport.py"),
        },
    ];
    files.extend(FILES);
    fixed_python_authority::materialize_steps(
        &files,
        "velnor-runner-authority",
        "VELNOR_RUNNER_AUTHORITY_DIR",
        "Materialize fixed runner image authority",
    )
}

const VERIFY: &str = r#"set -euo pipefail
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ "$GITHUB_REPOSITORY" == tailrocks/velnor-new ]]
authenticated="$RUNNER_TEMP/runner-family-authenticated"
mkdir -m 0700 "$authenticated"
for name in LICENSE NOTICE; do
  gh api --header 'Accept: application/vnd.github.raw+json' "repos/tailrocks/velnor-new/contents/$name?ref=$GITHUB_SHA" > "$authenticated/$name"
done
gh api --header 'Accept: application/vnd.github.raw+json' "repos/tailrocks/velnor-new/contents/.velnor/runner.lock?ref=$GITHUB_SHA" > "$authenticated/runner.lock"
gh attestation verify "$RUNNER_TEMP/runner-family-qualification/RUNNER_IMAGE_QUALIFICATION.json" \
  --repo tailrocks/velnor-new \
  --signer-workflow github.com/tailrocks/velnor-new/.github/workflows/runner-image-qualification-attest.yml@refs/heads/main \
  --signer-digest "$GITHUB_SHA" --source-digest "$GITHUB_SHA" --source-ref refs/heads/main \
  --deny-self-hosted-runners --predicate-type https://slsa.dev/provenance/v1 \
  --cert-oidc-issuer https://token.actions.githubusercontent.com --format json \
  > "$authenticated/qualification-verification.json"
"#;

pub(super) fn verification_step(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    Ok(run(
        "Authenticate qualification and exact source assets",
        format!("{}\n{VERIFY}", generator_release::gh_function(pins)?),
        &[("GH_TOKEN", "${{ github.token }}")],
    ))
}

pub(super) fn shell_steps(source: &str, path: &'static str, label: &'static str) -> Vec<Yaml> {
    fixed_python_authority::materialize_steps(
        &[FixedSource { path, source }],
        "velnor-runner-release-authority",
        "VELNOR_RUNNER_RELEASE_AUTHORITY_DIR",
        label,
    )
}
