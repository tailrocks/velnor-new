# Evidence pack: Velnor runner research (2026-11)

Facts only, no recommendations. Measured 2026-11-18 (UTC) by Claude Sonnet 5.5 subagents (effort medium) and assembled by the Claude Opus 5.5 manager. Each section names its sources: `gh api` endpoints and run ids, read-only `kubectl` commands, `path:line` in the repositories, and commands run on the manager's machine (macOS arm64). Raw data (JSON/TSV, job logs, build and test logs) stayed in the scratch directory `~/.cache/ci-ambition/evidence/` and is not committed.

Repositories read: tailrocks/velnor-new main @ 95de9b704b; tailrocks/velnor-legacy @ 1b9fc038 (archived); actions/runner @ 7afce9edc8 (v2.338.0); actions/actions-runner-controller @ 89f34c4f78; actions/scaleset v0.5.0 @ af394d86.

## Headline facts (each is detailed with its source below)

- **Where the queue time is (11-18).** PR wall time on 11-18 had a median of 74.7 min. Of a 71.8 min median PR run, a median 57 min (79%) was queue wait on three gate jobs that run on GitHub-hosted runners: Plan, Required verdict and Required. On 11-16 and 11-17 the same gate queue was 12.6 and 5.8 min. Over the 14-day window the PR median was 17.5 min and the p90 60 min; in the scale-set era (from 11-15) the median was 26.7 min and the p90 92.3. (A)
- **Hosted vs. cluster queue.** Jobs on the cluster scale set queue a median 0.2 min (p90 0.3). Jobs on hosted runners queue a median 4.1 min (p90 23.6, max 49.2). At 2026-11-18T16:54Z, 86 jobs were queued: all 86 on hosted `ubuntu-26.04`, 0 on scale-set labels, and 25 of the 86 not from CI. (A)
- **Jobs and runner-minutes.** Each run has 39 jobs. Runner-minutes per run (median) are 52.9 for a PR run, 64.6 for merge_group and 62.0 for push. (A)
- **Biggest jobs.** Check / maintenance-helpers takes 8.4–8.9 runner-min, with no cache steps and 104 crates compiled. runner-host plus its local-mode variant take 11.7 runner-min. velnor-actions-cli takes about 6.6. (A)
- **Cache behaviour.**
  - The Cargo sources cache was not found in 48 of 48 Rust job logs.
  - No Rust toolchain is restored, in 58 of 58 logs.
  - `MBX_REMOTE_URL` is empty in 68 of 68 logs.
  - The mbx local object cache hits 40–74% of lookups per pass.
  - The state-snapshot restore step was skipped in 3636 of 3636 jobs.
  - State seals succeeded only for generator (6 of 18 runs) and ci-helper (4), never for root, local or runner. (A, C)
- **Duplicate verification.** Each merge runs the same 39-job set three times, in the PR run, the merge_group run and the push-to-main run. 120 of 159 successful merge_group runs have a push run on the identical SHA. (A)
- **Cluster.** There are 5 workers with 96 threads and about 125 GiB each.
  - Three ARC scale sets share the nodes: tailrocks (max 60 runners), ChainArgos (60) and jackin (40).
  - Each runner pod requests 2 CPU and 4Gi with a 32Gi memory limit, runs a privileged dind sidecar, and uses only emptyDir volumes. No state survives between jobs.
  - Worker CPU was 92–95% in samples on 2026-11-18. The owner's earlier note recorded 18–29%.
  - The kubelet has no reservations, and the CPU manager is `none`. (B)
- **Workflows.** The engine (`velnor-actions` 0.1.80) generates them; the new `velnor-gen` and `velnor-ci` pair is not wired in yet. PRs skip unaffected jobs with a glob map; merge_group and push run everything. nextest sharding is unused. There is no blaze, no sccache and no linker configuration. The toolchain is pinned to Rust 1.99.1. (C)
- **velnor-new runner.** It is a separate 8-crate workspace of about 185k lines that runs the OFFICIAL runner in Docker. It builds, and 430 of 430 tests pass on macOS. Its V2-native stage is documented as superseded and not implemented. (D)
- **velnor-legacy.** It is about 804k lines. It has a native protocol lane (broker, run service and Results), an expression engine transcribed from actions/runner, and 26 native or allowlisted action adapters, with no generic JS runtime. It builds; nextest passed 8,172 of 8,222 tests, with the 50 failures caused by the macOS environment. It is archived as "reference only"; no recorded reason beyond the successor repository. No native-runner performance numbers are recorded. (E)
- **Protocol.** The scale-set API is: sessions, then long-poll messages, then `acquirejobs`, then `generatejitconfig`. The broker message is only a reference, and the job is fetched by run-service `acquirejob`. The lock is renewed every 60 s. Logs and timeline go through the Results twirp service. The runner contains no cache or artifact client. In runner v2.338.0 all JS actions are forced to node24. GitHub stops assigning jobs to runners more than 63 days out of date. (F)


## A. CI runs: timings, queues, caches, duplicate work

_Source file: ci-timings.md_

Repo tailrocks/velnor-new, workflow `CI` (`.github/workflows/ci.yml`, id 371301839) unless stated. Data pulled with `gh api` at 2026-11-18T16:54Z; window = runs created 2026-11-04..2026-11-18 (15 UTC days). All times in minutes unless "s". "med/p90" = median / 90th percentile (linear interpolation). Raw data, scripts and logs: `~/.cache/ci-ambition/evidence/ci-raw/`.

Two regimes appear in the data and are split everywhere below:
- **hosted era** (runs created 11-04..11-14): every job on GitHub-hosted `ubuntu-26.04`.
- **scaleset era** (runs created 11-15..11-18): the 31 Rust/Check jobs run on self-hosted "Velnor Scale Set" (runner_group `velnor-ci`, labels `velnor,ubuntu-26.04-scale-set`); 8 jobs (Plan, Ledger CLI, Cargo Deny, Jactionlint, Cargo Machete, Alint, Required verdict, Required) stay on GitHub-hosted `ubuntu-26.04` (group "GitHub Actions"). The scale-set era is further split by day: wall times jump on 11-18 (T1, T5, T8).

### T1. Run-level POPULATION (runs list only; 3009 CI runs created 2026-11-04..11-18; no jobs data)
| event | success | failure | cancelled | in progress | total |
|---|---|---|---|---|---|
| pull_request | 963 | 419 | 1156 | 20 | 2558 |
| merge_group | 159 | 31 | 0 | 3 | 193 |
| push | 218 | 13 | 26 | 0 | 257 |
| workflow_dispatch | 0 | 1 | 0 | 0 | 1 |

Wall = `run_started_at` -> `updated_at`, minutes, success+failure runs:
| event | n | med | p90 | max |
|---|---|---|---|---|
| pull_request | 1382 | 17.5 | 60.0 | 297.7 |
| merge_group | 190 | 22.8 | 87.3 | 113.7 |
| push | 231 | 16.2 | 39.7 | 332.4 |

By created day, success runs only (wall median min; n):
| day | PR | merge_group | push |
|---|---|---|---|
| 2026-11-04 | 7.3 (n=58) | - | 7.4 (n=24) |
| 2026-11-05 | 7.8 (n=50) | - | 6.8 (n=14) |
| 2026-11-06 | 13.7 (n=35) | - | 14.0 (n=11) |
| 2026-11-07 | 14.2 (n=34) | - | 13.9 (n=11) |
| 2026-11-08 | 15.0 (n=22) | - | 11.6 (n=3) |
| 2026-11-09 | 17.5 (n=8) | - | 21.0 (n=3) |
| 2026-11-10 | 20.1 (n=2) | - | 16.2 (n=1) |
| 2026-11-12 | 20.1 (n=28) | - | 21.4 (n=10) |
| 2026-11-13 | 21.1 (n=90) | - | 21.8 (n=21) |
| 2026-11-14 | 22.2 (n=80) | - | 22.5 (n=25) |
| 2026-11-15 | 69.2 (n=21) | - | 67.1 (n=8) |
| 2026-11-16 | 19.2 (n=236) | 19.4 (n=71) | 17.4 (n=46) |
| 2026-11-17 | 16.2 (n=183) | 14.0 (n=42) | 15.8 (n=31) |
| 2026-11-18 | 74.7 (n=116) | 70.7 (n=46) | 85.1 (n=10) |

### T2. Run-level, SAMPLE via jobs API (success+failure runs), split by era
era hosted = every job on GitHub-hosted ubuntu-26.04 (runs created 11-04..11-14); era scaleset = Rust/Check jobs on "Velnor Scale Set" cluster, only Plan/gates/lint on GitHub-hosted (runs created 11-15..11-18).
queue = run created_at -> earliest job started_at; wall = run created_at -> run updated_at; runner-min = sum(job completed_at-started_at). Format med/p90.
| era | event | n runs | queue min | wall min | jobs/run (med) | runner-min | worst-job-queue min | conclusion failure |
|---|---|---|---|---|---|---|---|---|
| hosted | pull_request | 28 | 0.3/1.1 | 15.8/22.0 | 29.0 | 61.6/87.8 | 1.6/4.4 | 14/28 |
| hosted | push | 16 | 0.1/9.1 | 21.4/67.1 | 37.0 | 85.2/90.5 | 3.4/33.5 | 0/16 |
| scaleset | pull_request | 77 | 3.7/26.3 | 26.7/92.3 | 39 | 52.9/63.9 | 10.2/36.5 | 27/77 |
| scaleset | merge_group | 50 | 1.9/15.3 | 19.9/84.1 | 39.0 | 64.6/81.0 | 6.7/32.8 | 14/50 |
| scaleset | push | 29 | 4.3/22.9 | 21.7/59.2 | 39 | 62.0/74.3 | 4.1/19.4 | 3/29 |

Success-only, scaleset era:
| event | n | queue | wall | runner-min |
|---|---|---|---|---|
| pull_request | 50 | 3.4/18.2 | 27.7/79.2 | 51.6/62.4 |
| merge_group | 36 | 1.1/10.0 | 16.1/47.0 | 61.8/76.2 |
| push | 26 | 4.3/23.1 | 19.4/65.4 | 61.4/71.2 |

Sampled runs by day (scaleset era), wall median min / queue-to-first-job median / worst-job-queue median:
| day | event | n | wall med | first-job queue med | worst-job queue med |
|---|---|---|---|---|---|
| 2026-11-15 | pull_request | 1 | 9.8 | 0.6 | 0.3 |
| 2026-11-16 | pull_request | 31 | 23.2 | 2.0 | 7.3 |
| 2026-11-16 | merge_group | 24 | 17.0 | 1.4 | 5.3 |
| 2026-11-16 | push | 13 | 22.1 | 5.4 | 3.9 |
| 2026-11-17 | pull_request | 24 | 16.4 | 0.7 | 5.1 |
| 2026-11-17 | merge_group | 14 | 13.7 | 0.2 | 2.1 |
| 2026-11-17 | push | 11 | 16.1 | 1.1 | 3.6 |
| 2026-11-18 | pull_request | 21 | 77.8 | 20.7 | 33.4 |
| 2026-11-18 | merge_group | 12 | 81.7 | 13.9 | 31.6 |
| 2026-11-18 | push | 5 | 75.8 | 16.3 | 20.9 |

### T3 [scaleset era, pull_request] per job name, 77 runs, ranked by total runner-min (top 20 of 39 names)
| job | ran/total | queue min med/p90 | duration min med/p90 | runner-min/run | failures | runner group / labels (count) |
|---|---|---|---|---|---|---|
| Check / maintenance-helpers | 77/77 | 0.0/0.2 | 8.5/9.9 | 8.41 | 5 | velnor-ci | velnor,ubuntu-26.04-scale-set (75), none | velnor,ubuntu-26.04-scale-set (2) |
| Rust / velnor-actions-cli | 77/77 | 0.0/0.2 | 6.5/7.7 | 6.57 | 1 | velnor-ci | velnor,ubuntu-26.04-scale-set (75), none | velnor,ubuntu-26.04-scale-set (2) |
| Rust / velnor-runner-host | 77/77 | 0.2/0.3 | 5.5/10.1 | 6.23 | 5 | velnor-ci | velnor,ubuntu-26.04-scale-set (75), none | velnor,ubuntu-26.04-scale-set (2) |
| Rust / velnor-runner-host (local-mode) | 59/59 | 0.2/0.3 | 6.5/10.4 | 5.49 | 5 | velnor-ci | velnor,ubuntu-26.04-scale-set (57), none | velnor,ubuntu-26.04-scale-set (2) |
| Rust / velnor-actions-orchestrator | 77/77 | 0.2/0.2 | 3.0/4.0 | 3.13 | 1 | velnor-ci | velnor,ubuntu-26.04-scale-set (75), none | velnor,ubuntu-26.04-scale-set (2) |
| Rust / velnor-runner-cli | 77/77 | 0.2/0.2 | 3.0/3.7 | 2.93 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (75), none | velnor,ubuntu-26.04-scale-set (2) |
| Rust / velnor-actions-workflow-renderer | 77/77 | 0.2/0.2 | 1.6/2.1 | 1.63 | 1 | velnor-ci | velnor,ubuntu-26.04-scale-set (75), none | velnor,ubuntu-26.04-scale-set (2) |
| Rust / velnor-ci | 77/77 | 0.2/0.2 | 1.2/2.3 | 1.40 | 5 | velnor-ci | velnor,ubuntu-26.04-scale-set (70), none | velnor,ubuntu-26.04-scale-set (7) |
| Rust / velnor-actions-mise | 77/77 | 0.2/0.2 | 1.4/1.8 | 1.37 | 1 | velnor-ci | velnor,ubuntu-26.04-scale-set (75), none | velnor,ubuntu-26.04-scale-set (2) |
| Rust / velnor-local | 77/77 | 0.2/0.2 | 1.1/1.4 | 1.14 | 2 | velnor-ci | velnor,ubuntu-26.04-scale-set (75), none | velnor,ubuntu-26.04-scale-set (2) |
| Rust / velnor-actions-tofu | 77/77 | 0.2/0.2 | 1.0/1.3 | 1.02 | 1 | velnor-ci | velnor,ubuntu-26.04-scale-set (75), none | velnor,ubuntu-26.04-scale-set (2) |
| Plan | 77/77 | 7.1/33.4 | 0.9/1.1 | 0.97 | 2 | GitHub Actions | ubuntu-26.04 (77) |
| Ledger CLI | 77/77 | 6.0/30.4 | 0.9/1.0 | 0.88 | 0 | GitHub Actions | ubuntu-26.04 (77) |
| Rust / velnor-gen | 77/77 | 0.2/0.3 | 0.9/1.2 | 0.70 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (51), none | velnor,ubuntu-26.04-scale-set (26) |

### T3 [scaleset era, merge_group] per job name, 50 runs, ranked by total runner-min (top 20 of 39 names)
| job | ran/total | queue min med/p90 | duration min med/p90 | runner-min/run | failures | runner group / labels (count) |
|---|---|---|---|---|---|---|
| Check / maintenance-helpers | 50/50 | 0.0/0.2 | 8.5/11.1 | 8.94 | 1 | velnor-ci | velnor,ubuntu-26.04-scale-set (50) |
| Rust / velnor-actions-cli | 50/50 | 0.2/0.4 | 6.5/7.7 | 6.81 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (50) |
| Rust / velnor-runner-host | 50/50 | 0.2/0.5 | 5.7/9.0 | 6.10 | 1 | velnor-ci | velnor,ubuntu-26.04-scale-set (50) |
| Rust / velnor-runner-host (local-mode) | 35/35 | 0.2/0.5 | 6.4/13.1 | 5.25 | 4 | velnor-ci | velnor,ubuntu-26.04-scale-set (35) |
| Rust / velnor-actions-orchestrator | 50/50 | 0.2/0.5 | 3.1/3.9 | 3.37 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (50) |
| Rust / velnor-runner-cli | 50/50 | 0.2/0.5 | 3.0/3.6 | 3.09 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (50) |
| Rust / velnor-actions-workflow-renderer | 50/50 | 0.2/0.5 | 1.6/2.3 | 1.81 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (50) |
| Rust / velnor-ci | 50/50 | 0.2/0.5 | 1.3/2.6 | 1.59 | 2 | velnor-ci | velnor,ubuntu-26.04-scale-set (50) |
| Rust / velnor-actions-rust | 50/50 | 0.2/0.6 | 1.3/1.6 | 1.36 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (50) |

### T3 [scaleset era, push] per job name, 29 runs, ranked by total runner-min (top 20 of 39 names)
| job | ran/total | queue min med/p90 | duration min med/p90 | runner-min/run | failures | runner group / labels (count) |
|---|---|---|---|---|---|---|
| Check / maintenance-helpers | 29/29 | 0.0/0.2 | 8.3/9.3 | 8.58 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (29) |
| Rust / velnor-actions-cli | 29/29 | 0.1/0.2 | 6.5/7.2 | 6.56 | 1 | velnor-ci | velnor,ubuntu-26.04-scale-set (29) |
| Rust / velnor-runner-host | 29/29 | 0.2/0.3 | 5.0/9.1 | 5.92 | 1 | velnor-ci | velnor,ubuntu-26.04-scale-set (29) |
| Rust / velnor-runner-host (local-mode) | 20/20 | 0.2/0.5 | 6.1/9.0 | 4.63 | 2 | velnor-ci | velnor,ubuntu-26.04-scale-set (20) |
| Rust / velnor-actions-orchestrator | 29/29 | 0.2/0.3 | 3.1/3.7 | 3.17 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (29) |
| Rust / velnor-runner-cli | 29/29 | 0.2/0.5 | 3.1/3.8 | 3.16 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (29) |
| Rust / velnor-actions-workflow-renderer | 29/29 | 0.2/0.3 | 1.6/1.9 | 1.69 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (29) |
| Rust / velnor-actions-mise | 29/29 | 0.2/0.4 | 1.4/1.6 | 1.39 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (29) |
| Rust / velnor-ci | 29/29 | 0.2/0.3 | 1.2/2.2 | 1.39 | 0 | velnor-ci | velnor,ubuntu-26.04-scale-set (29) |

### T3 [hosted era, pull_request] per job name, 28 runs, ranked by total runner-min (top 20 of 42 names)
| job | ran/total | queue min med/p90 | duration min med/p90 | runner-min/run | failures | runner group / labels (count) |
|---|---|---|---|---|---|---|
| Rust / velnor-actions-cli | 28/28 | 0.0/1.4 | 12.3/18.8 | 11.06 | 8 | GitHub Actions | ubuntu-26.04 (28) |
| Check / maintenance-helpers | 28/28 | 0.0/1.0 | 9.6/13.5 | 9.31 | 0 | GitHub Actions | ubuntu-26.04 (28) |
| Rust / velnor-actions-orchestrator | 28/28 | 0.2/2.8 | 9.3/11.7 | 8.17 | 4 | GitHub Actions | ubuntu-26.04 (28) |
| Rust / velnor-runner-host | 28/28 | 0.6/3.5 | 4.9/9.1 | 5.01 | 1 | GitHub Actions | ubuntu-26.04 (28) |
| Rust / velnor-actions-workflow-renderer | 28/28 | 0.5/2.4 | 3.4/5.3 | 3.57 | 2 | GitHub Actions | ubuntu-26.04 (28) |
| Rust / velnor-actions-mise | 28/28 | 0.0/3.3 | 2.0/2.5 | 1.96 | 0 | GitHub Actions | ubuntu-26.04 (28) |
| Rust / velnor-actions-rust | 28/28 | 0.1/3.5 | 1.7/2.4 | 1.73 | 0 | GitHub Actions | ubuntu-26.04 (24) |
| Rust / velnor-runner-cli | 28/28 | 0.1/2.5 | 1.8/2.2 | 1.68 | 1 | GitHub Actions | ubuntu-26.04 (28) |
| Rust / velnor-actions-tofu | 28/28 | 0.0/4.0 | 1.7/2.2 | 1.67 | 0 | GitHub Actions | ubuntu-26.04 (28) |

### T4. Queue wait (created->started) by runner class and era, all sampled jobs that started
| era | runner_group | labels | jobs | queue med/p90 | max |
|---|---|---|---|---|---|
| scaleset | velnor-ci | velnor,ubuntu-26.04-scale-set | 3944 | 0.2/0.3 | 4.5 |
| hosted | GitHub Actions | ubuntu-26.04 | 1321 | 0.1/7.2 | 119.3 |
| scaleset | GitHub Actions | ubuntu-26.04 | 1248 | 4.1/23.6 | 49.2 |
| scaleset | none | velnor,ubuntu-26.04-scale-set | 850 | 0.0/0.0 | 0.0 |
| hosted | none | ubuntu-26.04 | 60 | 0.0/0.0 | 0.0 |

Hosted-only jobs in scaleset era (queue by job name, PR+MG+push):
| job | n | queue med/p90 | max | duration med |
|---|---|---|---|---|
| Ledger CLI | 156 | 4.9/22.8 | 48.4 | 0.8 |
| Plan | 156 | 5.5/23.1 | 43.2 | 0.9 |
| Cargo Deny | 156 | 5.4/21.3 | 49.2 | 0.4 |
| Jactionlint | 156 | 5.5/24.9 | 42.9 | 0.3 |
| Cargo Machete | 156 | 4.8/25.8 | 45.3 | 0.2 |
| Alint | 156 | 5.9/24.7 | 45.8 | 0.3 |
| Required verdict | 156 | 1.6/24.3 | 41.9 | 0.7 |
| Required | 156 | 1.9/21.4 | 39.8 | 0.1 |

### T5. Gate chain (Plan -> fan-out -> Required verdict -> Required), scaleset era, success runs; queue = job created->started, minutes
| day | event | n | wall (created->updated) med | Plan queue med | Required verdict queue med | Required queue med | sum of 3 queues med | fan-out span (Plan end -> last Rust/Check end) med | 8 hosted-job queue med |
|---|---|---|---|---|---|---|---|---|---|
| 2026-11-16 | merge_group | 18 | 15.9 | 2.2 | 0.9 | 1.1 | 6.1 | 8.2 | 3.0 |
| 2026-11-16 | pull_request | 23 | 23.2 | 5.9 | 1.9 | 2.1 | 12.6 | 8.4 | 5.2 |
| 2026-11-16 | push | 12 | 21.9 | 3.3 | 0.6 | 1.3 | 7.8 | 8.1 | 2.6 |
| 2026-11-17 | merge_group | 12 | 12.8 | 0.0 | 0.1 | 0.9 | 2.3 | 9.1 | 0.1 |
| 2026-11-17 | pull_request | 13 | 16.0 | 2.3 | 0.7 | 1.2 | 5.8 | 9.3 | 2.6 |
| 2026-11-17 | push | 10 | 16.3 | 0.8 | 0.8 | 0.6 | 5.1 | 9.1 | 1.2 |
| 2026-11-18 | merge_group | 6 | 64.4 | 15.3 | 15.3 | 5.3 | 42.4 | 13.5 | 17.8 |
| 2026-11-18 | pull_request | 13 | 71.8 | 21.6 | 23.3 | 15.3 | 57.0 | 10.7 | 21.8 |
| 2026-11-18 | push | 4 | 88.4 | 16.3 | 15.7 | 22.1 | 54.1 | 11.2 | 14.5 |

### T6. Duration of top jobs by day (scaleset era, all events, executed jobs, median min) - does contention change run time?
| day | n runs | Check / maintenance-helpers | velnor-actions-cli | velnor-runner-host | velnor-runner-host (local-mode) | velnor-actions-orchestrator |
|---|---|---|---|---|---|---|
| 2026-11-15 | 1 | 7.7 | 6.5 | 3.5 | - | 3.1 |
| 2026-11-16 | 68 | 8.0 | 6.3 | 3.7 | 5.9 | 3.0 |
| 2026-11-17 | 49 | 8.8 | 6.7 | 6.1 | 5.6 | 3.1 |
| 2026-11-18 | 38 | 9.6 | 6.9 | 8.8 | 8.9 | 3.1 |

### T7. Duplicate verification
- merge_group runs completed success: 159; of those, 120 have a push-to-main CI run on the identical head SHA (the queue fast-forwards/squashes main to the merge_group commit). Push runs total 257 (success 218).
- distinct PRs entering the merge queue in window: 150; merge_group runs per PR: {2: 28, 1: 115, 3: 6, 4: 1}.
- sample of 45 merge_group runs (random, success+failure): PR head tree == merge_group commit tree in 7/45; PR head was behind the merge-queue base by >=1 commit in 34/45; (PR head fetched at 2026-11-18T16:54:53Z, may be newer than at MG time).
- of those 45, PR-event CI runs exist on the PR head SHA for 45; their conclusions: {'success': 49, 'cancelled': 4, 'failure': 2}.
- all 39 jobs execute in the PR run and in the merge_group run and in the push run (jobs executed/run median 39 for each event; see T2) - same job set, same workflow file, no job is event-gated in the sample.

- PR-event CI runs in window: 2558 across 454 head branches (median 3.0 runs/branch); cancelled 1156 (45%), failure 419 (16%), success 963 (38%).

### T8. Population wall time by hour (UTC), 2026-11-17 18:00 .. 11-18 16:00, success runs, all events (run_started_at->updated_at, min)
| hour UTC | n success | wall med | wall p90 |
|---|---|---|---|
| 2026-11-17T18 | 10 | 14.6 | 19.6 |
| 2026-11-17T19 | 8 | 15.0 | 19.4 |
| 2026-11-17T20 | 7 | 12.2 | 18.4 |
| 2026-11-17T21 | 4 | 10.5 | 10.7 |
| 2026-11-17T22 | 5 | 13.4 | 34.3 |
| 2026-11-17T23 | 2 | 21.0 | 22.2 |
| 2026-11-18T00 | 3 | 34.8 | 42.4 |
| 2026-11-18T01 | 12 | 54.6 | 62.9 |
| 2026-11-18T02 | 14 | 81.1 | 89.2 |
| 2026-11-18T03 | 7 | 99.7 | 131.2 |
| 2026-11-18T04 | 21 | 110.0 | 125.1 |
| 2026-11-18T05 | 14 | 94.1 | 117.2 |
| 2026-11-18T06 | 5 | 82.7 | 107.3 |
| 2026-11-18T07 | 9 | 89.0 | 90.9 |
| 2026-11-18T08 | 7 | 62.8 | 86.5 |
| 2026-11-18T09 | 10 | 66.8 | 72.4 |
| 2026-11-18T10 | 11 | 46.0 | 74.5 |
| 2026-11-18T11 | 15 | 93.2 | 106.2 |
| 2026-11-18T12 | 16 | 69.9 | 102.4 |
| 2026-11-18T13 | 11 | 59.1 | 78.6 |
| 2026-11-18T14 | 9 | 50.1 | 55.6 |
| 2026-11-18T15 | 8 | 44.1 | 55.6 |

#### T10 Check / maintenance-helpers  (n=154 executed jobs, scaleset era, all events; total 1344 runner-min; job median 508s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 2 | Checkout | 154 | 6 | 8 | 1% |
| 7 | Execute named check | 154 | 486 | 570 | 96% |

#### T10 Rust / velnor-actions-cli  (n=154 executed jobs, scaleset era, all events; total 1037 runner-min; job median 393s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 9 | Prepare pinned tools | 154 | 9 | 19 | 2% |
| 25 | Prepare image publication fixture | 154 | 89 | 115 | 23% |
| 28 | Clippy | 154 | 36 | 63 | 9% |
| 29 | Unit and integration tests | 154 | 238 | 284 | 61% |
| 30 | Documentation | 154 | 30 | 54 | 8% |

#### T10 Rust / velnor-runner-host  (n=154 executed jobs, scaleset era, all events; total 956 runner-min; job median 332s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 9 | Prepare pinned tools | 154 | 10 | 17 | 3% |
| 21 | Build test executables | 154 | 81 | 116 | 24% |
| 23 | Clippy | 154 | 70 | 91 | 21% |
| 24 | Unit and integration tests | 154 | 192 | 417 | 58% |
| 25 | Documentation | 154 | 60 | 81 | 18% |

#### T10 Rust / velnor-runner-host (local-mode)  (n=112 executed jobs, scaleset era, all events; total 820 runner-min; job median 382s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 9 | Prepare pinned tools | 112 | 9 | 15 | 2% |
| 21 | Build test executables | 112 | 88 | 126 | 23% |
| 23 | Clippy | 112 | 75 | 89 | 20% |
| 24 | Unit and integration tests | 112 | 246 | 497 | 64% |
| 25 | Documentation | 112 | 65 | 79 | 17% |

#### T10 Rust / velnor-actions-orchestrator  (n=154 executed jobs, scaleset era, all events; total 501 runner-min; job median 185s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 9 | Prepare pinned tools | 154 | 10 | 17 | 5% |
| 26 | Build test executables | 152 | 47 | 68 | 25% |
| 28 | Clippy | 152 | 50 | 70 | 27% |
| 29 | Unit and integration tests | 152 | 78 | 104 | 42% |
| 30 | Documentation | 152 | 33 | 48 | 18% |

#### T10 Rust / velnor-runner-cli  (n=154 executed jobs, scaleset era, all events; total 472 runner-min; job median 180s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 9 | Prepare pinned tools | 154 | 9 | 19 | 5% |
| 21 | Build test executables | 154 | 78 | 109 | 43% |
| 23 | Clippy | 154 | 43 | 67 | 24% |
| 25 | Documentation | 153 | 38 | 61 | 21% |

#### T10 Rust / velnor-actions-workflow-renderer  (n=154 executed jobs, scaleset era, all events; total 265 runner-min; job median 97s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 9 | Prepare pinned tools | 154 | 9 | 15 | 9% |
| 22 | Build test executables | 154 | 25 | 40 | 26% |
| 24 | Clippy | 154 | 18 | 32 | 19% |
| 25 | Unit and integration tests | 154 | 10 | 20 | 10% |
| 26 | Documentation | 154 | 13 | 23 | 13% |

#### T10 Rust / velnor-ci  (n=149 executed jobs, scaleset era, all events; total 227 runner-min; job median 76s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 9 | Prepare pinned tools | 149 | 10 | 17 | 13% |
| 21 | Build test executables | 149 | 14 | 23 | 18% |
| 23 | Clippy | 148 | 8 | 13 | 11% |
| 24 | Unit and integration tests | 148 | 10 | 62 | 13% |

#### T10 Rust / velnor-actions-mise  (n=154 executed jobs, scaleset era, all events; total 213 runner-min; job median 81s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 9 | Prepare pinned tools | 154 | 11 | 21 | 14% |
| 21 | Build test executables | 154 | 16 | 24 | 20% |
| 23 | Clippy | 154 | 13 | 20 | 16% |
| 25 | Documentation | 154 | 10 | 18 | 12% |

#### T10 Rust / velnor-local  (n=154 executed jobs, scaleset era, all events; total 181 runner-min; job median 67s)
| # | step | n | median s | p90 s | share of job median |
|---|---|---|---|---|---|
| 9 | Prepare pinned tools | 154 | 10 | 19 | 15% |
| 21 | Build test executables | 154 | 13 | 24 | 19% |
| 23 | Clippy | 154 | 7 | 12 | 10% |

### T9a. Cache behaviour from job logs (10 runs: 4 PR, 3 merge_group, 3 push; all created 2026-11-16 12:00Z..11-18; jobs = Plan + 6 top jobs; 68 job logs)
| job | logs | "sources" actions/cache restore: hit / not-found | `no restored Rust toolchain` line | mbx object cache per pass: mean hits/misses/not-looked-up (hit share of all lookups) | bytes remote down/up | MBX_REMOTE_URL set | crates compiled (med) | crates downloaded (med) |
|---|---|---|---|---|---|---|---|---|
| Plan | 10 | 10 / 0 | 10/10 | no mbx lines | 0/0 | 0/10 | 0 | 0 |
| Check / maintenance-helpers | 10 | 0 / 0 | 0/10 | no mbx lines | 0/0 | 0/10 | 104 | 0 |
| Rust / velnor-actions-orchestrator | 10 | 0 / 10 | 10/10 | p1: 65/3/96 (40%); p2: 134/2/66 (66%) | 0/0 | 0/10 | 196 | 522 |
| Rust / velnor-actions-cli | 10 | 0 / 10 | 10/10 | p1: 76/3/107 (41%); p2: 120/2/104 (53%) | 0/0 | 0/10 | 218 | 522 |
| Rust / velnor-runner-host (local-mode) | 8 | 0 / 8 | 8/8 | p1: 249/4/142 (63%); p2: 272/2/140 (66%) | 0/0 | 0/8 | 482 | 435 |
| Rust / velnor-runner-host | 10 | 0 / 10 | 10/10 | p1: 289/4/102 (73%); p2: 232/2/177 (56%) | 0/0 | 0/10 | 481 | 435 |
| Rust / velnor-runner-cli | 10 | 0 / 10 | 10/10 | p1: 301/4/103 (74%); p2: 244/2/163 (60%) | 0/0 | 0/10 | 497 | 435 |
Sources-cache entry restored by Plan (hosted): size 86 MB (Cache Size line, n=10). Plan `velnor: sources hit, skipping fetch` lines present; Rust jobs on scale set: `Cache not found for input keys: velnor-v2-sources-trusted-...` in 48/48 Rust-job logs (same key family that Plan hits).

### T9b. Cache / tool setup step durations, all sampled jobs in scaleset era (steps API; n=jobs; 1 s resolution)
| step | runner class | n | median s | p90 s | total runner-min in sample |
|---|---|---|---|---|---|
| Post Set up MBX | scale set | 3591 | 0 | 1 | 8 |
| Post Setup Mise | hosted | 936 | 0 | 1 | 3 |
| Post Setup Mise | scale set | 3944 | 0 | 1 | 12 |
| Prepare Rust components | hosted | 156 | 0 | 1 | 0 |
| Prepare Rust components | scale set | 3636 | 1 | 2 | 72 |
| Prepare pinned tools | hosted | 779 | 1 | 8 | 30 |
| Prepare pinned tools | scale set | 3636 | 10 | 18 | 706 |
| Restore Cargo sources | hosted | 311 | 2 | 3 | 9 |
| Restore Cargo sources | scale set | 3636 | 1 | 1 | 37 |
| Restore Mise tools | hosted | 624 | 3 | 10 | 46 |
| Set up MBX | scale set | 3635 | 1 | 2 | 88 |
| Setup Mise | hosted | 936 | 1 | 2 | 19 |
| Setup Mise | scale set | 3944 | 2 | 5 | 181 |
| Verify Rust channel manifest | hosted | 311 | 1 | 1 | 3 |
| Verify Rust channel manifest | scale set | 3636 | 1 | 1 | 55 |
State-snapshot steps (ci.yml `Restore state snapshot` / `Recover state snapshot`) conclusions across all sampled scale-set-era jobs: Restore state snapshot=skipped: 3636, Recover state snapshot=skipped: 3636.

### T9c. Repo caches right now (`GET /repos/tailrocks/velnor-new/actions/caches`; `actions/cache/usage`: {"full_name":"tailrocks/velnor-new","active_caches_size_in_bytes":3494706404,"active_caches_count":3})
| key family | ref | size MB | created | last accessed |
|---|---|---|---|---|
| velnor-v4-state-trusted-union-ci-helper-s256-31c... | refs/heads/main | 163 | 2026-11-18T16:24 | 2026-11-18T16:24 |
| velnor-v4-state-trusted-union-generator-s256-58c... | refs/heads/main | 131 | 2026-11-18T15:43 | 2026-11-18T16:07 |
| velnor-v2-sources-trusted-s256-e32b46924cd679f38... | refs/heads/main | 87 | 2026-11-18T15:18 | 2026-11-18T16:54 |
| velnor-v2-sources-trusted-s256-e32b46924cd679f38... | refs/heads/main | 86 | 2026-11-17T18:45 | 2026-11-18T16:40 |
| velnor-v4-state-trusted-union-local-s256-3a0e3a4... | refs/heads/main | 97 | 2026-11-17T05:23 | 2026-11-17T05:23 |
| velnor-v4-state-trusted-union-root-s256-5b6d0b80... | refs/heads/main | 1457 | 2026-11-07T12:24 | 2026-11-18T16:02 |
| velnor-v4-state-trusted-union-crates-velnor-runn... | refs/heads/main | 883 | 2026-11-07T09:25 | 2026-11-18T16:14 |
| mise-tools-v2-351e03e56a119396b95fbfa7963a97987c... | refs/heads/main | 401 | 2026-11-05T23:20 | 2026-11-18T16:48 |
| mise-tools-v2-3048804124c2d2ca2a01745a2c93f45690... | refs/heads/main | 68 | 2026-11-05T05:35 | 2026-11-18T16:47 |
| mise-tools-v2-865a79e181cda210da9277649e352bc415... | refs/heads/main | 62 | 2026-11-05T05:35 | 2026-11-18T16:47 |
| mise-tools-v2-dc1f0a8c65f6bbaf73b914c34bf7e881e4... | refs/heads/main | 60 | 2026-11-05T05:34 | 2026-11-18T16:53 |

### T9d. State seal workflow (`state-seal.yml`, id 407175825), 2026-11-04..11-18: 242 runs, conclusions {'success': 228, 'cancelled': 14}. Job-level in the 18 most recent completed runs (24 fetched, jobs API):
| job | success | failure | skipped |
|---|---|---|---|
| Plan | 18 | 0 | 0 |
| Sources producer | 1 | 0 | 17 |
| State producer | 2 | 16 | 0 |
| State producer / ci-helper | 4 | 5 | 9 |
| State producer / crates/velnor-runner | 2 | 16 | 0 |
| State producer / generator | 6 | 5 | 7 |
| State producer / local | 0 | 0 | 18 |
| State seal / ci-helper | 4 | 0 | 14 |
| State seal / crates-velnor-runner | 0 | 0 | 18 |
| State seal / generator | 6 | 0 | 12 |
| State seal / local | 0 | 0 | 18 |
| State seal / root | 0 | 0 | 18 |
| Tools producer / * | 44 | 28 | 0 |

### T11. Current queue state (snapshot 2026-11-18T16:54:53Z; `GET /actions/runs?status=queued|in_progress` then `/runs/<id>/jobs` for 52 runs)
| workflow | status | runner_group | labels | jobs | wait so far med / max (min) |
|---|---|---|---|---|---|
| CI | queued | unassigned | ubuntu-26.04 (GitHub-hosted) | 61 | 10.4 / 42.3 |
| Source qualification | queued | unassigned | ubuntu-26.04 | 23 | 9.1 / 22.2 |
| Job image candidate | queued | unassigned | ubuntu-26.04 | 1 | 5.9 / 5.9 |
| velnor-actions generator release jobs | queued | unassigned | ubuntu-26.04 | 1 | 9.0 / 9.0 |
| **total queued** | | | | **86** | |
| CI | in_progress | velnor-ci | velnor,ubuntu-26.04-scale-set | 14 | - |
| Source qualification | in_progress | GitHub Actions | ubuntu-26.04 | 10 | - |
| Job image candidate | in_progress | GitHub Actions | ubuntu-26.04 | 1 | - |
| generator release jobs | in_progress | GitHub Actions | macos-15 | 1 | - |
Queued runs: 20 CI, 19 Source qualification, 1 Job image candidate, 1 generator release. Queued jobs on scale-set labels: 0. Runs created on 2026-11-18 (partial day, `runs?created=2026-11-18`): CI 348, Job image candidate 306, Source qualification 267, Cache prune 40, State seal 19, generator release 19. Job-level load of the non-CI workflows on the hosted pool was not measured.

### Method
- Run list (population): `gh api --paginate "repos/tailrocks/velnor-new/actions/workflows/371301839/runs?created=<YYYY-MM-DD>&per_page=100"` for each of the 15 days 2026-11-04..11-18 (a range query is capped at 1000 results, so one query per day) -> 3009 runs (`runs_ci.tsv`). Wall in T1 = `run_started_at` -> `updated_at` (includes re-run attempts); in T2/T5 wall = `created_at` -> `updated_at`. Other CI workflow ids checked: velnor.yml (369420828) 0 runs; rust-ci.yml (421813894) 4 PR runs, not analysed.
- Job sample: `gh api --paginate repos/tailrocks/velnor-new/actions/runs/<id>/jobs?per_page=100` for 200 completed (success|failure) runs chosen with python `random.seed(7)` / `seed(11)`: 105 pull_request (28 hosted era + 77 scaleset era), 50 merge_group, 45 push (16 hosted era + 29 scaleset era). Cancelled runs are excluded from the sample, so runner-min of cancelled runs (45% of PR runs) is not in these numbers. queue = job `created_at`->`started_at`; duration = `started_at`->`completed_at`; runner-min = sum of durations; "ran" counts jobs with both timestamps, including jobs that report 0 s (e.g. `velnor-runner-host (local-mode)` appears in 59 of 77 scaleset-era PR runs; for `Rust / velnor-actions-contract` the median duration 0.0 reflects many near-0 s executions), so some medians include ~0 s jobs. Failure rate = jobs with conclusion failure / jobs listed.
- Steps (T10): `steps` array of the same job JSON, scaleset-era runs only (77+50+29 = 156 runs; 154 executed jobs for most names), executed jobs >5 s, steps with median >= 6 s and n >= 20 shown; `Unit and integration tests` and `Wait for parallel checks` are the same wall interval (tests run in parallel with clippy/doc; steps are not additive).
- Logs (T9a): `gh api --allow-escape-sequences repos/tailrocks/velnor-new/actions/jobs/<job_id>/logs` for 68 jobs from 10 runs (seed 5; 4 PR, 3 merge_group, 3 push; created 2026-11-16 12:00Z onward): Plan + maintenance-helpers + runner-host + runner-host (local-mode) + actions-cli + actions-orchestrator + runner-cli. Parsed (regex, `cache.py`) for `Cache hit for:`, `Cache not found for input keys:`, `Cache Size:`, `velnor: no restored Rust`, `velnor: sources hit|miss`, `mbx[cache]: object cache: H hits, M misses, N not looked up; D B downloaded, U B uploaded`, `MBX_REMOTE_URL:`, `Compiling <crate> v`, `Downloaded <crate> v`. No sccache, Swatinem/rust-cache, or other actions/cache keys appeared in any of the 68 logs besides `velnor-v2-sources-trusted-*`.
- Caches: `GET /actions/cache/usage`, `GET /actions/caches` (11 entries listed; usage endpoint reports 3 active / 3.49 GB, lagging). State seal: `GET /actions/workflows/407175825/runs?created=>=2026-11-04` (242 runs) + jobs of the 24 most recent.
- Duplicates (T7): merge_group head SHA vs push-event head SHA from the run lists; 45 random merge_group runs (seed 3): `GET /commits/<mg_sha>` (tree, parents), `GET /pulls/<n>` (PR number from branch `gh-readonly-queue/main/pr-<n>-<sha>`), `GET /commits/<pr_head>` (tree), `GET /compare/<mg_parent0>...<pr_head>` (`behind_by`). Pair table: `dup_pairs.json`.
- Queue now (T11): `GET /actions/runs?status=queued` and `status=in_progress`, then jobs per run.
- Scripts in ci-raw: `analyze2.py`, `steps.py`, `cache.py`, `cachetab.py`, `final_extra.py`, `dup.py`, `dup2.py`, `assemble.py`. Caveats: jobs API returned HTTP 502 for 6 runs on first try (retried successfully); GitHub API p90 on n<30 is coarse; hosted/scale-set contention differs by hour so medians mix regimes (see T5/T6/T8 for by-day splits).

### Discrepancies vs prior claims
| prior claim | re-measured | verdict |
|---|---|---|
| PR run ~92 min median | scaleset-era PR wall (created->updated) median 26.7, p90 92.3 (n=77; success-only 27.7 / 79.2). By day, sampled PR median: 11-16 23.2, 11-17 16.4, 11-18 77.8; population success PR median by day 19.2 / 16.2 / 74.7 (T1). Hourly (T8): 11-17 18:00-23:00Z medians 10.5-21.0; 11-18 02:00-12:00Z 46-110. Population 15 days: PR 17.5 / p90 60.0 (n=1382). Hosted-era PR 15.8 / 22.0 | 92 is about the window p90 and is reached as a median only in the 11-18 ~01:00-13:00Z regime |
| ~67 min of it queue wait | 11-18 PR success runs: Plan + Required verdict + Required queue sum median 57.0 of 71.8 wall (79%); first-job queue median 20.7. 11-16: 12.6 of 23.2; 11-17: 5.8 of 16.0. Scale-set jobs queue 0.2 / 0.3 (max 4.5) in all regimes; the waiting is on the 8 hosted jobs | confirmed for 11-18 only; numbers in the 11-16/17 regime are 5-13 min |
| ~39 jobs per run | median 39 in scaleset era (PR, MG); push 37.5-39; hosted-era PR run median 29 (workflow had fewer jobs) | confirmed for scaleset era |
| ~73 runner-min per run | scaleset-era median: PR 52.9 (p90 63.9), MG 64.6 (p90 81.0), push 62.0 (p90 74.3). Success-only per day, PR: 49.1 / 54.8 / 54.5; MG: 52.6 / 65.1 / 78.8 for 11-16/17/18. Hosted era: PR 61.6, push 85.2 | not matched for PR; 73 sits between MG median and p90; MG runner-min grows through 11-18 |
| CI runs twice per merge (PR + merge_group) | three event types run the full 39-job set: PR (2558 runs on 454 branches, median 3 per branch, 45% cancelled), merge_group (193 runs, 150 PRs; 115 PRs once, 28 twice, 7 three+ times), and push to main: 120 of 159 successful merge_group runs have a push CI run on the identical head SHA. Only 7/45 sampled PRs had a PR-head tree equal to the merge_group tree (34/45 PR heads were behind the queue base) | PR+MG confirmed, push is a third verification of the same SHA in 75% of merges |
| velnor-runner-host tests ~18 runner-min | `runner-host` + `runner-host (local-mode)` = 6.2 + 5.5 = 11.7 runner-min per PR run (MG 6.1 + 5.3). By day, job median duration: runner-host 3.7 / 6.1 / 8.8 and local-mode 5.9 / 5.6 / 8.9 for 11-16/17/18 (sum 17.7 on 11-18). `Unit and integration tests` step median 192 s (p90 417) and 246 s (p90 497) | ~18 holds on 11-18 only; durations rise with the day-18 load |
| Check / maintenance-helpers ~10 runner-min, no cache | duration median 8.5 (p90 9.9-11.1) in all three events; 8.4-8.9 runner-min per run; 9.6 on 11-18. Step `Execute named check` 486 s of 508 s. 10/10 logs: 0 cache lines, 104 crates compiled, 0 restore steps | confirmed (8.5 not 10, 9.6 on 11-18) |
| 12-34 min waits for GitHub-hosted runners | scaleset-era hosted jobs (1248 jobs): queue median 4.1, p90 23.6, max 49.2. Per name p90 21.3-25.8 min, max 42.9-49.2 (T4). 11-18 median of per-run hosted queue 14.5-21.8 (T5) | range confirmed at p90 level; median over window is 4-6 min, 15-22 on 11-18 |
| caches mostly miss | Rust jobs: sources cache not found 48/48 (Plan on hosted hits 10/10, same key family, 86 MB); toolchain "no restored Rust" 48/48 + Plan 10/10; mbx remote URL empty 68/68 and 0 B transferred; mbx local object-cache lookups 40-74% hit per pass (within-job reuse between passes); maintenance-helpers no cache at all; `Restore state snapshot` skipped 3636/3636 jobs | confirmed |
| state seal never succeeded because merge queue advances main | workflow-level 228 success / 14 cancelled of 242, but job-level (18 recent runs): `State seal / generator` success 6, `/ ci-helper` 4, `/ root`, `/ local`, `/ crates-velnor-runner` 0 successes (skipped 18/18). Failures of State producer (16 of 18 runs): 7 at step `Check state tip`, 9 at `Clippy`/`Wait for parallel checks`. Tools producer 44 success / 28 failure (all 28 at `Check state tip`). State caches for ci-helper (11-18 16:24Z) and generator (15:43Z) exist; root cache dated 11-07, runner cache 11-07 | partly refuted: some seals succeed (ci-helper, generator); root/runner never in window; not all failures are tip-race |
| no toolchain cache | rustup toolchain not restored (above); `mise-tools-v2-*` caches exist (4 entries 60-401 MB, last accessed 11-18) and `Restore Mise tools` runs only in hosted jobs (n=624, median 3 s). `Prepare pinned tools` in scale-set jobs median 10 s (p90 18), 706 runner-min in sample | confirmed for scale-set jobs |
| mbx remote cache not set | `MBX_REMOTE_URL:` empty in 68/68 logs; `bytes downloaded/uploaded` 0/0 in all 96 mbx summary lines | confirmed |
| 2026-11-18: 86 queued on GitHub-hosted, 0 on self-hosted | snapshot 2026-11-18T16:54:53Z: 86 queued jobs (61 CI, 23 Source qualification, 1 Job image candidate, 1 generator release), all unassigned `ubuntu-26.04`; 0 queued with scale-set labels; 14 CI jobs running on scale set | confirmed (the 86 include 25 non-CI jobs) |

## B. Cluster: capacity, utilisation, ARC, storage, network

_Source file: cluster.md_

Captured 2026-11-18 ~16:40-16:45 UTC, read-only (`kubectl get/describe/top/version`, `get --raw .../proxy/configz` and `.../proxy/stats/summary`).
Raw JSON scratch: `~/.cache/ci-ambition/evidence/cluster-raw/` (nodes.json, ars.json).
Docs dir: `~/Projects/tailrocks/velnor-runners/` = RN; files: `RN/README.md`, `RN/PLAN.md`, `RN/state/RESULT.md` (=RESULT), `RN/state/log.md` (=LOG). Note: `RN/GOAL.md` and `RN/PLAN.md` are at the repo root, not under state/. `RN/state/inventory.md` = INV.
Source tags: [K] live cluster command, [D file:line] docs. Docs are dated 2026-11-15/17; the cluster is 3d3h old.

### 0. Summary table per node

| Node | Role | Site | CPU (docs) | Threads | RAM cap [K] | Disks (docs, INV) | Requests cpu / mem [K describe] | Runner pods (arc-runners ns) [K] |
|---|---|---|---|---|---|---|---|---|
| velnor-237 (5.9.55.237) | control-plane, taint NoSchedule | DE (inferred, see 5) | i5-13500 | 20 | 65,650,440 Ki (~62.6 GiB) | 2 x 512 GB NVMe | 300m / 268Mi | 0 |
| velnor-240 (91.99.37.240) | worker | DE (LOG:9 says DE) | EPYC 9454P | 96 | 131,579,436 Ki (~125.5 GiB) | 2 x 3.84 TB + 3 x 15 TB | 24.1 / 49280Mi | 9 |
| velnor-105 (142.132.195.105) | worker | DE (inferred) | EPYC 9454P | 96 | 131,596,360 Ki | 2 x 15 TB + 2 x 3.84 TB | 24.1 / 49280Mi | 4 |
| velnor-241 (37.27.110.241) | worker | FI (LOG:9 says FI) | EPYC 9454P | 96 | 131,578,888 Ki | 2 x 3.84 TB | 22.1 / 45184Mi | 6 |
| velnor-5 (37.27.171.5) | worker | FI (inferred) | EPYC 9454P | 96 | 131,579,520 Ki | 2 x 3.84 TB | 22.1 / 45184Mi | 3 |
| velnor-190 (157.180.97.190) | worker | FI (inferred) | EPYC 9454 | 96 | 131,579,508 Ki | 2 x 3.84 TB | 24.1 / 49280Mi | 3 |

Sources: CPU model, threads, disks: RN/README.md:25-31 (docs only; CPU model/threads are not exposed by the k8s API; [K] confirms cpu capacity 96 / 20). RAM/requests: [K] `kubectl get nodes -o json`, `kubectl describe node`. Pod counts: [K] `kubectl get pods -A -o wide` (Running+Init, one snapshot; counts include the 4 Init:0/2 and 10 Init:1/2 pods, which are in the arc-runners namespace total; per-node split covers only the 25 pods with a node column in that sample, so it is indicative).
Site inference: Hetzner IP ranges (37.27/157.180 = Finland; 5.9/91.99/142.132 = Germany) are my inference; docs only name velnor-241 as FI and velnor-240 as DE (LOG:9). README.md:13 states "two Hetzner sites, Germany and Finland" without per-node mapping.

### 1. Nodes

#### 1.1 Common node facts [K `kubectl get nodes -o wide`, `-o json`]
- All six Ready, age 3d3h; MemoryPressure/DiskPressure/PIDPressure False on all.
- k3s / kubelet: v1.37.2+k3s1 (server and client kubectl v1.37.2). Kustomize v5.8.1 client.
- OS: Debian GNU/Linux 13 (trixie); kernel 6.12.94+deb13-amd64; arch amd64; runtime containerd://2.3.7-k3s1. Same on all six.
- Machine image per docs: Debian-1305-trixie-amd64-base.tar.zst (RESULT "Versions"; LOG:2).
- Pod capacity 110 per node; hugepages-1Gi/2Mi = 0 on all nodes [K].
- Labels on workers: standard kubernetes.io/*, `node.kubernetes.io/instance-type=k3s`, `velnor.io/runner=true`. No taints on workers [K].
- velnor-237 labels: `node-role.kubernetes.io/control-plane=true`, no runner label; taint `node-role.kubernetes.io/control-plane=true:NoSchedule` [K].
- No zone/region topology labels exist (no `topology.kubernetes.io/*`) [K nodes.json labels]. Site is therefore not a k8s label.
- IPs: INTERNAL-IP = EXTERNAL-IP = public IP on every node [K -o wide].

#### 1.2 Capacity vs allocatable [K nodes.json]
| Node | cpu cap = alloc | memory cap = alloc | ephemeral-storage cap | ephemeral alloc |
|---|---|---|---|---|
| workers (x5) | 96 = 96 | 131.58-131.60 million Ki = same | 3,777,979,830,272 B (~3.78 TB) | 3,589,080,835,944 B |
| velnor-237 | 20 = 20 | 65,650,440 Ki = same | 501,599,219,712 B | 476,519,258,353 B |
Allocatable cpu and memory equal capacity on every node (no kube/system reserved). Ephemeral allocatable = capacity less 5% (evictionHard nodefs.available 5%).

#### 1.3 Kubelet config (configz) [K `kubectl get --raw /api/v1/nodes/velnor-5/proxy/configz`, same on velnor-237]
- evictionHard: `imagefs.available: 5%`, `nodefs.available: 5%`. No memory.available hard eviction threshold is set; evictionSoft none.
- kubeReserved: none; systemReserved: none; enforceNodeAllocatable: [pods].
- cpuManagerPolicy: none; topologyManagerPolicy: none; memoryManagerPolicy: unset.
- maxPods 110; podPidsLimit -1; cgroupDriver systemd; failSwapOn false; memorySwap {}.
- imageGCHighThresholdPercent 85, imageGCLowThresholdPercent 80; containerLogMaxSize 10Mi.
- serializeImagePulls false; maxParallelImagePulls unset; featureGates none.
- configz read for velnor-5 and velnor-237 only; other workers not queried (same k3s args per node-args annotations, see 1.5).

#### 1.4 Filesystems [K `kubectl get --raw /api/v1/nodes/<n>/proxy/stats/summary`]
| Node | nodefs capacity / used | imagefs (containerd) capacity / used |
|---|---|---|
| velnor-105 | 3,777 GB / 71 GB | 34,564 GB / 7 GB |
| velnor-190 | 3,777 GB / 49 GB | 3,838 GB / 6 GB |
| velnor-237 | 501 GB / 1 GB | 511 GB / 4 GB |
| velnor-240 | 3,777 GB / 109 GB | 49,925 GB / 6 GB |
| velnor-241 | 3,777 GB / 110 GB | 3,838 GB / 8 GB |
| velnor-5 | 3,777 GB / 44 GB | 3,838 GB / 6 GB |
- Reported inodes (velnor-105 nodefs): 234,340,352 total; imagefs 3,375,657,344 total [K].
- Docs: OS on the smallest NVMe (not mirrored); every other NVMe joined into one LVM VG `data`, LV `rancher`, XFS, mounted at `/var/lib/rancher` with `defaults,noatime` (RN/README.md:35-38; RN/PLAN.md:66,77,80). Log: "VG data/LV rancher xfs at /var/lib/rancher (477G,46T,32T,3.5T x3)" (LOG:7).
- Disk inventory by node (nvme names, sizes; OS disk and data disks): RN/state/inventory.md. 237: 2 x 476.9G; 240: 2 x 3.5T + 3 x 14T; 105: 2 x 3.5T + 2 x 14T; 241/5/190: 2 x 3.5T each.
- Observation tying the two: imagefs capacity matches the LVM sizes in LOG:7 (240 ~50 TB, 105 ~34.6 TB, others 3.8 TB). nodefs (3.78 TB) differs from imagefs on all six nodes, so they are different filesystems. Where emptyDir volumes physically live is not directly stated in the docs; [K] shows emptyDir volumes only, and the kubelet root dir was not read. Unknown.
- NVMe model/RAID/stripe, filesystem on the OS disk (docs: ext4 `/` in PLAN.md:45), mount options for emptyDir path: not observable read-only except as cited.

#### 1.5 k3s start args [K node annotation `k3s.io/node-args`]
- Server (velnor-237): `server --node-ip 5.9.55.237 --node-external-ip 5.9.55.237 --advertise-address 5.9.55.237 --tls-san 5.9.55.237 --flannel-backend wireguard-native --flannel-external-ip --disable traefik --disable servicelb --node-taint node-role.kubernetes.io/control-plane=true:NoSchedule --write-kubeconfig-mode 600`.
- Agents: `agent --node-ip <ip> --node-external-ip <ip> --node-label velnor.io/runner=true`; K3S_URL https://5.9.55.237:6443.

#### 1.6 Unknown / not observable read-only
CPU governor, SMT state, NUMA topology, THP setting, sysctls (net.core.*, vm.*, fs.inotify), CPU model string, NVMe model and RAID/LVM stripe layout, kube-proxy mode (k3s embedded; no config read), MTU of flannel.1/wg interface. The only related doc fact: a BBR + 128 MB buffer sysctl experiment on the nodes gave no gain and was reverted (LOG:31, 2026-11-16). `RN/` has no CPU governor/SMT/NUMA/THP text (grep checked README, PLAN, GOAL, OWNER-STEPS, state/*.md).

### 2. Current utilisation

#### 2.1 `kubectl top nodes` (3 samples, ~100 s apart, 2026-11-18 UTC) [K]
| Node (cpu cores of 96) | 16:40:47 | 16:42:29 | 16:44:10 |
|---|---|---|---|
| velnor-105 | 18.2 (18%) | 47.2 (49%) | 88.5 (92%) |
| velnor-190 | 67.5 (70%) | 89.8 (93%) | 23.7 (24%) |
| velnor-237 (of 20) | 0.82 (4%) | 0.27 (1%) | 0.12 (0%) |
| velnor-240 | 89.6 (93%) | 91.3 (95%) | 88.9 (92%) |
| velnor-241 | 87.5 (91%) | 91.8 (95%) | 89.1 (92%) |
| velnor-5 | 25.5 (26%) | 72.2 (75%) | 88.7 (92%) |
Memory (MiB used; percent of node): sample 1: 105=35,939 (27%), 190=39,298 (30%), 237=1,396 (2%), 240=44,997 (35%), 241=42,301 (32%), 5=24,816 (19%). Sample 2: 105=19,290 (15%), 190=38,596 (30%), 240=49,665 (38%), 241=45,323 (35%), 5=37,874 (29%). Sample 3: 105=20,910 (16%), 190=21,412 (16%), 240=44,549 (34%), 241=39,164 (30%), 5=25,312 (19%).
- CPU is bursty and mostly high on workers while memory stays <40%.

#### 2.2 `kubectl top pods` [K]
- Top CPU runner pods (sample ~16:44): 83.7 cores / 16.8 GiB (runner-dd5q6), 74.9 / 14.7 GiB, 68.6 / 12.6 GiB, 36.7 / 13.4 GiB, 29.4 / 12.5 GiB, 18.8 / 5.3 GiB, 16.8 / 11.2 GiB; the rest of ~40 running runner pods were between 0.3 and 11 cores. Idle pods (19 containers listed at 1m CPU) use ~45 MiB.
- The 7 top runner pods are at the velnor-new ci.yml (workflows on PRs 1337/1340/1345/1354/1356 and merge queue) per ephemeralrunner JOBWORKFLOWREF [K].
- Pods have no CPU limit (see 3.3), so one pod can use most of a 96-thread node (observed 83.7 cores).

#### 2.3 Allocated requests/limits per node [K `kubectl describe node`]
| Node | cpu requests | cpu limits | memory requests | memory limits |
|---|---|---|---|---|
| velnor-105 | 24,100m (25%) | 0 | 49,280Mi (38%) | 385Gi (306%) |
| velnor-190 | 24,100m (25%) | 0 | 49,280Mi (38%) | 385Gi (306%) |
| velnor-240 | 24,100m (25%) | 0 | 49,280Mi (38%) | 385Gi (306%) |
| velnor-241 | 22,100m (23%) | 0 | 45,184Mi (35%) | 353Gi (281%) |
| velnor-5 | 22,100m (23%) | 0 | 45,184Mi (35%) | 353Gi (281%) |
| velnor-237 | 300m (1%) | 0 | 268Mi (0%) | 1,194Mi (1%) |
- Ephemeral-storage requests/limits are 0 on every node. Memory limits are overcommitted ~3x (32Gi limit per runner pod).
- Node PSI (velnor-105 summary): memory psi `some` avg300 0.03, `full` avg300 0 (velnor-5: 0.11 / 0.05) [K stats/summary].

### 3. ARC (Actions Runner Controller)

#### 3.1 Versions and layout [K; D RESULT "Versions", RN/README.md:8-20]
- Chart `gha-runner-scale-set` 0.15.1 (helm release `ubuntu-26-04-scale-set[-chainargos|-jackin]`), controller image `ghcr.io/actions/gha-runner-scale-set-controller:0.15.1` in ns arc-systems (1 controller pod, 3 listener pods) [K].
- Auth: GitHub App `tailrocks-arc-runners` (App ID 5771152) with secrets `github-app`, `github-app-chainargos`, `github-app-jackin` (names only) (RESULT "Runner scale set", multi-org table).

#### 3.2 Scale sets [K `kubectl get autoscalingrunnerset,ephemeralrunnerset -A`]
| Namespace | Org (githubConfigUrl) | Name | runnerGroup | min / max | runnerScaleSetLabels | EphemeralRunnerSet desiredReplicas |
|---|---|---|---|---|---|---|
| arc-runners | github.com/tailrocks | ubuntu-26.04-scale-set | velnor-ci | 4 / 60 | [velnor] | 26 |
| arc-runners-chainargos | github.com/ChainArgos | ubuntu-26.04-scale-set | velnor-ci | 4 / 60 | [velnor] | 4 |
| arc-runners-jackin | github.com/jackin-project | ubuntu-26.04-scale-set | velnor-ci | 2 / 40 | [velnor] | 2 |
- Listener config on tailrocks set: `scaler: {burst: 100, qps: 50}` [K ars.json].
- Docs show a different max for jackin (20 in LOG at 18:49Z, 40 in RESULT multi-org table); the live value is 40. Docs 35 max (RN/README.md:42) is stale vs live 60.
- Runner labels for `runs-on`: `[velnor, ubuntu-26.04-scale-set]` (RESULT "Runner scale set").
- Runner groups: tailrocks velnor-ci id 4 (velnor-new, velnor-lab, ruxel, tuiscotti, termpane), ChainArgos velnor-ci id 5 (15 private repos), jackin-project group id 4 (4 repos) (RESULT multi-org table).
- Control/release/seal jobs stay on GitHub-hosted runners by design (RESULT "Stays on hosted runners").

#### 3.3 Runner pod spec (same in all three scale sets except as noted) [K ars.json, chainargos/jackin spec]
- Runner container image `ghcr.io/tailrocks/velnor-ci-runner:2026-11-16.2`, imagePullPolicy IfNotPresent, command `/usr/local/bin/velnor-runner-start`.
- Env: `ImageOS=velnor-arc-ubuntu24`, `DOCKER_HOST=unix:///var/run/docker.sock`, `RUNNER_WAIT_FOR_DOCKER_IN_SECONDS=120`.
- Resources: requests cpu 2, memory 4Gi; limits memory 32Gi; no cpu limit; no ephemeral-storage request/limit.
- Init container `init-dind-externals` (same image) copies runner externals.
- Sidecar (init container with restartPolicy Always) `dind`: image `docker:dind`, `privileged: true`, args `dockerd --host=unix:///var/run/docker.sock --group=$(DOCKER_GROUP_GID)` (GID 123), startupProbe `docker info` period 5s x 24. No resources set on dind (so its usage counts toward node, outside the runner container's requests).
- Volumes: all four are `emptyDir: {}` (default medium, disk-backed, not tmpfs): `dind-sock` (/var/run), `dind-externals`, `work` (/home/runner/_work, shared with dind), `tmp` (/tmp in runner). No hostPath, no PVC, so dind's /var/lib/docker is on the dind container's writable layer (not a named volume in the spec).
- nodeSelector `velnor.io/runner=true`; restartPolicy Never; serviceAccount `ubuntu-26.04-scale-set-gha-rs-no-permission`. No tolerations, no affinity/topology spread, no priorityClassName in the template (50 of 60 pods report `none`) [K].
- Runner image: built from actions-runner base plus toolchain, mise tools and velnor helper seed baked in (RESULT "Runner image 2026-11-15.4"); `.2` = `.4` plus crates-mirror CA bundle only (LOG 2026-11-16T14:51:29Z).
- Observed from docs: heaviest Rust build peaked at 13.4 GiB (RESULT multi-org "Observed use"); live top shows 16.8 GiB (2.2).
- Image distribution: imported into containerd on every worker (privileged pod, `import-pod.sh`); also pushed to ghcr private package (RESULT). Live events show only "already present on machine" pulls: 0 of 1,424 Pulled events were real pulls [K events].

#### 3.4 Runner states right now [K `get ephemeralrunner -A`, `get pods -A`]
- Pods: arc-runners 40 Running + 14 in Init (4 `Init:0/2`, 10 `Init:1/2`) at first snapshot; chainargos 4 Running; jackin 2 Running; arc-systems 4; kube-system 3 + 1 Completed; velnor-system 6.
- EphemeralRunner phases (later snapshot): tailrocks 26 desired: ~21 Running (all with jobs from tailrocks/velnor-new ci.yml: 5 PR refs + merge-queue refs), 5 Pending; ChainArgos 4 Pending (idle min runners, no job); jackin 2 Pending (no job).
- Churn: in the last ~1 h window of events, 485 Scheduled and 489 "Killing ... Stopping container dind" events [K events].

### 4. Storage, caches, network, priority

#### 4.1 Storage [K `kubectl get sc,pv,pvc -A`]
- One StorageClass `local-path` (default), provisioner `rancher.io/local-path`, reclaimPolicy Delete, WaitForFirstConsumer, no expansion. local-path path: `/var/lib/rancher/k3s/storage` (configmap local-path-config).
- PVs: none. PVCs: none. All runner state is emptyDir.

#### 4.2 Cache/mirror services in cluster [K `get ds,deploy,svc -A`; D RESULT "crates.io mirror"]
- Only one: `velnor-system/crates-mirror` DaemonSet (6 pods, one per node incl. control plane via `tolerations: operator Exists`), image `nginx:1.29.3-alpine`, requests cpu 100m/mem 128Mi, limit mem 1Gi, priorityClassName system-node-critical. Cache dir hostPath `/var/lib/rancher/crates-mirror` (`proxy_cache_path ... max_size=200g inactive=60d`), HTTPS on 8443 (TLS secret `crates-mirror-tls`), Service ClusterIP 10.43.20.206 ports 80/443 (`trafficDistribution: PreferSameNode` per docs). Upstream index.crates.io and static.crates.io. The nginx cache proxies `/index/` and crate downloads.
- Docs: rolled back from default use on 2026-11-16 (image .1 broke cargo outside /home/runner); mirror still running; opt-in wiring merged in velnor-new #1002 (LOG 2026-11-17T05:21:04Z). No registry mirror, sccache, MinIO, Redis, or other cache service exists in cluster [K: only deployments listed are arc controller, coredns, local-path-provisioner, metrics-server].
- Docs-reported cache speed: GH cache restore 13.6-22.4 MB/s from pods; 120 ms RTT to US blob (LOG 2026-11-16 cache-speed probe).
- Docs-reported effect of crates mirror: `cargo fetch --locked` velnor-new (59 MB): 3.5-6.1 s direct to 2.7-3.3 s via mirror (RESULT crates section).

#### 4.3 Network [K annotations; D README]
- CNI: flannel with `wireguard-native` backend (annotations `flannel.alpha.coreos.com/backend-type: wireguard`, port 51820, `flannel-external-ip`); pod CIDR 10.42.0.0/16 (velnor-5 podCIDR 10.42.5.0/24), service CIDR 10.43.0.0/16 (services 10.43.x) (RN/PLAN.md and RESULT ufw note).
- Traefik and servicelb disabled (1.5). CoreDNS 1 replica (pod IP 10.42.0.4). metrics-server v0.9.0.
- kube-proxy mode: not observable; MTU: not observable. Host firewall: ufw on each node allows 22, peers, and 10.42/16, 10.43/16; API 6443 open only to one operator egress IP (RESULT "Operations", LOG:8-9).
- Control-plane is single node (no HA) (README:12).

#### 4.4 Priority classes [K]
Only `system-cluster-critical` (2000000000) and `system-node-critical` (2000001000). No custom classes; runner pods use none (default priority 0).

#### 4.5 Namespaces [K]
arc-runners, arc-runners-chainargos, arc-runners-jackin, arc-systems, velnor-system (crates-mirror), default, kube-system, kube-public, kube-node-lease.

### 5. Cross-site network facts (documented only)
- Pod ping velnor-241 (FI) -> velnor-240 (DE): 3/3 replies, 35 ms (LOG:9, 2026-11-15T13:10:45Z). Not re-measured live.
- Pod traffic crosses sites over the public internet, encrypted by flannel wireguard (README:13).
- 120 ms RTT from pods to the US GitHub blob store (LOG:31).
- No bandwidth, MTU, or packet-loss numbers documented or observable read-only. Site per node: only 241=FI and 240=DE are stated (LOG:9); others inferred in section 0.

### 6. Recent events [K `kubectl get events -A`]
- Events window retained: 2026-11-18T15:43:18Z to 16:44:14Z (~1 h; k3s default event TTL).
- Counts by reason: Pulled 1,424; Created 1,424; Started 1,423; Killing 489; Scheduled 485; Unhealthy 56 (type Warning count 57 total non-Normal).
- All 1,424 Pulled messages are of the form `Container image "ghcr.io/tailrocks/velnor-ci-runner:2026-11-16.2" already present on machine` (0 real image pulls in window).
- Non-Normal events: 57 `Unhealthy`, all dind startup probe failures where the probe's `docker info` output was printed (message begins "Startup probe failed: Client: Version: 29.9.3 ... "), i.e. the dind daemon not yet ready at probe time; startupProbe allows 24 x 5 s.
- No Evicted, FailedScheduling, OOMKilled, ImagePullBackOff, or node-condition events in the window.
- `Killing ... Stopping container dind` events are ordinary job-end pod teardown (about one every 10-20 s late in the window).

### 7. Discrepancies between docs and live (facts)
- README capacity (RN/README.md:36-42): 8 CPU/16 GiB per runner, ~7 runners per node, max 35. Live: requests 2 CPU/4Gi, limit 32Gi mem, max 60/60/40.
- RESULT early section says requests 6 CPU/12Gi and limit 24Gi; later section says 2/4Gi limit 32Gi; live matches the latter.
- README says all-disks-but-OS in one LV at /var/lib/rancher; live nodefs (3.78 TB) is a single-disk-sized fs separate from imagefs (sum-of-disks size).
- Runner image tag live `2026-11-16.2`; the RESULT heading "current, pinned" is `.4`.

## C. Workflows, tools, caches and tests as implemented

_Source file: workflows.md_

Facts only. Paths are relative to the repo root; `file:N` = line number. Raw helper outputs: `~/.cache/ci-ambition/evidence/wf-raw/` (jobs.txt = job table of every workflow; testcounts.txt; scripts jobs.rb/steps.rb).
Scope note: all `.github/workflows/*.yml` and `.github/actions/*` are GENERATED ("Generated by Velnor Actions 0.1.80; edit .velnor/config.toml and regenerate", e.g. `.github/actions/declared-task-0/action.yml:1`) by the existing engine `velnor-actions` (crates/velnor-actions-*). The new generator `velnor-gen` (generator/) does NOT write this repo's workflows yet (see section 6).

### 1. Workflow inventory

15 workflow files (line counts: ci 2898, state-seal 2201, qualification 1908, product-release-generator 1347, source-qualification 702, source-preview-publication 416, ...).

| Workflow (name) | Triggers | Concurrency | Jobs / notes |
|---|---|---|---|
| ci.yml (CI) | pull_request [opened, edited, synchronize, reopened, ready_for_review]; push branches [main]; workflow_dispatch (inputs campaign/phase/predecessor_*); merge_group (ci.yml:2-40) | group `velnor-${{ github.workflow }}-${{ pr.number \|\| github.ref }}`, cancel-in-progress only for pull_request (ci.yml:57-59) | 39 jobs, see below |
| state-seal.yml (State seal) | push main only (state-seal.yml:4) | group `velnor-state-seal-${{ github.ref }}`, cancel-in-progress false (state-seal.yml:29-31) | 17 jobs, all hosted `ubuntu-26.04`; cache producers (section 3) |
| cache-prune.yml | workflow_run of CI and State seal, completed, branches main | group `velnor-cache-prune`, no cancel | 1 job `prune`, 5 min, hosted |
| source-qualification.yml | pull_request (same types as ci) + workflow_dispatch | group `source-qualification-${{ github.ref }}`, no cancel | plan(15m) -> selection(90m) -> ordinary-harness / keep-harness / generation-checks (90m each) -> source-qualification (90m); all hosted ubuntu-26.04. Runs `scripts/generator-release/source-qualification.sh` steps (installs pinned Rust+MBX, prefetches locked sources). Not a required check |
| job-image-candidate.yml | push (all branches/tags: `push: {}`) + workflow_dispatch | `velnor-${{ github.workflow }}-${{ github.event_name }}-${{ github.ref }}`, cancel-in-progress only for push | build-image-candidate(10m) -> attest(10m) -> publish-job-image(5m, dispatch only) |
| freshness.yml | schedule `0 6 * * 1` + dispatch | group `freshness` | 1 job, 5 min |
| release-rust-ci-tools.yml | schedule `23 4 * * *`; dispatch; push tags `rust-ci-v*` | group `${{ github.workflow_ref }}` | tag-ruleset-drift, gate, build (matrix.runner), checksums, release; labels `ubuntu-24.04` (+ matrix); builds velnor-ci/velnor-gen binaries |
| product-release.yml | workflow_dispatch | group `${{ github.repository }}-product-release` | release-eligibility -> prepare-{images,binary,generator} -> reusable workflow_call jobs |
| product-release-generator.yml | workflow_call; push main and tags `sdk-preview-*` | - | build/qualify/attest for linux (ubuntu-26.04), macos-15, macos-15-intel; 5-30 min timeouts; source-preview-* jobs on push |
| product-release-binary.yml / -images.yml | workflow_call | - | macos-15 build/attest/publish; ubuntu-26.04 image build (30m) |
| homebrew-adoption.yml, source-preview-{publication,runtime,mac-proof}.yml | workflow_call | - | verify/publish jobs; mac-proof verify has a 140 min timeout |
| qualification.yml | workflow_dispatch (input mode) | - | ~70 jobs in hosted/scale-set pairs, 5-30 min, selected by `inputs.mode` (qualifies velnor runner vs GitHub-hosted) |

#### ci.yml job graph (the PR/merge-queue path)
No trigger-level `paths:` filters. Selection is done by the `plan` job's output `not_affected_jobs`, tested in each job's `if:`.

| Job | runs-on | timeout | needs | Notes |
|---|---|---|---|---|
| plan | `ubuntu-26.04` (GitHub-hosted) | 15 | - | 20 steps: checkout (fetch-depth 0 on PR, 1 on push), mise, verifies Rust toolchain, computes cache keys, selects attested state, `velnor-actions-0.1.80` plan-v1, uploads plan artifact (ci.yml:367-512) |
| alint | ubuntu-26.04 | 5 | - | asamarts/alint v0.21.0 |
| cargo-deny | ubuntu-26.04 | 5 | - | `cargo deny` x5 manifests (root, ci-helper, runner, generator, local) |
| cargo-machete | ubuntu-26.04 | 5 | - | cargo-machete 0.9.2 via mise `http:` backend with sha256 |
| jactionlint | ubuntu-26.04 | 5 | - | lints .github/workflows + actions |
| ledger-cli | ubuntu-26.04 | 10 | - | 18 steps; builds `velnor-actions` (`cargo build -p velnor-actions-cli`) once, uploads artifact consumed by every rust job |
| check-generator-samples | `["velnor","ubuntu-26.04-scale-set"]` | 5 | plan | `lint-generator-samples` mise task; if not_affected skip AND (push, merge_group, or same-repo PR) |
| check-maintenance-helpers | scale-set | 15 | plan | mise task `test-maintenance-helpers` (runs nextest for velnor-actions-freshness, velnor-archive-guard, bun test, python tests) |
| rust-* (29 jobs) | scale-set `["velnor","ubuntu-26.04-scale-set"]` | 45 each | ledger-cli, plan | one job per Cargo package (+ `rust-velnor-runner-host-local-mode` = features local-mode config). No matrix. 30-36 steps each |
| required ("Required verdict") | ubuntu-26.04 | 15 | plan + all check-*/rust-* | `if: !cancelled()`; uses velnor-actions **0.1.78** (older released helper, ci.yml required job step 3) to merge reports |
| required-gate ("Required") | ubuntu-26.04 | 15 | required | `if: always()`; fails unless `required` succeeded |

rust-* jobs: velnor-actions-{actionlint,ansible,bun,cli,contract,dbt,gradle,mise,orchestrator,rust,tofu,workflow-renderer}, velnor-cache-rules, velnor-ci, velnor-gen, velnor-local, velnor-model, velnor-release, velnor-render, velnor-resource-probe, velnor-runner-{attestation,cli,core,github,host,host(local-mode),image-manifest,qualification}, velnor-rust. (Packages with no tests of their own, e.g. freshness/archive-guard, are covered by the maintenance-helpers check, ci.yml `check-maintenance-helpers` / .velnor/mise.toml tasks.)

Step layout of a rust-* job (e.g. rust-velnor-runner-host, ci.yml:~1700-1900): checkout (sparse: `/*` minus `docs/**`) -> verify non-input paths absent -> Setup Mise (jdx/mise-action, install=false) -> resolve tool versions -> Acquire Velnor helper (from `/opt/velnor/seed/generator/` if present on runner, else download+sha256) -> download plan -> verify restored Rust toolchain -> `mise install rust mr-boxington jactionlint shellcheck cargo-nextest opentofu crane bun` -> rustup component add clippy rustfmt -> verify channel manifest -> restore Cargo sources (actions/cache/restore) -> jdx/mr-boxington-action (backend local) -> compute state key -> restore state snapshot -> `mbx cache import` ("Recover state snapshot") -> `cargo fetch`-equivalent per root -> then 5 `declared-task-0` steps: **Build test executables** (sequential), then in a parallel group (GitHub `background: true` steps joined by "Wait for parallel checks"): **Format, Clippy, Unit and integration tests, Documentation** -> MBX stats -> ledger -> upload crate reports -> `mbx clean`. (docs/implemented/native-rust-crate-jobs.md:8-20.) Doctests step appears only when a crate has doctests (hard-decisions 11).

Required checks:
- Repository ruleset `protect-main` (id 24396608, active; `gh api repos/tailrocks/velnor-new/rulesets/24396608`): rules deletion, non_fast_forward, required_linear_history, pull_request (0 approvals, squash only, thread resolution required, dismiss stale reviews), **required_status_checks = ["Required verdict", "Required"]**, strict_required_status_checks_policy=false, **merge_queue** (SQUASH, max_entries_to_build 2, min_entries_to_merge 1, max 10, min wait 5 min, grouping HEADGREEN, check_response_timeout 90 min).
- Classic branch protection: none (404 "Branch not protected"). Other rulesets: protect-tags, release-tags, sdk-preview-owner-creation (tag rulesets).
- Required verdict job names = jobs `required` and `required-gate` in ci.yml. Required demands every crate job the plan schedules (hard-decisions.md decision 1/3).

Runner pools: GitHub-hosted `ubuntu-26.04` for plan/lint/audit/state producers/releases; self-hosted Velnor scale set labels `["velnor","ubuntu-26.04-scale-set"]` for all rust-* and check-* jobs (.velnor/config.toml:271-280). Config also pins `max_parallel_jobs = 2`, `crate_timeout_minutes = 45`, `plan_timeout_minutes = 15`, `compiler_process_budget = 2`, `test_process_budget = 2` (.velnor/config.toml:6-27). Note `[[checks]]` entries declare `executor = "hosted"` / label ubuntu-26.04 (config.toml:~960-990) while the generated `check-*` jobs carry scale-set labels.

### 2. Actions and tools

#### Actions (SHA-pinned; versions from .velnor/version-policy.toml `[[actions]]`)
| Action | SHA | Version | Uses (count in workflows+actions) |
|---|---|---|---|
| actions/checkout | 3d3c42e5aac5 | v7.0.1 | 61 |
| actions/upload-artifact | cf430e030ddb | v7.0.2 | 80 |
| actions/download-artifact | 9000827ccba6 | v8.0.2 | 72 |
| actions/cache/restore | 55cc8345863c | v6.1.0 | 18 |
| actions/cache/save | 55cc8345863c | v6.1.0 | 5 (+2 `actions/cache`) |
| jdx/mise-action | f5be966e4a91 | v5.2.6 (installs mise 2026.11.1, sha256-pinned, cache=false, install=false) | 55 |
| jdx/mr-boxington-action | 88acf2bb5055 | v1.7.3 (`backend: local`, toolchain=rust pin, version=MBX pin) | 12 |
| actions/attest-build-provenance | 4d101475d8b2 | v4.2.2 | 8 |
| actions/attest | 1e69f48acb82 | v4.2.2 | 1 (state seal attestation) |
| actions/github-script | 3a2844b7e9c4 | v9.0.0 | 3 |
| asamarts/alint | d61e4c802b02 | v0.21.0 | 1 |
| local composites | - | `.github/actions/declared-task-0` (279 uses), velnor-tools-prelude-u26, velnor-tools-cache-restore/-scrub, velnor-tool-seed, velnor-verify-{restored-rust,rust-manifest,jactionlint}, u26, generator-release-* | |
Also `tailrocks/velnor-new/actions/setup-velnor@46f7a043...` and a pinned reusable workflow `source-preview-publication.yml@72701af6...`. version-policy also lists aws-actions/configure-aws-credentials v6.3.0, actions/create-github-app-token v3.3.0 (not in current workflows).

#### Tools
| Tool | Version | Source |
|---|---|---|
| rust (rustc/cargo) | 1.99.1, profile minimal, components clippy+rustfmt, targets aarch64/x86_64-unknown-linux-gnu | mise.toml:`rust`; rust-toolchain.toml; .velnor/rust-toolchain.toml |
| mr-boxington (`mbx`) | 1.29.2 (`workflow = "release.yml@refs/heads/main"`); local MBX wraps cargo (`[wrappers.cargo] command="mbx"`, env MBX_CARGO_SHIM_MODE=1) | mise.toml, .velnor/mise.toml |
| cargo-nextest | 0.9.151 (`aqua:nextest-rs/nextest/cargo-nextest`) | mise.toml |
| cargo-deny | 0.20.2 | mise.toml; ci.yml cargo-deny job |
| cargo-machete | 0.9.2 (tarball URL + sha256 via mise `http:` backend) | ci.yml cargo-machete job |
| jactionlint | 2.4.0; shellcheck 0.11.0 | mise.toml, .velnor/mise.toml |
| bun 1.4.4, opentofu 1.13.8, crane 0.22.1, gh 2.103.3, release-plz 0.4.3, cosign 3.1.4 | | .velnor/version-policy.toml `[tools]` |
| hk 2.8.0, cargo-binstall 1.26.0, kingfisher 2.14.0, betterleaks 1.9.0, gitleaks 8.30.1, rumdl 0.3.0 | local hooks | mise.toml |
| mise | 2026.11.1 (`.mise-version`; min_version 2026.10.18) | |
| velnor-actions helper | 0.1.80 (commit b71a0045), assets sha256-pinned for linux-x64, mac arm64/x64 | .velnor/generator.lock; `required` job uses 0.1.78 |
| GitHub runner (self-hosted image) | actions/runner 2.338.0, base ubuntu:26.04@sha256:4252..., protocol actions/scaleset e6daac70 | .velnor/runner.lock |
| sccache | NOT present (no match for sccache/RUSTC_WRAPPER anywhere in .github, .velnor, Cargo manifests) | grep |
| blaze | NOT present: no workflow, config, mise tool or doc names a "blaze" tool (only incidental words in freshness-evidence response bodies). Therefore also not configured for the pinned toolchain | grep -w blaze |

#### mbx (Mr. Boxington) usage
- `jdx/mr-boxington-action` "Set up MBX" per rust job with `working-directory` = the Cargo root (`.`, `crates/velnor-runner`, `ci-helper`, `generator`, `local`), env `MBX_CACHE_DIR=${{ runner.temp }}/velnor/mbx`, CARGO_HOME/RUSTUP_HOME under `$RUNNER_TEMP/velnor/`.
- Job env (all rust jobs): `MBX_BUILD_SCRIPT_EXECUTION=1`, `MBX_CACHE_EXPORT_GROUP=velnor`, `MBX_CC=0`, `MBX_GC_AUTO=0`, `MBX_SHARE_OUT_DIR=1`, `MBX_SUMMARY=ci`, `SOURCE_DATE_EPOCH=0`, `RUSTUP_AUTO_INSTALL=0`. Declared-task action sets `MBX_TARGET_VIEWS=0`. Compat key uses `MBX_BUILD_SCRIPT_EXECUTION=hybrid`.
- Commands: tasks execute cargo through mbx shim (`cargo nextest run --locked --offline --profile ci --manifest-path ... --package ...`, built by crates/velnor-actions-rust/src/argv.rs:87-200); `mbx cache import` (recover) and `mbx cache export --group velnor --format directory` (producer only); `mbx clean` at job end; mise task `check-generated-workflow-freshness` runs `mbx --config build.jobs=2 run --locked --package velnor-actions-cli --bin velnor-actions -- generate` (.velnor/mise.toml).
- MBX remote cache: none; backend is `local`; sharing across runs is only through GitHub Actions cache entries holding an opaque `mbx cache export` bundle (docs/proposed/cache-families.md:83-124).
- Target dir: `<workspace>/target` with lanes `target/velnor-clippy`, `velnor-doctest`, `velnor-doc` (docs/implemented/native-rust-crate-jobs.md:55-62); `CARGO_TARGET_DIR` only workspace-relative `target/velnor-<lane>`.
- Repo-local `target` symlink on dev machine points to `~/Library/Caches/mbx/targets/v1/<hash>` (observed).

### 3. Cache design as implemented

Budget: GitHub 10 GB repo cache; accounting ceiling 10e9, operating budget 8e9, reserve 2e9 (crates/velnor-actions-contract/src/config/workflow_cache_policy.rs:11,18,25; docs/proposed/compiled-state-persistence.md "Budget"). Workflow-level `cache-mode: read` in every workflow; only named writer jobs have job-level `cache-mode: write` (state-seal.yml:9,242, 1719 `write-only`). Decisions: docs/proposed/cache-decisions.md (5: no PR-scope writes; pull requests, forks, merge groups never write cache).

| Family | Key / pattern | Path | Restore | Save | Ceiling / retention |
|---|---|---|---|---|---|
| Cargo sources | `velnor-v2-sources-trusted-s256-<compat>-s256-<snapshot>`; compat = sha256 of {family=sources, schema=2, paths, platform, CARGO_HOME}; snapshot = sha256 of sorted `<root>\t<sha256(Cargo.lock)>` for the 5 lockfiles (root, ci-helper, crates/velnor-runner, generator, local) (ci.yml plan step "Compute cache keys", ci.yml:325-330) | `$RUNNER_TEMP/velnor/cargo/registry/index`, `registry/cache`, `git/db` | `actions/cache/restore` key = exact; restore-keys = `velnor-v2-sources-trusted-<compat>-` prefix; skipped on workflow_dispatch | only `sources-producer` job in state-seal.yml (push to protected default branch, cache-mode write, 15 min, only when plan reports key missing) | 128 MiB entry, 2 retained; measured 16.8 MiB |
| Mise tools | `mise-tools-v2-<identity>` (identity from `.github/actions/u26`, bound to tool pins + runner image) | mise installs dir | `velnor-tools-cache-restore` composite in hosted jobs (cargo-deny, machete, jactionlint, ledger-cli, state producers); scale-set jobs use runner-image seed `/opt/velnor/seed/...` | `tools-producer-{06f9acb0,924ced60,c957c0bd,caf63d48}` in state-seal.yml (needs sources-producer; 5-10 min) | allowance 1.5 GiB; measured 3.46 GB in 16 entries on 2026-11-03 (pre-prune); every restored toolchain re-verified (decision 10: "Verify restored Rust toolchain" with pinned tree digest, "Verify Rust channel manifest") |
| State (compiled-state bundle, MBX) | `velnor-v4-state-trusted-union-<root>-s256-<compat>-s256-<snapshot>` where root in {root, local, ci-helper, generator, crates-velnor-runner}; compat = sha256 of {family=state, schema=4, unit=union, mbx version, rust toolchain, root, MBX_SHARE_OUT_DIR=1, MBX_CC=0, SOURCE_DATE_EPOCH=0, MBX_BUILD_SCRIPT_EXECUTION=hybrid, uname -s/-m, ImageOS, store path, checkout path}; snapshot = sha256{lock hash, rust_inputs = sha256 of `git ls-files -s` over that root's Rust input globs, task_set digest, producer_recipe digest} | `$RUNNER_TEMP/velnor-gc1/transport/state/payload.tar.zst` (tar + zstd -3 --long=30) | in each crate job: `actions/cache/restore` of the one `restore_key` chosen by plan; no `restore-keys`; then `mbx cache import`. Disabled when `ImageOS` empty or snapshot missing | `state-producer-<root>` (60 min; builds with real tasks twice: skeleton at MBX_BUILD_SCRIPT_EXECUTION=1, then real members at 0) -> `state-seal-<root>` (15 min, chained root<-local<-generator<-crates-velnor-runner<-ci-helper) | 1.5 GiB stored / 12 GiB extracted; 1 retained per root; stored bound 1,616,920,640 B; 3-min save budget; save only while commit is still tip of main |
| Bun/dbt/ansible/tofu/playwright | defined in docs/proposed/cache-families.md; not used by velnor-new's own workflows (no Bun suite jobs in ci.yml) | | | | |
| Plan/report artifacts | `velnor-plan-r<run>-a<attempt>`, `velnor-crate-...`, `velnor-ledger-cli-...` (upload-artifact, retention 30 d) | | | | not cache |

State seal mechanism (why it depends on main):
1. Cache `trust` = `trusted` only when `GITHUB_EVENT_NAME=push` AND `GITHUB_REF == refs/heads/<default_branch>` AND `GITHUB_REF_PROTECTED == true`; otherwise `none` -> save=false (ci.yml plan "Compute cache keys"; per-job "Compute state snapshot key" sets provenance trusted/untrusted(pull_request)/ambiguous, decision admitted/restore-only).
2. `state-seal.yml` runs only on push to main (state-seal.yml:4). Producer P (state-producer-<root>) builds, exports bundle, uploads it as an artifact; sealer A (state-seal-<root>): validates artifact id, downloads payload, writes subject, **checks the state tip** (`gh api repos/.../git/ref/heads/main` must equal `GITHUB_SHA`, else "cache_unavailable: state seal tip check refused" and no save), admits (size/budget), `actions/cache/save`, binds, **`actions/attest` with predicate type `https://velnor.dev/attestation/state-seal/v1`**, then retires superseded entries.
3. Consumers (plan job step "Select attested state"): lists caches for the prefix on `--ref refs/heads/main` (up to 100, newest first, size <= 1,616,920,640) and for each candidate runs `gh attestation verify --signer-workflow <repo>/.github/workflows/state-seal.yml --source-ref refs/heads/main --predicate-type ...state-seal/v1`; first 3 verified are kept as `state_union_<root>_attested`. A crate job restores only an attested key with the same compat prefix (newest attested, which may differ in snapshot from the PR's current snapshot -> `exact=false`; `mbx cache import` then reuses object-by-object). If no attested entry exists the job runs cold.
4. Hence: the bundle that PRs read is only ever written by a main push after that push's State seal run completes (job chain of up to 5 producers, 60 min each, plus sealers, serialized by group `velnor-state-seal-<ref>` with cancel-in-progress false; a newer push replaces the pending run, so intermediate commits can have no seal). PR runs never write cache; GitHub scoping lets PRs read only their own ref and default-branch entries. Plan also emits `state_union_<root>_produce` (false when an attested entry with the exact snapshot already exists) so unchanged roots are skipped (`producer_skip`).
5. Main red does not starve cache: State seal is a separate workflow (cache-decisions.md "Cases"). Producer failure => previous entry stays.
6. Prune: cache-prune.yml `workflow_run` after CI / State seal on main (actions: write only).

Toolchain caching: Rust installed by mise (`rust[profile=minimal]@1.99.1`) into `$RUNNER_TEMP/velnor/rustup`; on hosted jobs restored from `mise-tools-v2-*` entry then verified; on scale-set runners seeded from the image (`/opt/velnor/seed`, `.github/actions/velnor-tool-seed`). Tools cache is also written by main-only producers.
Target dir caching: no `target/` payload any more ("`github-cache-mode: target` is removed", cache-decisions #3); compiled state moves only as `mbx cache export` bundle. Known gaps recorded in docs: `docs/implemented/cache-save-budget.md`, `cache-measurements.md` (e.g. restore step 10-14 s/job when no entry).
Cargo registry mirror: `.velnor/rust-ci.toml` `[crates_mirror]` registry `sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/`, ca `/opt/velnor/crates-mirror/ca-bundle.pem` (consumed only by the NEW generator; engine workflows ignore it, generator/CONFIG.md).

### 4. Change-awareness already present
- Affected selection on pull_request only: plan classifies every crate job `run` / `not_affected` from the merge-base diff; job-level `if: !contains(needs.plan.outputs.not_affected_jobs, ',<job>,')` (hard-decisions.md decision 3; docs/implemented/affected-selection.md:14-36). push, merge_group, workflow_dispatch run everything (`selects_affected_work` in crates/velnor-actions-contract/src/workflow/plan_jobs.rs; "Event policy" table). `pull_request` types include `edited` so a base retarget re-plans.
- Mapping rules in `.velnor/config.toml` `[[stacks.rust.inputs]]` (lines 42-240): glob -> packages, `complete = true` meaning only declared inputs select the package. E.g. `crates/velnor-runner/crates/velnor-runner-host/**`, `.velnor/runner.lock`, runner Cargo.toml/lock -> velnor-runner-host; `local/**` -> velnor-cache-rules, velnor-local; `ci-helper/**` -> velnor-ci; `generator/**`, `release-rust-ci-tools.yml` -> 5 generator crates; `.github/workflows/**`, `.velnor/config.toml`, release scripts -> velnor-actions-workflow-renderer; a last rule maps `.config/**`, `.github/**`, `.velnor/*` etc. Dependents of changed crates are also selected (affected-selection.md "Dependency graph"). Unowned file -> all (stack-wide); tool files (mise.toml, rust-toolchain.toml) force packages.
- `non_inputs = ["docs/**"]` (config.toml `[stacks.rust]`): docs-only PRs run no Rust job; every Rust job sparse-checkouts `/*` minus `/docs/**` and fails if any docs path exists (`non_input_present`).
- Named checks (`[[checks]]`: maintenance-helpers, generator-samples) have declared `inputs` lists and are skipped as whole jobs when unaffected; they also skip for fork PRs.
- Plan output `state_union_*_produce` avoids rebuilding state bundles when an attested entry with the exact snapshot exists (state-seal.yml plan `Select attested state`).
- Required verdict rejects any `not_affected` skip on a full-run event (affected-selection.md:27-31).
- Not present: path filters on triggers; test-level selection (tests of a selected package all run); `cargo nextest --partition` sharding is supported (`hash:i/n`, argv.rs) but `[test_sharding] default_shards = 1` (config.toml:29-30), so unused; workspace-wide compile for lint/test-build (hard-decisions.md decision 11 applies to velnor-gen output).

### 5. Test suites
- Runner: `cargo nextest run --locked --offline --manifest-path <root>/Cargo.toml --package <pkg> --profile ci --no-tests fail` (generated argv, crates/velnor-actions-rust/src/argv.rs:87-130, 133-160). Build step: `cargo nextest list --list-type binaries-only` (the "Build test executables" step). Clippy: `cargo clippy --locked --offline --manifest-path --package --all-targets`. Fmt: `cargo fmt --all --check`. Doc: `cargo doc --locked --offline --no-deps`. Doctests: `cargo test --doc` only if the crate has doctests.
- Repo's dogfood entrypoint: `.mise/tasks/test` -> `cargo nextest run --locked --profile ci --no-tests fail "$@"`.
- Nextest config per workspace:
  | File | profile ci | Notable |
  |---|---|---|
  | .config/nextest.toml | retries 0, test-threads num-cpus, status-level all | slow-timeout overrides for orchestrator `runner_image_build_public` (60s x10) and archive-guard golden tests (threads-required = num-test-threads); priorities 100/90/80 on the 3 slowest orchestrator tests (measured 41.8 s, 21.3 s, 14.9 s on "main a85") |
  | ci-helper/.config/nextest.toml | test-threads 1 | default slow-timeout 2s terminate-after 1; crates_mirror tests exclusive |
  | generator/.config/nextest.toml | num-cpus | wrapper `env PATH=/nonexistent` on all tests (no program lookups); default slow-timeout 2 s kill; 10 s overrides for real-git/cargo tests |
  | local/.config/nextest.toml | num-cpus | docker wrapper for `removal_audit::` tests on Linux |
  | crates/velnor-runner/.config/nextest.toml | num-cpus | test-group `docker-networks` max-threads 4 for `local_host_public` / `local_pair_public` binaries (docker via unix socket) |
- Sharding/partition: none in use (default_shards = 1; `.velnor/config.toml:29-33`). Compile parallelism capped by `compiler_process_budget = 2`, `test_process_budget = 2`, `--jobs 2` / `build.jobs=2` in maintenance tasks. Per-job parallel group (fmt/clippy/tests/doc as background steps) is the only intra-job concurrency; `max_parallel_jobs = 2`.
- Size: 5 Cargo workspaces, 30 packages (root 14, crates/velnor-runner 8, ci-helper 1, generator 5, local 2); root workspace excludes runner/ci-helper/generator/local (Cargo.toml:21). ~3,657 `#[test]`/`#[tokio::test]` attributes (grep count; excludes macro-generated tests). Heaviest by tests / LOC:
  | Package | #[test] | Rust LOC | tests/ entries |
  |---|---|---|---|
  | velnor-ci (ci-helper) | 781 | 40.6k | 1 (`it`) |
  | velnor-actions-orchestrator | 631 | 186.8k | 109 |
  | velnor-gen | 627 | 27.4k | 1 (`it`) |
  | velnor-runner-host | 446 | 171.1k | 26 (docker `local_host_public`, `local_pair_public`, image/scaleset/qualification suites) |
  | velnor-actions-workflow-renderer | 317 | 88.7k | 177 |
  | velnor-actions-cli | 226 | 29.6k | 44 |
  | velnor-local | 225 | 16.4k | 1 |
  | velnor-release | 149 | 6.8k | 1 |
  | velnor-actions-contract | 63 | 47.3k | 20 |
  | velnor-archive-guard | 55 | 11.5k | 10 |
  | others | <40 each | | |
  Total Rust LOC across packages about 700k (orchestrator + runner-host + renderer = 446k).
- CI job time limits: rust-* 45 min, check-maintenance-helpers 15, plan 15. The generator target "own tests under 1 minute" (docs/goals.md Targets table). No recorded per-job wall times are in the repo workflow files (docs/implemented/cache-measurements.md holds older measurements).

### 6. Generator: inputs -> outputs (where to change job layout)
Two generators coexist (docs/goals.md "What this file governs"):
1. Engine `velnor-actions` (crates/velnor-actions-cli -> orchestrator -> workflow-renderer; contract/rust/mise/actionlint/freshness crates) writes THIS repo's `.github/workflows/*.yml`, `.github/actions/*`, `.github/velnor/*.json`.
   - Input: `.velnor/config.toml` (schema 2: [workflow], [resources], [test_sharding], [stacks.rust] inputs/configurations/release, [execution] profiles hosted/local, [[checks]], [[qualified_tools]]), plus `mise.toml`, `rust-toolchain.toml`, Cargo metadata, `.velnor/version-policy.toml`, `runner.lock`, `generator.lock`.
   - Output: workflows above. Command: `velnor-actions generate --output-dir <dir>`; freshness gate: mise task `check-generated-workflow-freshness` diffs generated vs checked-in (.velnor/mise.toml); Plan job also fails on drift. The generator binary used is the released one pinned in `.velnor/generator.lock` (0.1.80), downloaded with sha256 (bootstrap).
   - Job layout knobs: `[stacks.rust.inputs]`, `[[stacks.rust.configurations]]` (default; `local-mode` for velnor-runner-host), `[workflow] crate_timeout_minutes / max_parallel_jobs / pull_request_cache_policy`, `[resources]`, `[execution.profiles]`, `[[checks]]`. Shape (one job per crate with parallel check steps) is fixed by hard-decisions.md decision 1; renderer code in crates/velnor-actions-workflow-renderer and orchestrator (crates/velnor-actions-orchestrator, 186k LOC).
2. New generator `velnor-gen` (generator/ workspace: velnor-model, velnor-rust, velnor-render, velnor-gen, velnor-release) with helper `velnor-ci` (ci-helper/crates/velnor-ci; subcommands include plan, prepare, gate, report, cache, restored, check-metadata, check-configurations, inputs, bench, timings, run, release*, crates-mirror).
   - Input: `.velnor/rust-ci.toml` (here: `runner = "ubuntu-26.04-scale-set"`, `exclude = ["fixtures/**"]`, `[crates_mirror]`), cargo metadata, `.velnor/rust-ci.lock` (pinned helper release), repo tree. Commands: `generate`, `check`, `pin-helper`, `check-tag`, `release`, `record-sample`, `ruleset-drift` (generator/crates/velnor-gen/src/cli.rs).
   - Output: `.github/workflows/rust-ci.yml` (+ release.yml). Not yet present in this repo's `.github/workflows`; the expected output for this repo is the golden `generator/samples/velnor-new/expected/.github/workflows/rust-ci.yml` (1713 lines): jobs rust-plan, rust-inputs-audit, rust-root, rust-ci_helper, rust-crates_velnor_runner, rust-generator, rust-local (one job per Cargo root, 30-min timeouts, `ubuntu-latest` in the sample), rust-trim, required, cache. 
   - Golden tests: `generator/crates/velnor-gen/tests/it/golden.rs`; update with `cd generator && UPDATE_GOLDENS=1 cargo nextest run` (generator/TESTING.md). 20 samples under generator/samples. Doc says "This repository runs on its own generator, and that is the final proof" (goals.md rule 12) but the repo's CI is still the engine ("until the generator takes over the Rust part").

### 7. Rust toolchain pin, flags, profiles, linker
- Toolchain: **1.99.1** (channel) in `rust-toolchain.toml:[toolchain] channel`, `.velnor/rust-toolchain.toml`, `mise.toml` rust, `.velnor/mise.toml` task, version-policy `[tools] rust`; profile minimal; components clippy, rustfmt; targets aarch64-unknown-linux-gnu + x86_64-unknown-linux-gnu. Hard decision 4: tool versions come from project mise config; generation fails if rust-toolchain.toml and mise.toml disagree (`rust_toolchain_mirror_mismatch`); toolchain file counts as a Rust declaration and `.velnor/rust-toolchain.toml` mirrors it. Workspace `rust-version = "1.99"`, edition 2024, resolver 3, version 0.1.80 (Cargo.toml).
- Rustc flags: none set anywhere (no RUSTFLAGS, `rustflags`, `[build]` or `[target.*]` in repo; there is no `.cargo/config.toml` at root, ci-helper, generator, local or runner; `.cargo/` holds only `mutants.toml`). No linker configuration (default `cc`/system linker; no lld/mold). No sccache/RUSTC_WRAPPER. Env set in CI: `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_DEV_STRIP=symbols` only in the ledger-cli build step (ci.yml:346-348); per-package `CARGO_BUILD_JOBS` via `build_jobs` option (unset here).
- Cargo profiles: root Cargo.toml only `[profile.test.package."sha2:0.11.1"] opt-level = 3` and the same for `profile.dev.package` (Cargo.toml:69-73). crates/velnor-runner/Cargo.toml:74 `[profile.dev.package.turso_core] opt-level = 1`. ci-helper and generator: `[profile.release]` opt-level "s", lto "thin", strip true, panic "abort" (ci-helper/Cargo.toml:13-17, generator/Cargo.toml:35-39). Tests/CI use the default dev/test profile (debuginfo default unless MBX/env overrides), not release.
- Workspace lints: `unsafe_code = "forbid"`, clippy pedantic warn plus deny on unwrap/expect/panic/todo/too_many_lines etc. (Cargo.toml `[workspace.lints]`), so clippy runs with `--all-targets` and fails on those.
- Pinned dependency versions (`=x.y.z`) in `[workspace.dependencies]`; Cargo.lock per workspace; all cargo calls use `--locked --offline`.

### Cross-checks / things not found
- No required-check list in repo files beyond generated ruleset logic; live ruleset read via gh API (read-only).
- Merge-queue runs execute the full ci.yml (event `merge_group` = full mode, no skip) and also state does not save there (merge groups read-only).
- HANDOFF.md/.audit/state/ untracked files in the working tree were not used.

## D. velnor-new runner code (crates/velnor-runner)

_Source file: runner-new.md_

Source: tailrocks/velnor-new, main at 95de9b704b (read 2026-11-18/19). Paths are relative to the repo root unless absolute.
Build/test copy: ~/.cache/ci-ambition/evidence/vn-copy (git archive HEAD), target dir ~/.cache/ci-ambition/evidence/vn-target.
Logs: ~/.cache/ci-ambition/evidence/vn-build.log, vn-test.log. Host: macOS arm64 (Darwin), Docker daemon reachable (`docker info` ok) during the run.

### 1. Where the runner code lives

- `crates/velnor-runner/` is a nested virtual Cargo workspace, excluded from the root workspace (`Cargo.toml:21`: `exclude = ["crates/velnor-runner", "ci-helper", "generator", "local"]`).
- Its own `Cargo.toml` members (crates/velnor-runner/Cargo.toml:4-13): velnor-runner-core, -qualification, -github, -host, -image-manifest, -cli, velnor-resource-probe, velnor-runner-attestation (8 crates).
- Toolchain: rust 1.99.1 (rust-toolchain.toml), edition 2024; lints forbid `unsafe_code`, deny unwrap/expect/panic/todo (Cargo.toml:`[workspace.lints]`).
- Other runner-related code outside that workspace:
  - `local/` (separate workspace, members `velnor-cache-rules`, `velnor-local`; local/Cargo.toml:5) = "velnor-local", the local-GitHub dev environment.
  - The root `crates/velnor-actions-*` (orchestrator, contract, renderer, cli...) are the older generator engine, not the runner. docs/goals.md:12 says the engine "is maintained, not developed".
  - `images/` (runner/dind/probe image recipes), referenced by local/README.md ("images/runner/ubuntu-26.04/README.md").
- Authority docs: docs/proposed/macos-scaleset-runner.md (324 lines, "Active specification"), docs/deferred/self-hosted-runner.md ("Historical / Superseded", line 3), docs/proposed/runner-resource-admission.md, resource-probe-controller.md, docs/implemented/macos-scaleset/{checklist,decisions,evidence,recovery-evidence,verification,...}.md.

### 2. Size (tokei, Rust code lines; `tokei <dir> -t Rust`)

| Crate | Rust files | Code LOC | `#[test]`/`#[tokio::test]` attrs (grep) |
|---|---|---|---|
| velnor-runner-host | 1002 | 156,822 (171,037 total lines) | 327 |
| velnor-resource-probe | 72 | 10,142 | 27 |
| velnor-runner-github | 51 | 6,878 | 18 |
| velnor-runner-cli | 23 | 3,546 | 7 |
| velnor-runner-qualification | 17 | 3,376 | 7 |
| velnor-runner-image-manifest | 15 | 2,348 | 3 |
| velnor-runner-attestation | 14 | 1,611 | 0 |
| velnor-runner-core | 8 | 719 | 0 |
| **crates/velnor-runner total** | **1202** | **185,442 (203,111 lines)** | 389 attrs |
| local/ (velnor-local 14,982 + velnor-cache-rules 638) | 96 | 15,620 | n/a |
Note: nextest actually ran 430 tests (attr grep undercounts, e.g. macro-generated tests).
For scale (raw `wc`-style line counts incl. blanks): root `velnor-actions-orchestrator` 186,799; `velnor-actions-contract` 47,277; `velnor-actions-workflow-renderer` 88,664 (older engine, not runner).

### 3. Crates: purpose, modules, binaries

Per macos-scaleset-runner.md section 2 (lines ~55-75) ownership table, plus code inspection.

- **velnor-runner-core** (719 LOC, deps: thiserror only). IDs, capacity, lifecycle state machine, ownership/cleanup proof types, evidence errors. Modules: capacity, error, evidence, identity, lifecycle, ownership, paths. `WorkerState` variants: Reserved, Acquiring, AcquisitionUncertain, Provisioning, IdleAssignable, Running, Finishing, Cleaning, Quarantined, Released (crates/.../velnor-runner-core/src/lifecycle.rs:9-75, transitions 214-265). `CleanupProof::covers` requires non-empty container id equal and same volume (ownership.rs:~30).
- **velnor-runner-github** (6,878 LOC; deps serde, serde_json, base64, sha1_smol, sha2, zeroize, core; NO http client dependency, so it is pure wire types/paths/state). Modules: acquire, error, paths, poll, refresh, registration (admin, groups, runners, scale_set, token), runner, secret, session (open, reopen, acknowledge, close, jobs, messages, retry, request), workflow_run. Implements the Actions Scale Set wire protocol (spec cites `actions/scaleset` commit e6daac70, macos-scaleset-runner.md section 4).
- **velnor-runner-host** (156,822 LOC; the bulk, ~85% of the workspace). Deps include bollard 0.21.1 (Docker), turso 0.8.3 (embedded SQLite-compatible journal), reqwest 0.13.5 (native-tls), hyper 1.12, tar, flate2, rustix, libc, zeroize. Top-level modules (src/): config, connect, assign, listen, launch (gate, inspect, retirement, completion, bootstrap, session, steady, turn, verified_images, jit_conflict, resource_probe, engine_context, admission_gate...), journal (+ journal_schema, journal_sql, ~35 schema modules under journal/schema/), reconcile, readiness(+scan), host_controller, controller_ipc, daemon_lock, keychain, plist (launchd), docker_client, docker_spec, worker (names, volumes, mounts, seccomp, storage_initialization, guest_image, execution_authorization, resource_budget), scale_set, stage, local (authority, pair, plan, remove, slot, storage, channel, supervisor, minted, policy, cleanup_audit), helper_supervisor, artifact_admission, https, curl_config.
  - Cargo feature `local-mode = ["tokio/signal"]` (host Cargo.toml:13); `local/` tests/bins are `required-features = ["local-mode"]`. README states a release build refuses the feature (local/README.md "Safety rules").
  - Bins: `velnor-resource-probe-release` (src/bin/velnor_resource_probe_release.rs), plus the test hook bin `velnor-local-test-job-hook` (feature-gated).
- **velnor-runner-cli** (3,546 LOC). Thin: clap parsing and presentation. Binary **`velnor-host`** (src/main.rs). Subcommands (src/args.rs:22-107): `connect`, `status [--json]`, `doctor [--probe]`, `logs [--follow]`, `drain [--wait]`, `resume`, `credential`, `service install|start|stop|uninstall`, `daemon run`, `compare --repo --run-id --attempt --evidence`, `journal <action>`, `disconnect --connection-id [--wait]`, and hidden `local` (feature `local-mode`: "Run local-mode pairs from a JSON plan on stdin until SIGTERM"). Modules: args, compare, daemon_run, dispatch, readiness_probe, service, state_directory.
- **velnor-resource-probe** (10,142 LOC). Guest-side probe: fixed-path numeric procfs/cgroup/statvfs parsers, bounded schema. Bins: `velnor-resource-probe`, `velnor-storage-init` (storage initializer), `velnor-job-start-hook` (official-runner job-start hook). Modules: cgroup, mountinfo, pressure, memory, load, filesystem, job_hook, job_facts, execution_channel, execution_publication, storage_init, worker_binding, cleanup_observer.
- **velnor-runner-qualification** (3,376 LOC). Probe containers, tool-version and host-config checks, volume mutation/cleanup tests: docker.rs, probes.rs, resource_dind.rs, host_config.rs, mutation_cleanup.rs, journal.rs, tool_versions.rs.
- **velnor-runner-image-manifest** (2,348 LOC). OCI/attestation grammar, runner family inventory, runner publication/qualification: attested_grammar, oci_*, provenance, runner_family_inventory.txt, mac_attestation/mac_qualification.
- **velnor-runner-attestation** (1,611 LOC). Bin `velnor-runner-attestation-helper`: one bounded stdin request, Sigstore bundle + Fulcio claim verification against pinned production TUF root (src/sigstore-production-tuf-root.json); run as a separate process, never linked (spec section 2).
- **velnor-local** (local/crates/velnor-local, 14,982 LOC) and **velnor-cache-rules** (638 LOC). Modules: authority, cli, conformance, doctor, event, images, jit, pairs, protocol, rest, scenario, side, standin, status, store, token, verdict.

### 4. Key concepts (as stated in docs and code)

#### 4.1 Official runner inside Docker, one pair per job
- macos-scaleset-runner.md header: "Runtime: the unmodified official GitHub Actions runner, Ubuntu 26.04, `linux/amd64`, registered with the Runner Scale Set protocol and JIT." Host is macOS with one explicitly selected local Docker engine. Selectors: `runs-on: [velnor, ubuntu-26.04-scale-set]` (scale set name `ubuntu-26.04-scale-set`; labels `ubuntu-26.04-scale-set` and `velnor` only; never the hosted label `ubuntu-26.04`).
- Section 1: "Velnor does not implement an Actions interpreter and does not ship a Go sidecar. The official runner handles actions, steps, logs, credentials, post steps, services, cancellation, and completion."
- Section 5: each admitted job gets one unmodified official runner container (not privileged) plus that worker's private DinD engine (privileged). `/var/run/docker.sock` in the runner resolves to the worker's private socket, never the outer engine. Named, nonce-named tmpfs-backed volumes for work, home, temp, actions, tools, socket; an owned storage initializer sets layout/owner/mode before the pair starts. No host-wide prune; "Delete only objects whose immutable id matches the journal. Names are not delete authority."
- Resource budgets are mandatory config `[host.resources]` (runner/dind cpu millicores + memory bytes + operational memory reserve), no defaults. Default `max_jobs` is 1; "Qualification must prove N>1." Docker named volumes have no per-job storage-size limit; "enforced storage isolation remains unproven" (section 5).
- Pair config details (docs/proposed/kubernetes-runtime.md section 1, a PR text): runner has 14 whitelisted capabilities, custom seccomp `worker/seccomp/runner-v1.json`, pids limit, tmpfs /tmp and /dev/shm; dind sidecar privileged.
- Docker access: bollard over unix socket only (`docker_client::connect_unix` rejects tcp://, per PR #1324 text); engine identity and docker-root digest captured and frozen (`launch/engine_context`). Docker spec audit in `docker_spec.rs` (`ContainerPlan`, `audit_plan`, `HOST_NEEDLES`) forbids host-path mounts, docker.sock mounts except private volume, JIT in env/cmd/labels, privileged runners.
- JIT delivery (section 5): raw JIT goes over a short-lived stdin or inherited fd 3; entrypoint passes it to `Runner.Listener` only as `ACTIONS_RUNNER_INPUT_JITCONFIG` on `exec`; max raw size 131040 bytes; absent from Docker Env/Cmd/labels, journal, TOML, plist, argv. Spec states JIT "is not confidential from the job it starts" (same-UID /proc/1/environ).

#### 4.2 How it talks to GitHub
- Scale Set protocol (spec section 4): Actions service base from registration; `api-version=6.0-preview`; path `_apis/runtime/runnerscalesets`; `POST .../{id}/generatejitconfig` (not classic repo `generate-jitconfig`); `POST .../{id}/acquirejobs` with int64 request ids; long-poll message queue (`messageType` `RunnerScaleSetJobMessages`; inner kinds JobAvailable/JobAssigned/JobStarted/JobCompleted); HTTP 202 = empty poll; header `X-ScaleSetMaxCapacity`; session create/refresh (401 -> one single-flight refresh; 403 = authorization defect); ack only when replay-safe. Create sends `RunnerSetting` with `disableUpdate: true`; re-read and label/name/scope verification after create/adopt. Disabled runner self-update is a registration invariant; the official runner is not patched.
- Admin credential: PAT-style token read from stdin by `connect`, stored in macOS Keychain as `keychain:<service>/<account>` (`github.credential_ref`); no PAT on argv, no secret in TOML (spec section 3). Open PR #1329 adds reading the host credential from a file on Linux.
- Org-level scale set with a named runner group is an open PR (#1330); responses naming another runnerGroupId are rejected in PR #1260.

#### 4.3 Journal (SQLite-compatible, Turso embedded)
- `turso` 0.8.3 local embedded DB, no cloud sync (deferred doc section 4; host Cargo.toml). Schema in `journal_schema.rs` plus ~35 files under `src/journal/schema/` (tables seen in code include `intents`, `journal_meta`, `capacity_reservation_*` (networks, storage_initialization, pre_jit_cleanup claims/progress/probes/settlements, unbound_retirement, cleanup_observer_witnesses), `connection_*` (scale_set_intents, session_creates, session_refreshes, runner_jits, request_authorizations, request_reservations, completion_inbox, completion_cleanup, launch_recovery, source_authorizations), `physical_attempt_lineages/attempts/current/outcomes`, `jit_dispatch_claim_times`, `jit_absent_after_deadline`, `jit_tombstone_completions`, `image_imports`, `doctor_probe_requests`, `execution_publication_operations`, `local_worker_exits`).
- Spec rule (section 4): "Persist intent, then perform the external call, then record success or uncertainty. Do not hold a database transaction across HTTP or Docker. `Drop` does not free a durable slot." Journal validates its version before schema mutation; unknown versions and malformed migrations fail closed; versioned migrations (tests reference e.g. "v47" journal, launch/jit_conflict tests).
- Capacity: reserved, acquiring, uncertain, provisioning, idle, running, finishing, cleaning, quarantined all occupy a slot (spec section 3). A recorded container is only "absent/stopped" on Docker 404 or explicit `exited`/`dead`; a missing/empty `Status` is uncertain.

#### 4.4 Ownership proofs and cleanup
- Core types: `OwnedIds`, `CleanupProof`, `OwnershipFailure {IdMismatch, MissingResponse, ForeignObject}` (core/src/ownership.rs). Host: `reconcile::before_advertise`, `launch/gate.rs` (`reconcile_gate`), `StorageCleanupObserverWitness`, `ObservedDockerCleanupAbsence`, `DeleteDecision {Delete, KeepForeign, NotDeleted}` (as described in PR #1324 text section 2.4, a description of current code).
- `observe_docker_cleanup_absence` (bootstrap/cleanup/production.rs, per PR #1324 text): re-captures engine identity and requires equality with journaled `engine_id` + `docker_root_digest` (else `HostError::Ownership`), requires 404 for each container id AND name and each volume name.
- Labels `velnor.role`, `velnor.volume`, `velnor.worker` on created objects (`docker_spec::runner_plan`, per PR #1324 text). Foreign objects survive; ownership mismatches retain capacity and leave the conflicting object intact (spec section 4).
- Execution-authorization / job-authority: `worker/execution_authorization`, `launch/execution_authorization`, job-start hook (`velnor-job-start-hook`) checks each job at start against an approved job-authority policy (JSON) bound at `connect --job-authority-policy`; fork PRs and `merge_group` are never approved in local policy check (local/README.md "approved-workflows check").

#### 4.5 Crash recovery
- Spec section 4: "Restart reconciles journal, exact Docker objects, and GitHub state before advertising capacity, and adopts still-running owned workers." Pending/uncertain acquire or JIT rows retain slot and owned volumes "until an authoritative settlement mechanism exists". Pre-JIT bootstrap recovery records an immutable target identity per owned object and removes that exact target; a crash after Docker deletes but before journal records survives; "Recovery never retries the original create or start request." Retirement fences keep a late object from being bound/started. An engine identity change stops automatic mutation.
- Evidence docs: docs/implemented/macos-scaleset/recovery-evidence.md, runner-journal-migration-evidence.md. Open issues #1303 ("reconcile_gate never observes GitHub: it compares a row against its own runner ids"), #1206 ("Startup admits work after legacy reconciliation returns Hold"), #1110/#1118/#1119 (scale-set create crash/retry gaps).
- Daemon: one per host, `daemon.lock`; second daemon fails before opening a session (spec section 3); per-user LaunchAgent (`velnor-host daemon run`), state in `~/Library/Application Support/Velnor/`, logs `~/Library/Logs/Velnor/`, private Unix socket (`controller_ipc`). Not a root LaunchDaemon. States: waiting_for_engine, waiting_for_credentials, reconciling, ready, draining, degraded.

#### 4.6 Local mode
- Two senses:
  1. `velnor-host local` (feature `local-mode`): runs worker pairs from a JSON plan on stdin until SIGTERM. Source `crates/.../velnor-runner-host/src/local/*` (pair, plan, remove, slot, minted, supervisor, policy). "Local mode keeps no reservation"; minted-agent recovery state in `runner/minted-agents.log`; cleanup audit in `cleanup.jsonl` (local/README.md "Safety rules").
  2. `velnor-local` (local/): "runs GitHub's official runner, unmodified, against `runner.server` on an Apple Silicon Mac, in OrbStack containers, with a cache that follows GitHub's cache rules" (local/README.md line 3). Parts: ChristopherHX/runner.server pinned commit 992ccbbb (patched Dockerfile), actions/runner 2.338.0 linux-arm64, `velnor-local cache-server`, mitmproxy recorder. Commands include `doctor`, `build`, `up`, `down`, `status`, `run <scenario>`, `supervise`. Scenarios JSON in local/scenarios (warm, cold, docs, leaf, hub, lock, fork, scheduled-run...). Runner modes: `--runner pair` (default; velnor-host pairs behind the job-start check) vs `--runner plain` (official runner image, no check). Fork PRs run on a separate network/stand-in/restore-only cache. Local runners are linux/arm64 (label set ubuntu-latest, ubuntu-22.04/24.04/26.04 + `-arm`), unlike production `linux/amd64`.
- Local authority listens on 127.0.0.1 only with bearer token; serves `/ledger`, `/jit` etc.

#### 4.7 Scale-set/JIT usage summary
- Repository-scoped Runner Scale Set (spec: "Do not implement Scale Sets as `--ephemeral` plus workflow-job polling"). One host controller manages multiple repository connections sharing one capacity budget; `disconnect` deletes only remote resources the controller recorded as created; adopting a set does not allow deleting it (spec section 3).
- Open PRs touch org-level scale set (#1330), controller using scale set for velnor-new's own hosted jobs (#1332), `disconnect` / adoption issues (#1113, #1110).

### 5. Build and test results (this machine)

Env: macOS arm64, rustc 1.99.1, `cargo` via mise wrapper (MISE_TRUSTED_CONFIG_PATHS set to the copy; mise installs untouched). Run from `vn-copy/crates/velnor-runner`, `CARGO_TARGET_DIR=vn-target`.
- `cargo build --workspace --locked`: exit 0, wall 246 s (cold target dir; mbx compiler-cache wrapper active: "673 hits, 167 misses" so wall time includes cache effect).
- `cargo nextest run --workspace --locked --no-fail-fast` (default features, so `local-mode` tests not included): `Summary [1353.672s] 430 tests run: 430 passed (114 slow), 0 skipped`; exit 0, total wall 1530 s including compile (~176 s). 19 test binaries.
  - By binary (PASS counts): velnor-runner-host lib 195; scaleset_installation_public 41; runner_image_qualification_public 39; registry_acquisition_public 9; image_session_public 8; process_death (qualification) 7; runner_platform_public 6; registry_journal_public 6; mac_qualification_facade_public 5; resource_probe_release_public 4; runner_manifest_admission_public 4; velnor-runner-image-manifest mac_qualification_public 3; guest_image_identity_pure 2; registry_projection_cache_pure 1; release_lock_public 1. The remaining ~89 are in binaries whose PASS lines did not map in my tally (other crates: resource-probe, github, cli, etc.; total 430 is nextest's figure).
  - Slow tests: single tests reported SLOW at over 120/180/540 s (e.g. `launch::bootstrap::unbound_retirement::tests::shape::...` >540 s; `remote_absence_boundary::upgrading_a_valid_v47...` 283 s; `registry_journal_public::journal_reopens_valid_registry_entries...` >120 s). Open PRs #1349 ("cache historical-schema journals in runner-host tests") and #1333 ("validate the journal schema once on open (linear, not quadratic)") concern this cost; I did not test whether they fix it.
  - No tests were skipped or failed on macOS. Docker daemon was reachable; I did not determine which tests used it (the `local_pair_public`/`local_host_public` tests need `--features local-mode` and were not run).
- Not run: local-mode feature tests, `local/` workspace build/tests (velnor-local), qualification against live GitHub (spec section 7: G4, G6 launchd, G7, G8 "stay external oracles").

### 6. docs/deferred/self-hosted-runner.md (382 lines) summary

- Status line 3: "Historical / Superseded. The active runner specification is macOS Scale Set runner". Still in force: workspace separation, no second task graph, no generator-adapter dependency, one task/cache authority, exact lock identities, durable recovery, truthful conclusions, ban on mounting host home or outer Docker socket.
- Stage table (lines ~15-20): V2 = one macOS host running the official runner in an owned Linux container; **V2-native** = "Replace the official runner for the restricted V1 profile with the real Rust protocol runtime", required reuse: "V1 runner profile plus V2 ownership, cleanup, and observability"; V3 = Debian host, same supervisor. Line ~22: "The native milestone is a separate runtime and MUST NOT be simulated by launching the official runner."
- Section 6 "Later native runtime" lists the protocol lifecycle the native runner would implement: JIT identity -> authentication -> broker session -> job assignment -> run-service acquisition -> V1 profile admission -> Docker execution -> live logs/step results -> job completion -> session/registration cleanup; must preserve auth refresh, session expiry, acquisition ownership, lease renewal, cancellation, ack ordering; "`already acquired` and `unavailable` MUST NOT be handled by rerunning repository code"; user code only in owned Linux containers; explicit Bash, Docker exec; "One real native job must complete in GitHub before adding more Actions features."
- Native built-in operations table (section 2): `actions/checkout`, `actions/cache/restore|save`, `actions/upload-artifact`, `actions/download-artifact` translated to built-ins; any other action fails admission. Marketplace JavaScript actions are not run natively.
- Section 7 `runner-rust-ci-v1` profile (generated sequential `run` steps, no arbitrary `uses:`, no services/nested actions): marked **Superseded**: "this restricted native interpreter is not a prerequisite for official Scale Set mode. Do not implement it."
- Section 10 acceptance gates all "Not implemented" in this document, including "Native protocol: One real native job acquires, executes, logs, completes, and cleans up in GitHub". Section 11 delivery order: contract freeze, V2 official, V2 recovery, V3 Debian, native protocol, controlled expansion.
- Planned dependency table (section 9) lists reqwest, jsonwebtoken, bollard, turso, tokio, tracing, etc. Code differs in places (velnor-runner-github has no HTTP dependency; host uses reqwest/hyper).
- Superseded clauses: classic repo `generate-jitconfig`, ARM64 baseline, ban on Scale Sets/nested Docker. Active spec instead: scale-set `generatejitconfig`, `linux/amd64`, private DinD.
- Out of scope in both docs: Kubernetes, Firecracker, remote hosts, fleet provisioning, dashboard/TUI (macos-scaleset-runner.md section 8 also lists ARC, Velnor-managed VMs, libvirt, Linux-host controller, native Darwin/Xcode jobs, multi-repository capacity pools).

### 7. docs/goals.md

- Line 16 (exact): `New work serves Rust projects on GitHub Actions, on GitHub-hosted or self-hosted runners. Jobs are always executed by GitHub's official runner: on GitHub's machines, or on the project's own machines inside the containers that `velnor-runner` starts.`
- Line 11: new work = generator/, ci-helper/, crates/velnor-runner, local/. Line 12: existing engine in `crates/` is "maintained, not developed". Line 20: runner "runs jobs on the project's own machines"; `velnor-local` "runs a checkout's workflows on a developer's machine without pushing, imitating GitHub, and starts each job through the runner". Line 22: "The runner is developed as a product in its own right, because local mode and self-hosted runs depend on it."
- Promises (lines 32-36): nothing to do means nothing runs; work is never done twice; fast never means checked less. Never-compromise (38-45): correct before fast; green means verified; broken cache is slow never wrong; secrets never reach the build; every speed claim has a measurement; inside GitHub's free limits (10 GB cache, standard hosted runners, "does not depend on paid storage or larger machines").
- Targets (47-61) are defined "on GitHub-hosted runners": no-crate run finishes within 1 minute; job start to first cargo command at most 30 s incl. cache restore; cache at most 8 GB of 10 GB.
- Design rule 11 (line ~74): "Local first. A workflow is verified on the developer's machine with GitHub's official runner, started by the Velnor runner, a local stand-in for GitHub and a local cache that enforces GitHub's rules. Real GitHub confirms it afterwards."
- Rule 9: no logic in YAML; every generated `run:` step is one command, actions from fixed pinned list, logic in a Rust helper (velnor-ci).

### 8. docs/hard-decisions.md (513 lines) - sections relevant to runner/toolchain

Scope (lines 14-19): binds the existing engine in `crates/`, `.velnor/`, generated workflows and supports; "They do not bind the new Rust generator: the goals are its rule book".
- Section 1 "One job per crate with parallel check steps" (l.21): rationale cites runner time with setup paid per job (l.46, 167).
- Section 4 "Tool versions come from the project's Mise config" (l.232-350): versions come from project `mise.toml`/`mise.lock` (also `rust-toolchain.toml`, `.tool-versions`); generated workflows never copy a declared version; resolved at run time by `mise --no-config --no-env --no-hooks config get` after Mise setup and checkout; errors for conflicting sources, malformed sources, symlinked sources, nested rust-toolchain; `MISE_NO_CONFIG=1` on install/exec because GitHub runners set `CI=true` under which Mise would auto-trust repo config and evaluate templates (l.316-319); `mise.lock` checksums not enforced in CI with `--no-config` (l.338); a lock file selects everything (l.301).
- Section 7 "Private dependency credentials" (l.388-427): scripts/Bun lifecycle/Mise tasks, tokens revoked before repository code (l.396-409).
- Section 8 "Publication only from the default branch or a release tag" (l.428); Section 11 "Compile steps build the whole workspace" (l.486), which bends goals promise 1 (l.508).
- Sections 2, 3, 6 (timeout, PR affected-only testing, Bun) are generator-level, not runner.

### 9. docs/proposed/kubernetes-runtime.md (PR #1324)

PR #1324 "docs: proposed Kubernetes runtime for velnor-host (one runner, two runtimes)", OPEN, not draft, head branch `fast/k8s-runtime-design`, head SHA 7e8b0805440503136a7676fd73bbd3cc4539ecdd, created 2026-11-18T13:08:21Z by the repo owner, 1 file added, 495 lines, 0 deletions. Raw content saved at ~/.cache/ci-ambition/evidence/k8s-1324.md. Status line: "Proposal. Design only; no code accompanies this document."
- Goal (s0): ONE runner; Docker and Kubernetes behind a single `Runtime` interface; scale-set protocol, journal, official runner binary, launch state machine and cleanup-proof discipline remain one implementation. Non-goals: second host binary, second journal, operator/CRD, replacing Docker as default, moving the Mac path to Kubernetes. Target: the tailrocks Hetzner k3s cluster "at full capacity with warm caches".
- s1 current coupling (counts from grep): about 5,150 lines mention "docker"; roughly 50 non-test source files import `bollard`; no trait today. Call counts: `info` 30, `inspect_container` 18, `remove_container` 14, `stop_container` 11, `inspect_volume` 11, `inspect_image` 7, `wait_container` 6, `start_container` 6, `create_container` 5, `create_volume` 3, `*_network` 3 each, `upload_to_container` 2, `list_containers` 2.
- s2 proposed trait `Runtime` (names proposed, not existing): `identity`, `verify_image`, `create_worker`, `start_worker`, `deliver`, `exec_helper`, `wait`, `logs`, `stop`, `delete`, `delete_storage`, `delete_network`, `inspect*`, `prove_owned`, `list_owned`, `cgroup_subjects`; types `RuntimeKind{Docker,Kubernetes}`, `WorkloadRef/StorageRef/NetworkRef {kind,scope,name,uid}`, `Observation<T>{Present, AbsentProven, Unknown}`, `AbsenceProof`, `OwnershipProof`, `StorageClass{Ephemeral, NodeCache, SharedRun}`. Rule: timeouts map to Unknown, never AbsentProven.
- s2.4 Docker-to-Kubernetes proof mapping: engine id+root digest -> cluster id (kube-system namespace UID)+namespace UID+host-instance Lease/ConfigMap; container id -> Pod UID; 404 by id -> GET 404 or UID mismatch; remove -> DELETE with `preconditions.uid`; ownerReferences to a per-job owner object. Requires quorum-read semantics for absence proof; 403/5xx/timeouts/watch gaps are Unknown; terminating = live.
- s4 mapping: one Pod per job (not a Job; restartPolicy Never) with containers `runner` and `dind` (shared `/run` emptyDir); caps drop ALL + 14 added; seccomp Localhost or RuntimeDefault (open question); `pids_limit` has no Pod field (kubelet `podPidsLimit`, flagged gap); tmpfs -> emptyDir Memory (no inode limit, gap); log limit is kubelet-wide.
- Storage/caches: Work/DindData as `NodeCache` or emptyDir; recommended option A = node-local PersistentVolumes (k3s `local-path`, topolvm, openebs-localpv), one long-lived PVC per (node, cache family), ReadWriteOncePod, shard K cache slots per node; option B hostPath (faster, weaker isolation, "Not recommended beyond a spike"). Placement via node affinity label `velnor.io/cache-<family>=<digest>`. Shared cache PVCs never deleted by job cleanup.
- RBAC: namespaced Role (pods create/get/list/watch/delete; pods/exec, pods/log; PVCs; configmaps/secrets for owner object and JIT; events list) + tiny read-only ClusterRole (namespaces, nodes). Job Pods use a no-token ServiceAccount. JIT in a per-Pod Secret mounted read-only, never env/args/labels.
- dind: privileged dind kept in a dedicated tainted node pool for v1; sandboxed RuntimeClass (Sysbox/Kata/gVisor) deferred as hardening; host docker socket stays forbidden.
- Exec/logs/wait: `pods/exec` WebSocket (`v5.channel.k8s.io`) for helper protocol, needs half-close spike; completion = `runner` container terminated state, not Pod phase.
- s6 security: privileged only dind in tainted pool; NetworkPolicy default-deny, egress allow-list, deny metadata (169.254.169.254) and API server; etcd encryption required; fork fence: `pr` trust scope gets read-only cache and separate cache slot family/node label; enforced host-side `audit_spec` and cluster-side admission.
- s5 stays identical: journal tables and `LaunchPhase` machine (phase names `docker_*` renamed to `runtime_*` with read alias; rows gain `runtime_kind`, `scope`), scale-set protocol, official runner binary and image contract, pure logic, and Local mode is Docker-only.
- s7 config: `[runtime] kind = "docker"|"kubernetes"`, `[runtime.kubernetes]` keys (kubeconfig_credential or in_cluster, namespace, node_pool_label, cache_storage_class, cache_slots_per_node, runtime_class); `[docker]` remains an alias; no mixing runtimes in one host process in v1.
- s8 work breakdown: 12 PRs (1 trait + DockerRuntime, 2 k3s spike, 3 proofs migration (L, High risk), 4 call-site migration (L, High), 5 journal columns, 6 local mode on trait + lint banning bollard outside docker module, 7 KubernetesRuntime core (L), 8 exec/logs/JIT secret, 9 node-local cache PVCs (L), 10 qualification suite, 11 config, 12 hardening). Order: 1,2,5,3,4,6 then 7-12.
- s9 nine open questions for the owner (privileged dind vs sandbox RuntimeClass; cache model; where host runs; cluster dedicated vs shared; seccomp distribution; pids/inode limits; journal rename; Mac stays Docker-only; autoscaling vs node-local caches).

### 10. Open PRs and issues touching the runner (title + number; gh read-only, listed 2026-11-18)

Open PRs touching `crates/velnor-runner` or `local/` (by changed files):
#1359 fix(qualification): remove owned container after uncertain unjournaled create; #1349 perf(runner): cache historical-schema journals in runner-host tests; #1347 refuse tag filters on schedule and merge_group authority contexts; #1346 reject multiplexed log frames with missing/truncated payload; #1344 report f_bavail as docker_root_free_bytes; #1343 fix(local): malformed runner inventory is no proof of deletion; #1342 fail the minted-journal bound check when the journal cannot be read; #1333 validate the journal schema once on open (linear, not quadratic); #1331 admit work only when the legacy gate says Advertise (#1206); #1330 feat(runner): org-level scale set with a named runner group; #1329 feat(runner): read the host credential from a file on Linux; #1309 reconcile gate releases only on proven runner absence; #1294 first lineage bind survives a leftover anchor temp file; #1289 fail closed when the lineage lock file is replaced; #1288 keep an unlinked dockerd as the daemon candidate; #1287 count an owned volume mount covered by another mount; #1285 keep setup receipt across a failed poll; #1283 post-JIT retirement claim stops a runner from starting; #1281 bound Docker /info body in readiness check; #1280 refuse pre-JIT cleanup progress over a broken claim premise (#1076); #1279 count live connection_intents rows in readiness scan; #1277 keep a recorded execution source and published challenge identity immutable (#1077); #1275 CleanupProof::covers requires full container id and named volume; #1273 reject embedded unix:// in Docker socket path; #1272 qualification volume cleanup re-checks identity before delete; #1269 refuse noncanonical probe Docker directory; #1264 accept identical job-authority policy on repeat connect; #1263 refuse a second daemon when daemon.lock is unlinked; #1261 idle-wait instead of busy-spin in frozen-clock helper; #1260 reject scale-set responses naming another runner group; #1255 report draining from controller readiness when drain marker exists; #1249 refuse unresolvable HOME instead of using cwd; #1050 Docs: match the contracts to the code.
Other open PRs naming runner/scale set/k8s: #1332 ci: run velnor-new's hosted jobs on the scale set (hosted queue is the bottleneck); #1324 docs: proposed Kubernetes runtime for velnor-host; #1280 (above).
Open issues (runner-related): #1303 reconcile_gate never observes GitHub; #1206 startup admits work after legacy reconciliation returns Hold; #1187 minted-journal bound check passes when journal unreadable; #1186 torn write to minted-agent journal read back as a record; #1185 `velnor-host local` reports a crashed runner container as a served pair; #1183 probe reports f_bfree as docker_root_free_bytes (10 GiB gate wrong number); #1177 local authority reports deletion proved on malformed inventory; #1170 local plain mode reports ready when runner.server says offline; #1143 CleanupProof::covers accepts short id/empty volume; #1142 Capacity::reserve replaces a live worker slot; #1140 on macOS control socket answers before request written; #1139 Doctor reports ready while drain marker set; #1120 scale-set response with different runnerGroupId accepted; #1118 scale-set create that did not create is never retried; #1117 removing daemon.lock lets a second daemon open a session; #1113 disconnect leaves a scale set this controller created; #1110 crash after scale-set create leaves matching set unadopted; #1076/#1075/#1074 pre/post-JIT retirement gaps; #1005 cancelled run's reservation stays reserved; #627 [FAST-L] Local GitHub: official runner, stand-in service and own cache on one machine; #662 [OWNER] Run Qualification workflow once on the scale set; #1221 velnor-local: wait for replacement pair before dispatching next event; #1220 steps after repository code can't be trusted; trust boundary at the job; #1305 P3 CI/CD performance for velnor-new (tracker); #1290 P1 velnor-new runs entirely on velnor-gen.
(Roughly 60 open issues mention runner journal/ownership/scale-set defects; list above is a subset.)

## E. velnor-legacy (archived Rust runner)

_Source file: legacy.md_

Facts only. Sources are file:line or commit/issue refs in the clone at
`~/.cache/ci-ambition/refs/velnor-legacy` (HEAD `1b9fc038`, 2026-10-17). Raw logs:
`~/.cache/ci-ambition/evidence/legacy-raw/` (`build.log`, `nextest.log`, `test-cargo-test.partial.log`, `failed.txt`).
Host for build/test: macOS Darwin arm64, 18 CPUs, rustc 1.98.1 toolchain (rustup), no mise.

### 0. Repo identity and state

- `gh repo view`: isArchived=true; createdAt 2026-05-31; pushedAt 2026-10-17; description (verbatim):
  "LEGACY, reference only. The old Velnor implementation (self-hosted GitHub Actions runner and control plane). Not maintained. Current work: tailrocks/velnor-new".
- 2231 commits: 2026-05 88, 06 576, 07 131, 08 359, 09 1070, 10 7 (git log by month). First commit `8c0682b1` "Initial commit" 2026-05-31; last `1b9fc038` "build: migrate to velnor-actions 0.1.12 (#1141)".
- Same tree also holds the docs site (Fumadocs/TanStack/Bun; README.md) - `content/docs/**`, `src/`.
- AGENTS.md:3-4: "`actions/runner` ... is the protocol source of truth ... Never guess." AGENTS.md:5-6: "This is a research project. It is unsafe and expected to contain breaking changes; never treat it as production-ready."
- Product names: `velnor-runner` (daemon), `velnorctl` (operator CLI), `velnor-workflow` (workflow generator/policy), `velnor-actions` (the generator binary now published from velnor-new; `.velnor/release-manifest.json` repository = "tailrocks/velnor-new", artifact `velnor-actions-0.1.12-*`).
- `.github/AGENTS.md:4,19-28`: generated workflows are owned by `velnor-actions` from tailrocks/velnor-new; fixes go upstream to velnor-new, then `velnor-actions generate` re-renders the legacy repo's workflows.

### 1. Workspace inventory (Rust LOC = `*.rs` lines under crates/*/ incl. tests, `find|xargs cat|wc -l`)

Total approx 804k Rust lines in 11 crates (Cargo.toml:2-13 lists 10 members + tools/unit-collector; `velnor-workflow-contract` dir exists but is not a member).

| Crate | LOC | Purpose (Cargo.toml description / module docs) |
|---|---:|---|
| velnor-runner | 337,384 | Runner daemon: native broker/run-service client, executor, Docker + Firecracker backends, caches, scale-set lane, node controller |
| velnor-workflow | 318,390 | "Static GitHub Actions workflow generator and CI execution client": repo scanner, policy, closure/pin checks, renders `.github/workflows` (not a scheduler) |
| velnor-tools | 60,696 | GitHub live collectors, workflow monitor, audit_ci, fleet policy, evidence checks, homebrew preview |
| velnor-control | 42,726 | Daemon-side control-plane services: journal, permit ledger, store/migrations, lifecycle, telemetry |
| velnor-bench | 13,958 | "First-class benchmark harness that measures the Velnor product" (scenarios, stages, stats, soak, fault) |
| velnor-model | 13,685 | Shared versioned model types (job, condition, vsock protocol, microvm, storage) |
| velnorctl | 12,844 | Operator CLI (host install, preflight, diagnostics, completion, man) |
| velnor-client | 1,796 | Unix/HTTP client for control plane |
| velnor-render | 1,200 | Output renderers |
| velnor-storage-snapshot | 931 | Portable catalog rules / storage snapshot discovery |
| velnor-workflow-contract | 824 | not in workspace members |

Largest runner files (wc -l): executor.rs 37,028; runner.rs 36,759; docker_lease.rs 21,607; protocol.rs 18,419; gha_cache.rs 17,140; buildkit.rs 10,326; node/controller.rs 9,244; leftover_disk.rs 7,743; action.rs 6,978; job_message.rs 6,799.
Test attribute count not summed; nextest ran 8,222 tests (section 5).

#### 1.1 Expression engine
- `crates/velnor-runner/src/expression/` (3,565 lines: lexer 549, parser 497, eval 679, value 512, spans 82, mod 257, tests 989).
- mod.rs:1-14: real lexer + shunting-yard parser + evaluator "transcribed from actions/runner v2.337.0 (commit 397b032c...) under src/Sdk/DTExpressions2/Expressions2/". It replaced an earlier "rewriting strings over Option<String> with a fail-open tail" (mod.rs:3-6).
- Built-in functions in eval.rs:216-247: `case`, `contains`, `startsWith`, `endsWith`, `format`, `join`, `toJson`, `fromJson`; others dispatch to `context.call_function` (eval.rs:24). Extension functions `success/failure/always/cancelled` (arity 0) and `hashFiles` (1..255 args) in executor.rs:15694-15698, evaluated at executor.rs:15732-15742; `hashFiles` modeled on `HashFilesFunction.cs:19-60` (executor.rs:15644). Limits `MAX_DEPTH`, `MAX_LENGTH` exported (mod.rs:36).
- Note: executor.rs `JobExecutionState` holds contexts; expressions are resolved by the runner (native lane) from the job message the run service returns; the service does not pre-evaluate.

#### 1.2 Step, composite and action execution (native lane)
- `action.rs:441-460`: `ActionMetadata::runtime()` accepts `using` node12/node16/node20/node24 -> `ActionRuntime::JavaScript`, `composite` -> Composite, `docker` -> Docker; `plugin` rejected.
- Composite: parsed/normalized action.rs:1237-1294; scoping `execution/composite_scopes.rs`, `executor.rs:15193-15214` (push/pop composite), umbrella conditions/outcomes tests executor.rs:27474-31104.
- Script steps: `script_step.rs` (2,160 lines); workflow commands `workflow_command.rs` (1,521); command files `command_files.rs`.
- Admission is allowlist-based (see 1.3): docs/guides/execution.mdx:124-131 "Arbitrary JavaScript actions. A runs.using: node20/node24 action that is not in the allowlist is rejected at planning. There is no generic JavaScript action runtime open to workflows." Same for Docker actions (`docker://` refs rejected) and composites ("Only allowlisted composites are expanded").

#### 1.3 JavaScript actions: how node actions run
- Allowlisted JS actions run in an ephemeral sidecar container, not a host `node`. `node_action_image()` executor.rs:652-662 maps node20 -> `node:20-bookworm`, node24 -> `node:24-bookworm`. Launch: `container.prepare_run_node_action_args("/__w", ...)` executor.rs:4640-4650.
- action.rs:409-422 (`unsupported_action_error`): actions that install tools into the discarded sidecar are rejected by name: `dtolnay/rust-toolchain`, `baptiste0928/cargo-install`, `EmbarkStudios/cargo-deny-action` ("sidecar images are plain node images with no Rust tooling", action.rs:407-410). Supported alternative: `jdx/mise-action`.
- Scale-set lane avoids all of this: it delegates the job to the unmodified official runner (section 2.3).

#### 1.4 Native Rust adapters (action.rs:379-408, `native_action_adapter`, 26 identities)
actions/checkout, actions/cache, upload-artifact, download-artifact, upload-pages-artifact, configure-pages, deploy-pages, attest-build-provenance, create-github-app-token, dorny/paths-filter, jdx/mise-action, mozilla-actions/sccache-action, rui314/setup-mold, extractions/setup-just, Swatinem/rust-cache, crazy-max/ghaction-github-runtime, actions/github-script, renovatebot/github-action, docker/setup-buildx, login, metadata, build-push, bake, hadolint-action, docker/setup-qemu, sigstore/cosign-installer. Manifest schema v15 (manifest.rs:20; MANIFEST_VERSION).
- **actions/checkout** (`checkout.rs`, 5,128 lines + `git_mirror.rs` 1,613): native git; accepts only `repository, ref, token, persist-credentials, path, clean, fetch-depth, fetch-tags, lfs` (execution.mdx:137-141). `submodules`, `sparse-checkout`, `ssh-key` fail admission; submodules always false. LFS: pointers left unless `lfs` set (checkout.rs:70, 1343-1345). Echoes the git command with token masked "the way actions/checkout does" (checkout.rs:1418). Mirror-based fetch with PR-ref fallback (checkout.rs:47-52).
- **actions/cache** and v1/v2 cache protocol: not a re-implementation of the JS action only; `gha_cache.rs` (17,140 lines) is a Velnor-hosted cache *service* because "Self-hosted job messages never carry a CacheServerUrl, so BuildKit type=gha and actions/cache@v4 silently no-op on Velnor" (gha_cache.rs:1-4). Serves v1 `_apis/artifactcache/{cache,caches}` and v2 Twirp `CacheService/{CreateCacheEntry,FinalizeCacheEntryUpload,GetCacheEntryDownloadURL}` (gha_cache.rs:6-12). Storage content-addressed on host, namespaced by server origin + repo id + ref (`tenants/repo-<sha256>/{blobs,entries,reservations}`), lookups ref then base namespace, writes only to own ref, fork-trust chain (gha_cache.rs:14-40). Native action kinds: root, `restore`, `save` only (action.rs:370-374).
- **Swatinem/rust-cache**: `NativeActionAdapter::RustCache` (action.rs:394); native restore/save with report "Backend: rust-cache (native)" (executor.rs:9153-9156, `native_rust_cache_save` tests executor.rs:22743-22750); on host-persistent store the step prints "restore/save skipped" (executor.rs:9153).
- **sccache**: `sccache_compat.rs`; explicit compat mode only when a job requests `mozilla-actions/sccache-action`; default image does not ship sccache (sccache_compat.rs:1-25). Version + per-arch SHA-256 pinned (`LOCKED_VERSION`), downloaded at step time, exports `RUSTC_WRAPPER`, starts server. Default path instead is "transparent Mr Boxington" (below).
- **mbx / Mr Boxington** (external cache-accelerating cargo shim, version pinned 1.12.0): README.md:7-12 and execution.mdx "Docker Rust acceleration" (~line 288-300): `mbx setup --yes` baked into the job image; generated cargo launcher first on PATH so plain `cargo` enters mbx; "no default RUSTC_WRAPPER"; docker backend only ("does not apply to the MicroVM backend"). `mbx_store.rs` (1,870 lines): one store per (trust scope, repository), mounted `/var/cache/mbx`, per-slot `slots/<slot>` (MBX_CACHE_DIR) and `targets/slots/<slot>` (MBX_TARGET_ROOT), because mbx serializes on a registrar flock and cargo on target-dir lock (mbx_store.rs:1-12). Startup migration deleted ~144 GiB of old-layout trees observed on one live host (mbx_store.rs:14-19). `mise.rs` + `docker/job-mise.toml`: tool provisioning via mise.
- Generic `cache.rs` (6,150 lines): host-persistent cache stores, entry flock, GC (`CacheEntryLock`, emergency-reclaim min idle 15 min, cache.rs:15-24).

### 2. Protocol: how it talks to GitHub

#### 2.1 Native lane (Velnor itself acts as the Actions runner)
Documented in content/docs/reference/runner-protocol.mdx. Pinned to actions/runner `v2.337.0` commit `397b032cbf865e9c3ddfab89d533ec19325e1273` (runner-protocol.mdx:17-23). Four service roles (runner-protocol.mdx:34-41): JIT registration endpoint, Broker (long-poll session), Run Service (full job, lease renew, completion), Results + timeline services. One JIT V2 session per ephemeral slot (runner-protocol.mdx:28-31).
Endpoints visible in `protocol.rs` (18,419 lines) and tests:
- Registration: `POST .../actions/runners/generate-jitconfig` for repo/org/enterprise scopes.
- Run Service: `{run_service}/jobs/{id}/acquirejob`, `/renewjob`, `/completejob` (protocol.rs test strings; "run-service error object" parsing protocol.rs:2780).
- Broker: long-poll classification `classify_broker_poll(_error)` protocol.rs:2683-2696; busy ack.
- Legacy pipelines timeline API: `.../_apis/distributedtask/hubs/build/plans/.../timelines/.../records`, `/logs`, `/events`, `.../feed`, `api-version=5.1-preview.1`; `pools/{id}/messages`, `pools/{id}/jobrequests/{id}`.
- Results service: Twirp `github.actions.results.api.v1.ArtifactService`, `results.services.receiver.Receiver` (step/log upload); artifacts via Results with a same-host local fallback (integrations.mdx:81).
- Error taxonomy: `BrokerErrorCategory`, typed acquire 409/422 non-retriable (protocol.rs:88-157).
- Cache: self-hosted `_apis/artifactcache` + Twirp CacheService (gha_cache.rs, see 1.4).

#### 2.2 Job lifecycle (architecture.mdx:105-120)
Broker message -> Run Service acquire -> local claim -> action-closure/policy admission before side effects -> backend `preflight -> reserve -> prepare -> start -> execute|cancel -> collect -> teardown` -> progress + lease renewal -> publishers drain -> completion staged locally then sent -> local cleanup. Host-wide permit ledger bounds capacity (`permit_ledger.rs`, `capacity.rs`, `host_capacity.rs`).

#### 2.3 Scale-set lane (`crates/velnor-runner/src/scaleset/`, ~20 files)
- `client.rs` (1,488 lines): "Admin-plane scale-set client (client.go at UPSTREAM_COMMIT)" - mirrors actions-runner-controller's Go `scaleset` client: registration-token -> admin connection (`/actions/runner-registration`) -> JWT with 60 s skew refresh, runner groups `/_apis/runtime/runnergroups/`, `_apis/runtime/runnerscalesets`, `generate-jit-config`, `api-version=6.0-preview` (client.rs:1-9, 30-37).
- `session.rs`: message session create/refresh/close, long poll `GET {queue}?lastMessageId=N` with `X-ScaleSetMaxCapacity`, 202 = no message, ACK `DELETE {queue}/{id}`, `AcquireJobs` (`_apis/runtime/runnerscalesets/{id}/acquirejobs`) (session.rs:1-8).
- Worker (`scaleset/worker/runner.rs:1-20`): per job, one digest-pinned `ghcr.io/actions/actions-runner` container + a private DinD daemon; JIT config passed through `ACTIONS_RUNNER_INPUT_JITCONFIG` in a 0600 env-file; entrypoint `exec /home/runner/run.sh --once` (runner.rs:694). So scale-set jobs are executed by GitHub's OFFICIAL runner, not by Velnor code (architecture.mdx:142-155; runner-protocol.mdx:43-47; execution.mdx:118-120).
- Offer classification uses event name and owner/repo only; `pull_request` and `workflow_run` stay `unknown` because offers lack head-repo data (architecture.mdx:196-203).

### 3. Isolation backends

- **Docker** (default): `docker/client.rs` (6,414 lines) typed wrapper over the `docker` CLI with per-class deadlines; `docker/engine.rs` (4,137) minimal async Engine API client over the Unix socket, read-only GETs plus container start/stop/remove ("GOAL 17, option C", engine.rs:1-20). `docker_lease.rs` (21,607 lines): per-job proxy in front of the Docker socket injecting `velnor.job-id`/`velnor.daemon-id` labels so teardown reaps guest-created objects; BuildKit cancel hangs dealt with by bidirectional splicing (docker_lease.rs:1-20). `buildkit.rs` (10,326). macOS hosts run Linux containers via Docker VM; macOS/Windows jobs are not executed (execution.mdx:144-146).
- **Firecracker** (`execution/firecracker.rs` 1,339 lines + guest_agent, guest_runtime 2,794, guest_actions, snapshot, net, isolation, artifacts): jailed, guest-local Docker, no host docker.sock (firecracker.rs:1). Boot-source/drive/net/vsock/snapshot create via the Firecracker Unix-socket API (`FirecrackerApi` trait). Reproducible guest in `microvm/guest.toml`: kernel 6.1.102, Ubuntu snapshot 20260826, rootfs with containerd/runc/docker-engine/buildkit, no sshd, job image `velnor/job-ubuntu:26.04`; "macOS cannot produce the kernel/rootfs bytes" (guest.toml:1-2). Snapshot identity tuple binds kernel/rootfs/agent SHA-256 (snapshot.rs:13). Host<->guest vsock protocol in `velnor-model/src/vsock_protocol.rs`; bins `velnor-guest-agent`, `velnor-guest-image`.
- MicroVM is narrower (execution.mdx:148-168): only script steps + checkout, actions/cache, rust-cache, sccache adapters in the guest; but `actions/cache` and `rust-cache` are refused at planning on microVM "because key-to-blob transport is not yet admitted"; no `lfs`, no host Docker socket, no mbx.
- No automatic fallback between backends (architecture.mdx:187).
- `node/` (controller.rs 9,244, slot, scheduler, guardian, watchdog, canary, microvm.rs) manages per-node slot processes and fleet; `velnor-control` persists journal/permits in a store with migrations.

### 4. Caching design (summary of what code/docs state)
- Host-persistent stores per trust scope (`StoreCatalog`, `stable_workspace.rs`, `trust_class.rs`, `stale_trust_scope.rs`); "Velnor ignores GitHub cache transport for host-persistent paths entirely" (execution.mdx:~283).
- GitHub-lane vs Velnor-lane: trusted producers (push to default, schedule, trusted dispatch) save GitHub-lane snapshots; PRs/fork PRs restore-only (execution.mdx:~277-281).
- Velnor-hosted cache service (gha_cache.rs) so unmodified `actions/cache@v4` / BuildKit `type=gha` work on self-hosted; ref->base scoping and fork isolation emulate GitHub's scope rules.
- Plan 2026-09-16 (`plans/2026-09-16-ci-workflow-and-cache-plan.md`:38): GitHub cache account at 10.54 GB / 109 entries (98% of 10 GiB cap) despite daily maintenance; mbx caches 22 entries / 5.34 GiB.
- Cold-start accounting in same plan (line 164): "First run / new host / new slot: One-time cold. Acceptable; N slots => N cold starts". Line 161: "`mise --yes install ...` runs every job (~20 s observed)".

### 5. Build and tests (this clone, this host)

Build: `cargo build --workspace` with `CARGO_TARGET_DIR=~/.cache/ci-ambition/evidence/legacy-target`, rustup 1.98.1 (rust-toolchain.toml pins 1.98.1, edition 2024), **success, 145 s wall (cold deps), 0 warnings**, `build.log` `exit=0 wall=145s`. Note: first attempts went through the mise shim and failed ("mise.toml not trusted"); rebuilt with PATH=~/.cargo/bin and MISE_DISABLE=1.
Dependencies include vendored OpenSSL (workspace Cargo.toml native-tls-vendored) and aws-lc-sys, sigstore.

`cargo test --workspace --no-fail-fast` (plain cargo test; project does not use it): first ~28 test binaries ok (e.g. control 379 pass 30.6 s); velnor-bench 191 pass / 1 fail; then velnor-runner lib (3,281 tests) deadlocked: 2,503 ok then 18 tests ("running for over 60 seconds", all `runner::tests::*` using the process-wide `github_http_transport_env` async mutex in `test_support.rs:46-53`) never finished; killed by PID after ~3 min. Project's own runner is nextest (mise.toml:45 `cargo nextest run --workspace --locked --features velnor-runner/test-support`; `.config/nextest.toml` only extends slow-timeout for one fuzz test).

`cargo nextest run --workspace --locked --features velnor-runner/test-support --no-fail-fast` (nextest 0.9.148 installed to a scratch root):
- **8,222 tests across 87 binaries, 5 skipped; 8,172 passed, 50 failed, 6 slow; 708 s run (747 s wall incl. compile)**. I SIGTERMed (by PID) the single remaining test `homebrew_preview_009 macos_build_contract_is_valid` after 515 s: it spawns a nested `cargo build --locked --release --bins -p velnor-runner -p velnorctl` and appears stuck; counted among the 50.
- Failures by binary: velnor-workflow 46 (44 lib, 1 in `velnor_first_ci`, 1 SIGTERM in `homebrew_preview_009`), velnor-runner 3, velnor-bench 1 (=50).
- Causes (from nextest.log):
  - 17 panics `ModuleNotFoundError: No module named 'tomllib'` (closure_inputs/consumer_negatives call `python3` stdin scripts; macOS system python < 3.11): host tooling, not a Linux/macOS code difference per se (`crates/velnor-workflow/src/lib.rs:874`, `consumer_negatives.rs:1039`).
  - ~11 candidate-sandbox / s2 policy tests: "run candidate in uid-dropped sandbox: no executable Docker CLI found in absolute PATH directories" (needs Docker + uid-drop; `policy/tests.rs:25`, `s2/policy/tests.rs:22`); expected Linux/Docker environment.
  - Hash-pin mismatches `left: None, right: Some("f2c566...")` (10 asserts): tests expecting a pinned runtime closure digest; cause not isolated.
  - `scan::file_walk ... allows_confined_symlinks` (file_walk.rs:1366): symlink handling differs.
  - velnor-runner: `trust_class_benchmark_derivation_throughput` (trust_class.rs:2197, 12.3 s), `workflow_command_parse_benchmark`, `idle_scaling::idle_resource_scaling_from_one_to_sixteen_slots_is_bounded` (idle_scaling.rs:769): timing/throughput-threshold tests failing on this host.
  - velnor-bench `drivers::docker::tests::unresolved_container_cleanup_retains_ownership` (docker.rs:2034) assertion left 0 / right 1.
  - Slow (>120 s): homebrew_preview_005/009, bootstrap_transport, velnor-tools github_raw_store, runner `acquired_null_timeline_keeps_recovery_state_when_completion_retries_exhaust`.
- No Linux/Docker/Firecracker run was attempted; Firecracker and multi-slot daemon paths cannot execute on macOS (no KVM; microvm guest needs Linux CI per guest.toml:1-2). Runner tests that need a Docker daemon are `#[ignore]` (`live_*` in docker/client.rs, 3 ignored in lib).
- Upstream CI numbers for comparison (plans/ci-performance/baseline.md, README.md:~20-26): runner job "checks 173 s" for 2,516 tests (nextest, Linux); later 2,522 runner / 1,955 generator tests.

### 6. Why archived and what problems it hit

No ARCHIVED/DEPRECATED note, README banner, or commit/PR/issue stating the reason exists in repo or GitHub. Evidence searched: `git log --all -i --grep='velnor-new|archiv|deprecat|supersed'` (no repo-level deprecation commit; hits are about archive parsing and superseded CI runs), `gh search issues` for "velnor-new", "archive", "deprecate", "rewrite" in the repo (no matches), repo grep for "velnor-new" (only .github/AGENTS.md, ci.yml, .velnor/release-manifest.json, all about the generator).
Decisive statements that do exist:
1. GitHub repo description (set at archive): "LEGACY, reference only. ... Not maintained. Current work: tailrocks/velnor-new" (isArchived=true; last push 2026-10-17T03:55:22Z).
2. Last two commits `459159ea`/`1b9fc038` (2026-10-17) "build: migrate to velnor-actions 0.1.10/0.1.12": legacy repo ends as a consumer of a generator whose source now lives in velnor-new (.github/AGENTS.md:4).
3. Before that: `e24509dc` "Consolidate all correction branches into main (#1139)" 2026-10-14; `84d41850` "Consolidation: integrate all useful branch work into main (#1138)" 2026-10-07; docs/operations/velnor-consolidation-2026-09-22.md: census of 29 non-main branches, 20 open PRs, 89+369 worktree records, 11 stashes, ~1040 unreachable objects reviewed before landing PRs #1069-#1071.
(The reason beyond these is not stated in the legacy repo. I did not read velnor-new.)

Problems visible in the repo (not claimed as archive reasons):
- Release/operations incidents (open issues; 10 open): #389 runner 0.1.211 supervised slots "never register - supervisors log 'supervising' then go silent (rolled back on sentry)", 50 inert slot processes, 9 daemons (2026-08-25). #329 runner 0.1.190 job container dies after ghaction-github-runtime step, exec hangs "container is not running" in docker-lease workflows (2026-08-25, reproduced 3x). #408 "runner: eliminate idle CPU": Sentry host 600-700% CPU with no jobs, ~9-10 daemons each 50-84% CPU; registration lost, JIT reconfig loops, broker session 409s, poll 404s (2026-08-26). #670 results-service rerun poisons job-log ownership across attempts, runner rejects rerun (0.1.273, 2026-09-12); #671 artifacts keyed by name without per-run scoping; #672 job log upload broken for failed jobs on velnor lane. #327 `store.job.transition.illegal` for jobs queued across fleet drain. #360 v0.1.199/200 tags structurally unpublished. #311 (closed) job artifact uploads silently lost on velnor lane. #348 (closed) BuildKit hangs at 'creating container' on lease proxy. #275 (closed) persistent mise store poisoned by interrupted cargo compile.
- Compatibility gaps stated in docs (execution.mdx:116-146): no arbitrary JS/Docker/composite actions, limited checkout inputs, no submodules, no macOS/Windows, no Kache; microVM further restricted.
- Scale-set lane exists because native lane could not run arbitrary actions: scale-set delegates acquired jobs to the official runner (architecture.mdx:144-147).
- Maintenance load signals: 1,070 commits in Sept 2026; 60 of last 100 commits are `fix`; runner.rs/executor.rs ~37k lines each; AGENTS.md says "No legacy code... Breaking changes are preferred".
- plans/ci-performance/reliability.md:1-8: objective "structural prevention of PR-green/main-red failures"; "No observed sample currently establishes" the 99.9999% green-main target; catalog of 7,594 retained runs (2026-06-04..2026-09-20) with 268 earlier attempts on 182 rerun runs; 30 earlier failed attempts hidden by later success.
- Same file ledger: `scaleset_daemon::crash_with_dead_workers_fails_explicitly` failing in PR run 35520025700; policy job waited 900 s for a candidate artifact then failed (Main CI 35520255650).
- Protocol breakage: no record found of a GitHub-side protocol break forcing the archive; protocol pinned to v2.337.0 and "may intentionally lag the newest actions/runner release" (runner-protocol.mdx:19-20).

### 7. Measured performance claims found (all with source)

Repo states no accepted speedup: plans/ci-performance/README.md:3 "Status: active; no performance improvement accepted yet. Started 2026-09-20." and "Substantive optimization iteration credit remains zero" (README.md:~27). Baseline.md:3-5: "starting observations, not matched-cohort estimates".
- Velnor PR run 35477566106 (GitHub-hosted ubuntu-24.04, attempt 1, head `eaae46c4`): trigger->`ci-required` 322 s; last `Control / Required` 363 s; aggregate execution of 9 non-skipped jobs 705 s; jobs: Rust velnor-workflow 274 s (MBX setup 65, checks 108, candidate prep 59), velnor-runner 208 s (checks 173), Docker 78 s, topology 68 s, control 33 s (Rust restore 7, MBX setup 5), model 29 s (restore 7). Required-job scheduling adds 45 s after last Rust completion (baseline.md:15-46). Campaign target: baseline/10 (36.3 s) "Targets and estimates are not achievements".
- Five later PR observations (README.md:11-22): trigger->required 443/541/559/546/539 s; aggregate 1,266-1,359 s; runner 413-525 s; generator 170-186 s. 68 job records (20 executed, 48 skipped). "No ... compiler-cache reuse is established"; "1,879/1,807 MBX operations not looked up and 131 bypasses" (README.md:~58-60).
- 5,538 runs in 30 days for tailrocks/velnor (baseline.md "Collection access").
- Plan 2026-09-16: nightly <2 h verified at run 35090298803 (plan:518); `mise` install ~20 s each job (line 161).
- Jackin macOS native job 985 s active (docs/goal-handoffs/...:787), neighbors 14m04s and 17m28s (:801).
- V-MBX-001 (plans/ci-performance/experiments/V-MBX-001.md): run 35481387575 Rust workflow unit 01:26:36-01:28:29 (113 s), runner unit 01:26:35-01:36:35 (600 s, 2,516 nextest tests), Docker unit 26 s; "not controlled baseline/candidate timing samples and do not establish a 10x result".
- velnor-bench (crates/velnor-bench/README.md): replaces `scripts/benchmark/benchmark.sh` which "never invoked Velnor, Docker, or a job - every number it produced was a statement about cargo". Rules: nothing simulated, mandatory environment identity, internal vs external latency never mixed, statistics only when sample supports. Remote-dispatch driver "does not exist" so most VelnorJob rows remain "unrun"; local lifecycle/persistent-host rows run, with broker/acquisition/checkout stages unobserved.
- Job startup time (broker->first step) and cache restore time for the native runner: no measured, recorded number found in repo docs/plans. Telemetry exists (`TelemetryLane` Github/Velnor split in velnor-model/telemetry.rs; "Restore: {restore_ms} ms" in native rust-cache report, executor.rs:9156; `live_engine_latency_bench`/`live_exec_latency_bench` ignored tests in docker/client.rs) but results are not committed.
- Docs guard: handoff doc :97, :1243 "do not label three runs a reliable p95 estimate", ">=3 reps, median/range, no invented p95/speedups" (goal-handoffs file, :599-600, :1457).

### 8. Verification notes
- LOC counts via `find crates/X -name '*.rs' | xargs cat | wc -l`; test counts from nextest summary line "8222 tests run: 8172 passed (6 slow), 50 failed, 5 skipped" (nextest.log).
- Processes I started were killed by PID only (cargo test PIDs 41372/34125/34115; test PID 9884 and its child cargo 97488). cargo-nextest installed under `legacy-raw/nextest-root` only; ~/.local/share/mise untouched; rex not run; no push.

## F. GitHub runner protocol surface and ARC

_Source file: protocol.md_

Sources and shorthand (all read-only):
- `R` = github.com/actions/runner @ 7afce9edc8 (2026-11-17), `src/runnerversion` = 2.338.0. Paths below are relative to `src/`.
- `A` = actions/actions-runner-controller @ 89f34c4f78 (2026-10-27). Paths relative to repo root.
- `S` = github.com/actions/scaleset @ af394d86 (tag v0.5.0, the Go client ARC imports: `A/go.mod:9`). Cloned to `~/.cache/ci-ambition/evidence/proto-raw/scaleset`.
- `W:` = public GitHub docs page fetched on 2026-11-18. Doc-page content was returned via a summarising fetch tool, so quotes are as returned by it.
- Not verifiable from these clones: anything the GitHub server does (broker, run service, results service, launch service). Only the client side of each call is visible. Server-only behaviour is marked "server side, not in clone".

### 1. Registration and identity

#### 1.1 Three runner configuration paths exist in the Listener
1. Classic `config.sh` with a registration token:
   - `ConfigurationManager.cs:153` calls `GetRunnerTokenAsync(command, inputUrl, "registration")` (impl `:710`).
   - Then `GetTenantCredential(inputUrl, registerToken, RunnerEvent.Register)` (`:154`, impl `:~820-880`).
   - `GetTenantCredential` does `POST {api}/actions/runner-registration`, body `{url, runner_event}`, header `Authorization: RemoteAuth <registration token>`. Host is `api.{host}` when hosted, `{host}/api/v3` on GHES (`:831-835`).
   - The response (`GitHubAuthResult`) supplies `TenantUrl` (stored as `ServerUrl`), an auth token, and `UseRunnerAdminFlow` (`:155-157`).
   - Removal uses a `"remove"` token (`:562, :579`).
2. "Runner admin" flow, when the server returns `UseRunnerAdminFlow`:
   - `RunnerDotcomServer.cs:62-80` posts to `{api}/actions/runners/register` with body `{url, group_id, name, version, updates_disabled, ephemeral, labels, public_key}`. On replace it adds `runner_id` and `replace`.
   - The result's `RunnerAuthorization.ServerUrl` becomes `ServerUrlV2`, and `UseV2Flow=true` is set (`ConfigurationManager.cs:287-291, :345-349`).
   - Runner groups: `GET {entity}/runner-groups` (`RunnerDotcomServer.cs:54`). Lookup by name: `GET {entity}/runners?name=` (`:47`). Delete: `{entity}/runners/{id}` (`:108`).
   - `{entity}` is `.../orgs/{o}/actions`, `.../repos/{o}/{r}/actions` or `.../enterprises/...` (`:181-223`).
3. JIT config (`run.sh --jitconfig <b64>`, or env `ACTIONS_RUNNER_INPUT_JITCONFIG`):
   - Listener reads `--jitconfig` (`Runner.cs:234`). The value is base64 of a JSON map `{filename: base64(content)}` (`Runner.cs:239-240`).
   - Each entry is written into the runner root dir and marked hidden (`:242-253`). On Windows the RSA params file is DPAPI-protected (`:246-251`).
   - Only then is `LoadSettings()` called (`:266`). No network call happens in this step.
   - Env-to-argument convention: any env var with prefix `ACTIONS_RUNNER_INPUT_` becomes a command-line arg (`CommandSettings.cs:111-138`).
   - ARC injects `ACTIONS_RUNNER_INPUT_JITCONFIG` from a Secret (`A/controllers/actions.github.com/constants.go:13`, `resourcebuilder.go:908-915`).
   - `IsHostedServer` for a JIT runner is inferred from `ServerUrl`/`ServerUrlV2` host suffixes (`.actions.githubusercontent.com`, `.githubapp.com`, `.ghe.com`, ...) (`Runner.Common/ConfigurationStore.cs:84-115`).

#### 1.2 Identity and credential files
- Settings persisted in `.runner`: `AgentId, PoolId, ServerUrl, GitHubUrl, ServerUrlV2, UseV2Flow, UseRunnerAdminFlow, Ephemeral, DisableUpdate, WorkFolder` (`Runner.Common/ConfigurationStore.cs:30-60`).
- Credential provider `OAuth` (`Runner.Listener/Configuration/OAuthCredential.cs:25-55`):
  - Reads `clientId`, `authorizationUrl`, optional `authorizationUrlV2`, `oauthEndpointUrl` from `.credentials`.
  - Signs with the RSA key from `.credentials_rsaparams` (`IRSAKeyManager`).
  - Builds `VssOAuthJwtBearerClientCredential(clientId, authorizationUrl, signingCredentials)` with grant `ClientCredentials`.
  - Wire constants: `grant_type=client_credentials`, `client_assertion_type=urn:ietf:params:oauth:client-assertion-type:jwt-bearer`, `client_assertion=<JWT>` (`Sdk/WebApi/WebApi/OAuth/VssOAuthConstants.cs:23,39,44,70`).
  - `authorizationUrlV2` replaces `authorizationUrl` only when `allowAuthUrlV2` is true and `HostContext.AllowAuthMigration` is true (`OAuthCredential.cs:33-36`). The broker connection is created with `allowAuthUrlV2: true` (`BrokerMessageListener.cs:~125`).
- A second provider, `OAuthAccessTokenCredential`, takes a literal `token` arg (`CredentialProvider.cs:32-51`).
- Session creation errors of type `VssOAuthTokenRequestException` are retried every 30 s up to 30 min (clock-skew allowance). Session conflicts are retried up to 4 min (`MessageListener.cs:52-55, :552-564`).
- An `AccessDeniedException` with `ErrorCode == 1` makes Listener exit with `TerminatedError` (exit 7 only if env `ACTIONS_RUNNER_RETURN_VERSION_DEPRECATED_EXIT_CODE` is truthy) (`Runner.Listener/Program.cs:142-167`, `Runner.Common/Constants.cs:162,280`). ARC sets that env to `1` on every runner pod (`A/.../resourcebuilder.go:918-921`).

#### 1.3 Runner scale sets API (client side, used by ARC via `S`)
Base endpoints (`S/client.go:23-24`): `_apis/runtime/runnerscalesets`, `_apis/distributedtask/pools/0/agents`. Default query `api-version=6.0-preview` (`S/client.go:394-395`).

| Operation | Method and path | Source |
|---|---|---|
| Get/list scale set by group + name | `GET /_apis/runtime/runnerscalesets?...` | `S/client.go:431` |
| Get scale set by id | `GET .../runnerscalesets/{id}` | `:495` |
| Create scale set | `POST` (CreateRunnerScaleSet) | `:576` |
| Update / delete scale set | `PATCH`-style update `:610`, `DELETE` `:642` | `:610,:642` |
| Runner groups lookup | `/_apis/runtime/runnergroups/` | `:522` |
| JIT config | `POST .../runnerscalesets/{id}/generatejitconfig`, body `{name, workFolder}`, 200 OK -> encoded config | `:765-796`, `types.go:93` |
| Get/delete runner | `/_apis/distributedtask/pools/0/agents/{id}` | `:798,:858` |
| Create message session | `POST .../runnerscalesets/{id}/sessions` | `session_client.go:45` |
| Delete / refresh session | `.../sessions/{sessionId}` | `:75, :90` |
| Get message (long poll) | `GET` session message path with query `lastMessageId`, header `X-ScaleSetMaxCapacity`, `Accept: application/json; api-version=6.0-preview` | `:143-168`, `client.go:39` |
| Delete (ack) message | `DELETE` message; 204 expected | `:215-240` |
| Acquire jobs | `POST .../runnerscalesets/{id}/acquirejobs` with request ids; 200 expected | `:278-303` |

Message poll semantics:
- 202 Accepted means no message (returns nil). 200 returns a message. 401 means the message-queue token expired and triggers a session refresh (`session_client.go:175-190`).
- Per-attempt poll timeout `MessagePollTimeout = 2 * time.Minute` (`S/http_client.go:73`).
- A message stays queued and is re-delivered until `DeleteMessage` (`session_client.go:100-103`).
- Message body types: `JobAvailable{acquireJobUrl}`, `JobAssigned`, `JobStarted{runnerId,runnerName}`, `JobCompleted{result,...}`. Common fields: `runnerRequestId, repositoryName, ownerName, jobId, jobWorkflowRef, jobDisplayName, workflowRunId, eventName, requestLabels` (`S/types.go:21-57`).
- Each message carries statistics: `totalAvailableJobs, totalAcquiredJobs, totalAssignedJobs, totalRunningJobs, totalRegisteredRunners, totalBusyRunners, totalIdleRunners` (`types.go:133-141`).

#### 1.4 Scale-set client authentication (not the runner's own credential)
- Registration-token bootstrap (`S/client.go`):
  - Org: `/orgs/{org}/actions/runners/registration-token`. Enterprise: `/enterprises/{e}/actions/runners/registration-token`. Repo: `/repos/{o}/{r}/actions/runners/registration-token` (`:1045-1053`).
  - That token is exchanged at `POST /actions/runner-registration` with `Authorization: RemoteAuth <token>`, body `{url, runner_event:"register"}`. This returns `{url, token}`, the Actions service URL plus an admin JWT (`:977-1010`).
  - `updateTokenIfNeeded` (`:1083`) refreshes the admin token. Admin requests send `Authorization: <adminToken.authorizationHeader>` (`:365`).
- Credential kinds accepted by the client (`S/client.go:141-165`):
  - PAT token, mutually exclusive with GitHub App.
  - GitHub App (client id + private key, JWT signed via `jwt_provider.go`, then `POST /app/installations/{id}/access_tokens` expecting 201) (`:937-966`).
- ARC docs state the PAT or App token "is no longer passed to the runner pod" (`A/docs/gha-runner-scale-set-controller/README.md:~24-28`).

### 2. Job acquisition and execution messages

#### 2.1 Two transports in the same binary
- Legacy "pipelines / distributed task" session: `MessageListener` uses `IRunnerServer.CreateAgentSessionAsync` and `DeleteAgentMessageAsync` (`Runner.Listener/MessageListener.cs:115, :429`).
  - Message bodies are encrypted with a per-session AES key. The key is RSA-wrapped with OAEP-SHA1, or OAEP-SHA256 for FIPS (`:458-495`).
- Broker session: `BrokerMessageListener` is used when `UseV2Flow` is set. It needs `ServerUrlV2`, else throws "ServerUrlV2 is not set" (`BrokerMessageListener.cs:~85-90`).
  - Session create: `TaskAgentSession{BrokerSession=true}` with name `"{machine} (PID: n)"` (`:~100-110`). `POST {broker}/session` (`Sdk/WebApi/WebApi/BrokerHttpClient.cs:143`).
  - Get message: `GET {broker}/message?sessionId&status&runnerVersion&os&architecture&disableUpdate` (`BrokerHttpClient.cs:70-101`).
  - Delete session: `DELETE {broker}/session?sessionId` (`:195-203`).
  - Acknowledge: `POST {broker}/acknowledge?...` with body `runnerRequestId` (`:225-259`).
  - Optional WebSocket long-poll probe: `ConnectRunnerLongPollWebSocketAsync` (`BrokerHttpClient.cs:174-180`, `Runner.Common/BrokerServer.cs:92-190`). Reconnect delay 10 s to 300 s (`BrokerServer.cs:46-47`).
  - Server can send `BrokerMigrationMessage` carrying a new `BrokerBaseUrl`. The runner then calls `UpdateConnectionIfNeeded` (`MessageListener.cs:264-273`).
- Broker host name `broker.actions.githubusercontent.com`: not hard-coded in the clone. The URL comes from `ServerUrlV2` in registration or JIT settings. Only suffix checks exist (`ConfigurationStore.cs:100-115`).
- Retry/backoff in get-message loop: random 15-30 s then 30-60 s (`MessageListener.cs:356-361`). Idle heartbeat log at 30 min (`:392-394`).

#### 2.2 Message types the Listener handles (`Runner.Listener/Runner.cs:626-838`)
`AgentRefresh` and `RunnerRefresh` (self-update), `PipelineAgentJobRequest` (full legacy job message inline), `RunnerJobRequest` (a reference), `JobCancel`, `HostedRunnerShutdown`, `ForceTokenRefresh`, `RunnerRefreshConfig`. Unknown types are logged and dropped (`:838`).

`RunnerJobRequest` body is `RunnerJobRequestRef{id, runner_request_id, should_acknowledge, run_service_url, billing_owner_id}` (`Runner.Listener/RunnerJobRequestRef.cs`).

Handling of `RunnerJobRequest` (`Runner.cs:699-780`):
1. If `should_acknowledge`: best-effort `POST /acknowledge`. For an ephemeral runner, a job-not-found response makes the runner exit successfully (`:711-729`).
2. If `run_service_url` is empty: use the legacy path (`IActionsRunServer.GetJobMessageAsync`) against `settings.ServerUrl` (`:731-738`).
3. Else: connect to `run_service_url` with V2 creds and call `RunServer.GetJobMessageAsync(runnerRequestId, billingOwnerId)` (`:742-750`).
   - Run service calls `POST {run_service_url}/acquirejob` with `{jobMessageId, runnerOS, billingOwnerId}` (`Sdk/RSWebApi/RunServiceHttpClient.cs:70-90`).
   - 404, 409 and 422 map to JobNotFound, AlreadyAcquired and Unprocessable. These are skipped and throttled by `_acquireJobThrottler` (`Runner.cs:756-770`, `RunServiceHttpClient.cs:100-112`).
   - Other errors disable auth migration for 60 min (`Runner.cs:772-778`).
4. `jobDispatcher.Run(jobRequestMessage, runOnce)`. In run-once or ephemeral mode, later job messages are not deleted/served (`Runner.cs:701-705`).

#### 2.3 AgentJobRequestMessage shape (`Sdk/DTPipelines/Pipelines/AgentJobRequestMessage.cs`)
Constructor fields (`:27-47`) and properties:
- Identity and routing: `MessageType` (`PipelineAgentJobRequest` or `RunnerJobRequest`, `JobRequestMessageTypes.cs:7-8`), `Plan` (TaskOrchestrationPlanReference: planId, scope), `Timeline` (TimelineReference), `JobId`, `JobDisplayName`, `JobName`, `RequestId`, `LockedUntil`, `BillingOwnerId` (`:92-169, :250`).
- Containers and outputs (all `TemplateToken`, i.e. unevaluated workflow-template trees): `JobContainer`, `JobServiceContainers`, `JobOutputs` (`:134-148`). Also `Snapshot` (`:243`).
- `Resources` = `JobResources{Containers, Endpoints (ServiceEndpoint), Repositories}` (`JobResources.cs:15-45`).
- `ContextData`: `IDictionary<string, PipelineContextData>`, holding github/needs/inputs/vars/secrets and similar (`:177`).
- `Workspace` options (`:184`). `EnvironmentVariables` and `Defaults` as `IList<TemplateToken>` (`:208, :223`).
- `MaskHints`: list of `{Type: Variable=1|Regex=2, Value}` (`:193`, `MaskHint.cs`, `MaskType.cs:6-12`).
- `ActionsEnvironment` (`:236`), `ActionsDependencies` (`:288`), debugger fields `EnableDebugger, DebuggerTunnel, DebuggerWelcomeMessage` (`:257-279`).
- Serialised extras: `Variables` (map to `VariableValue{value,isSecret}`), `Steps` (`IList<JobStep>`), `FileTable` (`IList<string>`, the source-file index for step locations), `JobSidecarContainers` (`:486-508`).
- Step shape: `ActionStep : JobStep` with `Reference` (`ContainerRegistryReference{Image}` | `RepositoryPathReference{Name,Ref,RepositoryType,Path}` | `ScriptReference`; `ActionSourceType` 1/2/3), `DisplayNameToken`, `ContextName`, `Environment`, `Inputs` (TemplateTokens), `Background` (`ActionStep.cs:34-54`, `ActionStepDefinitionReference.cs:10-26,37-134`). `JobStep` adds `Condition`, `ContinueOnError`, `TimeoutInMinutes`, `ParallelGroupId` (`JobStep.cs:29-50`).
- Well-known endpoint: `SystemVssConnection` in `Resources.Endpoints` (`WellKnownServiceEndpointNames.cs`). Its `Url`, `Authorization.Parameters["AccessToken"]` and `Data` map (`CacheServerUrl`, `PipelinesServiceUrl`, `GenerateIdTokenUrl`, `ResultsServiceUrl`, `FeedStreamUrl`) drive all runtime env and services (see section 4).
- Job variables the worker reads for services: `system.github.results_endpoint`, `system.github.results_upload_with_sdk`, `system.github.launch_endpoint`, `actions_uses_cache_service_v2`, `actions_cache_mode`, `Actions.EnableHttpRedirects` (`JobServerQueue.cs:121-137`, `JobRunner.cs:68-73, :~97`, `NodeScriptActionHandler.cs:76-83`).

#### 2.4 Job lock renewal, completion, cancellation
- Listener spawns `Runner.Worker spawnclient <pipeOut> <pipeIn>` and sends the job message over an anonymous-pipe `ProcessChannel` (`JobDispatcher.cs:424-480`). The Worker exits after 30 s if no message arrives (`:432`, `Runner.Worker/Worker.cs:23,60`).
- The Listener refuses to start the worker until the first renewal succeeds (`JobDispatcher.cs:380-410`).
- Renewal, run service path: `POST {run_service_url}/renewjob` returns `RenewJobResponse{lockedUntil}` (`RunServiceHttpClient.cs:184-200`, `RenewJobResponse.cs`).
  - Loop interval is 60 s (`JobDispatcher.cs:775-776`).
  - First-renew failure: 5 retries at about 1-10 s backoff (`:798-822`).
  - Later failures: retry until `lockedUntil + 5 min` with 5-15 s backoff, or 15-30 s after more than 5 errors (`:807-840`).
  - Job-not-found stops renewal (`:781`). The legacy path has the same shape with `IRunnerServer` (`:856-930`).
- Completion, run service path: the Worker (not the Listener) calls `POST {run_service_url}/completejob` with `CompleteJobRequest{PlanID, JobID, Conclusion, Outputs, StepResults[], Annotations[], Telemetry[], EnvironmentUrl, BillingOwnerId, InfrastructureFailureCategory}` (`RunServiceHttpClient.cs:123-160`, `Contracts/CompleteJobRequest.cs:13-40`, `JobRunner.cs:290-332`).
  - `StepResult` = `{ExternalID, Number, Name, ActionName, Ref, Type, Status, Conclusion, StartedAt, CompletedAt, CompletedLogURL, CompletedLogLines, Annotations}` (`Contracts/StepResult.cs:14-52`).
  - Retries in `JobRunner.CompleteJobAsync` use 5 s waits (`JobRunner.cs:357, :444`).
  - Legacy path completes via `CompleteJobRequestAsync` in the Listener (`JobDispatcher.cs:600`).
- Cancellation:
  - Server sends `JobCancel{JobId, Timeout}`. Listener calls `WorkerDispatcher.Cancel(timeout)` (`JobDispatcher.cs:140-155`, `Runner.cs:790-800`).
  - Listener-initiated cancel on shutdown uses a 60 s timeout (`JobDispatcher.cs:180`). After worker exit it waits up to 45 s (`:306`).
  - Listener sends `CancelRequest`, `RunnerShutdown` or `OperatingSystemShutdown` over the pipe to the Worker (`Worker.cs:122-134`).

### 3. Reporting

#### 3.1 Worker-side queue (`Runner.Common/JobServerQueue.cs`)
Four independent dequeue loops, started in `Start` (`:170-181`):

| Queue | Cadence | Source |
|---|---|---|
| Web console lines | 250 ms for the first 4*60 dequeues ("aggressive"), then 500 ms; queue cap 1024 lines | `:32-33, :91-94, :242` |
| Timeline record updates | 500 ms | `:34` |
| Legacy file/attachment upload | 1000 ms | `:35` |
| Results (blob) uploads | 1000 ms | `:36` |

- Web console lines and file uploads are "always best effort" (`:185, :202`).
- On the run service path the Worker starts the queue with `resultsServiceOnly: true` (`JobRunner.cs:85`). It skips legacy websocket init (`JobServerQueue.cs:115-117`) and takes `FeedStreamUrl` from the `SystemVssConnection` data (`:131-135`).

#### 3.2 Timeline, logs and live feed (legacy "pipelines" services)
Interface `IJobServer` (`Runner.Common/JobServer.cs:30-38`): `CreateTimeline`, `UpdateTimelineRecords`, `GetTimeline`, `CreateLog`, `AppendLogContent`, `CreateAttachment`, `AppendTimelineRecordFeed`, `RaisePlanEvent`, `ResolveActionDownloadInfo`.
- Live feed: if `SystemVssConnection.Data["FeedStreamUrl"]` exists, lines go over a `ClientWebSocket`. Otherwise REST `AppendTimelineRecordFeed` (`JobServer.cs:172-192, :229-293`, `ResultsServer.cs:287`). The frame payload wraps `{stepId, lines, startLine}` (`TimelineRecordFeedLinesWrapper`, `ResultsServer.cs:~224`).
- Timeline model files: `Timeline.cs`, `TimelineRecord.cs`, `TimelineRecordState.cs` in `Sdk/DTWebApi/WebApi/`.

#### 3.3 Results service (twirp) (`Sdk/WebApi/WebApi/ResultsHttpClient.cs:610-632`)
Base is the URL in job variable `system.github.results_endpoint` (`JobServerQueue.cs:121`).
- `twirp/results.services.receiver.Receiver/`:
  - `GetStepSummarySignedBlobURL`, `CreateStepSummaryMetadata`
  - `GetStepLogsSignedBlobURL`, `CreateStepLogsMetadata`
  - `GetJobLogsSignedBlobURL`, `CreateJobLogsMetadata`
  - `GetJobDiagLogsSignedBlobURL`
- `twirp/github.actions.results.api.v1.WorkflowStepUpdateService/WorkflowStepsUpdate` (step status updates).
- The signed-URL responses carry a blob URL plus `BlobStorageType`. Upload then goes straight to storage:
  - Azure block blob upload with header `x-ms-blob-type: BlockBlob` (`:270`).
  - Append blob create/append for step and job logs. Header `x-ms-blob-sealed` is used when finalising (`ResultsHttpClient.cs:291-361, :628-632`). The Azure SDK `AppendBlobClient` is used when `system.github.results_upload_with_sdk` is true (`:210-223`).
- Timestamp format `yyyy-MM-dd'T'HH:mm:ss.fffK`; default network timeout 30 s; blob upload retries 3 (`:612-627`).
- Step summaries: `GITHUB_STEP_SUMMARY` file is read by `FileCommandManager` and uploaded via `CreateResultsStepSummaryAsync` (`FileCommandManager.cs:~278-290`, `ResultsServer.cs:88-97`).

#### 3.4 Annotations and outputs
- Annotations: `Issue` objects (error/warning/notice) are collected in the ExecutionContext. Per-type cap 10 (`_maxCountPerIssueType`), message cap 4096 chars "since we can't forward them ... to check annotation" (`Runner.Worker/ExecutionContext.cs:144-146`).
- They ship inside `CompleteJobRequest.Annotations` and per `StepResult.Annotations`. The check-run rendering itself is server side, not in clone.
- Job outputs: `CompleteJobRequest.Outputs` (`Dictionary<string, VariableValue>`) (`RunServiceHttpClient.cs:123-146`).
- Step outputs, env, path and state use the file commands `GITHUB_OUTPUT, GITHUB_ENV, GITHUB_PATH, GITHUB_STATE, GITHUB_STEP_SUMMARY` handled in `Runner.Worker/FileCommandManager.cs`. Blocked env keys are rejected (`:181`).
- Telemetry: `JobTelemetry` and `ActionsStepTelemetry` are sent with completion (`Sdk/DTWebApi/WebApi/JobTelemetry.cs`).

### 4. Server-side services used by actions, and the env handed to them

#### 4.1 Env vars set by the Worker on action processes
In `NodeScriptActionHandler.cs:54-93` and `ContainerActionHandler.cs:222-251`, all taken from `SystemVssConnection`:

| Env var | Source | Condition |
|---|---|---|
| `ACTIONS_RUNTIME_URL` | `systemConnection.Url`, replaced by `Data["PipelinesServiceUrl"]` if set | always |
| `ACTIONS_RUNTIME_TOKEN` | `Authorization.Parameters["AccessToken"]` | always |
| `ACTIONS_CACHE_URL` | `Data["CacheServerUrl"]` | if non-empty |
| `ACTIONS_RESULTS_URL` | `Data["ResultsServiceUrl"]` | if non-empty |
| `ACTIONS_ID_TOKEN_REQUEST_URL` | `Data["GenerateIdTokenUrl"]` | if non-empty |
| `ACTIONS_ID_TOKEN_REQUEST_TOKEN` | same AccessToken as runtime token | with the above |
| `ACTIONS_CACHE_SERVICE_V2=True` | job variable `actions_uses_cache_service_v2` | node actions only (`NodeScriptActionHandler.cs:76-79`) |
| `ACTIONS_CACHE_MODE` | job variable `actions_cache_mode` | node actions only (`:81-84`) |
| `ACTIONS_ORCHESTRATION_ID` | variable `system.orchestrationid`, gated by runner feature flag `SetOrchestrationIdEnvForActions` | node, container and script handlers |

- `run:` script steps get only `ACTIONS_ID_TOKEN_REQUEST_URL/TOKEN` and `ACTIONS_ORCHESTRATION_ID` (`ScriptHandler.cs:313-327`). They do not get runtime token, cache or results URLs.
- `GITHUB_TOKEN` is not set by this code path. It reaches steps via the `github.token` context in `ContextData` and expression `${{ secrets.GITHUB_TOKEN }}`; the job message carries it. (The clone has no `GITHUB_TOKEN` string match in Worker/Common/Sdk .cs files.)

#### 4.2 What the runner does NOT implement
- Cache v2, artifacts v4 and OIDC token minting are server endpoints called by action code (the actions toolkit) using the env above. The runner clone has no cache or artifact client. It only has `ArtifactsListFileCommand.cs`, `CreateArtifactsFileCommand.cs`, `ArtifactSubject.cs` (file-command helpers for attestation subjects) in `Runner.Worker/`.
- Facts about cache v2 shape (twirp `CacheService`, `ACTIONS_RESULTS_URL`) are not in the runner clone. The runner only forwards `ACTIONS_RESULTS_URL` and `ACTIONS_CACHE_SERVICE_V2` (above). Not verified further here.

#### 4.3 Launch service (action resolution)
- `POST {launch_endpoint}/actions/build/{planId}/jobs/{jobId}/runnerresolve/actions` with `Bearer` token and JSON `ActionReferenceList` (`Sdk/WebApi/WebApi/LaunchHttpClient.cs:35,42,85`). `launch_endpoint` is variable `system.github.launch_endpoint` (`JobRunner.cs:68`).
- On the legacy path the same resolution goes through `IJobServer.ResolveActionDownloadInfoAsync` (`ActionManager.cs:1066-1075`).
- Behaviour: 3 attempts; 422 (`UnresolvableActionDownloadInfoException`) and non-retryable rate-limit errors are not retried (`ActionManager.cs:1060-1092`, `LaunchHttpClient.cs:54-73`).

### 5. What the Worker does locally

#### 5.1 Expression and template evaluation
- Worker evaluates `if:` conditions, step display names, `continue-on-error`, env/inputs templates, container and service container tokens and `defaults` locally (`Runner.Worker/PipelineTemplateEvaluatorWrapper.cs:58-75`, `ConditionTraceWriter.cs`).
- The wrapper runs a legacy evaluator (`Sdk/DTPipelines`, `DTExpressions2`, `DTObjectTemplating`) and a new one (`Sdk/WorkflowParser`) and compares them (`EvaluateAndCompare`, `PipelineTemplateEvaluatorWrapper.cs:63-75`). Same dual pattern for action manifests (`ActionManifestManagerWrapper.cs:569`).
- `hashFiles()` is implemented by spawning the bundled node (`Runner.Worker/Expressions/HashFilesFunction.cs:73,206`).
- Format of context data (`github`, `runner`, `job`, `steps`, `env`, `matrix`...) is from `ContextData` in the job message plus runner-set `runner.*` (`JobRunner.cs:~145-165`) and `GitHubContext.cs`, `StepsContext.cs`.
- Secrets masking uses `MaskHints` plus every secret `Variable` value (`ISecretMasker` in `Sdk/Expressions2`).

#### 5.2 Action download and resolution
- Worker runs `PrepareActionsAsync` for all steps before the first step, recursing into composite actions and Docker `uses` (`JobExtension.cs:~299`, `ActionManager.cs:79-181`).
- Per-job dedupe: same `owner/repo@ref` resolved once (`ActionManager.cs:84-87`).
- Download: the resolve call returns `ActionDownloadInfo{NameWithOwner, Ref, ResolvedSha, TarballUrl, ZipballUrl, PackageDetails (immutable action package)}`. Linux/macOS downloads `TarballUrl` to `_actions/_temp_<guid>/<guid>.tar.gz`, then extracts with `tar -xzf`. Windows uses `ZipballUrl` (`ActionManager.cs:1195-1360`).
- A completion watermark file avoids re-download (`:1195`).
- Optional pre-seeded cache: env `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`, optional symlink mode (`:1236-1290`, `Runner.Common/Constants.cs:293`).
- Docker actions: `docker pull` or `docker build`, 3 attempts with back-off (`ActionManager.cs:937-1011`).

#### 5.3 Node, composite, container, script
- Manifest `using:` accepts `node12|node16|node20|node24`, `docker`, `composite` (`ActionManifestManager.cs:464-518`).
- All JavaScript actions are forced to `node24` regardless of the manifest: `nodeData.NodeVersion = "node24"` (`Runner.Worker/Handlers/HandlerFactory.cs:58-59`).
- Only node24 is bundled in this release: `NODE24_VERSION="24.22.3"` (`Misc/externals.sh:9`), `BuiltInNodeVersions = {"node24"}` (`Runner.Common/Util/NodeUtil.cs:7`). Git history shows a commit "remove bundled node20" (`cf65f3e`, `git log -- src/Misc/externals.sh`).
- Alpine variant `node24_alpine` is also fetched for Linux x64 (`externals.sh:169`). Internal node use is `node24` for hashFiles and macOS invoker (`NodeUtil.cs:9-11`, `ScriptHandler.cs:308`).
- Env `ACTIONS_RUNNER_FORCE_ACTIONS_NODE_VERSION` is documented as affecting internal functions (`Constants.cs:289-290`).
- Composite actions: `CompositeActionExecutionData*`, steps are inlined into the job's step queue with cached pre/post steps (`ActionManager.cs:820-845, CachedEmbedded*`; design docs `docs/adrs/1144-composite-actions.md`, `0549-composite-run-steps.md`, `1438-conditional-composite.md`).
- Container actions and job/service containers run through the local `docker` CLI (`Runner.Worker/Container/DockerCommandManager.cs`). The job creates a network with `docker network create` (`ContainerOperationProvider.cs:410`) and prunes networks (`:86`). Container-start is a pre-job step "Initialize containers" (`JobExtension.cs:~325-335`).
- Plugin steps (`ActionExecutionType.Plugin`) run via `RunnerPluginHandler` (`HandlerFactory.cs:68-72`).

#### 5.4 Hooks
- Job hooks: env `ACTIONS_RUNNER_HOOK_JOB_STARTED` becomes a pre-job step named "Set up runner" (`JobExtension.cs:304-313`); `ACTIONS_RUNNER_HOOK_JOB_COMPLETED` a post-job step "Complete runner" (`:573-580`, names in `Constants.cs:245-246`). Design doc: `docs/adrs/1751-runner-job-hooks.md`.
- Container hooks: active only if env `ACTIONS_RUNNER_CONTAINER_HOOKS` is set AND job variable `DistributedTask.AllowRunnerContainerHooks` is true (`Runner.Worker/FeatureManager.cs:8-13`, `Constants.cs:170, :247`).
  - Hook file must end in `.js`, `.sh` or `.ps1` (`ContainerHookManager.cs:190-196`).
  - Commands: `prepare_job`, `cleanup_job`, `run_script_step`, `run_container_step` (`HookInput.cs:21-27`).
  - Protocol: the hook is run as a script step; JSON `HookInput{command, responseFile, args, state}` is passed on stdin; the response JSON is read from a temp file under `_temp/<folder>/<guid>.json`; response `state` is saved as `ContainerHookState` and passed to the next call (`ContainerHookManager.cs:131-170`, `:216-232`).
  - When enabled, `ContainerOperationProvider`, `ActionManager` (`:143, :672`) and `ContainerActionHandler` bypass the Docker CLI path. Design doc `docs/adrs/1891-container-hooks.md`.

### 6. Startup costs between assignment and first step

Sequence on a runner that already holds a session (Listener, `Runner.cs`, `JobDispatcher.cs`):
1. Long-poll `GET /message` returns `RunnerJobRequest` (or `PipelineAgentJobRequest`).
2. Optional `POST /acknowledge` (best effort).
3. `POST /acquirejob` to run service, returns the full job message (`Runner.cs:742-750`).
4. `JobDispatcher.Run` -> `RunAsync`: start renewal loop and wait for first successful renew (`JobDispatcher.cs:380-410`).
5. Start the `Runner.Worker spawnclient` child process (new .NET process) and send the job message over the pipe (`:424-480`). Worker start timeout is 30 s (`Worker.cs:23`).
6. Worker: connect run server, init `JobServerQueue` (results client, websocket feed), create ExecutionContext (`JobRunner.cs:60-130`).
7. `JobExtension.InitializeJob`: resolve and download actions, build step list, container setup (`JobExtension.cs:~299-340`).
8. If `GenerateIdTokenUrl` is present, wait up to 1 s for the job record update (`JobRunner.cs:~185-190`).
9. Steps run in `StepsRunner`.
- In ARC, a pod must also be created and the JIT config registered before step 1 (section 7).
- Perf counters written to the diag log by name: `MessageReceived_*`, `JobRequestRenewed_*`, `WorkerJobServerQueueStarted_*`, `WorkerJobInitialized_*` (`Runner.cs:625`, `JobDispatcher.cs:421`, `JobRunner.cs`).
- Ephemeral/run-once: the Listener accepts one job then exits (`Runner.cs:701-705, :720-727`).

#### 6.1 Auto-update and version enforcement
- Client side:
  - The Listener sends its version in every broker get-message and acknowledge (`BrokerMessageListener.cs:~300`, `BrokerHttpClient.cs:86, 239`) together with `os`, `architecture`, `disableUpdate`.
  - Update is triggered only when the server sends `RunnerRefresh{target_version, download_url, sha256_checksum, os, ...}` (or legacy `AgentRefresh`) (`Sdk/DTWebApi/WebApi/RunnerRefreshMessage.cs:12-41`, `Runner.cs:626-679`).
  - `SelfUpdater` downloads (retries `RunnerDownloadRetryMaxAttempts`), waits for the current job, writes an update script, exits and relaunches (`SelfUpdater.cs:60-131, :187-241`).
  - A comment states runners must be older than the server's version to get a refresh message (`SelfUpdater.cs:197`).
  - `SelfUpdaterV2.cs` exists as a second implementation.
  - `--disableupdate` sets `DisableUpdate` and the server must support it (`ConfigurationManager.cs:315, :373`, `Runner.cs:1169`).
- Rejection side (server side, not in clone):
  - The clone shows only the client's reaction: `AccessDeniedException(ErrorCode==1)` exits as deprecated (`Program.cs:142-167`).
  - W: "you must update your runner version within 63 days of a newer version being made available"; otherwise "the GitHub Actions service will not assign jobs to it"; critical security updates can block assignment until updated (docs.github.com/en/actions/reference/runners/self-hosted-runners, "Runner software updates on self-hosted runners").
  - W: auto-update happens "when a job is assigned" or "within a week of release if the runner hasn't been assigned any jobs" (same page).
  - W: the REST API per-version fields: `registration_deprecates_at` ("when you can no longer register a runner using that version") and `runtime_deprecates_at` ("when runners on that version can no longer receive jobs") (same page).

### 7. Actions Runner Controller (scale sets mode)

#### 7.1 Components (A@89f34c4f78)
- Controller manager reconcilers in `controllers/actions.github.com/`: `autoscalingrunnerset_controller.go`, `autoscalinglistener_controller.go`, `ephemeralrunnerset_controller.go`, `ephemeralrunner_controller.go`.
- Listener binary `cmd/ghalistener` (main, `scaler/scaler.go`, `config/`, `metrics/`). Runs as a pod built by `resourcebuilder.go:330-476` (`newScaleSetListenerPod`).
- Chart `charts/gha-runner-scale-set` (scale set) and `charts/gha-runner-scale-set-controller`. Experimental charts also exist.
- Documented flow (`A/docs/gha-runner-scale-set-controller/README.md:9-19`):
  1. `AutoScalingRunnerSet` controller fetches the runner group id and gets/creates the scale set in the Actions Service.
  2. `AutoScaling Listener Controller` deploys the listener pod, which opens a long-poll HTTPS connection.
  3. Listener receives `Job Available`, acquires jobs within its advertised capacity, patches `EphemeralRunnerSet` replicas through the Kubernetes API.
  4. `EphemeralRunner` controller requests a JIT config and creates a runner pod.
  5. Pod registers via the JIT token, long-polls for the job.
  6. After completion the controller checks with the service whether the runner can be deleted.
- Messages are acknowledged "only after their job updates and desired runner count have been successfully handled" (README `:14`; `listener.go` deletes the message after `Scale` succeeds, `S/listener/listener.go:169-177`).

#### 7.2 Scale-up latency path (sequential steps in code)
1. Message arrives by long-poll (`S/listener/listener.go:159-169`).
2. `Scaler.Scale` (`A/cmd/ghalistener/scaler/scaler.go:135-186`): record statistics; if `JobAvailable` messages exist call `AcquireJobs` (`:139-153`); handle JobStarted and JobCompleted; then `HandleDesiredRunnerCount(TotalAssignedJobs)` (`:170`).
3. Target count formula: `targetRunnerCount = min(MinRunners + count, MaxRunners)` (`scaler.go:339`). Count is `TotalAssignedJobs`.
4. Listener merge-patches `EphemeralRunnerSet.spec.replicas` plus a `PatchID` (`scaler.go:270-310`).
5. `EphemeralRunnerSetReconciler` creates `EphemeralRunner` objects when `scaleUpTotal < Spec.Replicas` (`ephemeralrunnerset_controller.go:341-359`). Batch concurrency is 8 for creates and deletes (`:68, :1007-1041`).
6. `EphemeralRunnerReconciler` calls `GenerateJitRunnerConfig` (`ephemeralrunner_controller.go:834-860`), stores it in a Secret (`:305-316`), then creates the pod (`createPod :896-953`). Pod env: `ACTIONS_RUNNER_INPUT_JITCONFIG` from the Secret.
7. Quota exceeded: requeue in 30 s (`:430`). Ordinary reconcile errors back off exponentially, capped at 30 s (README `:48`).
8. Pod pulls image, runs `run.sh`, Listener registers via JIT, long-polls, receives job (section 2/6).
- Controller service calls have a 10 s deadline and are not retried in-client (README `:44`).
- Failure handling: a failed runner pod is retried; more than `maxFailures = 5` recorded failures deletes the EphemeralRunner so it is recreated (`ephemeralrunner_controller.go:92-102, :364-367`). README: "controller retries up to 5 times. After 24 hours the Actions Service unassigns the job if no runner accepts it" (`:19`).
- Scale to zero: with `minRunners=0` and no assigned jobs the target is 0 (`scaler.go:339`; `ephemeralrunnerset_controller.go:633` comment on idle publishing).

#### 7.3 Pod per job, min/max
- Each runner is an `EphemeralRunner` with its own pod; `RestartPolicy: Never` default (`resourcebuilder.go:446`). Labels include `actions-ephemeral-runner=True` and a pod-template hash (`:~886-895`).
- The runner is registered as ephemeral: one job then exit.
- `minRunners` semantics were redefined as minimum idle runners; total = assigned jobs + `minRunners`, capped by `maxRunners` (`A/docs/adrs/2023-11-02-min-runners-semantics.md`; chart `values.yaml:60-65`).
- Name limit: "The name is limited to 45 characters" (W: docs.github.com/en/actions/concepts/runners/actions-runner-controller).

#### 7.4 Container modes (`charts/gha-runner-scale-set/values.yaml:114-130`, `templates/_helpers.tpl`)
- `containerMode.type` in `dind | kubernetes | kubernetes-novolume`.
- `dind`:
  - Pod gets an init container copying `/home/runner/externals/.` into a shared volume, a `docker:dind` sidecar container, shared `dind-sock` and `dind-externals` volumes, and a readiness probe running `docker info` (`_helpers.tpl:250-297, :265-281`; `values.yaml:345-402`).
  - Needs privileged sidecar by chart design (not asserted here beyond the template existing).
- `kubernetes`:
  - Sets `ACTIONS_RUNNER_CONTAINER_HOOKS=/home/runner/k8s/index.js`, `ACTIONS_RUNNER_POD_NAME` (fieldRef) and `ACTIONS_RUNNER_REQUIRE_JOB_CONTAINER` (`_helpers.tpl:494-508`; `values.yaml:404-432`).
  - Uses a work volume PVC (`kubernetesModeWorkVolumeClaim`).
  - `kubernetes-novolume` sets the same hook env with `ACTIONS_RUNNER_IMAGE` (`_helpers.tpl:571-607`, `values.yaml:434-455`). Cleanup annotations exist for the per-scale-set role, binding and service account (`constants.go:78-80`).
- runner-container-hooks k8s package (W: github.com/actions/runner-container-hooks `packages/k8s/README.md`):
  - Launches container workflows as separate Kubernetes resources; requires RBAC for pods, pods/exec, pods/log and secrets.
  - `ACTIONS_RUNNER_REQUIRE_JOB_CONTAINER` set to true "to prevent the runner from running any jobs outside of a container".
  - Runner pod maps a PVC into `_work`; claim name via `ACTIONS_RUNNER_CLAIM_NAME`, default `${ACTIONS_RUNNER_POD_NAME}-work`.
  - Listed limitations: every job must use a job container; building container actions from a Dockerfile is not supported; container actions get no access to the services or job container network; docker create options not supported; container actions must declare their own entrypoint; action images need `sh`, `env`, `tail`.

#### 7.5 Documented architectural limits found in the repos
- One message session per scale set and a single message queue: the Listener is one pod per scale set with `lastMessageId` ordering (`S/listener/listener.go:151-177`).
- Scale set name length 45 (W above).
- Runner pod creation depends on the Kubernetes API and image pull; ARC has no warm-process pool other than `minRunners` idle pods (`values.yaml:63-65`, ADR 2023-11-02).
- Pod-failure retry limit 5; job unassigned after 24 h with no runner (README `:19`).
- Controller-to-Actions-service call deadline 10 s, no client retry; rate-limited requests requeued (README `:44-56`, PR #4726 cited there).
- Windows runner doc exists: `A/docs/configuring-windows-runners.md` (belongs to legacy mode; not examined in detail).
- GitHub-hosted docs (W) do not state whether service containers or container actions work in Kubernetes mode; the hooks README lists the limits above.

### Gaps not verifiable from these clones
- Broker/run-service/results-service server behaviour: long-poll duration on the server, lock duration (`LockedUntil` is server-assigned), maximum job message size, rate limits.
- Cache v2 twirp method names and artifact v4 API: implemented in actions toolkit repos, not the runner.
- `GITHUB_TOKEN` provenance: only observable as job message content, not found in runner code by string match.
- Exact version-rejection mechanics beyond the 63-day documentation statement.
