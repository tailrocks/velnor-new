# Bun browser suites (hard decision 6 amendment)

Status: implemented: suites, their jobs, timeouts and Required membership,
the Playwright leg matrix, leg blob verification and artifacts, the merged
report job. Proposed: the single cache writer for legs and the one-job
release-asset fetch for suites (until then suites refuse pinned release
assets). Hard decision:
[decision 6](../hard-decisions.md#6-one-job-per-bun-workspace-member-with-parallel-script-steps).
Base contract: [Bun contract](bun-contract.md).

## Decision record

Hard decision 6 is amended (coordinator decision, 2026-10-22, under the owner's
autonomy rule "never ask; decide and continue", recorded in the Velnor sharding
design's "Coordinator decision"). Each Bun member keeps exactly one member job.
Optional, opt-in Bun suites may run as separate jobs beside it, the same way
Rust suites and the dependency-seed job sit beside crate jobs. Repositories that
declare no suite keep byte-identical output.

Reasons: the change is additive, its default is unchanged, and it is required by
the owner's instruction that browser suites run in CI (ChainArgos platform: a
30-minute member job ran `ci`, vitest and 8 Playwright projects on one runner;
documentation-portal: about 230 runner-minutes of Playwright). The owner may
revisit it. Unsolicited splits stay forbidden: decision 6 lists any split of a
member's scripts other than through declared `suites` as must-not-reintroduce.

Owner choice made with it: a member job whose scripts the suites claim
entirely is omitted; `bun_member_no_scripts` still fails a member with neither
scripts nor suites.

## Suites


A member keeps exactly one member job. A *suite* is a named group of one or
more of the member's scripts that runs in its own job beside the member job,
the way Rust suites and the dependency-seed job sit beside crate jobs.

```toml
[stacks.bun.members."@chainargos/platform"]
scripts = ["ci", "test", "test:browser"]
timeout_minutes = 20                         # member job only

[stacks.bun.members."@chainargos/platform".suites.unit]
scripts = ["test"]
timeout_minutes = 25

[stacks.bun.members."@chainargos/platform".suites.browser]
scripts = ["test:browser"]                   # exactly one script with [playwright]
browsers = ["chromium", "webkit"]
timeout_minutes = 25                         # per leg when sharded
# required = true                            # false: runs, but outside Required

[stacks.bun.members."@chainargos/platform".suites.browser.playwright]
projects = ["chromium-desktop", "webkit-desktop"]   # optional, sorted, unique
shards = 2                                   # 1..=64, per project when projects is set
# max_parallel = 4                           # 1..=legs
# fail_on_flaky = false                      # true appends --fail-on-flaky-tests
# report = "merge"                           # "none" (default) or "merge"
# report_retention_days = 14                 # 1..=90
# results_retention_days = 7                 # 1..=90
```

- Job id `bun-<member-slug>--<suite-id>`, display name
  `Bun / <name> / <suite-id>`. The id cannot collide with a member job,
  because `slug()` never emits `--`.
- A script a suite claims leaves the member job. When suites claim every
  script, the member job is omitted; `bun_member_no_scripts` still fails a
  member with neither scripts nor suites.
- A suite job has the member job's prefix steps, permissions and validator
  invariants. It installs browsers only when the suite declares them.
- Timeouts resolve most specific first: `suite.timeout_minutes`, then
  `member.timeout_minutes`, then `stacks.bun.timeout_minutes`, then
  `workflow.crate_timeout_minutes`. The suite value bounds each leg and the
  report job; the member value bounds the member job only.
- Every gating suite (`required = true`, the default) joins Required. A
  matrix job reports one aggregate result, so one job id covers all its legs.
  The plan gives every suite job its member's disposition, so a
  `not_affected` member's suites are skipped and pass; a member job the
  suites made empty has no disposition and no job.
- `required = false` runs the job outside Required's `needs`; it is still
  reconstructed and checked like every other generated job.
- A suite in a repository whose `.velnor/deps.lock` pins release assets fails
  with `bun_suite_private_assets_unsupported:<member>` until the one-job asset
  fetch exists, so the App secret never reaches a suite job.

Validation codes (`stacks.bun.members.suites`): `bad_suite_id:<id>`
(`[a-z][a-z0-9-]{0,31}`, no trailing `-`), `reserved_suite_id:<id>`
(`report`, `assets`), `empty_suite_scripts:<id>`,
`suite_script_claimed_twice:<script>`, `playwright_suite_one_script:<id>`,
`playwright_suite_without_browsers:<id>`, `bad_shards:<n>:expected_1..=64`,
`too_many_legs:<n>:expected_..=64`, `bad_project_name:<p>`,
`projects_must_be_sorted_and_unique`, `bad_max_parallel`,
`report_requires_shards_or_projects:<id>`, `bad_retention_days:<n>`.
Generation adds `bun_script_missing` for a suite script the member lacks.

## Playwright sharding

A suite with a `[playwright]` table runs as a static
`strategy.matrix.include` of legs, sorted by `(project, shard)`, with
`fail-fast: false`. Each leg runs the suite's one script as
`bun run --cwd=<member> <script> "--project=$VELNOR_PW_PROJECT"
"--shard=$VELNOR_PW_SHARD"`; the values arrive through step `env` from
`matrix.*`, never through the `run` text, and a flag is omitted when it does
not apply. Bun appends trailing arguments to the script command, so the
script must be one Playwright command. Generation reads the `package.json`
text and fails with:

| Code | Rule |
|---|---|
| `bun_shard_script_compound:<script>` | `&&`, `\|\|`, `;`, `\|`, `&`, `$(`, a backtick, `>` or a newline |
| `bun_shard_script_not_playwright:<script>` | no `playwright test` token pair |
| `bun_shard_script_sets_shard:<script>` | `--shard` or `--project` in the text |
| `bun_shard_script_updates_snapshots:<script>` | `--update-snapshots` or `-u` |
| `bun_shard_script_name_shadow:<member>` | the member has a script named `playwright` |

The leg list is the canonical product of `projects` (one unnamed group when
empty) and `1..=shards`, numbered from 1; the renderer re-derives it from the
marker it scrubs and refuses any other list (`bun_matrix_*`). Each leg is one
job of the matrix, so `timeout_minutes` bounds every leg. GitHub reports the
aggregate result under the one suite job id, which is what Required needs.
The tools cache never elects a matrix job as its writer.

```yaml
    strategy:
      fail-fast: false
      max-parallel: 3              # only when configured
      matrix:
        include:
          - leg: 1
            project: chromium-desktop
            shard: 1
            total: 2
```

### Blobs, artifacts and the merged report

Each leg writes its blob to
`PLAYWRIGHT_BLOB_OUTPUT_FILE=$RUNNER_TEMP/velnor/pw-blob/leg-<n>.zip`.
Velnor never appends `--reporter` (it would replace the repository's
reporters), `--update-snapshots`, `--retries` or `--fully-parallel`. After the
script, `Verify Playwright blob` (`!cancelled()`) reads the zip's
`report.jsonl` metadata (Playwright 1.64.1 `reporters/blob.ts`: first line
`onBlobReportMetadata` with `version: 2` and `shard: {current, total}`, absent
when unsharded; one `onProject` event per project that ran) and fails with:

| Code | Meaning |
|---|---|
| `playwright_blob_missing` | no blob: the consumer config lacks the blob reporter under CI |
| `playwright_blob_version:<v>` | an unknown blob schema (fail closed) |
| `playwright_blob_shard_mismatch:<want>:<got>` | the shard never reached Playwright (compound script, or a wrapper that drops argv) |
| `playwright_blob_project_mismatch:<want>:<got>` | a foreign project ran, or no project event appeared in an unsharded leg |

*(refinement)* The design names an internal operation
(`playwright-blob-verify-v1`) over the hardened archive reader. The check is
instead a fixed program the pinned Bun runs over `unzip -p <blob>
report.jsonl` (filtered to the metadata and project events): it extracts
nothing, holds no credential, and needs no new step role while hard decision
10's lint lane rewrites the step-role table. The blob comes from the same job
and trust scope. Adopted per the coordinator decision; owner may revisit.

| Artifact | Name | When | Retention | Missing files |
|---|---|---|---|---|
| Leg blob | `velnor-pw-blob-<job-id>-leg-<n>` | `!cancelled()` | 1 day | `error` |
| Leg failure results | `velnor-pw-results-<job-id>-leg-<n>` (`<member>/test-results`) | `failure()` | `results_retention_days` (7) | `warn` |
| Merged report | `velnor-pw-report-<job-id>` (`html/`, `report.json`) | report job | `report_retention_days` (14) | `error` |

Every upload sets `overwrite: true`, so "Re-run failed jobs" replaces the
earlier attempt's artifact of a re-run leg. The Bun job validator admits only
these pinned upload and download actions (a `velnor-pw-` name or pattern, no
role), and the job reconstruction compares every input.

With `report = "merge"` (only with two or more legs), the job
`bun-<slug>--<suite>--report` (`Bun / <name> / <suite> / report`) needs the
plan and the suite job, runs `if: !cancelled() && needs.<suite>.result !=
'skipped'` with `continue-on-error: true`, installs the workspace (no
browsers), downloads every leg blob (`merge-multiple`) and runs
`bun run --cwd=<member> playwright merge-reports --reporter=html,json` with
`PLAYWRIGHT_HTML_OUTPUT_DIR` and `PLAYWRIGHT_JSON_OUTPUT_FILE` under
`$RUNNER_TEMP/velnor/pw-report`. It is never in Required: the legs' own
conclusions are the verdict, and a broken merge must neither mask nor fake it.

## Consumer obligations

- The Playwright config adds the blob reporter under CI, for example
  `reporter: process.env.CI ? [['dot'], ['blob']] : 'html'`.
- `fullyParallel: true` is recommended: it shards tests, not files, so legs
  balance.
- Keep the default `outputDir` (`test-results`), which the failure upload reads.
- The sharded script is one command (`playwright test ...`, `bunx playwright
  test`, or a wrapper that forwards its arguments); no `--shard`, `--project`
  or `--update-snapshots` in it. CI never writes goldens.
- Declare `projects` when any project sets `default: false` (Playwright 1.64):
  shards over "all projects" silently skip such projects.
- Size `shards` as `ceil(serial test minutes / target leg minutes)` with the
  leg target at most `timeout_minutes` minus 2-4 minutes of setup.

Matrix legs restore the `bun-install` and `playwright-browsers` families
read-only; only leg 1 may save them (`VELNOR_FAMILY_WRITER`, proposed). No leg
ever holds the App secret (see the private-asset rule above).

Playwright blob metadata does not record `--project`. For a non-empty
project-selected leg, Velnor requires an `onProject` event for the requested
project, follows its dependency and teardown names, and rejects emitted
projects outside that closure. Referenced projects can be absent from the
events with `--no-deps` or when Playwright removes empty suites. An empty shard
therefore proves its exact shard metadata only; it cannot prove which project
selector was passed when there are no project events. An empty unsharded blob
fails verification because it provides no project evidence.

## Verification

- Contract: suite tables, every suite code, timeout bounds and defaults
  omitted (`config/bun_tests.rs`).
- Adapter: claimed scripts, member-job omission and suite job ids
  (`impl_bun_workspace.rs`).
- Renderer: suite job shape, permissions, gate and the shared member
  invariants (`impl_renderer_bun.rs`).
- Orchestrator: claimed scripts, member-job omission, non-gating suites, the
  timeout chain, configuration and private-asset refusals, determinism
  (`impl_bun_suites.rs`), suite dispositions (`impl_bun_select.rs`), the
  suite-only browser install (`impl_bun_playwright.rs`) and the Required folds
  of a suite job id (`required_dispositions_tests.rs`).
- Playwright matrix: leg grammar and bounds (`config/bun_tests.rs`), script
  admissibility (`impl_bun_playwright_script.rs`), leg derivation, marker
  parsing and `strategy` emission (`matrix_bun_tests.rs`), generated legs,
  flags only where they apply, env-only matrix values and refusals
  (`impl_bun_playwright.rs`).
- Blobs and report: leg verify, project dependency/teardown closure, empty-shard
  evidence, blob and results uploads, the report job, its gate and the shadow
  refusal (`impl_bun_report.rs`); report and retention bounds
  (`config/bun_tests.rs`); the artifact-action allowance and the report policy
  (`impl_renderer_bun.rs`).
- Goldens: `fixtures/bun-workspace` (no suite) stays byte-identical;
  `fixtures/bun-workspace-suites` is the opt-in golden (`impl_bun_golden.rs`).
