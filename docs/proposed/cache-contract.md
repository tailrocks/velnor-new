# Velnor V1 Cache and Report Contract

Status: proposed; defines task identity, cache ownership/trust, reports, and final status; not an implementation claim.

## 1. Task identity and canonical digests

Every generated task has a stack-neutral `TaskIdentity` with these fields:

```text
schema_version
stack_id, project_root, and component_id
task_kind, task_id, argv, and repository-relative working directory
configuration (target, profile, features, flags, task contract, compile driver, and test runner)
declared input paths and content digests
toolchain, platform, and explicit environment identities
output contract and generator identity
stack_extension (typed, versioned adapter data)
```

The contract crate owns this envelope and canonical serialization. Each detector supplies a typed
`stack_extension`; Mise supplies tool, environment, and generated-task identity. Only the orchestrator
combines task identity and decides reuse eligibility. The V2 tools layer separately derives its static
archive digest from typed tool obligations; the renderer adds only the qualified runtime identity. An unknown
extension schema disables reuse.

For `stack_id = "rust"`, `stack_extension` is a `RustTaskIdentityExtension` containing the Cargo package ID,
normalized manifest path, workspace/local-package graph digest, target kinds and names, features and required
features, Rust target/profile, detected `compile_driver = cargo|mbx`, detected
`test_runner = cargo_test|cargo_nextest`, Cargo config and build-script input
digests, `.config/nextest.toml` digest for Nextest profiles, Rust task kind, and
selected test/archive identity. It MUST NOT include advisory `rust-toolchain.toml` content unless the task actually
consumes it; the project tool files are inspection inputs only.

Velnor MUST reject undeclared task inputs before execution. Environment values that affect behavior MUST be
declared; secrets MUST never enter an identity or report. Publishing, deployment, notification,
network-service, clock-dependent, random-dependent, and undeclared-external-state tasks are ineligible for
task result reuse.

Canonical bytes MUST be UTF-8 JSON with lexicographically sorted object keys, declared array order, no
insignificant whitespace, normalized repository paths using `/`, and no non-finite numbers. The schema MUST
reject duplicate keys. The digest MUST be BLAKE3 over those bytes and encoded as `b3-` plus 64 lowercase hex
characters. Any canonicalization change requires a schema version bump.

The cache identity MUST contain these exact fields:

```text
schema_id       = "v1"
stack_id        = registered detector ID
repository_id   = BLAKE3(canonical remote origin or configured repository ID)
project_root    = normalized repository-relative detected project root
component_id    = detector-defined stable component identity
workspace_id    = BLAKE3(repository_id, stack_id, project root, inventory)
lane_id         = BLAKE3(workspace_id, component ID, task/build kind, configuration, writer lane)
platform_id     = BLAKE3(OS, architecture, exact runs-on label, ImageOS, execution target)
toolchain_id    = BLAKE3(Velnor-pinned tool IDs, versions, components, and selected driver/runner)
cache_format_id = BLAKE3(adapter-reported Cargo or MBX compiler/cache formats and generation)
stack_extension_id = BLAKE3(canonical typed adapter extension)
input_digest    = BLAKE3(TaskIdentity canonical bytes)
```

`rust-toolchain.toml`, `mise.toml`, and `mise.lock` are not execution authorities and are excluded from
`toolchain_id`. They remain explicit repository inputs for scan findings and recommendations; Velnor never
writes them. Their digests MUST NOT enter task `input_digest` for Velnor-generated compile, lint, test, or
documentation tasks. Inspection reports may include their digests. `workspace_id`, `lane_id`, `platform_id`,
`toolchain_id`, and `cache_format_id` MUST be recorded in the plan/report. For
MBX profiles, Rust's cache format includes the reported MBX object format and
cache-generation; if MBX cannot report them, preflight MUST fail instead of
guessing compatibility. Cargo profiles use their Cargo source/build-cache
format and emit no MBX object-cache steps. Velnor MUST compute
`compatibility_id` as BLAKE3 of the canonical object
containing `schema_id`, `repository_id`, `workspace_id`, `lane_id`, `platform_id`, `toolchain_id`,
`cache_format_id`, and `stack_extension_id`.

Trust is an access and promotion boundary, not semantic task input. Keep trust out of `input_digest`; encode
it in cache namespaces and permissions. This permits PRs to read trusted cache entries without allowing
PR results to become trusted. An input digest MUST include task definition and
arguments, working directory, complete declared input closure, dependency and tool identities, relevant
environment, generator/schema, and output contract. Unknown or dynamic inputs disable result reuse.

`lane_id` MUST also include task/build configuration and a distinct concurrent writer lane. Concurrent Cargo
writers never share a target directory. Graph, change selection, and status definitions: [parallelism
contract](parallelism-and-selection-contract.md), [affected selection](../implemented/affected-selection.md).

The GitHub cache key MUST be:

```text
velnor-v${schema}-${layer}-${trust}-${compatibility_id}-${snapshot_id}
```

Runtime-computed families take `layer`, `schema`, their one path list, and their size ceiling from the typed
family spec (`workflow::cache_family`); a path change requires a schema bump (pinned by test). The families, their
keys, writers and budgets are specified in [cache families](cache-families.md). The Plan `Compute cache keys` step
computes `trust` (`trusted`: protected default-branch push; `pr`: same-repo PR; `none` otherwise) and `s256-<64 hex>`
digests with coreutils `sha256sum`. The cache doctor rejects keys with `hashFiles`, job/workflow/run IDs, action
SHAs, literal versions, or `ImageVersion`. Keys over 512 bytes fail generation (family bound) and the key step.
Restore prefixes omit `snapshot_id` only within the same compatibility ID and the `trusted` scope; a `pr` prefix is
never restored. A commit SHA alone MUST NOT be a cache identity.

## 2. V1 Rust cache paths, transport, fallback, and [host seeds](host-seed-contract.md)

The Cargo/MBX paths below map generic cache rules for V1 Rust. Future adapters define their own paths and
compatibility fields under the same invariants.

The push plan keeps state producer tasks under their existing crate obligations.
The generated
`.github/velnor/producer-executions-v1.json` records those job and task bindings
and the digest of the marked task execution manifest. The resolver and report
writer read both files from the plan's commit, require `GITHUB_SHA` to match the
plan head, and reject a missing or mismatched binding. Producer reports use
separate temporary directories for each pass and never enter the crate report
artifacts.

Each path has one owner:

| Data | Owner | Rule |
|---|---|---|
| Mise tools and Rust components | V2 tools layer | Archive exact tool, Rustup, and Cargo-bin paths under typed static and runtime-qualified identity; disable Mise's built-in cache |
| Cargo registry and Git sources | Velnor source layer | Archive only `registry/index`, `registry/cache`, and `git/db`; exclude binaries and credentials; Rust jobs set `CARGO_CACHE_AUTO_CLEAN_FREQUENCY=never` |
| Compiler objects and scheduler state | Velnor bundle steps over `mbx cache export` and `mbx cache import` when MBX is selected | The pinned `jdx/mr-boxington-action` is install-only (`backend: local`); `github-cache-mode: target` is removed. A `Compute state snapshot key` step derives the `mbx-bundle` family key in the job (key `velnor-v4-state-<trust>-<unit>-<root>-s256-<compat>-s256-<snapshot>-s256-<payload>`, amended under owner standing grant 2026-11-02, velnor-fast-ci GOAL.md, decisions X53 and X57; compat: family, schema, path, `unit` `union` or `suite`, MBX version, toolchain, workspace root, `uname -s`, `uname -m`, `ImageOS`, the resolved store and checkout paths and the literals `MBX_SHARE_OUT_DIR=1`, `MBX_BUILD_SCRIPT_EXECUTION=hybrid` (the shape of the bundle, never an environment value), `MBX_CC=0` and `SOURCE_DATE_EPOCH=0`, with no ceiling line; snapshot: sha256 over four lines in this order, `lock` (the root `Cargo.lock`), `rust_inputs` (the root's Rust inputs), `task_set` (the producer task set) and `producer_recipe` (the S256 of the producer's steps from the state skeleton step to the end of the job and the generated task wrapper files those steps run, so a change to the producer's lane set, scripts, env or wrapper moves the key; a generator change to them re-produces every root once and restore stays warm through the predecessor entry; with `compiled_state` off the Plan job is unchanged), amended under owner standing grant 2026-11-02, velnor-fast-ci GOAL.md, decision X78; payload: sha256 of the stored file. Publish refuses to export unless the producer job env holds `MBX_SHARE_OUT_DIR=1`, `MBX_CC=0`, `SOURCE_DATE_EPOCH=0` and `MBX_BUILD_SCRIPT_EXECUTION=0`). An empty or unset `ImageOS` makes the key step exit 0 with `reason=image_os_missing`, `save=false` and `restore=false`, writes no discovery record, and the restore step (conditioned on `steps.mbx-bundle-key.outputs.restore == 'true'`) and every later bundle step stay skipped. The Plan job's `Select attested state` step lists the newest entries of each `trusted` unit and root prefix on the default branch scope and keeps, newest first, only the first three whose attestation `gh attestation verify` accepts for the exact subject, the repository, the signer workflow `.github/workflows/state-seal.yml`, `--source-ref` of the default branch and the state seal predicate type (a missing `gh`, a nonzero exit, a hang past 60 s or unparsable output selects nothing and never fails Plan). The subject is a file holding the exact v4 key, the cache entry id and its size in bytes, separated by single spaces, and one line feed. Job A runs the tip check, admit, save, bind, attest and retire after its download and subject steps; a head that is no longer the tip of the default branch stops it before admit, and the producer P, the sources producer and the tools producers check the same tip as their first step and fail before building, fetching or saving, so no lane runs and `produced` stays unset. Save writes the entry first, bind then lists the exact key on the default branch with `gh cache list` (which needs `actions: read`) and refuses unless exactly one entry carries that key, attest attests the subject read back from that listing, and retire verifies the attestation before it deletes the root's superseded generation and the handoff artifact. Plan lists the same fields (its job needs `attestations: read`) and verifies the subject built from each listed entry before any download, so a key resaved later under a new id, or with other bytes, finds no attestation; because the key ends in the payload segment (the SHA-256 of the stored file), the digest check below still binds the bytes. `Compute state snapshot key` takes the first attested entry under its own compat prefix as `restore_key`, and `Restore state snapshot` restores exactly that key with no `restore-keys`, so a forged or newer unattested entry under the snapshot or compat prefix is never restored. A `restore_key` under the snapshot prefix is an `exact-hit` that sets `producer_skip=true` and skips the save, any other match is a `predecessor-fallback`, and every other generation is saved, even when it is smaller. And `Recover state snapshot` first requires the matched key to equal the `restore_key` Plan selected, exactly (`refused-selection`, because `actions/cache/restore` treats its primary key as a prefix and could otherwise return a longer unattested key), then requires the SHA-256 of the restored file to equal the payload segment of the matched key (`refused-digest`, before any decoder runs), then treats the restored `payload.tar.zst` as untrusted bytes: it refuses a payload over the stored bound of 1.5 GiB (`over_stored stored=<bytes> ceiling=<bytes>`), requires the four zstd frame magic bytes before any decoder runs (`refused-format` for gzip, xz, a skippable frame first or any other bytes, because `zstd -dc` also decodes gzip and xz), decodes it with `zstd -dc --long=30` (the zstd manual requires `--long=<windowLog>` at the decoder for a window above 2^27, `programs/zstd.1.md`, and the packer writes a 2^30 window) twice and never writes the decoded tar to disk. A first-header gate runs before any listing: it reads the first 512 decoded bytes (`head -c 512`) and refuses unless they carry the `ustar` magic (which also rules out a nested compression) and begin with the `state/` family name (`refused-shadow`). The first full decode feeds `tar -tf` (names) and `tar -tvf` (types) through `tee` and two fifos, under a byte cap of the extracted bound plus the tar overhead of the member budget (`MAX_ARCHIVE_MEMBERS` times 1024 bytes plus 10,240), and `wc -c` gives the exact decoded size. A decoded size beyond the cap is a stream bomb (`over_extracted extracted>=<bytes> ceiling=<bytes>`). The decoder writes an `inflated` marker only when it reaches the end of the stream, so the marker proves that the stream is complete (a missing marker is `payload unreadable`). The screen then requires equal `-tf` and `-tvf` member counts of at most `MAX_ARCHIVE_MEMBERS`, regular files and directories only by type, and only names in the `state/` family with no `..`, absolute or backslash names (`refused-shadow` otherwise). It checks, after the screen and before any extraction, that the filesystem of `RUNNER_TEMP` (`df -Pk`) has the measured decoded bytes plus one eighth plus 4096 bytes per member free (`velnor: state snapshot cold (disk_free=<bytes> need=<bytes>)` otherwise, silently, with the payload removed; the 4096 bytes stand for the filesystem block every extracted member occupies), then extracts with the second full decode, under the same cap, into a fresh private stage. Velnor checks the payload SHA-256 before the first decode and again after the extraction (`refused-digest` if it changed). It sweeps the staged tree (no links, special files or siblings), refuses a tree with no regular file (`cache_corrupt`), admits the stage by `du -sb` against the extracted bound (`over_extracted extracted=<bytes> ceiling=<bytes>`), and logs each phase as `velnor: recover_phase <name> <seconds>` (`digest`, `decode_list`, `screen`, `extract`, `verify`, `measure`, `import`). The step computes no file manifest of the staged tree, and the recovery record has no `manifest=` line. The step runs `mbx cache import` on the staged directory from a working directory outside any checkout (importing from a matching checkout restores Cargo workspace state, so a working directory inside `$GITHUB_WORKSPACE` goes cold as `import_in_workspace`). mbx consumes the directory; a failed import is discarded as `cache_corrupt` and writes no recovery record. A missing payload ends cold silently (`no_entry`); a present payload that is empty, unreadable (mode 000), not a regular file (directory, FIFO, symlink, dangling link) or behind a symlinked `velnor-gc1`, `transport` or `state` directory, and a symlinked state root, ends cold with `::warning::cache_corrupt` and is removed without following any link. The screen also refuses a member path deeper than 64 components (`refused-depth`) or a component over 255 bytes (`refused-name`) before extraction. The expected and matched keys reach the script only through step `env` (no generated `run:` script holds `${{ }}`), and a matched key outside `[A-Za-z0-9._-]` is `refused-provenance`. One cleanup runs on every exit path as an `EXIT` trap: it unlocks the stage, state root and payload without following symlinks, removes the stage and the scratch directory with `rm -rf ... || :`, and always exits 0, so any unexpected failure goes cold with `recovery failed`, the transport directory is made writable before the payload is removed, and the only residue is what the operating system refuses to remove. Every hostile, unreadable or oddly permissioned payload therefore ends cold with exit 0, leaves no stage, scratch or inflated archive and writes nothing outside the stage; no failure after extraction can fail the job. Two job kinds write it and neither is a crate job, under the hybrid state protocol of [compiled-state persistence](compiled-state-persistence.md) (amended under owner standing grant 2026-11-02, velnor-fast-ci GOAL.md, decisions X61 and X64). The union producer P of each Cargo root runs the crate jobs' lane records by digest, never `--workspace`, in two phases: a skeleton of the workspace members at `MBX_BUILD_SCRIPT_EXECUTION=1`, then the real members at `0`. P restores read-only, exports the one bundle, and uploads it as one artifact with `retention-days: 1`. The write-only attest job A of the same root checks out nothing, holds `contents: read` for the tip read, and runs only when P's last step sets `produced == 'true'` and gives an artifact id. Writers are ordered by a `needs` chain without per-job concurrency groups (the `state-seal.yml` workflow has one non-cancelling group of its own, `velnor-state-seal-<ref>`): the sources and tools writers run first, the first root's A job needs both and its P job, and each later A job needs the A job before it and its own P job, and admission reserves the outer bound of the stored ceiling for each later root against the 8e9 B repository budget. Suite jobs never write state. The `Dependency seed` job and its direct `Save MBX bundle` are removed. `[stacks.rust] compiled_state` defaults to `"union"` in the generator, so velnor-new sets nothing, and that default generates P and A, and `"off"` generates neither, so no job writes the bundle and every restore misses. In P, after the compile phases a `Record state producer success` marker is written; the final `Publish state snapshot` runs only when `steps.mbx-bundle-key.outputs.save == 'true' && steps.mbx-bundle-import.outputs.producer_skip != 'true'` and refuses any provenance other than `trusted`, exports `mbx cache export --group velnor --format directory` only when the marker exists and the directory is within the extracted bound by `du -sb` (`over_extracted extracted=<bytes> ceiling=<bytes>` otherwise), packs it with GNU `tar --sort=name ... -cf - | zstd -3 --long=30 -T4` (a non-GNU tar is `tar_not_gnu`, a missing zstd is `zstd_missing`, both cold with exit 0), and refuses a payload file over the stored bound by `wc -c` (`over_stored stored=<bytes> ceiling=<bytes>`). Velnor sets `MBX_GC_AUTO=0`, `MBX_SHARE_OUT_DIR=1`, `MBX_CC=0`, `MBX_BUILD_SCRIPT_EXECUTION=1` (`0` in the producer's real-members phase), `SOURCE_DATE_EPOCH=0` (a C compile that expands `__DATE__` or `__TIME__` otherwise never repeats, so its compiled-state units never hit) and `MBX_CACHE_EXPORT_GROUP=velnor` during active work (the qualification job keeps `MBX_SHARE_OUT_DIR=0`; decision X61 amends X48: a bundle written wholly under `1` replays a workspace member's stale build-script output in another job, so only the registry crates are built under `1` and the members under `0`), and `Clean MBX workspace outputs` runs `mbx clean` last. The stable logical store path is `$RUNNER_TEMP/velnor/mbx`. The bounds apply to the payload file after `actions/cache` has extracted its outer archive; Velnor does not cap the bytes or paths `actions/cache` writes while extracting a hostile outer archive, and no row or text here claims a pre-download bound. Admission reserves the outer entry with `outer_entry_bound(inner)`: the inner size rounded to 512 bytes, plus 5,632 bytes of entry and extended headers and 1,024 bytes of end-of-archive, rounded to the 10,240-byte tar record, plus the `ZSTD_COMPRESSBOUND` worst case (`n + n/256`, `lib/zstd.h` v1.5.7 line 249), which is 1,616,920,640 bytes at the 1.5 GiB stored bound (`actions/toolkit` `packages/cache/src/internal/tar.ts` creates the outer archive with `--posix` and `zstdmt --long=30`). Velnor does not install MBX through Mise or implement the object format |
| Mutable target directory | Matrix job | Reuse sequentially; never share concurrently. MBX manifest and Rust-source input hashes recurse through custom member layouts and exclude only this workspace's mutable `target/` (`!target/**`, or `!<workspace>/target/**` for a scoped workspace). A source member named `target` below another directory remains an input; never use a blanket `!**/target/**` exclusion. The mutable output root itself is not a source location. |
| Successful task result | Mise task cache | Use only for qualified deterministic tasks and complete outputs |

The orchestrator decides whether each cache operation is allowed and records that decision in the plan. The
workflow renderer serializes approved GitHub cache operations from typed workflow IR; it does not choose
keys, trust, eligibility, or save timing. For an MBX profile, the pinned Mr. Boxington action installs the
exact MBX binary and nothing else; MBX owns the object format. Velnor's GitHub cache steps move only the
opaque directory `mbx cache export` writes, and restore it only through `mbx cache import`.

Cache save is allowed only after its producer succeeded, the current run is trusted for that namespace, and
the export has a useful delta within the family's size ceiling ([cache decisions](cache-decisions.md)). Every cache writer job except the tofu providers producers (the sources and tools producers, the state producers and the attest jobs) lives in `state-seal.yml`, which is triggered by `push` to the default branch only. The tofu providers producers stay in `ci.yml` because they are chained into the tofu crate lane by `needs` (`tofu_stage.rs`), so on a repository with tofu roots the CI run can stay open after `Required` on a provider-cache miss; a repository without tofu roots ends its CI run when `Required` ends. Sources have one writer, the `sources-producer` job, which runs only for a push to the protected default branch, only when Plan reports that the exact sources key is missing, and saves with `trusted` trust after its fetch succeeded; it is `continue-on-error` and outside `Required`. Pull requests, forks and merge groups never write sources (the `pr` trust scope does not exist). Every generated workflow declares `cache-mode: read` at workflow level, and only the named writers (the tools producers, the tofu providers producers and `sources-producer`; in `qualification.yml` the four jobs dispatched from a non-default branch) declare job-level `cache-mode: write`; Plan and crate jobs restore read-only (compiled bundles are written only by the attest jobs, which declare `cache-mode: write-only`; the union producers keep the workflow-level `read` and hand the bundle over as an artifact), forks and merge groups never write. A task-result cache hit, failed/cancelled task, untrusted PR, empty export, or
unavailable producer MUST NOT trigger a trusted save. Restore and save destinations MUST not overlap active
compiler writers.

The generated workflow MUST use these paths. `CARGO_TARGET_DIR` is never an archive path:

```text
VELNOR_CACHE_ROOT     = $RUNNER_TEMP/velnor/cache
CARGO_HOME            = $RUNNER_TEMP/velnor/cargo
CARGO_SOURCE_PATHS    = $CARGO_HOME/registry/index $CARGO_HOME/registry/cache $CARGO_HOME/git/db
CARGO_TARGET_DIR      = $RUNNER_TEMP/velnor/target/<lane_id>
MBX_CACHE_DIR         = $RUNNER_TEMP/velnor/mbx (preflight exports through GITHUB_ENV; the action main and post receive it explicitly)
MISE_TASK_CACHE_DIR   = $VELNOR_CACHE_ROOT/mise-task
MISE_TASK_ARTIFACTS   = $MISE_TASK_CACHE_DIR/task-artifacts/v2
REPORT_DIR            = $RUNNER_TEMP/velnor/<run-key>/<matrix-key>
```

`CARGO_HOME` is set to this isolated path for every generated task. The source archive MUST include only
the listed index, registry-cache, and Git-database paths, never credentials, binaries, or other files under Cargo home. Mise stores task artifacts under
`$MISE_TASK_CACHE_DIR/task-artifacts/v2`; CI sets that environment variable before Mise starts and archives
only that directory.

Velnor's cache steps MAY archive only `CARGO_SOURCE_PATHS`, qualified `MISE_TASK_ARTIFACTS`, and the opaque
output of `mbx cache export --format directory` ([cache decisions](cache-decisions.md) 2 and 3); they MUST NOT
parse or hand-build MBX objects, and restore only through `mbx cache import`. The protected-push writer rule applies to ordinary task workflows. A
separately authorized `workflow_dispatch` qualification job grants `actions:write`, enables its action save,
and uses a run/attempt/source-SHA-bound generation.

Velnor MUST NOT configure Mise `task.cache.remote_url`, remote namespaces, remote tokens, or OIDC task-cache
credentials in V1. There is no Velnor cache server. The selected task-result transport is an opaque GitHub
archive of the Mise local directory above; Mise remains the only authority for task keys, checksums, output
replay, and cache invalidation.

Task-result caching is disabled until Gate 6. Before that gate, workflows use direct pinned `mise exec`
commands; no task definition is written into the repository. At Gate 6, the Mise adapter may write a
versioned, deterministic task TOML file under `$RUNNER_TEMP/velnor/tasks/` and invoke that task with the
pinned Mise binary. The temporary task file MUST declare fixed command arguments, complete `sources`, explicit
`outputs` (including `outputs = []` when appropriate), and complete `cache.command_inputs`. Its first line
MUST carry the Velnor Actions version marker without a date. It MUST NOT read, create, or modify project
`mise.toml`, `mise.lock`, or `.mise/tasks`.

Before Gate 6, task-cache access remains off. After Gate 6, only qualified tasks MAY enable Mise's
experimental artifact cache. The task command is invoked as:

```text
mise run --task-cache <read-only|read-write|off> <task-name> --file <runner-temp-task-file>
```

Local runs use `local-only` by default. PR and `merge_group` runs use `read-only`; protected default-branch
pushes use `read-write`; release jobs use `off`. The exact task TOML cache field is a schema change and MUST
be enabled only with Gate 6 qualification fixtures; no task-cache behavior is implied before that gate.

Pull requests and merge groups MAY restore trusted default-branch archives but MUST NOT write trusted or
release archives. They MUST NOT promote PR-produced executable contents. Fork pull requests are read-only.
Protected default-branch pushes MAY write trusted archives only after the family's producer step passes and the
run is not cancelled; test, lint, and doc verdicts do not gate a save (decision 1). Release jobs MUST reject PR archives and MUST NOT restore or save MBX or task-result archives;
release outputs use a clean or trusted-source-only path.

Cache restore MUST verify compatibility and ownership before use. A missing archive is `no_entry`; a GitHub
restore/save failure is `cache_unavailable`; failed MBX import is `cache_corrupt`; a failed task-cache
read/write is a Mise-reported miss. Corrupt, incomplete, or mismatched data MUST be discarded and the task
MUST execute when otherwise eligible. A cache miss MUST never fail a task by itself. A cache save failure MUST
leave a successful task successful and report `cache_write_disabled` or `cache_unavailable`. Native Cargo-source, tools and OpenTofu-provider saves follow the [shared optional-save budget](../implemented/cache-save-budget.md): one-minute best-effort attempts use one first-user-step deadline and a thirty-second startup/cleanup reserve; invalid or exhausted clocks refuse the write.

Velnor MUST NOT save unchanged data, use an unbounded stable key, globally prune caches (family pruning of
superseded exact keys is not global pruning; decision 4), or restore one cache layer through two mechanisms. MBX objects, Cargo sources, and Mise task artifacts MUST remain independently
restorable. The default branch MUST produce warm snapshots through ordinary CI; no unconditional warm-up
workflow is allowed.
Family pruning uses pinned `ubuntu-26.04` when the selected runner is non-hosted, in accordance with the Ubuntu 26-only policy.
Family pruning runs only in `cache-prune.yml`, which triggers on `workflow_run` of the CI and State seal workflows and proceeds only for a non-cancelled `push` to the default branch from this repository (decision 4). It prunes the refs of the completed run's branch, built as `refs/heads/<head_branch>` from `github.event.workflow_run.head_branch` passed through the step environment and checked against the branch-name charset under `LC_ALL=C`; `GITHUB_REF` is the default branch of the repository on `workflow_run` and never selects the pruned ref. Its one job alone holds `actions: write` (never
the Plan job or a pull request), and keeps the newest `retained_entries` per unit of a family's `trusted` entries. State entries are kept as the newest complete entry per trust, unit and root, where complete means an attestation verifies with signer `.github/workflows/state-seal.yml`, plus every newer entry; keys that do not parse are never deleted. It also deletes every entry of every retired state schema in the accumulating `RETIRED_STATE_PRUNE_PREFIXES` collection in `crates/velnor-actions-workflow-renderer/src/cache_prune.rs`, which lists `velnor-v1-state-trusted-`, `velnor-v2-state-trusted-` and `velnor-v3-state-trusted-` (retained count 0). Each later state schema adds its retired prefix to that collection and never replaces it.

## 3. Miss reasons, artifacts, and report schema

The only allowed `miss_reason` values are:

```text
no_entry
compatibility_mismatch
input_digest_mismatch
trust_scope_mismatch
cache_unavailable
cache_corrupt
cache_expired
cache_write_disabled
task_not_eligible
task_result_incomplete
forced_uncached
tool_missing
source_missing
```

Each task report MUST be JSON with this shape:

```json
{
  "schema": 1,
  "task_report_id": "task-r123-a1-m-<16 hex>-<16 hex>",
  "run_key": "r123-a1",
  "event": "pull_request",
  "trust": "pr",
  "matrix_id": "pkg:velnor-actions-contract|features:default|target:host|profile:test",
  "matrix_key": "m-<16 lowercase hex characters>",
  "task_id": "stack/rust/crates/velnor-actions-contract/clippy/default",
  "task_digest": "b3-<64 lowercase hex characters>",
  "status": "reused|executed|empty_partition|not_selected|failed|cancelled",
  "not_selected_reason": null,
  "cache": {"layer": "task", "key": "...", "result": "hit|miss|not_attempted", "miss_reason": null},
  "exit_code": 0,
  "duration_ms": 1234,
  "outputs": []
}
```

Reports are run-scoped evidence written outside the repository; they are never task-cache outputs and are
never reused across runs. `outputs` lists only declared outputs of the task itself. Reports MUST contain no
tokens, credentials, or secret environment values. `reused`, `executed`, `empty_partition`, and `not_selected`
are distinct. An `empty_partition` is valid only for one test shard whose exact inventory manifest assigns no
tests while the complete selected test inventory is nonempty. A cache hit is `reused` only when all declared
outputs are present and verified. A `not_affected` obligation appears in the plan and final counts, never as a
task report; `Required` proves its whole-job skip against the plan's job disposition. A failed or cancelled task MUST retain its report and MUST NOT save a
successful task result. `task_report_id` is `task-<run-key>-<matrix-key>-<task-digest-prefix>`, where
`task-digest-prefix` is the first 16 lowercase hexadecimal characters of the task digest without its `b3-`
prefix. A duplicate task-report ID is a planning or reporting error. `not_selected_reason` is required when
status is `not_selected` and MUST be one of `upstream_failed`, `not_in_plan`, `unsupported`, or
`cancelled_by_policy`.

Each matrix job MUST also write one aggregate report at
`$RUNNER_TEMP/velnor/<run-key>/<matrix-key>/matrix-report.json`:

```json
{
  "schema": 1,
  "report_id": "report-r123-a1-m-<16 lowercase hex characters>",
  "run_key": "r123-a1",
  "matrix_id": "pkg:velnor-actions-contract|features:default|target:host|profile:test",
  "matrix_key": "m-<16 lowercase hex characters>",
  "status": "passed|failed|cancelled|not_run",
  "expected_task_ids": ["..."],
  "task_report_ids": ["..."],
  "tasks": [{"task_report_id": "...", "status": "executed", "exit_code": 0}],
  "selected": 4,
  "reused": 0,
  "executed": 4,
  "empty_partition": 0,
  "not_selected": 0,
  "failed": 0,
  "cancelled": 0
}
```

The matrix aggregate MUST contain exactly one task report for every scheduled task ID, including
`not_selected` reports created after an upstream failure. `tasks` and `task_report_ids` MUST be sorted by task
ID. The matrix artifact named by `artifact_id` MUST contain `matrix-report.json` and the task report files
under `tasks/`; no report may be written only to the job log.

The plan MUST also contain one obligation record per task, including the decision (`execute`,
`reused_from_task_cache`, or `not_affected`), reason, task/input digest, owning job, and cache evidence
reference. The matrix contains only `execute` obligations. `not_affected` is valid only on pull-request and
local plans (`not_affected_on_full_event` otherwise). The final job validates every obligation
against its required evidence.

The final job writes `$RUNNER_TEMP/velnor/<run-key>/final-report.json` with report ID `final-<run-key>` and
uploads `velnor-final-<run-key>`. It MUST record the `plan_id`, every expected matrix `report_id`, every
downloaded artifact ID, each required job ID and conclusion (including `alint` when emitted), the
computed final result, and selected/reused/executed/empty-partition/covered/not-affected/failed/cancelled/blocked/not-run
counts. Its `status` MUST use the final result enum below. `required_job_results` MUST include every
non-matrix required job's ID and conclusion, including `alint` when emitted. The final report is an
aggregate and is not a task-cache input.

Generated workflows do not emit a generic Candidate job or candidate report. Schema-1 merge requests retain
the optional `candidate_attestation` field and the `candidate` required-job identifier for wire compatibility
with previously emitted requests; when that identifier is present, merge checks the attested commit against
`plan.head`. New workflow generation rejects `generator_validation = "candidate"`.

Field grammars here are normative over examples. `*_digest` fields are path-independent semantic identities:
BLAKE3 over canonical JSON with absolute paths stripped and repository-relative POSIX paths. Artifact `*_id`
values (`velnor-<kind>-<run-key>-…`) are derived NAMEs for upload/download matching only. Bare numeric service
IDs are permitted ONLY in evidence records (the final report's `downloaded_artifacts` array), never in identities or NAME derivation beyond the run-key correlator. Platform, toolchain,
cache-format, or declared-input changes invalidate. `shard-1-of-1` runs in one step; multi-shard entries use
archive-plus-partition fan-out. Publication is atomic: validate in staging, then rename-swap; failure leaves
prior bytes byte-identical.

## 4. Final gate and exit rules

Every matrix entry uploads the artifact named by its matrix `artifact_id` with `if: always()`.
`required` MUST download exactly the plan artifact `velnor-plan-<run-key>` and every expected matrix
artifact. It MUST fail when an expected artifact or report is absent, malformed, duplicated, or has an unknown
schema. Artifact names MUST be matched against the plan's derived IDs; wildcard downloads are prohibited.

The final job MUST validate, in order:

1. `plan.json` and `matrix.json` agree and their `plan_id`/`run_key` match the
current workflow run.
2. The set of expected matrix IDs in the plan equals the set of downloaded
matrix report IDs exactly.
3. Every matrix report contains exactly the task IDs declared by its matrix
entry, and every task report ID is unique and has the matching task digest.
4. The result precedence below is applied only after structural validation.
5. The plan's `generator` `{version, sha256}` MUST equal the lock record
for the runner target (consumers: the embedded release descriptor);
mismatch is `planning_failed`.

The final gate computes exactly one result:

| Result | Condition | Exit |
|---|---|---:|
| `no_work` | Valid plan has zero obligations, and every always-required plan/policy/workflow/format check passed | 0 |
| `passed` | Every obligation is `reused`, `executed`, validly `empty_partition`, or `not_affected` on an affected-selection event with its crate job skipped | 0 |
| `failed` | A valid required task report records failure or a nonzero child exit | 1 |
| `cancelled` | Any required task was cancelled | 1 |
| `blocked` | A required task was blocked (`not_selected`), below cancelled | 1 |
| `not_run` | A selected matrix entry lacks a valid report and no task failure was reported | 1 |
| `planning_failed` | Discovery, generation, formatting, matrix validation, or plan-report validation failed | 1 |

The verdict job `required` (`Required verdict`) MUST run with `if: ${{ !cancelled() }}`; the branch-protection
check MUST be the always-run gate `required-gate` (`Required`) that fails unless the verdict succeeded
([workflow contract](workflow-contract.md), job 5), because GitHub reports a skipped check as successful. A
skipped job, missing report, cache miss, or omitted task MUST NOT be treated as success; `planned_job_skipped`,
`gate_mismatch`, and `not_affected_on_full_event` fail closed. The final summary MUST show selected, reused,
executed, empty-partition, not-affected, failed, cancelled, blocked, and not-run counts plus every miss reason.

If more than one result condition applies, choose the result in this order: `planning_failed`, `failed`,
`cancelled`, `blocked`, `not_run`, then `passed` or `no_work`. A shard with `empty_partition` is valid only with its
inventory proof and is counted separately in the final report. A real failed task takes precedence over
dependent tasks blocked by that failure. If no task failure is reported, a missing, malformed, or duplicate
matrix report is `not_run`.

## 5. Human plan report

`velnor-actions plan` MAY describe cache layers and eligibility from the validated workflow plan. It MUST NOT
restore, write, or publish cache contents. Cache reads and writes belong only to generated workflow execution.
`plan` MUST NOT claim cache hits or task-result reuse because it does not perform
event-time lookup or execute the generated workflow. It may show which cache layers would be used and which
planned tasks are eligible; the evidence result remains unknown until workflow execution.

## 6. Acceptance gates

| Gate | Required evidence |
|---|---|
| Determinism | Two generation runs from identical inputs produce byte-identical YAML and task IDs |
| Topology | Triggers, permissions, concurrency, action SHAs, plan, matrix, and final jobs match this contract |
| Selection | Fixtures prove reverse-dependency selection, rename/delete handling, and conservative fallback |
| Focused execution | Clippy gates each crate; detected test runner and doctests are visible separate tasks |
| Tool boundary | CI proves locked Mise tools and the detected Cargo/MBX route; direct Cargo/tool installers are rejected |
| Cache ownership | Sources, MBX objects, target directories, and task results have no duplicate owner |
| Cache correctness | Warm reuse works; each changed identity component invalidates the relevant result |
| Trust | PR runs cannot write trusted/release caches or use release credentials |
| Fallback | Disabled, unavailable, corrupt, and incomplete task caches execute normally and report the reason |
| Final status | Missing, failed, cancelled, skipped, and no-work cases produce the exact final result above |
| Dogfood | Velnor's own committed workflow is generated and checked by Velnor |
| Performance | Warm fixture and dogfood runs report setup, cache, compiler, test, and queue time separately |

The implementation MUST add fixtures for a standalone package, multi-crate workspace, shared dependency chain,
build script/shared fixture, feature target, doctest, failing format/Clippy/test, empty suite, renamed
package, warm rerun, and negative cache identities. A green result with missing qualification runs is not
acceptance. ¶