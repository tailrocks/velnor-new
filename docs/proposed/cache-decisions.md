# Cache redesign decisions

Status: adopted by the human owner for the CI cache redesign (seven decisions,
recorded 2026-10-20; decisions 8 and 9 adopted per the split brief, owner may revisit). Each decision below states what main implements now and
what the follow-up PRs deliver. This page is the decision record that
[`cache-contract.md`](cache-contract.md) section 2 and
[`hard-decisions.md`](../hard-decisions.md) decision 1 cite. A change to any
entry needs the owner's explicit approval.

Budget rule (applies to every entry): the repository cache quota is 10 GB.
Hot families plus the allowance for superseded entries stay at or below 85 %
of the configured quota, sizes come from
[`cache-measurements.md`](../implemented/cache-measurements.md), and a save that
would exceed its family ceiling is refused and reported, never attempted.
Successful and failed jobs are both covered: see decision 1 and the cases table.

## Decisions

| # | Decision | State on main | Remaining |
|---|---|---|---|
| 1 | **Save on build success even when tests fail.** A family saves when its producer step succeeded on a trusted run, regardless of test, lint or doc verdicts. It never saves on cancellation or a failed producer. | **Implemented** for every family. Sources (Plan job) and tools save on producer success; the compiled `mbx-bundle` family saves through `Record MBX producer success` (a marker written right after the compile step) and an `always() && !cancelled()` export and save that run only on trusted pushes, so a failing test, lint or doc step no longer starves the cache. Suite jobs that compile and test in one step mark after that step. | The dependency-seed job (2) becomes the sole bundle writer. |
| 2 | **Dependency-seed family (D).** One Velnor-owned family archives the compiled dependency closure of the three fixed lane trees, minus `incremental/`, `examples/`, `tmp/`, `doc/` and every Cargo-reported executable. Written only by a non-crate seed job on a trusted main push; crate jobs restore read-only. | Not implemented. | Seed job, family spec, key schema, size ceiling. |
| 3 | **Opaque `mbx cache export` bundles.** Velnor may archive the output of `mbx cache export --format directory`. It treats the bundle as opaque bytes; MBX owns the format and `mbx cache import` restores it. Velnor never parses or composes MBX objects. | **Implemented.** The `mbx-bundle` family ([cache families](cache-families.md)): key, restore, import, export and save steps in crate and suite jobs; the native action is install-only and `github-cache-mode: target` is removed with no compatibility path. The Plan job restores nothing (it installs MBX only). | None. |
| 4 | **Per-family pruning of superseded entries.** Deleting older exact keys of the same family on a trusted default-branch run, with `actions: write` scoped to that job, is not "global pruning". Other families and other trust scopes are never touched. | Not implemented; LRU eviction only. | Prune step and permission, keep newest K per family. |
| 5 | **PR-scoped sources writes for same-repo PRs, with cleanup on close.** A same-repo PR writes a `pr` trust sources entry visible only to that PR; the default branch never restores `pr` entries; forks and merge groups never write. A `pull_request: closed` handler deletes the PR's refs entries. | **Implemented.** Writes as before. Under `pull_request_cache_policy = "same-repository-scoped"` the generator emits `.github/workflows/cache-cleanup.yml` (like `dco.yml`): on `pull_request: closed` for a same-repository pull request one job with `actions: write` lists exactly `refs/pull/<n>/merge` and deletes each entry. It checks out no code and takes the number through `env`; a failed delete is a `cache_unavailable` warning. A non-hosted runner label falls back to `ubuntu-latest` because the job needs the GitHub CLI. | None. |
| 6 | **Drop ImageVersion from keys.** Runner image version is not part of any cache identity. `ImageOS`, runner OS and architecture, and the toolchain digest remain. | **Implemented** (platform identity, qualification runtime platform, tools runtime identity `velnor-tools-runtime-v3`). | None. |
| 7 | **Reword hard decision 1.** Lanes live in the dependency-seed family and the MBX bundle, not in a target-mode payload. The seed job is not a crate job: it runs no checks, is excluded from Required, and the one-job-per-crate rule governs crate jobs only. | Wording updated with this record. | None. |
| 8 | **Bun families.** `bun-install` (Bun package cache, keyed by `bun.lock`) and `playwright-browsers` (hash-pinned browser install, `trusted` writers only) join the typed families, each with a size ceiling and retained-entry count under the budget rule. `node_modules` is never cached. | Specs, ceilings, pins, the budget test and the fail-closed cache-doctor arms are implemented ([cache families](cache-families.md)). | Plan-job keys, restore and save steps in Bun jobs, per-family pruning. |
| 9 | **dbt and Ansible families.** `dbt-packages` (per project, keyed by `package-lock.yml`), `python-venv` (uv wheel cache, keyed by the hash-pinned requirements file) and `ansible-collections` (verified tarballs, keyed by the declared set) join the typed families with ceilings under the budget rule; `trusted` writers only; every restored byte is re-verified against a committed hash or lock. | Specs, ceilings, pins, the budget test and the fail-closed cache-doctor arms are implemented ([cache families](cache-families.md)). | Plan-job keys, restore and save steps in the dbt and Ansible jobs, per-family pruning. |

## Cases the design covers

| Case | Behavior |
|---|---|
| Main green | Seed job saves D on dependency change; sources saved by Plan. |
| Main red because tests fail | Producer steps (fetch, dependency compile) succeeded, so sources and D still save. A red main no longer starves the cache. |
| Dependency compile fails | No D save (producer failed); the previous entry stays valid. |
| Cancelled run | Nothing saves. |
| Same-repo PR | Restores trusted entries; writes only a PR-scoped sources entry; cleaned up on close. |
| Fork PR, merge group | Read-only. |
| Over ceiling | Save refused and reported as `over_ceiling`; older entries untouched. |

## Source notes

Design notes and measurements: the cache research notes in the internal
scratchpad (designs A, B, C and review rounds 1 and 2). The adopted shape is
review 2's hybrid H with the owner's decisions above.
