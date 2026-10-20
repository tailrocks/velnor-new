# Release config schema (`[stacks.rust.release]`)

**Status:** Proposed. Implements `docs/proposed/release-contract.md` §1.
Reference for the typed config in `velnor-actions-contract`
(`config::release`); every rule below is enforced by
`RustReleaseConfig::validate` plus serde `deny_unknown_fields`.

## Field table

| Key | Type | Default | Rule |
|---|---|---|---|
| `enabled` | bool | `false` | `false` emits no release work; `true` requires a nonempty `packages` allowlist (`empty_packages`). |
| `manifest_path` | string | `"Cargo.toml"` | Repo-relative, `[A-Za-z0-9/._-]`, no leading `/`, no empty/`..` segments, must be or end with `/Cargo.toml`. |
| `packages` | string list | `[]` | Sorted, unique, never auto-expanded; each matches Cargo name shape (letter/`_` start, then alnum/`-`/`_`). |
| `environment` | string | `"release"` | Nonempty, unpadded, `[A-Za-z0-9-_/]` plus `.`, no empty/`..` segments. Binds the publish jobs. |
| `registry` | enum | `"crates-io"` | `"crates-io"` or `"git-tag"`. `git-tag` creates tags and GitHub releases only; see [git-tag mode](#git-tag-mode). |
| `authentication` | enum | absent (`"trusted-publishing"` on crates.io) | `"trusted-publishing"` or `"bootstrap-token"` (kebab-case). Exactly one mode; modes never mix. Forbidden with `registry = "git-tag"` (`contradictory_authentication`). |
| `release_pr` | bool | `true` | Whether release-plz preparation PRs run (crates.io only; `git-tag` releases are prepared by a maintainer's version bump). |
| `tag_name` | string | `"{{ package }}-v{{ version }}"` | Safe charset, balanced braces, must contain both `{{package}}` and `{{version}}` (whitespace-insensitive). |
| `bootstrap` | table or absent | absent | Required iff `authentication = "bootstrap-token"`; forbidden otherwise, including in `git-tag` mode (`missing_bootstrap_record` / `contradictory_authentication`). |
| `version_groups` | string→string-list map | `{}` | Group names use package-name shape; members must be sorted, unique, listed in `packages`, and in at most one group. Non-lockstep: unchanged members are not forced to release. |

## Bootstrap record

| Key | Type | Rule |
|---|---|---|
| `package` | string | Package-name shape; the one crate authorized for first publication. |
| `version` | string | Strict `major.minor.patch`, numeric parts only (`bad_version` otherwise). |
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
packages = ["termpane"]
environment = "release"
authentication = "trusted-publishing"
release_pr = true
tag_name = "{{ package }}-v{{ version }}"
```

Multi-crate with a version group (members coordinate, unchanged stay):

```toml
[stacks.rust.release]
enabled = true
packages = ["aaa-crate", "demo-crate"]

[stacks.rust.release.version_groups]
core = ["aaa-crate", "demo-crate"]
```

Bootstrap first publication (retire after the OIDC handover):

```toml
[stacks.rust.release]
enabled = true
packages = ["demo-crate"]
authentication = "bootstrap-token"

[stacks.rust.release.bootstrap]
package = "demo-crate"
version = "1.2.3"
source_sha = "0123456789abcdef0123456789abcdef01234567"
```

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

- `authentication` and `bootstrap` are forbidden (`contradictory_authentication`):
  the mode holds no registry credential and no `id-token`.
- `publishable_workspace` is forbidden (`git_tag_requires_packages`); the
  allowlist is explicit and `publish = false` and registry restrictions do
  not apply, because nothing is uploaded.
- Generation emits `publish.yml` and `.github/release-plz.toml` and no
  `release.yml` or bootstrap config. The release job holds `contents: write`
  only, runs in the `release` environment and needs the default-branch
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

