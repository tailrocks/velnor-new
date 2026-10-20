# Affected selection: pull requests test only what they affect

Hard decision: [3. Pull requests test only what they affect](../hard-decisions.md#3-pull-requests-test-only-what-they-affect).

A pull request runs only the crate jobs its change can affect. The other crate
jobs are skipped as whole jobs: no runner, checkout, tool setup, cache restore,
or build. `Required` proves every skip against the plan. Pushes to the default
branch, merge groups, and dispatched qualification runs still run every job.

Example (two libraries and eight apps): touching library `util`, used only by
app `a2`, runs `Rust / util` and `Rust / a2`; the other eight crate jobs are
skipped. Touching only app `a1` runs only `Rust / a1`.

## Event policy

| Event | Mode | Why |
|---|---|---|
| `pull_request` (same-repository and fork) | affected | Owner requirement. |
| local pre-push plan | affected | Developer preview over the working tree against `HEAD` only; unpushed commits are not in its change set. |
| `push` | full | The default branch is the backstop and refreshes protected caches. |
| `merge_group` | full | The last gate before the default branch. |
| `workflow_dispatch` (qualification) | full | Qualification measures complete runs. |

`selects_affected_work` in
`crates/velnor-actions-contract/src/workflow/plan_jobs.rs` is the single event
policy, an exhaustive match: a new event must choose a mode there. The planner
(`selection_mode`) uses it, `Plan::validate` rejects any `not_affected` job or
obligation on a full-run event, and `Required` rejects the same against the
merge-time actual event (`not_affected_on_full_event`), so a planner bug cannot
skip work on `push`, `merge_group`, or `workflow_dispatch`. Full runs record
`full_run_event:<event>:all_changed`.

The generated `pull_request` trigger includes `edited`. Retargeting a pull
request's base branch re-runs the plan, so a narrow green `Required` computed
against the old base never certifies a diff it never saw.

## Change set

The plan compares `base...head` (three-dot, merge-base) with `--no-renames`, so
both sides of a move are listed. The plan checkout is the pull request merge
ref, so the head graph already describes the merged tree. That holds only when
the merge commit's first parent is the pull request's `base.sha`: after a
retarget or a base push, `refs/pull/N/merge` can lag, and the graph would
describe a tree the change set never saw. Any of these selects everything and
records a stable `*:all_changed` or `*:selecting_all` warning:

- a merge checkout (`HEAD != head`) whose `HEAD^1` is not `base`
  (`comparison_unavailable:stale_merge_ref:all_changed`);
- a missing or invalid base, a Git failure, non-UTF-8 index names, untracked
  files, or a modified tracked file in the checkout (`dirty_worktree`): the tree
  under test must be the committed head;
- any `Cargo.lock` change (`cargo_lock_changed:all_changed`); a lock-entry diff
  is deferred to phase 1b;
- the root `Cargo.toml` or root `.cargo/config*` (`root_config_changed`);
- `.velnor/**`, `.github/**`, or a path outside the repository;
- a workspace with no complete resolved graph
  (`comparison_unavailable:resolve_graph:<root>:<reason>:all_changed`);
- a tracked symlink owned by a package that escapes the repository, dangles, or
  loops (`symlink_unbounded:<path>:<reason>:all_changed`);
- any changed path no rule below claims (`unclassified_files:all_changed`).

A change to a **declared tool version** selects everything
(`project_tool_config_changed:all_changed`): generated CI runs the versions
the project declares (hard decision 4). Every location Mise reads `[tools]`
from counts (`mise.toml`, `.mise.toml`, `mise.<env>.toml`, `mise.local.toml`,
`.config/mise.toml`, `.config/mise/config.toml`, `.config/mise/conf.d/*.toml`,
`mise/config.toml`, `.mise/config.toml`, `.tool-versions`, `rust-toolchain.toml`,
`rust-toolchain`). The base and head files are parsed with the generator's
reader (`project_tool_sources.rs`) and only the declared catalog versions are
compared, so editing a task, a Node pin, or a comment selects nothing on its
own; such a file is reported like an advisory tool file
(`toolfiles_only:all_unchanged` when nothing else changed). `mise.lock` and
`.mise.lock` always select everything, as does any doubt (a file that does not
parse, or a side git cannot show). Nested Mise tool files stay advisory and
never select work.

## Ownership

`crates/velnor-actions-orchestrator/src/select_affected.rs` claims each changed
path for every package that can read it. Rust claims and the unowned check run
over the full change set. OpenTofu root selection sees only the paths a Tofu
node owns (`select_tofu.rs`), as its own independent claim: a path inside a
crate directory that a Tofu root also owns selects the crate, and a path a Tofu
root owns but no Rust rule claims is unowned for Rust and runs every Rust job
(a repository-root Tofu project therefore never hides Rust changes). Claims are
a union:

1. **Package directory.** The deepest package directory containing the path.
   Packages are workspace members plus every in-repository path package in the
   full resolve, so a vendored or `[patch]`ed crate owns its own directory.
2. **Scoped Cargo tool configs.** `clippy.toml`, `.clippy.toml`,
   `rustfmt.toml`, `.rustfmt.toml`, and `.cargo/config` / `.cargo/config.toml`
   at directory `D` claim every member at or below `D`, because those tools
   search upward from each member.
3. **Workspace roots.** A workspace's root `Cargo.toml` or
   `.config/nextest.toml` claims every member of that workspace.
4. **Declared inputs.** Every `[[stacks.rust.inputs]]` rule whose glob matches
   the path claims its named packages. A file inside package X's directory that
   is declared as an input of package Y selects both X and Y.
5. **Tracked symlinks.** Each tracked symlink (`git ls-files -s`, mode
   `120000`) is resolved component by component with full symlink semantics
   (chained and directory links). The packages that own the link (rules 1-4)
   claim the resolved target and every link hop on the way.

The root package (manifest at the repository root) owns every path by prefix.
It is selected for paths it owns, but it never counts as a claim for the
unowned check: a stray file under a root package still selects everything.

There is no `ignore` list in phase 1. Java, frontend, documentation, and other
non-Rust paths are unowned, so a pull request that changes them runs every
Rust crate job.

### Declaring inputs

A package that reads files outside its own directory declares them in
`.velnor/config.toml`. Unknown package names fail configuration loading. Until
it does, the read scan below forces the package on every pull request.

```toml
[[stacks.rust.inputs]]
globs = ["backend/eth-model/src/main/proto/**", "backend/tailrocks-type/src/main/proto/**"]
packages = ["eth-processor-app"]

[[stacks.rust.inputs]]
globs = ["scripts/lightdash-migration/evidence/**"]
packages = ["lightdash-csv-delivery-app"]
```

Globs support `**`, `*`, `?`, `[...]`, and `{a,b}`. Packages are exact Cargo
package names. `complete = true` on a rule states that the named packages'
rules list all their out-of-directory inputs (see the read scan below).

velnor-new declares its package directories (plus the runner workspace
manifest) with `complete = true` for
`velnor-actions-cli`: its repository-policy
tests read every crate's manifests, sources, and tests in both workspaces (for
example MSRV parity with `crates/velnor-runner/Cargo.toml`). Its other crates
whose tests read the repository through `CARGO_MANIFEST_DIR` stay forced until
their inputs are declared.

### Read scan (fail-safe)

`src/select_reads.rs` and `src/select_reads_scan.rs` scan every tracked `.rs`
file and attribute it to its deepest package. Detected reads:

| Form | Target |
|---|---|
| `include_str!`, `include_bytes!`, `include!` of a literal | resolved from the file's directory |
| `concat!(env!("CARGO_MANIFEST_DIR"), "...")`, anywhere | resolved from the package directory |
| `#[path = "..."]` | resolved from the file's directory; the target is scanned as part of the package |
| `read`, `read_to_string`, `read_dir`, `open`, `metadata`, `symlink_metadata`, `canonicalize`, `copy` with a literal starting `..` | resolved from the package directory (Cargo's test and build-script cwd) |
| `CARGO_MANIFEST_DIR` or `current_dir` plus a literal with a `..` component, or `parent()`/`ancestors()`, in one file | unbounded (`manifest_dir_escape`) |
| an include whose argument is not a literal or manifest-dir `concat!` | unbounded (`include_argument`) |

`include!` and `#[path]` targets are compiled into the package: each tracked
target (any owner, any extension) is scanned in turn, with `CARGO_MANIFEST_DIR`
resolved against the including package, and the anchor and escape signals of
all of a package's files combine. `env!("OUT_DIR")` includes read build output
and are ignored. A resolved read is covered when the package claims the target
(rules 1-5 above), or when no package claims it and nothing claimable lies
below it (a change to an unowned file runs everything already; a directory
such as the repository root reaches every package beneath it). A
read outside the repository can never change with a pull request and is
ignored. An unbounded read is covered only once a `[[stacks.rust.inputs]]`
rule naming the package sets `complete = true`: the owner's statement that the
package's rules, together, list every path it reads outside its directory. A
rule without `complete` adds claims but never silences an unbounded read. Every uncovered package records
`undeclared_reads:<package>:<file>:<reason>` and counts as changed.

Limits, by construction of a static scan: a read through a computed path with
none of these anchors, an environment variable other than `CARGO_MANIFEST_DIR`,
a spawned process (`git rev-parse --show-toplevel`, a script), a non-Rust file,
or a macro that expands to a read is invisible, and a declared input is not
verified. These are the residual false negatives that phase 1b's in-job read
verification closes; `push` and `merge_group` full runs remain the backstop.

## Dependency graph

`qualify_workspaces` (`src/inventory_resolve.rs`) already runs
`cargo metadata --format-version 1 --locked --offline --all-features` for every
lockful workspace after the plan job fetches Cargo sources. Selection parses
that document (`velnor_actions_rust::parse_resolved_graph`) for every resolved
dependency edge of every kind and target, plus every in-repository path
package. Registry crates therefore connect apps to `[patch]`ed and vendored
crates (`app -> lapin -> amq-protocol -> amq-protocol-types`). A qualification
failure fails the plan; an unparsable resolve fails closed to everything.

A lockless workspace has no resolve. If none of its packages declares a
registry or git dependency, its path edges are the complete graph; otherwise it
fails closed.

The affected set is the reverse closure of the claimed packages over the union
of the resolved edges and the base and head path-dependency edges (so an edge
removed by the pull request still selects its old consumer).

## Plan dispositions

Each obligation is `execute`, `reused_from_task_cache` (fails closed in
`Required` until restore proofs exist), or `not_affected`. On an affected-mode
event, an unchanged Rust obligation is `not_affected`. Fail-safe signals are
applied before the reverse closure, so they reach dependents:

- `undeclared_reads` (build script, or an uncovered source read): the package
  joins the claimed set on every pull request, empty change sets included, and
  its dependents run with it;
- `archive_source_unbound`: an unchanged sharded Nextest archive whose source
  identity is not content-bound fails the plan rather than executing alone
  (unreachable today: the closure digest always binds the source).

Each crate job then gets one disposition in `plan.jobs`:

- `not_affected` when every obligation it owns is `not_affected` and it is a
  Rust crate job; OpenTofu root jobs and named check jobs always `run` in phase
  1;
- otherwise `run`, and any `not_affected` obligation it owns executes with
  reason `job_runs`;
- a job another running job needs (a cross-job gate) also runs, with reason
  `needed_by_running_job`, so an implicit `needs` skip never hides it.

`Plan::validate` enforces the pairing: a `not_affected` job owns only
`not_affected` obligations and no matrix entry; a `run` job owns at least one
matrix entry; every obligation names a planned job.

## Generated workflow

- The plan job exports `not_affected_jobs` (`,rust-a,rust-b,`, sorted, or empty)
  and `plan_attempt` (`github.run_attempt` of the attempt that ran the plan).
- Each Rust crate job carries
  `if: "!contains(needs.plan.outputs.not_affected_jobs, ',<job-id>,')"`. The
  list is negative: an empty or missing output runs everything, and the
  implicit `success()` skips every crate job when the plan fails.
- Every job downstream of the plan binds run-scoped identities (plan artifact,
  run directory, crate artifact names, report run keys) to
  `VELNOR_RUN_KEY_ATTEMPT`, set from `needs.plan.outputs.plan_attempt`. A "re-run
  failed jobs" attempt therefore finds the plan its first attempt produced, and
  crate report uploads use `overwrite: true` so a re-run replaces the failed
  attempt's report.

## Container image jobs

`image-<id>-<platform>` build jobs ([image publication
contract](../proposed/image-publication-contract.md)) own no obligations, so
they are planned from what an image build reads instead of from Rust
ownership. `plan.jobs` lists each image job with the image's disposition, the
same `not_affected_jobs` output gates the job (`needs: [plan]`, the shared
`!contains(...)` condition) and `Required` folds it with the table below. An
image job owns no obligation or matrix entry (`image-` prefix); `Plan::validate`
rejects one that does.

An image on an affected-mode event (`pull_request`, fork, local) is selected
when any of these holds, and otherwise `not_affected`:

- a changed path is one of its Dockerfiles, a `.dockerignore` (in the context
  root or beside the Dockerfile), or a path a context `COPY`/`ADD` source
  names (the static Dockerfile parse; a glob claims the directory before its
  first wildcard, `.` claims the whole context);
- its Dockerfile contains any `--mount` (it can read any context path), which
  claims the whole context;
- for `prebuilt.kind = "cargo_bin"`, its package is in the affected package
  set, so a changed dependency, declared input or package file selects the
  image exactly as it selects the crate job; an undiscovered package selects;
- the classification broadened for a global-config path (`.velnor/**`,
  `.github/**`, an outside-project path) or the comparison is unknown (missing
  or stale base, git failure, dirty worktree, declared tool version change).

Rust-only broadening (an unowned file, `Cargo.lock`, root `Cargo.toml`, a
tool-config file) does not select a plain image: an image's inputs are fully
described by its own claims, so editing documentation never rebuilds it. It
does select a `cargo_bin` image, because the broadening marks every package
affected and the prebuilt compile is Rust work. An unreadable or
unanalyzable Dockerfile selects the image. Pushes, merge groups and
qualification runs never skip an image (`not_affected_on_full_event`).

## Required verdict

The protected check `Required` is the `required-gate` job. It needs only the
verdict job `required` (`Required verdict`), runs under `always()`, and its one
step fails unless `needs.required.result == 'success'`. The verdict keeps
`if: ${{ !cancelled() }}` so a cancelled run never spends minutes judging
reports while holding the concurrency group (the hang #187 fixed). Without the
gate, a cancelled run left the verdict skipped, and GitHub reports a skipped
required check as successful: a PR whose only run was cancelled (by hand, or
superseded) satisfied protection. The gate fails within one runner start
instead; with `cancel-in-progress`, a superseding run in the same concurrency
group starts only after that and reports its own gate later.

`merge-v1` compares each crate job's plan disposition with its observed result
(`needs.<job>.result`):

| Plan | Observed job | Verdict |
|---|---|---|
| `run` | success + valid reports for every obligation | pass |
| `run` | skipped | fail (`planned_job_skipped`) |
| `run` | failure / cancelled / missing | fail / cancelled |
| `not_affected` | skipped | pass; no report expected |
| `not_affected` | success | fail (`gate_mismatch`): the generated gate disagreed with the plan |
| `not_affected` | failure / cancelled | fail / cancelled |
| plan missing, invalid, or failed | any | fail (`planning_failed`) |
| any `not_affected` work on a full-run event | any | fail (`not_affected_on_full_event`) |

Image build jobs use the same table. Jobs the plan does not list (plan,
actionlint, named checks, OpenTofu roots) keep the existing rule: skipped is
never success. `fetch-reports-v1` downloads
only the artifacts of matrix entries, which exist only for `run` jobs.

## Phase 1b (deferred, tracked here)

- **In-job read verification.** Each running crate job collects rustc dep-info
  and build-script `rerun-if-changed` paths per member unit and fails
  `Required` with `undeclared_input:<package>:<path>` for any read outside its
  own directory, its path dependencies, and its declared inputs. Until it
  ships, packages with a build script or an uncovered scanned read always run
  (`undeclared_reads`), and the scan's limits above are residual risk.
- **Runtime-read capture** on test and doctest steps, required before any
  `ignore` list may exist.
- **`ignore` list** for paths asserted irrelevant to every Rust task.
- **Lock-entry diff** instead of running everything on any `Cargo.lock` change.

## Tests

- `crates/velnor-actions-orchestrator/src/select_matrix_tests.rs`: a real Cargo
  workspace (two libraries, eight apps, a vendored crate behind a registry crate
  served from a directory source) with real Git commits: library used by one
  app selects exactly two packages; one app selects one; lockfile change,
  unowned file, and root Cargo config select everything; declared proto input;
  cross-crate declared input selects owner and reader; vendored patch crate
  selects its registry dependents; symlinked fixture; scoped `clippy.toml`.
- `crates/velnor-actions-orchestrator/src/select_affected_tests.rs` and
  `src/select_symlinks.rs`: ownership rules, chained/escaping/dangling links.
- `crates/velnor-actions-orchestrator/tests/impl_select_affected_jobs.rs`:
  end-to-end `plan-v1` crate-job dispositions for the owner's examples, push
  full runs, and the job `if:` gate.
- `crates/velnor-actions-orchestrator/src/plan_dispositions_tests.rs`: whole-job
  skipping, mixed jobs, transitive `needed_by_running_job` promotion, and
  non-crate jobs that always run.
- `crates/velnor-actions-orchestrator/src/required_dispositions_tests.rs`: every
  row of the Required verdict table at the fold level;
  `tests/impl_merge_plan.rs` (`empty_diff_skips_every_crate_job`,
  `planned_run_job_skipped_fails_required`) proves the same rows through
  `merge-v1`.
- `crates/velnor-actions-orchestrator/tests/impl_select_fail_safe.rs` and
  `tests/impl_select_read_scan.rs`: the
  false negatives of review 2 as plan-level regressions. A repository-root
  Tofu project neither hides a library change nor an unowned file; a Tofu
  root inside a crate still selects the crate; an undeclared `include_str!`
  and a manifest-dir fixture read into a sibling run the reader; a declared
  include narrows again; an unbounded manifest-dir read runs until inputs are
  declared (`complete = true`); a read of an unowned path never forces, but a
  read of a directory above packages does; anchor and escape split across
  files, included support files and fragments, and another package's
  declared module all count for the includer; a build-script library's
  dependents run, on an empty change set too; `merge_group` and
  `workflow_dispatch` run everything; fork pull requests select affected
  work; a dirty worktree and a stale merge ref run everything.
- `crates/velnor-actions-orchestrator/src/select_reads_scan_tests.rs` and
  `src/select_reads_lex.rs`: every scanned read form, comments, raw strings,
  and literals that are not reads.
- `crates/velnor-actions-orchestrator/tests/impl_merge_full_event.rs`:
  `Plan::validate` and `merge-v1` reject `not_affected` work on full-run
  events.
- `crates/velnor-actions-workflow-renderer/src/required_gate_tests.rs`: the
  `Required` gate always runs, fails unless the verdict succeeded, and cannot
  be altered or duplicated.
- `crates/velnor-actions-cli/tests/impl_cli_rerun_attempt.rs`: an attempt-2
  `write-request-v1` plus `merge-v1` with `VELNOR_RUN_KEY_ATTEMPT=1` passes the
  attempt-1 plan's skipped jobs; without the binding it fails closed.
