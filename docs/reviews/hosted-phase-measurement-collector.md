# Hosted GitHub Actions phase measurements

This standard-library Python collector analyzes retained GitHub Actions API
responses and raw per-step logs. It is read-only: it does not call GitHub, start
workflows, run repository tasks, or alter generated CI.

## Evidence and identity

The collector binds the run, attempt, head SHA, repository, job API rows, and
retained log archive. Optional workflow evidence binds the path, Git blob ID,
and exact content bytes. Optional final-report artifacts and task reports are
accepted only when their run, attempt, and source identity match. Run it on
retained input files; keep those files and the collector's JSON output in the
separate evidence packet.

## What the observations mean

- Job and step durations come from GitHub API `started_at` and `completed_at`.
  A step duration is its enclosing API span, not a transfer-only interval.
- `created_at` to `started_at` is reported as unclassified prestart time.
  It can include dependency waiting and runner startup. Queue duration remains
  `null` because these API records do not split those causes.
- The earliest-start to latest-completion envelope is labeled an observed job
  span, not a workflow critical path. A DAG path is reported only when the
  supported rendered `needs` graph maps exactly to successful run jobs and
  required job/step/log evidence is complete. It excludes the initial prestart
  interval from run creation to the first path job start. For later jobs, it
  includes the gap from the latest dependency completion to child start as
  `unclassified_post_dependency_ready_wait_ms`. That gap may contain scheduling
  or runner startup delay; it is not isolated queue time, and it contributes to
  the DAG duration. If dependencies finish together, the path selects the one
  with the greatest accumulated path duration, then the ascending source job
  ID. Per-phase sums across jobs can overlap and are not added to that path.
- Phase labels use normalized, exact whole-step-name matching. In particular,
  `Restore MBX objects` is restore and `Post Restore MBX objects` is upload.
- Network byte totals use only explicit cumulative `Received N of M` and
  `Sent N of M` counters in matching step logs. Progress snapshots are counted
  once per step at their maximum. Restore and save directions stay separate.
  API cache sizes, artifact/archive storage sizes, and MBX local storage are
  never treated as network bytes. MBX remote object counters are a separate
  measurement. The v4 MBX parser accepts byte, decimal SI, and binary IEC
  suffixes (`B`, `kB`/`KB`, `KiB`, `MB`, `MiB`, `GB`, `GiB`, `TB`, `TiB`),
  and the pinned hit, miss, not-looked-up, and bypassed labels. A line carrying
  the MBX summary marker that does not match this grammar fails collection;
  it is never silently omitted. Lookup totals aggregate only parsed MBX
  summaries and are not compiler freshness totals.
- The log archive receipt separates all ZIP entries, selected immediate
  job-folder `.txt` candidates, and exact API-step log joins. Candidate count
  includes archive summaries such as `system.txt`; those do not count as API
  step joins unless an API step maps to that exact archive path.
- Tool and Cargo progress lines remain observations. Human-readable progress
  is not converted into authoritative download bytes or isolated download
  time.

Tool download bytes/duration and Cargo download bytes remain `null` because
the retained output has no authoritative byte counter or isolated transfer
interval. Compiler/link durations, fresh compiler-unit totals, lock wait,
hosted CPU, runner queue time, and isolated cache-transfer duration also stay
`null` when the retained sources do not measure them. Text such as
`Compiling`, MBX lookup counts, local compiler observer reports, and legacy
zero timing fields do not fill those gaps.

The report validator keeps each unavailable `actual_counters` field null and
requires its corresponding unknown-reason field. Numeric zero is rejected as
an unmeasured sentinel.

## Run

The implementation uses only the standard library. With Python 3.14
available, run the isolated regression suite from the repository root:

```sh
python3 -B -m unittest discover -s scripts/hosted_phase_measurement/tests -v
```

The collector can be invoked against previously retained files:

```sh
PYTHONPATH=scripts python3 -m hosted_phase_measurement.measure_hosted_run \
  --repository OWNER/REPO \
  --run-json PATH/run.json \
  --jobs-json PATH/jobs.json \
  --logs-zip PATH/logs.zip \
  --workflow-json PATH/workflow-api.json \
  --workflow-file PATH/workflow.yml \
  --out PATH/hosted-phases.json
```

Optional `--artifact-json`, `--artifact-id`, `--final-artifact`, and
`--task-report-dir` add source-bound report evidence. The JSON output records
the exact identities, log hashes, phase map, measured values, and explicit
unknown reasons.
