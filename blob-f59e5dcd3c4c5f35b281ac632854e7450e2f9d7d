# Consumer Rust binary release contract

**Status:** Implemented as a disabled-by-default consumer workflow. This adds
one generated workflow and does not change crates.io release-plz behavior.

## Configuration and selection

The optional setting belongs to the consumer's `.velnor/config.toml`:

```toml
[workflow]
policy = "consumer-v1"

[stacks.rust.binary_release]
enabled = true
manifest_path = "Cargo.toml"
package = "repo-scan"
bin = "repo-scan"
```

Omitted configuration defaults to disabled and emits no
`.github/workflows/binary-release.yml`. Enabling it under
`velnor-repository-v1` is an error. The selected manifest must be a discovered
workspace-root manifest with a tracked `Cargo.lock`; the exact package must be
a member and the exact `bin` target must exist once. Targets with
`required-features` are rejected until the feature set has an explicit typed
contract. This first version builds one target only:
`aarch64-apple-darwin`.

At workflow runtime, every identity check runs after checkout of the same
source SHA. Pinned `cargo metadata --locked --no-deps` must find one matching
workspace package and binary, confirm that package's workspace membership,
including packages in a virtual workspace, and return its Cargo version. The
build uses `cargo build --release --locked`
with the same manifest, package, binary, and target. The workflow compares the
runtime version and tag again before publication. Its receipt records the
repository, source SHA, package, Cargo version, binary, target, and tag.
Each consumer workflow pins Mise 2026.10.6 for its Linux eligibility/publish
jobs and Apple Silicon build job using the verified platform checksums. This
consumer pin does not update Velnor's own generator, bootstrap, or release
runtime pins.

## Eligibility and job boundaries

The workflow has `workflow_dispatch` with no inputs. Its read-only eligibility
job requires the live repository default branch, workflow ref, workflow
authority SHA, event SHA, and checked-out SHA to agree. It waits for the
latest successful push run of `.github/workflows/ci.yml` on that exact source
and verifies exactly one successful `Required` job in the same run attempt.
The eligibility wait is bounded by 240 15-second polls and its job allows 75
minutes. The publish job's initial gate can wait for CI; its rechecks before
tag and release mutations make one poll and fail closed if the latest run
changed while publication was in progress. The publish job also allows 75
minutes for its initial wait and publication steps.

Build, attestation, and publish are separate jobs because GitHub's
`id-token: write` applies to every step in its job. Build can upload a
run-scoped artifact but has no repository write permission. Attestation can
write artifact metadata and provenance, but cannot write repository contents.
Only the final job has `contents: write`; it does not build or execute the
selected program. Checkout credentials are disabled in build and publish.

The final job runs in the protected `consumer-binary-release` environment.
Repository owners must create that environment, restrict deployment to
protected branches, and configure at least one required reviewer with
self-review prevented. The publisher checks this environment configuration
through GitHub's environment API before creating a tag and again before
publishing; an absent or insufficiently protected environment fails closed.
The workflow's source gate still limits a release to the exact default branch.
Repositories whose GitHub plan cannot configure required reviewers cannot use
this release path. Owners must also create the environment's
`IMMUTABILITY_READ_TOKEN` secret as a fine-grained PAT or GitHub App token
scoped to this repository with **Administration: read** only. This token is
used only to call GitHub's `GET /repos/{owner}/{repo}/immutable-releases`
preflight; the separate `GITHUB_TOKEN` has only the workflow's
`contents: write`, `actions: read`, and `attestations: read` permissions.
The workflow fails before creating a tag if the secret is absent, the API call
fails, immutable releases are disabled, or the environment lacks reviewer and
branch protection. It repeats both setup checks immediately before publishing
the draft. Missing admin-read access returns an error and is fail-closed. The
immutable-release setting and protected environment are required repository
prerequisites; generated YAML cannot configure either one. GitHub's API
requires Administration: read for immutable-release status and Actions: read
for environment configuration.

The release tag is `<Cargo package>-v<Cargo version>`. The publisher creates a
tag ref at the exact source SHA with the non-forcing GitHub API operation,
creates a draft release only after checking for an existing tag or release,
uploads the binary, `SHA256SUMS`, and `release.json` without overwrite flags,
then publishes and verifies the exact asset set. A pre-existing tag, release,
unavailable absence check, disabled immutable-release setting, mismatched
source, changed CI attempt, or partial release fails closed; the workflow
never moves a version tag or clobbers an asset. After publication it requires
GitHub to report the release as immutable and downloads the assets again to
compare their exact bytes, receipt, and checksums.

## Verification and limits

Tests cover disabled output, enabled generated rendering, invalid package/bin
selection, targets with required features, generic non-`main` branch
eligibility, exact target/tag rendering, and publisher permission separation.
Bounded script tests also exercise a virtual workspace member, nested package
manifest, fresh explicit Cargo target directory, required-feature rejection,
identity mismatches, immutable-setting and protected-environment preflights,
stale CI before tag and publish mutations, existing or partially colliding
identities, and non-404 absence-check failures.
Preview output is produced only by the regular renderer/generator path; the
workflow namespace remains generated-file-owned.

This feature does not attest a reproducible rebuild, establish branch
protection or environment rules, perform a seed release, sign platform
packages, or support Linux/Windows targets. Repository owners must review the
generated workflow, protect the default branch and `Required` CI gate, and
qualify the first release before depending on its assets.
