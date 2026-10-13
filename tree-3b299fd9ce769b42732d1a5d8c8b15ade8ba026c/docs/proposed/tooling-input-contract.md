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
These files are not task inputs by default: changing them refreshes inspection
findings, but does not rerun crate checks when Velnor's actual tool pins and
task inputs are unchanged.

## 1.1. `MISE_LOCKFILE=0` in fixed versus custom steps (F6)

Every generated step that invokes Mise carries `MISE_LOCKFILE=0` from the
shared validated step-env constructor, but the rationale differs by step
class; the split is intentional, not drift:

- Fixed generated steps (`Prepare pinned tools`, obligation steps, fetch
  steps) run fully hermetic: `MISE_LOCKFILE=0` plus `MISE_NO_CONFIG=1`
  (and the `--no-config` / `--no-env` / `--no-hooks` argv flags). The
  compiled catalog's exact pins are the sole tool authority; the
  project's lockfile is neither read (moot under `--no-config`) nor
  written. Removing `MISE_LOCKFILE=0` here would be theater that
  weakens defense-in-depth, so fixed steps keep it.
- Custom `mise run` allowlist steps intentionally load the repository's
  own Mise configuration (no `--no-config` flags), yet inherit the same
  `MISE_LOCKFILE=0`: CI MUST NOT create or update the project's
  lockfile as a side effect, so lockfile writes stay disabled while
  project tool versions resolve from the committed `mise.toml`.

Determinism follows per class: fixed steps from exact catalog pins,
custom steps from the committed project configuration. See
[task-execution §2](task-execution-contract.md) for the fixed vectors
and [workflow §3](workflow-contract.md) for the prepare step.

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
