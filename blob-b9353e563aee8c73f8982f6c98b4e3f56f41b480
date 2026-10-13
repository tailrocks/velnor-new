# Velnor V1 Rust Quality Contract

**Status:** Proposed specification. Landed implementation records live in
`docs/implemented/` and must change in the same implementation pull request.

This contract governs the selected Rust stack in the Velnor V1 workflow
generator. It specifies the Rust repository shape, tool selection, test
recommendations, dependency use, and verification. Alint is the only
repository-structure linter in V1; it is limited to configured generic
file/path, required-file, and line-count rules. V1 has no custom Rust
source-structure, Cargo-architecture, or test-layout linter. Automatic stack
detection, composition, `[stacks].ignore`, and the stack-neutral
`workflow.policy` belong to the workflow contract. The deferred macOS/Debian
self-hosted runner is outside this contract.

The words **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are normative.

## 1. Repository shape and authority

The repository MUST start with this shape:

```text
Cargo.toml                 # virtual workspace and shared policy
Cargo.lock                 # committed resolution
rustfmt.toml
clippy.toml
deny.toml
.mise-version
.alint.yml
.config/nextest.toml
AGENTS.md
CODEOWNERS                  # repo root; Velnor never emits, reads, or validates it
.velnor/config.toml
.velnor/version-policy.toml
crates/
  velnor-archive-guard/       # repository-only archive security utility
  velnor-actions-contract/
  velnor-actions-rust/
  velnor-actions-mise/
  velnor-actions-actionlint/
  velnor-actions-workflow-renderer/
  velnor-actions-orchestrator/
  velnor-actions-cli/
  velnor-actions-freshness/   # repository-only freshness maintenance
  velnor-actions-tofu/
# Optional, repository-owned, read-only Velnor inputs:
rust-toolchain.toml
mise.toml
mise.lock
```

All first-party Rust packages MUST be under `crates/`; root MUST be virtual with
exactly the eight product package names below plus repository-only member `velnor-actions-freshness`
and utility `velnor-archive-guard`. Cargo metadata defines membership; V1 adds
no custom architecture linter; §5, Alint, Clippy, tests, and review govern boundaries.
Within V1 product behavior, Rust/Cargo metadata and `rust-toolchain.toml` belong to `velnor-actions-rust`; Mise syntax, environment, and task metadata belong to `velnor-actions-mise`; actionlint metadata belongs to `velnor-actions-actionlint`.
Freshness may inspect Velnor-owned Cargo/Mise sources for private maintenance;
results MUST NOT feed V1 planning. Scripts provision archive guard. Neither
helper enters V1 Rust task derivation or the product crate matrix. The
canonical Velnor repository uses the existing named-Mise-check contract to
run both helper packages' Rust test suites as one required
`maintenance-helpers` repository-quality check; ordinary ConsumerV1
configurations do not declare this repository-owned check. Alint enforces only configured
generic file/path, required-file, and line-count rules.
The CLI package MUST declare binary `velnor-actions`, the only target name without the package-purpose suffix. Non-Rust directories MAY remain in their own conventional locations.

The eight V1 product crates have fixed boundaries; the two repository-only members are separately labeled in the table. Every V1 Cargo package MUST use the `velnor-actions-<purpose>` namespace. Generic names such as `velnor-model`, `velnor-core`, `velnor-rust`, `velnor-common`, and `velnor-utils` are forbidden.
The `velnor-actions` binary is owned by package `velnor-actions-cli`. Future stacks use dedicated names such as `velnor-actions-node`; they remain independent of other stack crates.

| Crate | MUST own | MUST NOT own |
| --- | --- | --- |
| `velnor-actions-contract` | Stack-neutral workflow/task contracts, task graph, identities, reports, recommendations | Rust/Cargo, Mise, process, filesystem, YAML implementation, CLI, generic application models |
| `velnor-actions-rust` | All Rust/Cargo discovery, metadata conversion, targets, graph, affected selection, Rust task proposals, `rust-toolchain.toml` inspection | Mise config/commands, workflow YAML, process execution, or non-Rust stack behavior |
| `velnor-actions-mise` | Mise tool selection, pinned command construction, fixed subprocess/environment wrapper, `mise.toml`/`mise.lock` inspection, Mise cache integration | Cargo metadata, Rust graph/selection rules, `rust-toolchain.toml`, GitHub YAML, stack discovery |
| `velnor-actions-actionlint` | actionlint pin/capability metadata, generated config, action-schema validation | Mise process execution, stack scanning, generic workflow rendering |
| `velnor-actions-workflow-renderer` | Generic GitHub Actions workflow YAML from typed workflow IR | Rust/Cargo, Mise syntax, repository scanning, subprocesses, stack policy |
| `velnor-actions-orchestrator` | Composition, obligation selection, cache evidence, scheduling, generation coordination, typed process-request coordination | Parsing Cargo/Mise files, direct YAML templates, CLI parsing, OS process details and process creation/control, except the read-only `rustix::process::geteuid()` caller-owner lookup in the private generation-stage validator |
| `velnor-actions-cli` | Clap parser, typed dispatch, concise deterministic human plan renderer, generation output, and exit-code formatting; emits binary `velnor-actions` | Orchestration algorithms or Rust, Mise, and renderer domain rules |
| `velnor-actions-tofu` | All OpenToFu/HCL discovery, root/module interpretation, task payloads, affected selection, identity extensions | Rust/Cargo, Mise execution, workflow YAML, process details, non-tofu stacks |
| `velnor-actions-freshness` (repository-only support) | Read-only repository freshness/pin/lock/advisory checks and bounded bootstrap metadata operations behind the existing CLI private gate | V1 planning, task graph or selection, runner behavior, product evidence claims, public commands, or workflow generation |
| `velnor-archive-guard` (repository-only security utility) | Bounded validation of owned candidate and Cargo package archive bytes through its explicit modes | V1 stack behavior, task derivation, workflow generation, or product dependencies |

Both repository-only members are outside the eight-product V1 task graph. The
CLI private gate consumes freshness at runtime, repository scripts provision
archive guard, and the orchestrator does not link either helper as a library.
The canonical repository's `maintenance-helpers` named Mise check runs their
package test suites independently of Rust task derivation and the product
matrix; this repository's VelnorRepositoryV1 configuration makes it required.

Each repository-only package declares its task owner with Cargo package metadata:

```toml
[package.metadata.velnor]
v1-task-owner = "repository-maintenance"
```

The Rust metadata adapter maps that value to a typed owner and rejects unknown owner values. Under `VelnorRepositoryV1`, the orchestrator omits only root-workspace packages with this owner from V1 task derivation and feature union; `prepare` verifies the canonical origin first. Under `ConsumerV1`, the same metadata does not exclude a same-named project. Package names and paths alone never grant this exclusion.

Hard invariants 10–12 (spec §3.3) are normative throughout. 10 — One owner per
domain rule per the table above; extend an existing owner before making a new
module, and fix cycles through ownership changes. 11 — Never duplicate domain
knowledge: on a second use move the rule to its owner, switch every caller,
verify identical behavior, then add the new caller; record unconverted
instances with reasons. 12 — No domain logic in rendering or transport:
rendering, dispatch, display, and transport layers render or execute owners'
decisions; boundary safety validation MUST NOT duplicate domain planning.

The authority order MUST be:

1. Cargo manifests and `Cargo.lock` define packages and dependencies.
2. The compiled catalog defines generated-workflow versions. Velnor's
   `.velnor/version-policy.toml` mirrors it for freshness checks; consumers do
   not need that file. Optional `mise.toml`, `mise.lock`, and
   `rust-toolchain.toml` are repository-owned inspection inputs.
3. `.velnor/config.toml` defines Velnor policy and explicit exceptions.
4. The generator emits workflows and task definitions; generated files MUST NOT
   be hand-maintained.
5. The separate required Alint job and CI branch rules gate repository-policy
   changes.

Rust task configurations MUST live under `[stacks.rust]` in
`.velnor/config.toml`. They describe feature, target, and task variants emitted
after Rust detection. Rust detection does not require a Rust profile, and this
document does not define a CLI stack selector. `workflow.policy` remains the
stack-neutral choice between `consumer-v1` and `velnor-repository-v1`.

## 2. Toolchain and update policy

The workspace MUST use Rust 2024 and resolver 3. Gate 0 MUST select the latest
patched stable release then available and record it; the 2026-09-27 research
snapshot is Rust 1.98.1 with MSRV 1.98. Never use a placeholder MSRV.

```toml
[workspace]
members = ["crates/velnor-actions-contract", "crates/velnor-archive-guard", "crates/velnor-actions-rust", "crates/velnor-actions-mise", "crates/velnor-actions-actionlint", "crates/velnor-actions-workflow-renderer", "crates/velnor-actions-orchestrator", "crates/velnor-actions-cli", "crates/velnor-actions-freshness", "crates/velnor-actions-tofu"]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.98"
```

Mise MUST execute Rust tooling locally and in CI. Generated commands use exact
Rust, MBX, and auxiliary-tool pins from the compiled-in generator catalog
through `mise exec`; they MUST NOT depend on consumer `.velnor/version-policy.toml`,
project `mise.toml`, or `mise.lock`. Velnor's dogfood gates compare the catalog
to its repository-owned version-policy mirror and recheck current releases.
Cargo `rust-version` declares MSRV and MUST be
qualified for every product crate on dependency/toolchain/policy updates and
release qualification, not every pull request. It MUST match the selected
stable toolchain's major/minor unless compatibility requires otherwise. A
pinned nightly MAY exist only for a check that needs it.

Velnor MUST check for `rust-toolchain.toml`, `mise.toml`, and `mise.lock`; it
MAY report their versions, components, tasks, lock coverage, and conflicts.
They are never generated, synchronized, locked, rewritten, or used to override
Velnor pins. Missing/malformed files produce manual recommendations, not
automatic repair or generation failure. See the [tooling input contract](tooling-input-contract.md).

Every member MUST inherit the workspace metadata and lints:

```toml
[package]
edition.workspace = true
rust-version.workspace = true

[lints]
workspace = true
```

`Cargo.lock` MUST be committed and normal commands MUST use `--locked`. The
latest-stable selection, freshness inventory, update cadence, exceptions, and
required policy behavior is normative in the [version policy](version-policy.md).
`Cargo.lock` MUST NOT remain stale merely because its current build passes.
Optional `mise.lock` is repository-owned; Velnor reports findings but never
creates or refreshes it.

## 3. Mise and MBX ownership

`.mise-version` MUST pin the exact Mise release. GitHub CI MUST bootstrap this
one binary from the immutable artifact and SHA-256 in `.velnor/generator.lock`
before invoking it; local developers MUST use the same pinned release. The
bootstrap verifies version and digest. Mise itself is the bootstrap exception;
all subsequent tools MUST be installed and selected by Mise.

Mise MUST install and execute Rust, components, MBX, selected Nextest, `gh`,
policy tools, and Rust binaries. Exact pins come from the compiled catalog and
explicit Mise arguments; Velnor's version-policy file mirrors them for
freshness checks. Consumer `mise.toml` and `mise.lock` are optional,
repository-owned, read-only inputs. See [version policy](version-policy.md).
An illustrative hand-maintained project config is:

```toml
[tools]
rust = { version = "<exact-version>", profile = "minimal", components = ["clippy", "rustfmt"], mr_boxington = true }
mr-boxington = "<exact-version>"
```

If a project chooses to maintain this configuration, its owner may enable
Mise's Rust/MBX integration and declare exact tools. Velnor may recommend this
setup, but MUST NOT create or edit it. Generated tasks invoke the exact pinned
MBX executable explicitly; a project's older selector cannot downgrade it.
Use Velnor-owned persistent `MISE_RUSTUP_HOME` and `MISE_CARGO_HOME` paths.
Every generated Cargo invocation sets `RUSTUP_TOOLCHAIN` to Velnor's exact
Rust pin so an inspected project toolchain file cannot select another compiler.

Normal validation MUST run through `mise run` or `mise exec`. Direct calls to
an absolute rustup Cargo binary, `cargo install`, ad hoc component installation,
or a second compiler-cache installer are forbidden. After preparation, Mise
automatic installation MUST be disabled for verification so missing tools fail
as preparation errors.

The preflight MUST prove the effective route per workspace. For MBX profiles,
report its selected Mise tool, version, and compiler invocation handled by MBX.
For Cargo profiles, prove the exact Cargo toolchain and no MBX wrapper. Merely
finding `mr_boxington` in a lockfile is insufficient. Each mutable concurrent
Cargo lane MUST have its own target directory; MBX owns compiler reuse for MBX
profiles, and no second cache may archive its store.

## 4. Tests and source layout

Detail lives in [rust-test-policy.md](rust-test-policy.md).

## 5. Size and architecture limits

The following limits are hard errors for new handwritten code:

| Item | Limit |
| --- | ---: |
| Rust source, including tests | 400 physical lines |
| `src/lib.rs` and `src/main.rs` | 150 physical lines |
| Function or method | 80 lines using Clippy accounting |
| Velnor configuration and instruction documents | 400 physical lines |

Counts include comments and blank lines. Lockfiles, fixtures, vendored code,
and generated output MUST have explicit classifications and owners. Agents MUST
NOT raise limits, add arbitrary exclusions, relabel handwritten code as
generated, or reduce test assertions to satisfy a limit.

A crate MUST provide a real boundary: independently testable responsibility,
optional heavyweight dependency, platform separation, stable API, or measured
rebuild reduction. Pure model code MUST NOT depend on UI, database, HTTP,
platform, or process crates. Do not create wrapper or `utils` crates only to
meet a count. The allowed dependency directions are architectural requirements
reviewed from Cargo metadata through this mechanism allowlist only: (1) a `cargo-metadata` edge test over the manifest graph, (2) an exact workspace member-set test, (3) literal-substring ownership tests over declared paths, (4) sizes via generic Alint line-count rules plus Clippy `too_many_lines`, (5) human-reviewed insta snapshots. Custom linters are forbidden; Alint stays generic-only.

Timeless principles (spec §3.2): separation of concerns per §1,
`pub(crate)` by default, one authoritative implementation, KISS/YAGNI (no
plugin framework, second DAG, or speculative third stack), traits only for
demonstrated interchangeable capabilities, fail-early validation at input
boundaries, and disciplined open/closed (generalize the demonstrated
Rust-plus-OpenToFu boundary, not every internal subsystem).

## 6. Compiler, Clippy, and formatting policy

The root `Cargo.toml` MUST contain this baseline; members MUST inherit it:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"
unexpected_cfgs = "deny"
unfulfilled_lint_expectations = "deny"
missing_docs = "warn"
missing_debug_implementations = "warn"
unreachable_pub = "warn"
rust_2018_idioms = { level = "warn", priority = -1 }

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
too_many_lines = "deny"
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "deny"
unimplemented = "deny"
dbg_macro = "deny"
mem_forget = "deny"
await_holding_lock = "deny"
await_holding_refcell_ref = "deny"
let_underscore_future = "deny"
let_underscore_must_use = "deny"
undocumented_unsafe_blocks = "deny"
allow_attributes_without_reason = "deny"
allow_attributes = "warn"

[workspace.lints.rustdoc]
broken_intra_doc_links = "deny"
private_intra_doc_links = "deny"
```

`clippy.toml` MUST contain:

```toml
too-many-lines-threshold = 80
allow-unwrap-in-tests = false
allow-expect-in-tests = true
allow-panic-in-tests = true
check-incompatible-msrv-in-tests = true
```

Pull-request CI MUST run Clippy once per selected package with
`--package <name> --all-targets --locked -- -D warnings`. It MUST NOT use a
workspace-wide compile as the default. A workspace-wide Clippy run MAY be a
manual diagnostic.
Do not enable all of Clippy's `restriction` or `nursery` groups. Exceptions
MUST use narrow `#[expect(..., reason = "...")]` and be reviewed; broad allows
are policy changes. `forbid(unsafe_code)` applies to all V1 crates. If an FFI
need appears, change the policy deliberately, isolate unsafe code in one
reviewed crate, and keep `forbid(unsafe_code)` in every safe crate.

`rustfmt.toml` MUST contain `edition = "2024"`, `style_edition = "2024"`,
and `newline_style = "Unix"`. Formatting MUST be checked with
`cargo fmt --all -- --check` through Mise. Unstable formatter options are not
part of this baseline. This is cargo-fmt via rustfmt, distinct from `tofu fmt`
in `opentofu-contract.md` §4.5.

Rust idiom: `Result<T, E>` and `?` in the existing `thiserror` style with
contextual path/task/phase information; enums with validated constructors or
`TryFrom` for invariants; immutable borrows and owned values first (`Box`
only for needed indirection, `Arc` only for genuinely shared ownership);
small public APIs with documented contracts; `cfg` and OS/process detail
stays in infrastructure owners.

## 7. Dependencies and supply chain

Detail lives in [rust-dependency-policy.md](rust-dependency-policy.md). A
mature HCL parser MUST be researched and qualified for maintenance, license,
MSRV, behavior, and build cost; do not hand-write a regex parser for HCL.

## 8. Repository policy checks

Velnor's own `.velnor/config.toml` MUST set
`workflow.policy = "velnor-repository-v1"`, which emits a dedicated
`alint` job. The job MUST use the configured Alint action, defaulting to the
full-SHA pin (repository-structure alint, distinct
from workflow-syntax actionlint):

```yaml
- uses: asamarts/alint@d93c0283b19dd78afcd8a4b303f1556a7759ba81 # v0.17.0
```

It runs only the generic rules configured in `.alint.yml`: file/path rules,
required-file rules, and line-count rules. Alint MUST run as its own job even
when no product crate is selected. Dependency policy, Rust formatting/Clippy,
and workflow-security checks remain separate verification jobs. The final
required status depends on the Alint job and every other required job.

Ordinary `consumer-v1` workflows MUST NOT emit the Alint job or require
`.alint.yml`. Alint is enabled only for Velnor's own repository policy in V1.
Policy changes to `.alint.yml`, the workflow, the Alint version, and approved
exceptions require review and negative fixtures where the selected tool
supports them.

## 9. Required verification

The binding task templates, test scope, workflow checks, and risk-triggered
verification requirements are specified in the [Rust verification
contract](rust-verification-contract.md).
