# Release config schema (`[stacks.rust.release]`)

**Status:** Proposed. Implements `docs/proposed/release-contract.md` §1.
Reference for the typed config in `velnor-actions-contract`
(`config::release`); every rule below is enforced by
`RustReleaseConfig::validate` plus serde `deny_unknown_fields`.

## Field table

| Key | Type | Default | Rule |
|---|---|---|---|
| `enabled` | bool | `false` | `false` emits no release work; `true` requires a nonempty `packages` allowlist unless `publishable_workspace` is enabled. |
| `manifest_path` | string | `"Cargo.toml"` | Repo-relative, `[A-Za-z0-9/._-]`, no leading `/`, no empty/`..` segments, must be or end with `/Cargo.toml`. |
| `packages` | string list | `[]` | For crates.io, at most 64 packages. Sorted, unique, never auto-expanded; each matches Cargo name shape (letter/`_` start, then alnum/`-`/`_`). |
| `publishable_workspace` | bool | `false` | Select the publishable Cargo workspace packages instead of an explicit package allowlist; crates.io expansion is limited to 64. |
| `publish_features` | package → string-list map | `{}` | Each key names a selected package. Feature selectors are sorted and unique; omitted packages use Cargo default features. Cargo validates feature existence during per-package verification. Not available for `git-tag`. |
| `new_crates` | table or absent | absent | Explicit creation intent with a full `source_sha` and a `packages` map of selected crate names to exact versions. Permitted only for crates.io with `bootstrap-token`; cannot coexist with `recovery`. The plan binds every listed version and the source. Protected admission and post-upload owner verification are required. |
| `environment` | string | `"release"` | Nonempty, unpadded, `[A-Za-z0-9-_/]` plus `.`, no empty/`..` segments. Binds the publish jobs. |
| `registry` | enum | `"crates-io"` | `"crates-io"` or `"git-tag"`. `git-tag` creates tags and GitHub releases only; see [git-tag mode](#git-tag-mode). |
| `authentication` | enum | absent (`"trusted-publishing"` on crates.io) | `"trusted-publishing"` or `"bootstrap-token"` (kebab-case). Exactly one mode; modes never mix. Forbidden with `registry = "git-tag"` (`contradictory_authentication`). |
| `required_workflow_path` | string | empty | Enabled crates.io releases require a workflow path under `.github/workflows/`, ending in `.yml` or `.yaml`. |
| `trusted_reviewer_id` | integer | `0` | Enabled crates.io releases require a nonzero GitHub user ID for the protected environment reviewer. |
| `release_pr` | bool | `true` | Whether release-plz preparation PRs run (crates.io only; `git-tag` releases are prepared by a maintainer's version bump). |
| `tag_name` | string | `"{{ package }}-v{{ version }}"` | For crates.io, must be `{{ package }}-v{{ version }}` (whitespace-insensitive). Git-tag releases support safe templates containing both variables. |
| `recovery` | table or absent | absent | Binds an exact package, version, and source for an approved recovery dispatch; push events are rejected. Cannot coexist with `new_crates`. Bootstrap-token authentication requires either `recovery` or `new_crates`. |
| `journal_predecessor` | table or absent | absent | Binds a reviewed previous run ID, attempt, and workflow SHA for durable release recovery. Requires `recovery` or `new_crates` to bind the exact source; only attempt `1` is accepted. |
| `version_groups` | string→string-list map | `{}` | Group names use package-name shape; members must be sorted, unique, listed in `packages`, and in at most one group. Native release-plz groups use the maximum next version across members; an otherwise unchanged member is updated when its current version differs from that maximum. |

For example, to verify `demo-crate` with its `pty` feature during publication:

```toml
[stacks.rust.release.publish_features]
demo-crate = ["pty"]
```

Cargo default features remain enabled. The publisher must pass the same reviewed selectors to Cargo verification and release-plz.

## Recovery record

To resume an interrupted first attempt, review a recovery configuration that
names its exact run ID, attempt `1`, and workflow SHA in
`journal_predecessor`, then start a fresh approved dispatch. GitHub workflow
reruns are not supported by the current approval verification. If a later
attempt performed an operation, stop and reconcile it separately before
publishing; do not identify attempt `1` as a substitute for that operation.

| Key | Type | Rule |
|---|---|---|
| `package` | string | Package-name shape; the one crate authorized for recovery. |
| `version` | string | Cargo SemVer, including prerelease and build metadata, up to 150 ASCII bytes (`bad_version` otherwise). |
| `source_sha` | string | 40 lowercase hex, the immutable approved source (`bad_source_sha` otherwise). |

Unknown keys anywhere in this section fail deserialization with
`unknown_config_field`. There are no shell/YAML/`uses` override fields by
design. Validation runs whether `enabled` or not, so drafted config stays
safe; only the nonempty-allowlist rule is gated on `enabled`.

## Examples

Disabled (the default; nothing is emitted):

```toml
[stacks.rust.release]
enabled = false
```

Single crate on trusted publishing:

```toml
[stacks.rust.release]
enabled = true
manifest_path = "Cargo.toml"
packages = ["demo-crate"]
required_workflow_path = ".github/workflows/ci.yml"
trusted_reviewer_id = 12345
environment = "release"
authentication = "trusted-publishing"
release_pr = true
tag_name = "{{ package }}-v{{ version }}"
```

Multi-crate with a version group (members converge to the maximum next version):

```toml
[stacks.rust.release]
enabled = true
packages = ["aaa-crate", "demo-crate"]
required_workflow_path = ".github/workflows/ci.yml"
trusted_reviewer_id = 12345

[stacks.rust.release.version_groups]
core = ["aaa-crate", "demo-crate"]
```

Bootstrap first publication:

```toml
[stacks.rust.release]
enabled = true
packages = ["demo-crate"]
required_workflow_path = ".github/workflows/ci.yml"
trusted_reviewer_id = 12345
authentication = "bootstrap-token"

[stacks.rust.release.new_crates]
source_sha = "0123456789abcdef0123456789abcdef01234567"

[stacks.rust.release.new_crates.packages]
demo-crate = "1.2.3-rc.1+build.42"
```

Creation is limited to an approved dispatch for the listed source and versions.
After configuring trusted publishing, remove `new_crates` and switch
`authentication` to `"trusted-publishing"` in a reviewed configuration change,
then regenerate the workflow. This restores normal push releases; retaining the
creation configuration keeps the release dispatch-only. Revoke the bootstrap
token and remove its secret after the handover.

## git-tag mode

Adopted per the split brief; owner may revisit. Private Rust crates that
other repositories consume as `git` dependencies release by tag instead of
publishing to a registry. A git dependency checks out the whole repository,
so paths such as `embed_migrations!("../../backend/eth-flyway/...")` stay
valid, which a registry package could not guarantee.

```toml
[stacks.rust.release]
enabled = true
registry = "git-tag"
packages = ["eth-migration", "eth-model-proto", "eth-processor-fixtures"]
tag_name = "{{ package }}-v{{ version }}"
```

Rules (`RustReleaseConfig::validate`, then generation):

- `authentication`, `recovery`, and `new_crates` are forbidden:
  the mode holds no registry credential and no `id-token`.
- `publishable_workspace` is forbidden (`git_tag_requires_packages`); the
  allowlist is explicit and `publish = false` and registry restrictions do
  not apply, because nothing is uploaded.
- Generation emits `publish.yml` and `.github/release-plz.toml` and no
  `release.yml` or bootstrap config. The release job holds `contents: write`
  and `pull-requests: read` only, runs in the `release` environment and needs the default-branch
  authority job. See the
  [package publication contract](package-publication-contract.md) §6.
- `release-plz.toml` sets `publish = false`, `git_only = true`,
  `git_tag_enable = true`, `git_release_enable = true`,
  `release_always = true` and `semver_check = false`. `git_only` takes
  versions from tags instead of asking crates.io about a private crate.
  `release_always` is safe here because tagging is idempotent and the
  maintainer's version bump is the authorization; registry modes keep
  `release_always = false`. `semver_check` needs a registry baseline.
- Any `git` source on the repository owner's own GitHub organization must be
  pinned with `tag` (`private_git_dependency_unpinned`); a branch moves and a
  bare revision hides the release.
