# Generic Rust binary-release contract

**Status:** Proposed capability, implemented locally on an unmerged branch.
This workflow is separate from `[stacks.rust.release]`, which publishes
crates.io packages through release-plz.

## Configuration

`[stacks.rust.binary_release]` is disabled by default and is valid only under
`workflow.policy = "consumer-v1"`. Unknown fields fail configuration loading.

| Key | Type | Default | Rule |
|---|---|---|---|
| `enabled` | bool | `false` | Emits no binary release workflow unless true. |
| `manifest_path` | string | `"Cargo.toml"` | Repository relative path to `Cargo.toml`; rejects absolute paths, empty or `..` segments, and other filenames. |
| `package` | string | empty | Required when enabled; exact Cargo package name. |
| `binary` | string or absent | absent | Exact Cargo binary target; defaults to `package`. |
| `source_commit_env` | string or absent | absent | Optional Rust build environment variable set to the verified source SHA. Use it for compile-time values such as `env!("REPO_SCAN_SOURCE_COMMIT")`. |

Example for a repository whose binary embeds its source commit at compile
time:

```toml
[stacks.rust.binary_release]
enabled = true
manifest_path = "Cargo.toml"
package = "repo-scan"
binary = "repo-scan"
source_commit_env = "REPO_SCAN_SOURCE_COMMIT"
```

Environment variable names must match `[A-Za-z_][A-Za-z0-9_]*` and cannot
replace workflow, runner, Mise, Cargo, path, token, or source identity
variables. Package, binary, and manifest values are validated before becoming
fixed argv or generated output. There are no user supplied workflow, action,
runner, shell, or permission fields.

## Generated workflow

`generate` emits `.github/workflows/binary-release.yml` only when the option is
enabled. The workflow is generator owned and `plan` lists it under the same
condition. Successful generation retires the path when the option is disabled.

The workflow uses an hourly schedule (`17 * * * *`). GitHub loads and runs the
workflow from the repository's default branch for `schedule` events. The
read-only source gate also checks the API-reported default branch and requires
the scheduled commit to still equal its current head. A stale queued schedule
is a no-op and is retried at the next poll.

The source gate inventories `<package>-v*` tags and releases visible to its
read-only API token. GitHub hides manually staged draft releases from this
inventory. The resolver strictly parses SemVer 2.0.0 and considers tags in
descending SemVer precedence. Stable versions outrank prereleases; build
metadata does not affect precedence, and equal-precedence tags use ascending
tag name as a deterministic tie-break. Malformed versions, including numeric
identifiers with leading zeroes, are skipped. The resolver uses the runner's
Python 3 standard library for this ordering. It also skips tags that already
have a release, tags that do not point directly or through one annotated tag
to a commit, and commits that are not ancestors of the captured
default-branch SHA. For each remaining candidate, it reads locked Cargo
metadata from a Git archive and requires the exact package and binary plus a
matching `<package>-v<Cargo package version>` tag.
The read token is removed from the environment before parsing tag-controlled
manifests. The gate selects at most one candidate per poll; if none qualify,
build and publish jobs do not run. A selected tag must already be merged into
the default branch. Historical tags from commits no longer reachable from
the default branch are intentionally ineligible.

Two independent native jobs build the exact package and binary with pinned
Mise managed Rust, `--locked`, and `--release` for:

- `x86_64-unknown-linux-gnu` on `ubuntu-24.04`;
- `aarch64-apple-darwin` on `macos-15` (GitHub's standard arm64 label).

Each job checks the exact source checkout, tag target, Cargo package version,
Rust host, and executable format, then uploads one asset. The publisher runs
in a fresh job with no checkout. It downloads the artifact IDs produced by
those exact jobs in the same workflow run, rejects missing, extra, empty,
symlinked, or non-regular files, checks that each target archive contains
exactly one regular binary with executable mode, computes and verifies
`SHA256SUMS`, and rechecks the remote tag target and source ancestry against
the captured default-branch SHA before creating the GitHub Release. It does
not execute the downloaded binary or archive contents. Asset names are
`<binary>-<version>-<target>.tar.gz`. For example, to extract and run the
macOS ARM64 artifact:

```sh
tar -xzf repo-scan-1.2.3-aarch64-apple-darwin.tar.gz
./repo-scan --help
```

No job runs for pull requests or tag push events. Verification and build jobs
have only `contents: read`; their checkout steps do not persist credentials,
and the build jobs receive no write token or other release secret. Build
artifact upload uses the Actions runtime artifact service. Only the publisher
has `contents: write`, plus `actions: read` for exact artifact downloads.
Concurrent polls serialize for the default branch. Published releases and
drafts visible to the read-only resolver are skipped during candidate
selection. If a manually staged draft is hidden and belongs to the highest
eligible tag, the resolver can select that tag. The publisher then detects the
draft with its write token and fails before mutating it, with an error directing
maintainers to reconcile or remove the draft. It does not fall through to a
lower eligible tag in that poll; the hidden draft is a hard blocker until
maintainers resolve it, and lower tags wait. Any release or draft found at the
publisher preflight is rejected without mutation. A draft created after that
preflight is a residual time-of-check-to-time-of-use race; the current workflow
cannot guarantee that it will detect the draft before submitting the release
creation request. Release creation validates the SemVer value again and marks
versions with a prerelease identifier as GitHub prereleases; build metadata
alone never sets that flag.

GitHub does not expose an atomic operation that both compares a tag's object
SHA and creates a release against that same immutable comparison. The
publisher therefore reads and validates the remote tag immediately before
release creation and uses `--verify-tag`; a tag changed or deleted before
that check fails closed. A concurrent ref change in the small interval
between the check and the release API call cannot be excluded by the available
GitHub API. Repositories using this workflow must enforce immutable release
tags with a repository ruleset or equivalent policy. Without that policy, the
remaining tag-move race is a release blocker, not a condition this workflow
can make atomic.
