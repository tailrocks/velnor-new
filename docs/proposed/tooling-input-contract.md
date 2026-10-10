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
inspect the repository Mise configuration only after credential variables are
removed. They resolve only the selected task's declared tool closure, then
install from a private configuration containing those exact source-bound lock
rows; they do not install the project-wide tool inventory. The repository
lock still needs maintenance whenever a selected task's tools change. These
jobs remain unconditional and uncached; they do not change the Rust lane's
install path.

Repository-owned Mise execution uses explicit top-level `[[checks]]` declarations,
independent of Rust task generation. Each declaration binds a task name,
directory, input files, runner platform, tool pins, and optional named scenario
evidence. Tools installation remains isolated; a separately qualified task
projection grants only the declared task closure access to repository inputs.
The removed Rust custom-task option is rejected as an unknown field. Explicit
verification and native jobs use the same sorted `[[workflow.tasks]]` list with
a strict `kind` discriminator. See the
[implemented named-check contract](../implemented/named-mise-checks.md) for the
execution boundary, trust admission, and Required evidence rules.

## 1.2. Isolated verification tasks

Each verification entry in `workflow.tasks` declares `id`,
`kind = "verification"`, an exact
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
cache inputs. The job resolves only the selected task's `tools` maps from root
`mise.toml`, including tasks reached through declared dependencies and simple
nested `mise run` calls. Every task in that closure must have a nonempty inline
`run` body. File-task-only dependencies and metadata-only TOML tasks fail
closed: pinned Mise can merge command-less TOML metadata onto a same-named
task-file script while retaining the script body, which is not represented in
the checked task closure. Unsupported custom task directories and task-file
includes also fail closed. If the selected closure is nonempty, every
selected tool must have an exact version and a source-bound current-platform
lock row with an accepted prebuilt backend, HTTPS release URL, and SHA-256.
Cargo registry and Git source
backends, unlocked tools, unsupported selector or lock options, and
unsupported task shapes fail closed. The only modeled options are the exact
BoltFFI asset regex and Rust components/targets copied from the idiomatic
toolchain file. Present settings must set `lockfile = true`; the only accepted
idiomatic-version selector is `rust`, and Cargo binstall settings cannot turn
either modeled flag off. Declared Cargo wrappers bind exactly to `mbx` with
`MBX_CARGO_SHIM_MODE=1`. Other root tools are excluded from the private
install config, so an unrelated `cargo:` tool cannot be installed by a
verification job. An empty
closure does not emit an install step. When tools are installed, the task runs
with `mise run --skip-tools`; all runs disable auto-install, env loading, and
hooks. The checked-out task config, lock, and Rust toolchain inputs are
hash-bound to planning and checked again before the task runs. It creates no
task cache, artifact, or downstream output. The verification kind is for
platform and other non-Rust checks only. Keeping task scripts free of Rust
compilation is an authoring and review invariant; V1 does not inspect or
enforce task bodies. Rust compilation remains in the existing MBX-backed lane
or the typed `build` variant below.
Protected release/signing is separate and is not provided by this kind.

## 1.3. Isolated native build variant

At most one `workflow.tasks` entry with `kind = "build"` declares a task that
may compile repository code. It carries a stable `id`, exact `mise_task`,
`runner = "macos-26-arm64"`, a `timeout_minutes` from 1 through 180,
`cargo_build_jobs` from 1 through 2, and `nextest_test_threads` from 1 through
2. The task uses the same workflow-wide ID namespace and sorted list as every
other task kind. The generated job key is `task-{id}`. This native job remains
a single hosted macOS 26 ARM64 job in every schema-2 execution mode; it checks out the same run source and
does not accept a repository or ref override.

The job checks for Xcode 26.6 build 17F113 and macOS SDK 26.5 under
`/Applications/Xcode_26.6.app/Contents/Developer`. Checkout disables
persisted credentials. The pinned Mise action sets up only Mise itself. The
job verifies the checked-out `mise.toml`, `mise.lock`, and
`rust-toolchain.toml` against the source-bound digests, then creates a private
task-owned Mise home containing only the declared tool closure and its exact
locked rows, including the locked ARM64 checksum artifacts where required.
It installs that isolated closure once with
`MISE_CARGO_BINSTALL_ONLY=1`; any selected tool without an accepted locked
prebuilt backend or artifact fails closed. The project Cargo selector is not
installed as a tool: the checked-out `mise.toml` must route Cargo through the
verified `mbx` wrapper, and the task's Rust compilation runs through that
wrapper. All Mise inspection, install, exec, and task commands set
`--no-env`; the task also inherits `MISE_NO_ENV=1`, so repository env files
and `env._.source` directives cannot run or alter the selected tools. The
checked config-file chain is compared before install and before task
execution. The declared task runs from the checked-out root with
`CARGO_BUILD_JOBS` and `NEXTEST_TEST_THREADS` from the validated limits,
Mise task and exec auto-install disabled, and the same Xcode developer
directory. Every shell step clears credential variables; the job grants only
`contents: read` and creates no cache, artifact, or downstream output. The consumer's own
task must bound SwiftPM invocation in its explicit task argv; Velnor does not
invent a SwiftPM environment alias.

Each declared native job is reconstructed and compared exactly during strict
rendering. Undeclared `task-*` jobs, alternate runner labels, checkout
source overrides, extra permissions, caches, artifacts, or altered task steps
fail closed. Every native job ID is added to `Required.needs`, so failure,
skip, or cancellation prevents the required check from succeeding. The
compile-free `verification` variant remains separate in behavior and cannot
acquire the native build capability by adding fields.

## 1.4. Native platform image validation

One `workflow.tasks` entry with `kind = "native-image"` declares an
unconditional native-host image validation job in the same sorted ID
namespace and generated `task-{id}` job namespace. Its closed fields are
`id`, `platform = "linux-arm64"`, a repository-owned `script` below
`maintained-image-build/`, `timeout_minutes` from 1 through 60, and
`cache = "task-owned-builder"`. The script must be a tracked regular file
whose path components are not symlinks. The runner is fixed to
`ubuntu-26.04-arm`. The job verifies the checked-out script bytes against the
planning-time digest, verifies the runner and Docker daemon are native Linux
ARM64, then runs the script with the task ID and the `linux/arm64` OCI
platform as its positional arguments. The build runs on native ARM64 hardware
without QEMU emulation.

The declared cache policy reserves task-owned BuildKit builder lifecycle to a
later take: no builder is created and no cleanup step is emitted yet, so the
repository script owns any image-layer caching it performs. Native-image jobs
are hosted-only in every schema-2 execution mode, grant only `contents: read`,
and join `Required`. Strict rendering reconstructs the exact runner, checkout,
source guard, host guard, and task command; undeclared `task-*` jobs or
altered steps fail closed.

Checksums are TOFU (trust on first use): the first download that
records a checksum trusts the bytes it received. Accepted residual,
stated explicitly: if an upstream artifact mutates after the lock
recorded its checksum (registry compromise, CDN poisoning, or a
re-published release tarball under the same version), neither the
hygiene audit nor CI installs detect it — CI installs the mutated
bytes under the same exact pin. Out of scope for V1 (independent
shasum channel, artifact transparency).

Determinism follows per class: fixed Rust-lane steps from exact catalog
pins, and all typed-task jobs from the sorted committed task declarations.
See
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
the CLI contract. `generate` produces product output only under `.github` and
the orchestrator coordinates its atomic replacement or preview write. For
in-place generation only, it may retain the private, self-ignored
`.github.velnor-stage/` runtime container with a root-bound owner record and a
persistent same-filesystem spare; cleanup removes spare children but keeps the
container and spare root. This state is not product output. In-place generation
requires atomic directory exchange and is supported on Linux and macOS; other
platforms fail before creating staging state. Linux requires kernel 5.8 or
newer with `STATX_MNT_ID` reported; Ubuntu 22.04 and newer require a compatible
running kernel, or generation fails closed before staging. `plan` and preview
generation do not create or modify it. In-place generation checks that the
repository, `.github`, staging state, and real output directories remain on
one mount, including same-device mount boundaries, before staging, before exchange, and
before cleanup. Mount topology must remain stable during the operation because
cleanup uses path-based removal. The mount identity source and cleanup-failure
behavior are specified in the generated-file contract. If `.github` exists,
its root and every real directory below it must be owned by the caller, as
specified in the generated-file contract. Velnor MUST not create
`.mise/tasks` or any other generated repository path. Before output replacement
it MUST verify byte-for-byte that `rust-toolchain.toml`,
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
