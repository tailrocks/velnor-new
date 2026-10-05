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
| `authentication` | enum | `"trusted-publishing"` | `"trusted-publishing"` or `"bootstrap-token"` (kebab-case). Exactly one mode; modes never mix. |
| `release_pr` | bool | `true` | Whether release-plz preparation PRs run. |
| `tag_name` | string | `"{{ package }}-v{{ version }}"` | Safe charset, balanced braces, must contain both `{{package}}` and `{{version}}` (whitespace-insensitive). |
| `bootstrap` | table or absent | absent | Required iff `authentication = "bootstrap-token"`; forbidden otherwise (`missing_bootstrap_record` / `contradictory_authentication`). |
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
