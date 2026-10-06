# Authenticated Cargo inventory and early planning

Status: implementation active. This record describes source inspected on
2026-10-03. Early integration source is written; integrated repository gates
remain open, and hosted performance is unmeasured. Source inspection and
authored regression cases are not passing execution evidence. The separately
committed ZIP dependency has its own recorded locked/deny checks.

## Architectural cause and boundary

Planning previously coupled repository discovery to fresh Cargo metadata, and
base dependency graphs issued another Cargo request. A consumer could therefore
require Rust installation before determining that ordinary source or existing
documentation edits preserve Cargo inventory. Reusing reduced DTOs or trusting
caller JSON would introduce a separate, incomplete planning authority.

The structural change supplies one `InventoryProvider`: `FreshCargo`,
`FreshWithoutCargo`, or a `ValidatedInventory` capability. All feed normal discovery, qualification,
profiles, task derivation, selection, and the existing plan response. Cargo
remains the interpreter of fresh membership and dependencies; this adds no V1
runner or manifest replacement. See [provider](../../crates/velnor-actions-orchestrator/src/inventory_provider.rs),
[inventory](../../crates/velnor-actions-orchestrator/src/inventory.rs),
[discovery](../../crates/velnor-actions-orchestrator/src/discover.rs), and
[early planner](../../crates/velnor-actions-orchestrator/src/analysis_plan.rs).

`plan_early_internal` returns typed `Ready` or `NeedsCargo`. Authentication or
inventory admission misses request fresh Cargo. Malformed requests, checkout,
config, policy, and ordinary planner failures remain failures. `Ready` runs
`plan_prepared`, checks freshness when requested, and retains the ordinary
schema-1 response. Uncovered Plan-owned Rust obligations, including Format,
produce typed `NeedsCargo`. Final publication reauthenticates remote inventory,
recomputes the complete normal response, and compares canonical bytes; it binds
request, matrix, helper digest/version/target, and current inputs. CLI staging
uses exclusive creation and bounded regular-file reads, rejecting symlinks,
malformed, missing, and stale responses. See [CLI boundary](../../crates/velnor-actions-cli/src/dispatch_plan.rs).
The renderer requires Cargo whenever the early output is absent
or differs from literal `false`; see [early step](../../crates/velnor-actions-workflow-renderer/src/early_plan.rs).

Consumer Plan prepares Gh and the generation validators before early planning;
full tools, Rust components, source fetch, and full cache transfers require the
Cargo fallback guard. Planning tools have a separate cache domain. See
[job ordering](../../crates/velnor-actions-orchestrator/src/workflow_jobs.rs) and
[cache phases](../../crates/velnor-actions-workflow-renderer/src/cache_tool_phases.rs).

Fresh native/Tofu admission uses `FreshWithoutCargo` when the registered Rust
detector has no candidate. Provider guards repeat before metadata and workspace
qualification; Rust work appearing after admission returns typed `NeedsCargo`
before any Cargo request. Config identity is rechecked after preparation.
Native-only full scope needs no Rust inventory or base; normal obligations and
baseline coverage remain in the common planner. See
[admission](../../crates/velnor-actions-orchestrator/src/analysis_admission.rs),
[qualification guard](../../crates/velnor-actions-orchestrator/src/inventory_qualify.rs),
and [six native cases](../../crates/velnor-actions-orchestrator/src/analysis_native_tests.rs).

## Inventory schema and input identity

Schema 1 carries candidate-to-complete-workspace records, members, package IDs,
manifests, versions, features, target kind/name/test/doctest/required features,
build-script flags, and local/skipped edges with kind, optionality, and target
filters. Transport records reject unknown fields; required collections have no
transport defaults. Strict JSON rejects duplicate keys. Raw Cargo decoding is
a separate boundary. See [schema](../../crates/velnor-actions-orchestrator/src/analysis_inventory.rs),
[records](../../crates/velnor-actions-rust/src/metadata.rs), and
[edges](../../crates/velnor-actions-rust/src/metadata_edges.rs).

Relocation normalizes checkout paths without discarding retained fields. It
rejects external packages, duplicate candidates, noncanonical relative paths,
invalid modern path IDs, manifest/ID disagreement, and missing edge endpoints.
Rehydration binds the proof to the canonical current checkout;
[path relocation](../../crates/velnor-actions-orchestrator/src/analysis_inventory_paths.rs)
preserves cross-workspace skipped path edges.

Resolution identity binds effective indexed paths, Cargo manifest/lock/toolchain
and config bytes, observed package ancestors, relevant effective child
environment hashes, and actual filesystem target paths and node kinds under
`src`, `examples`, `tests`, `benches`, and `build.rs`. The filesystem walk includes
ignored/excluded targets; source bytes and existing documentation bytes belong
to independent normal planner semantics. Ambient Cargo configuration, unsupported
member patterns, target symlinks, unreadable/non-UTF-8 paths, and bound
limits disqualify reuse. Validation repeats at discovery entry/exit and before
base graph consumption. See [input identity](../../crates/velnor-actions-orchestrator/src/analysis_inventory_inputs.rs).

Membership capture now binds source-qualified Cargo expansion inputs for
`members` and `default-members`: count raw matches before filtering directories,
and use literal fallback only for zero raw matches. Every match binds node kind
and manifest bytes, including hidden/excluded and empty matching directories.
The finite supported subset is literal paths and one star in the final
component; opaque scopes, unsafe paths, symlinks, and non-UTF-8 inputs require
fresh Cargo. It does not independently compute Cargo membership. The blanket
administrative/output filesystem walk is removed. See
[membership capture](../../crates/velnor-actions-orchestrator/src/analysis_inventory_membership.rs)
and [ten authored mutation cases](../../crates/velnor-actions-orchestrator/src/analysis_inventory_membership_tests.rs).

Cargo compatibility is qualified: the publisher observes `cargo --version`
through the selected pinned isolated command and records a release-shaped
version/commit/date, alongside the exact toolchain catalog pin. The consumer
cannot observe local Cargo before installation; compatibility depends on the
authenticated producer, matching helper digest/catalog pin, and unchanged input
identity. Reuse additionally requires qualified source commit prefix `797e8a9bc`
under Rust toolchain `1.98.1`; unknown commits refuse reuse. A Cargo version
string need not equal the rustup toolchain pin. The
[official distribution manifest](https://static.rust-lang.org/dist/channel-rust-1.98.1.toml)
records internal Cargo version `0.99.0 (797e8a9bc 2026-08-05)`.
Rust source commit `48a229ceaefd4985c50990b14116b6d856af0985` identifies full Cargo
commit `797e8a9bca276c1c9f9f738d2a20f484fa4eea9d` through the
[Cargo submodule API](https://api.github.com/repos/rust-lang/rust/contents/src/tools/cargo?ref=48a229ceaefd4985c50990b14116b6d856af0985).
Native `aarch64-apple-darwin` binary observation independently reports Cargo
release `1.98.1`, that full commit, and date `2026-08-05`; the proof captures
binary SHA-256 `6e17e865f3a20dd55a1d212f849f58b77124179f0de7c52973096d84ba34118d`
and installed distribution-manifest SHA-256
`e44f4ea0a633aa3e497e4dd161424419fbd7bdc203606ef840634fdd22dab9bf`.
Evidence file: `/tmp/velnor-rust-final-pin-proof/cargo-version-proof.json`.
This establishes an observed Cargo version rather than inferring Rust's patch
number; Linux runtime identity remains unobserved. See
[publisher](../../crates/velnor-actions-orchestrator/src/analysis_publication.rs).

## Remote authority and lifecycle

Only live authenticated pinned `gh` retrieval constructs production authority.
Lookup reconciles origin/request repository and GitHub default branch, selects
an exact-base completed successful push on the expected workflow and repository,
and verifies the exact current run attempt. Artifact selection requires one
matching unexpired artifact, matching run/head/branch, bounded nonzero size,
and GitHub `sha256:` digest. Downloaded ZIP bytes must match that digest.
Payload identity binds source SHA, workflow SHA, repository, branch, run ID,
attempt, helper digest, and Cargo pin. Live run evidence is read again after
payload verification to reject racing superseding reruns. A separate SHA-256
capability binds every payload byte. Caller/local JSON supplies no authority.
See [authority](../../crates/velnor-actions-orchestrator/src/analysis_authority.rs).
Repository slug comparison uses canonical GitHub casing through the existing
slug validator; workflow paths and branch names retain exact case checks.

ZIP reading stays in memory: exactly one ordinary nonexecutable `analysis.json`,
8 MiB archive maximum, 4 MiB payload maximum, and bounded compression ratio.
No archive entry is extracted. See [archive](../../crates/velnor-actions-orchestrator/src/analysis_archive.rs).

Publication requires fresh complete qualified inventories and protected default
branch push context with head/workflow/origin agreement. Cached inventories
cannot republish. Proof-publication failure returns typed `Unavailable`, allowing
the successful fresh plan to proceed with a warning; cancellation remains a failure. Payload
staging uses an exclusive separate runner directory;
artifact bytes do not enter plan/task evidence. Immutable artifact names bind
base/helper/schema/Cargo pin; upload retention is 90 days. Missing, expired,
stale, incompatible, ambiguous, or unauthenticated evidence falls back to Cargo.
See [capture](../../crates/velnor-actions-orchestrator/src/discover_capture.rs),
[publication](../../crates/velnor-actions-orchestrator/src/analysis_publication.rs), and
[upload](../../crates/velnor-actions-workflow-renderer/src/analysis_publication.rs).

Exact authenticated base SHA plus current validated inputs permits base graph
reuse; graph qualification still runs, cross-workspace edges remain checked, and
owners map to candidate IDs. Other bases use immutable-tree Cargo metadata.
See [base graph](../../crates/velnor-actions-orchestrator/src/select_base_graph.rs).

## Evidence and remaining work

Authored negative cases cover substituted payload bytes, duplicate JSON keys,
missing fields, changed lock/manifests/config/ignored targets/node kinds, workspace
globs, wrong checkout/base, wrong attempt/workflow/repository, ambiguous/expired
artifacts, missing digests, and malformed ZIP names/kinds/modes. Relocation cases
cover URI escaping, complete target/edge fields, cross-workspace edges, and removed
owners. See [inventory cases](../../crates/velnor-actions-orchestrator/src/analysis_inventory_tests.rs),
[authority cases](../../crates/velnor-actions-orchestrator/src/analysis_authority_tests.rs), and
[graph cases](../../crates/velnor-actions-orchestrator/src/select_base_inventory_tests.rs).
An independently specified full normalized JSON fixture now compares exact
canonical payload bytes, beyond producer/consumer roundtrip agreement; see
[canonical fixture](../../crates/velnor-actions-orchestrator/src/analysis_inventory_canonical_tests.rs).

Final independent inventory/schema, authority, base-graph, and case source
reviews reported no known finding after fixes.
Early integration P1 fixes are reflected in source: full response reauthentication,
uncovered Plan Format fallback, minimal planning tools, expensive-step guards,
and guarded staged CLI reads. Shared orchestrator checkpoint reported 539 passing
and 88 failing tests, with a common Rust-preparation/renderer blocker. It does
not prove integrated gate success or establish new membership cases passed.
Input audit fixes now bind hidden membership inputs and actual Cargo command
cwd/environment. Independent membership source review found no known unsafe
finite subset; ten stronger mutation cases are authored and integrated. Early
planning has nineteen authored cases, including six registered native cases;
CLI staging/promotion has seven. Independent source review accepted the
`FreshWithoutCargo` metadata/qualification guards; execution remains pending.
The added native case changes the checkout after the original detector index:
a new Cargo manifest must return `NeedsCargo` with exactly zero Cargo attempts.
Independent fixture review closed both P2 findings and reported no remaining
finding; the canonical and admission cases have not yet executed.
Source modules are ready for the queued new root checkpoint; the previous
539/88 result is not a result for that checkpoint.

Structural membership capture is wired and exact `glob = "=0.3.3"` is present
in workspace dependencies; its separate root commit is pending. Effective
indexed paths and package target paths remain bound, so unrelated indexed-path
additions can still conservatively miss. No filesystem-wide admin/output walk
is required by membership capture. Acceptance still
requires complete negative coverage, repository gates, and hosted cold/warm/
third-run proof with a real source change, exact plan equivalence, correct
obligations, and the eligible no-Cargo path. This document closes none of those
remaining obligations.

C09 report staging source is complete and registered: fixed typed
`stage-reports-v1` runs with `always()` before crate report upload. The validated
plan supplies a closed list of matrix/task/shard/action evidence paths; original
plan authority is excluded. Existing failed/partial report bytes are preserved
exactly and missing reports stay missing. Plan Format stages before upload with
the same covered-task condition on both steps. Independent source review found
no remaining finding. See [staging](../../crates/velnor-actions-orchestrator/src/report_staging.rs),
[crate ordering](../../crates/velnor-actions-orchestrator/src/crate_jobs_render.rs),
and [Format ordering](../../crates/velnor-actions-orchestrator/src/wire_w1.rs).

The shared staged reader opens with `NOFOLLOW | NONBLOCK`, then validates the
open handle as a regular file before bounded reads, preventing FIFO-open hangs.
Eleven focused staging cases and the covered-Format constructor regression are
authored; execution remains pending. See [reader](../../crates/velnor-actions-orchestrator/src/staged_reads.rs),
[staging cases](../../crates/velnor-actions-orchestrator/src/report_staging_tests.rs),
and [constructor case](../../crates/velnor-actions-orchestrator/src/workflow_tools_tests.rs).
Runtime behavior, uploaded-byte reduction, and transfer savings remain unmeasured.

The reviewed archive dependency is `zip = "=8.6.0"`, with
`default-features = false` and only `deflate` selected in [workspace dependencies](../../Cargo.toml).
Its packaged root license is MIT ([primary manifest](https://docs.rs/crate/zip/8.6.0/source/Cargo.toml),
[packaged license](https://docs.rs/crate/zip/8.6.0/source/LICENSE)). That feature
selects the zlib-rs backend; the current lock resolves zlib-rs 0.6.8. Its packaged
root license was inspected as Zlib ([primary manifest](https://docs.rs/crate/zlib-rs/0.6.8/source/Cargo.toml),
[packaged license](https://docs.rs/crate/zlib-rs/0.6.8/source/LICENSE));
[deny policy](../../deny.toml) explicitly allows Zlib. Dependency freshness,
locked resolution and deny checks for this dependency are recorded in commit
`328bc0a9d0996d70aee175ba1a5f09337c1acfa9`. Full integrated repository gates
still require execution.
