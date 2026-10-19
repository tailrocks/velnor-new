# Affected selection: pull requests test only what they affect

Hard decision: [4. Pull requests test only what they affect](../hard-decisions.md#4-pull-requests-test-only-what-they-affect).

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
| local pre-push plan | affected | Developer preview of the same selection. |
| `push` | full | The default branch is the backstop and refreshes protected caches. |
| `merge_group` | full | The last gate before the default branch. |
| `workflow_dispatch` (qualification) | full | Qualification measures complete runs. |

`selection_mode` in `crates/velnor-actions-orchestrator/src/select_mode.rs` is
an exhaustive match: a new event must choose a mode there. Full runs record
`full_run_event:<event>:all_changed`.

The generated `pull_request` trigger includes `edited`. Retargeting a pull
request's base branch re-runs the plan, so a narrow green `Required` computed
against the old base never certifies a diff it never saw.

## Change set

The plan compares `base...head` (three-dot, merge-base) with `--no-renames`, so
both sides of a move are listed. The plan checkout is the pull request merge
ref, so the head graph already describes the merged tree. Any of these selects
everything and records a stable `*:all_changed` or `*:selecting_all` warning:

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

A root project tool source (`mise.toml`, `.mise.toml`, `mise.lock`,
`.mise.lock`, `rust-toolchain.toml`, `rust-toolchain`) selects everything
(`project_tool_config_changed:all_changed`): generated CI runs the tool
versions those files declare (hard decision 5). Nested Mise tool files stay
advisory and never select work.

## Ownership

`crates/velnor-actions-orchestrator/src/select_affected.rs` claims each changed
path for every package that can read it. Claims are a union:

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
`.velnor/config.toml`. Unknown package names fail configuration loading.

```toml
[[stacks.rust.inputs]]
globs = ["backend/eth-model/src/main/proto/**", "backend/tailrocks-type/src/main/proto/**"]
packages = ["eth-processor-app"]

[[stacks.rust.inputs]]
globs = ["scripts/lightdash-migration/evidence/**"]
packages = ["lightdash-csv-delivery-app"]
```

Globs support `**`, `*`, `?`, `[...]`, and `{a,b}`. Packages are exact Cargo
package names.

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
event, an unchanged Rust obligation is `not_affected` unless it carries a
fail-safe signal that forces execution:

- `undeclared_reads`: the package has a build script, whose reads are not
  verified yet (see phase 1b);
- `archive_source_unbound`: a sharded Nextest archive whose source identity is
  not content-bound.

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

## Required verdict

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

Jobs the plan does not list (plan, actionlint, named checks, OpenTofu roots)
keep the existing rule: skipped is never success. `fetch-reports-v1` downloads
only the artifacts of matrix entries, which exist only for `run` jobs.

## Phase 1b (deferred, tracked here)

- **In-job read verification.** Each running crate job collects rustc dep-info
  and build-script `rerun-if-changed` paths per member unit and fails
  `Required` with `undeclared_input:<package>:<path>` for any read outside its
  own directory, its path dependencies, and its declared inputs. Until it
  ships, packages with a build script always run (`undeclared_reads`).
- **Runtime-read capture** on test and doctest steps, required before any
  `ignore` list may exist.
- **`ignore` list** for paths asserted irrelevant to every Rust task.
- **Lock-entry diff** instead of running everything on any `Cargo.lock` change.
- **`"../"` literal lint** over member sources as an extra plan-time guard.

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
