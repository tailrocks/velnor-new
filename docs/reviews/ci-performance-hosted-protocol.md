# Hosted measurement execution contract

Status: source-bound baseline timeline measured; new controlled hosted runs pending.
This supplements [qualification](ci-performance-qualification.md), not a passing
T01–T26 verdict. Scope remains the exact 47 repositories in `scope.json`.

## Source-bound timeline

`scripts/analyze-ci-performance.py` consumes the existing collector's private
`run.json`, all `jobs.json` pages and `summary.json`. It currently admits only
`push` events and requires the summary/run event identities to match. For those
events it authenticates workflow bytes through the GitHub contents API at that
run's immutable head SHA, checks the
Git blob digest and records SHA-256. Dependencies come from those workflow bytes,
never a caller's claimed DAG. Duplicate keys, unknown dependencies, cycles,
incomplete jobs and mismatched run/attempt/source fail analysis. Static job names
must match API names exactly. Dynamic matrices/reusable workflows need a qualified
expansion mapping and are currently rejected rather than guessed.
Job-level `uses` is rejected explicitly: matching API names cannot reveal the
called workflow's dependency graph. Step-level actions remain ordinary job steps.

GitHub's [event source rules](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request)
distinguish a PR's feature commit from its merge revision. Fetching a PR workflow
at the API head SHA therefore cannot authenticate the executed workflow graph,
even when its job names match. PR, merge-group, dispatch, schedule and other
events remain unsupported until their executed immutable workflow revision is
authenticated. They fail before the workflow API lookup with
`Timeline unavailable: ExecutedWorkflowRevisionUnavailable`; their timeline is
unknown. The analyzer never guesses a merge SHA or reads a mutable merge ref.
Previously saved PR analyses remain historical, unqualified evidence; their
files are preserved and their DAG calculations cannot supply qualification proof.

```sh
rtk proxy /usr/bin/python3 scripts/analyze-ci-performance.py \
  /absolute/private/evidence/OWNER-REPO/rRUN-aATTEMPT
```

The analysis environment requires exact PyYAML 6.0.3. Install
[the hashed requirements](../../scripts/ci-performance-analysis-requirements.txt)
in an isolated environment using `pip install --require-hashes --only-binary=:all:`;
the analyzer rejects another version. See
[dependency source evidence](ci-performance-analysis-dependencies.md).
This is an offline reporting dependency, not a generated workflow dependency.
The analyzer performs a read-only source lookup and writes private evidence;
it never dispatches runs or interprets task commands. Keep raw files outside Git.

For failed baseline run
[37012391691](https://github.com/tailrocks/velnor-new/actions/runs/37012391691),
attempt 1, source `c57c700459bbe1549fe7eedcb7d8689585c38986`, workflow Git blob
`73203b8d8cbde94b68f8c278afd1cebe85bb31de`, SHA-256
`59ad40cdc50c699e253906d01c6b5f8f822ecb715ef3a00e139c1c0d70d0f895`:

| API-derived observation | Seconds | Meaning |
| --- | ---: | --- |
| Completion path: Plan → Rust/CLI → Required | 210 | Job walls plus observed dependency/start delays |
| Longest dependency path of job walls | 203 | Excludes scheduling gaps |
| Sum of executed job walls | 708 | Concurrent runner allocation, not elapsed run time |

The completion path includes Plan 75 s, Rust/CLI 112 s, Required 16 s and
7 s of observed start gaps. API timestamps have one-second resolution; tolerated
overlaps are separately recorded and subtracted. Pure queue and provision time
remain unknown. These API intervals differ from timestamped log intervals and
must retain their measurement origin. This failed run never qualifies a baseline.

## Private collector acquisition

Fresh collection requires an unused evidence directory. The collector admits all
job pages against the exact run, attempt, head SHA and completed terminal status
before fetching logs. It retains `original-acquisition.json` once, with source, job inventory, availability,
original log digests and hashes of all acquired `run.json`, `jobs.json` and
`artifacts.json` response bytes. `--reuse` verifies every original response and log
digest before reading API evidence or rewriting a summary; it never rewrites the
original manifest or raw evidence. Changed timestamps or artifact metadata fail
collection. Logs originally
unavailable remain unknown. Historical raw evidence without this manifest stays
preserved; acquire into a new directory rather than mint original digests from
those existing bytes.

`original_run_creation_to_first_job_start_seconds` names its actual timestamp
origin. On reruns it includes elapsed time since the original run was created,
including time between attempts; it is not attempt queue or provision time.
Those metrics remain null. Regex log counters remain unauthenticated observations,
separate from supported owning-tool session reports and native authority.

## Executable unchanged-run sequence

1. Freeze independently reviewed generator source, generated validation workflow,
   tool/action/runtime identities, all required obligations and a new isolated cache
   generation. Record runner class and full canonical payload/version inputs.
   Publish qualified generator/MBX transport artifacts before consumer execution.
2. Inspect the generated event contract: push uses affected scope; the supported
   full scope currently belongs to dispatch/schedule. Before accepting a push
   sequence, prove its affected plan actually executes the complete qualification
   domains. If it does not, a supported trusted full-validation contract is a
   pending prerequisite. Do not patch YAML or invent a full-push input. Affected
   omission proves selection, not compiler persistence. Keep publishing/deployment
   workflows outside the experiment.
3. Seed the namespace through the authorized protected default-branch validation
   push. It must complete Required and all producer reports successfully. PR and
   workflow-dispatch read-only runs cannot seed a novel trusted namespace.
4. After every late producer/post-job export finishes, rerun **all jobs** of that
   exact immutable push run for T02. Wait for completion, then repeat for T03.
   Original push event/base/SHA/config remain unchanged. Baseline discovery may
   still change; compare the actual complete executed obligation domains across
   every attempt, not just immutable inputs. Confirm fresh hosted runner allocation
   for every attempt from actual logs. Reject unequal/omitted qualification domains.
5. Collect each completed attempt, supported MBX completed-session reports,
   task reports, tool-availability/repair observations, cache actions and owning-tool
   identities. Bind artifact names/digests to source/run/attempt; run-wide artifact
   listing alone does not prove an attempt. Independently recompute the DAG timeline.
6. Trace the late T01/T02 validation snapshot into T03's restored snapshot and
   actually executed workload. Compare real process work and Cargo-fresh evidence,
   not merely MBX miss counters. Unchanged snapshots must have no upload attempt.

If the chosen push retains task coverage that omits required measurement domains,
this sequence is blocked until a supported trusted qualification contract exists
or a real semantic candidate causes the complete domains to execute consistently.
New cache namespaces do not invalidate validation baselines. An unchanged full
dispatch can supplement read-only consumption proof, but
does not substitute for the original push's trusted late-writer persistence.
The integration coordinator must choose the exact qualified namespace/source;
this document has performed no dispatch or cache mutation.

## Changed and negative experiments

Create real fixture changes at immutable revisions and compare their conservative
full and selected plans before hosted execution. Cover leaf/shared source,
proc-macro/build-script, dependency resolution, features/targets/profile/flags,
package addition/deletion/rename and consumed Markdown/schema/native fixtures.
Map each to T06–T12/T19–T21/T24 with selected/covered obligation identities and
actual execution outcomes. Unknown inputs must broaden selection.

Use isolated fixture caches for corruption, missing Rust proxies/targets and
service unavailability; do not edit shared consumer caches. Reverse sibling writer
order and retry the same SHA for T04/T05/T17. Inject actual test failures and
missing/canceled reports for T14/T23. Validate fork/merge-group/base-advance proof
rejection and server-enforced read-only policy for T15/T16. T26 uses safe release
verification only; no tag, deployment or publication purely for a benchmark.

Three runs support persistence behavior, not p95. Percentiles require a disclosed
larger paired sample (specification's initial target: at least 20 comparable
observations), failures/outliers and queue effects. Keep unavailable measurements
explicitly separate from successful CI and static security review.

## Instrumentation gaps requiring supported owners

| Metric | Current supported evidence | Remaining qualification |
| --- | --- | --- |
| Tool download/reinstallation | Mise logs, verified owned inventory, install step interval | Payload download bytes/requests; complete warm zero-download proof |
| Cargo sources | Fetch logs and supported offline fetch result | Actual selected closure and source payload bytes, not line counts |
| Compiler work | MBX `MBX_STATS_REPORT` version 5 measured aggregate by outcome | Aggregate mixes adapters; nested fixed-path reports overwrite; precise adapter/origin data needed |
| Native linking | Included in rustc subprocess interval | No independently supported separate link timer currently established |
| Build scripts/rustdoc/C/C++ | Some MBX adapter intervals | Bypass paths lack timers; adapter separation requires owner extension |
| Cargo fresh units | Supported Cargo JSON `compiler-artifact.fresh` | Capture complete command/unit streams without changing execution semantics |
| Tests | Actual Nextest test summary and obligation wall | Separate build/preparation from test-only duration; inject real failure |
| Cache transfers | Actions archive byte lines and step intervals | Source-bound restored keys, compression/import/export split and useful delta |
| Queue/provision | API initial/start/dependency delays | Not pure queue or allocation latency without supported runner telemetry |

MBX version-5 statistics are supported, but cumulative `mbx stats --json` is not
per-command work. `compiler.duration_ns` is measured overlapping wall work with
adapter overhead, not CPU or pure rustc time. Estimated avoided compiler time
remains an estimate. Internal session JSONL is explicitly unstable and must not
become a Velnor parser. A supported immutable completed per-session report and
adapter/subprocess extension is being implemented in the MBX owner workstream;
until independently qualified, these categories remain unknown.
