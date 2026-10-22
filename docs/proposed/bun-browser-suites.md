# Bun browser suites (hard decision 6 amendment)

Status: proposed. Each part names the change that implements it; until then
generation rejects its keys as unknown. Hard decision:
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
  `not_affected` member's suites are skipped and pass.

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
| `bun_shard_script_name_shadow` | the member has a script named `playwright` |

Each leg writes its blob to
`PLAYWRIGHT_BLOB_OUTPUT_FILE=$RUNNER_TEMP/velnor/pw-blob/leg-<n>.zip`.
Velnor never appends `--reporter`, `--update-snapshots`, `--retries` or
`--fully-parallel`. The internal operation `playwright-blob-verify-v1` reads
the blob's metadata and fails with `playwright_blob_missing`,
`playwright_blob_shard_mismatch:<want>:<got>` or
`playwright_blob_project_mismatch`, so a wrapper that drops arguments is
caught at run time. With `report = "merge"`, a non-gating
`bun-<slug>--<suite>--report` job (`continue-on-error: true`, never in
Required) downloads every leg blob and runs `playwright merge-reports
--reporter=html,json`.

| Artifact | Name | When | Retention |
|---|---|---|---|
| Leg blob | `velnor-pw-blob-<job-id>-leg-<n>` | `!cancelled()` | 1 day |
| Leg failure results | `velnor-pw-results-<job-id>-leg-<n>` (`<member>/test-results`) | `failure()` | `results_retention_days` |
| Merged report | `velnor-pw-report-<job-id>` | report job | `report_retention_days` |

Matrix legs restore the `bun-install` and `playwright-browsers` families
read-only; only leg 1 may save them (`VELNOR_FAMILY_WRITER`). A suite in a
repository whose `.velnor/deps.lock` pins release assets fails generation with
`bun_suite_private_assets_unsupported` until a one-job asset fetch exists, so
no leg ever holds the App secret.

