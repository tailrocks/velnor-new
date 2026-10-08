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

The workflow accepts tag push events matching `<package>-v*`. A read-only
source gate checks the event and ref shape, exact checked out commit, direct
or one-level annotated tag target, and the selected package and binary from
locked Cargo metadata. It then requires the tag to equal
`<package>-v<Cargo package version>`. It intentionally allows historical
version tags whose commits are no longer the default branch tip.

Two independent native jobs build the exact package and binary with pinned
Mise managed Rust, `--locked`, and `--release` for:

- `x86_64-unknown-linux-gnu` on `ubuntu-24.04`;
- `aarch64-apple-darwin` on `macos-15` (GitHub's standard arm64 label).

Each job checks its Rust host and executable format, then uploads one asset.
The publisher downloads the artifact IDs produced by those exact jobs in the
same workflow run. It rejects missing, extra, empty, symlinked, or non-regular
files, checks that each target archive contains exactly one regular binary
with executable mode, computes and verifies `SHA256SUMS`, and rechecks the
remote tag target against the verified source SHA before creating the GitHub
Release. Asset names are `<binary>-<version>-<target>.tar.gz`. For example,
to extract and run the macOS ARM64 artifact:

```sh
tar -xzf repo-scan-1.2.3-aarch64-apple-darwin.tar.gz
./repo-scan --help
```

No job runs for pull requests. Verification and build jobs have only
`contents: read`; build artifact upload uses the Actions runtime artifact
service. Only the publisher has `contents: write`, plus `actions: read` for
exact artifact downloads. The publisher does not check out or execute
downloaded binaries.
Concurrent runs serialize by tag. Existing releases cause creation to fail
rather than being overwritten.

GitHub does not expose an atomic operation that both compares a tag's object
SHA and creates a release against that same immutable comparison. The
publisher therefore reads and validates the remote tag immediately before
release creation and uses `--verify-tag`; a tag changed or deleted before
that check fails closed. A concurrent ref change in the small interval
between the check and release API call cannot be excluded by the available
GitHub API. Repositories requiring that race to be impossible need a separate
tag immutability rule at the GitHub repository level.
