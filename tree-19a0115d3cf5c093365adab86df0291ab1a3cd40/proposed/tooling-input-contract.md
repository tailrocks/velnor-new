# Repository Tooling Input Contract

**Status:** Proposed. Velnor never owns the project's Rust or Mise tool files.

This contract defines how Velnor checks `rust-toolchain.toml`, `mise.toml`,
and `mise.lock`. Velnor may inspect their existence and contents, extract
versions/components for diagnostics or planning, and recommend concrete changes.
It MUST NOT create, modify, delete, normalize, lock, or otherwise manage them.

## 1. Ownership and behavior

| File | Velnor may read | Velnor may write | Missing-file behavior |
|---|---:|---:|---|
| `rust-toolchain.toml` | Yes | Never | Report `missing_recommended_input`; recommend a user-authored toolchain pin. |
| `mise.toml` | Yes | Never | Report `missing_recommended_input`; recommend a user-authored Mise tool config. |
| `mise.lock` | Yes | Never | Report `missing_recommended_input`; recommend the maintainer create/refresh it with Mise. |

These files are optional repository inputs, not prerequisites for `init` or
stack detection. Missing or malformed files MUST produce a structured finding
and a concrete human-readable recommendation; they MUST NOT trigger a write.
Generation continues with Velnor's own exact tool versions from its locked
version policy through Mise, so it does not silently inherit an older or loose
project tool selection.

`velnor-actions-rust` owns read-only inspection of `rust-toolchain.toml`.
`velnor-actions-mise` owns read-only inspection of `mise.toml` and `mise.lock`.
Neither adapter may inspect the other stack's files. The orchestrator merges
their findings into the internal inventory, plan, and generation report. The Rust
adapter may report the selected Rust version and requested components. The
Mise adapter may report tool versions, Mise tasks, and lockfile coverage. Each
finding MUST identify
which file supplied each value. It MUST NOT treat a value in these files as a
Velnor version pin, silently change a generated command to an older tool, or
claim that a loose `mise.toml` selector is an exact resolved version.
Conflicting or unsupported values produce findings and recommendations.
Generated Cargo commands set `RUSTUP_TOOLCHAIN` to Velnor's exact pinned Rust
version; Rustup gives that environment override precedence over a directory's
`rust-toolchain.toml`. The file remains useful for editor and non-Velnor use.
These files are not inputs to Velnor's generated Rust lanes by default:
changing them refreshes inspection findings, but does not alter Rust checks
when Velnor's actual tool pins and task inputs are unchanged. Explicit
`workflow.tasks` declarations are the separate exception described below.

## 1.1. Install isolation everywhere; lock hygiene for local dev (F6, G2)

Every generator-owned Rust-lane install step (`Prepare pinned tools`) carries
`--no-config` plus `MISE_NO_CONFIG=1`, so mise loads no repository
config during install — none of the paths mise reads (`mise.toml`,
`.mise.toml`, `.mise/config.toml`, `mise/config.toml`, `*.local.toml`
variants, `mise/conf.d/*.toml`, `.mise/conf.d/*.toml`,
`.config/mise*`, `MISE_ENV` variants) are ever opened. With no config
loaded, no `[plugins]` backend shadowing, no `bin/install` execution,
no `postinstall`, no `[hooks]`, no `[tasks]`, and no env loading can
fire: there is nothing to read them from. `generate` proves no
executable surface in these lanes because isolation leaves no surface
to prove — there is no config gate to bypass when no config is loaded.

Download bytes in CI trust upstream TLS plus exact catalog pins:
explicit `tool@exact` specs from Velnor's locked version policy are
the sole version authority. Generator-owned Rust-lane installs do not
consult the committed `mise.lock` — a lone lock is not enforced by mise
(tampering is ignored), and config-visible verification would re-arm
the code-execution paths those lanes avoid. The lock audit remains
advisory hygiene for local development. Explicit verification-task jobs
load repository Mise configuration only after credential variables are
removed. They run `mise install --locked` before the declared task, so
the project lock defines their tool closure and must be maintained with
each task's change coverage. These jobs remain unconditional and
uncached; they do not change the Rust lane's install path.

Repository-owned Mise execution uses explicit top-level `[[checks]]` declarations,
independent of Rust task generation. Each declaration binds a task name,
directory, input files, runner platform, tool pins, and optional named scenario
evidence. Tools installation remains isolated; a separately qualified task
projection grants only the declared task closure access to repository inputs.
The removed Rust custom-task option is rejected as an unknown field. See the
[implemented named-check contract](../content/docs/implemented/named-mise-checks.mdx) for the
execution boundary, trust admission, and Required evidence rules.

## 1.2. Isolated verification tasks

Each `workflow.tasks` entry declares `id`, `kind = "verification"`, an exact
`mise_task`, `runner`, and bounded `timeout_minutes`. IDs must be sorted,
unique, and safe as job keys; the generated base ID is `task-{id}`. The only
V1 runners are `linux-x64` (`ubuntu-26.04`) and `macos-arm64` (`macos-15`),
with separate pinned Mise binary digests. Task jobs are unconditional on pull
requests, pushes, and merge groups, have no dependencies of their own, and
join the `Required` fan-in. In schema 2, Linux tasks follow `hosted`,
`scale-set`, or `both` mode; `both` emits a hosted and a Scale Set job, and
both IDs are required. macOS ARM64 tasks stay hosted in all modes because the
current Scale Set is Linux/amd64; they remain required but do not qualify as
paired execution. Missing, failed, skipped, or cancelled tasks fail that
required check.

Task jobs grant only `contents: read`; other workflow permission scopes are
explicitly `none`. Checkout disables persisted credentials. The pinned Mise
setup action does not install project tools, activate repository env, or use
cache inputs. The job unsets the credential denylist before both
`mise install --locked` and `mise run <mise_task>`. It creates no task cache,
artifact, or downstream output. The verification kind is for platform and
other non-Rust checks only. Keeping task scripts free of Rust compilation is
an authoring and review invariant; V1 does not inspect or enforce task bodies.
Rust compilation remains in the existing MBX-backed lane pending a separate
reviewed capability.
Protected release/signing is separate and is not provided by this kind.

Checksums are TOFU (trust on first use): the first download that
records a checksum trusts the bytes it received. Accepted residual,
stated explicitly: if an upstream artifact mutates after the lock
recorded its checksum (registry compromise, CDN poisoning, or a
re-published release tarball under the same version), neither the
hygiene audit nor CI installs detect it — CI installs the mutated
bytes under the same exact pin. Out of scope for V1 (independent
shasum channel, artifact transparency).

Determinism follows per class: fixed Rust-lane steps from exact catalog
pins, and verification jobs from the sorted committed task declarations.
See
[task-execution §2](task-execution-contract.md) for the fixed vectors
and [workflow §3](../content/docs/proposed/workflow-contract.mdx) for the prepare step.

## 2. Read-only command contract

`plan` and `generate` MUST ask all registered adapters to check their respective
paths. In V1 this means the Rust and Mise adapters. Both commands use the same
inventory and MUST report the same presence, parse status, extracted values,
and content digests where readable. `plan` prints findings in its report;
`generate` writes exact suggested manual changes or Mise commands to stderr.
Velnor MUST NOT execute those suggestions. `init` does not inspect or modify
these files.

Malformed optional tool files MUST be reported as `tooling_input_invalid`.
Velnor continues using its own exact pinned tool selection and MUST NOT pass
malformed or project-defined configuration through to tool execution. A missing
lock or version mismatch MUST be visible as a recommendation, not an implicit
lock refresh.

`init` creates only the missing root `.velnor/config.toml` sample defined by
the CLI contract. `generate` produces only the `.github` tree and the
orchestrator coordinates its atomic replacement or preview write. Velnor MUST
not create `.mise/tasks` or any other generated repository path. Before output
replacement it MUST verify byte-for-byte that `rust-toolchain.toml`,
`mise.toml`, and `mise.lock` remain unchanged. `generate --output-dir PATH`
performs the same read-only inspection while writing only the preview
destination; it never edits the repository.

## 3. Recommendation format

Every finding has a stable code, path, observed value, recommended value or
action, and reason. Examples:

```text
missing_recommended_input: mise.toml
  recommendation: add the Velnor-recommended Rust and MBX tools manually

tool_version_not_latest: mise.toml [tools].rust
  observed: <version>
  recommended: <current exact stable version from Velnor's version policy>
  action: review and edit mise.toml, then refresh mise.lock with Mise
```

Recommendations are advisory output. Velnor MUST NOT create a patch, apply a
fix, run `mise use`, `mise lock`, `mise upgrade`, or `rustup` on behalf of the
repository. A later explicit user-authored change is checked on the next run.
