# Velnor V1 Rust Cache Transport Contract

Status: mixed. V1 generated Rust CI implements Cargo-source transport and action-owned MBX object
transport. Mise task-result caching remains inactive pending Gate 6. The generic cache identity, report,
and final-status contract lives in the [cache and report contract](cache-contract.md).

## V1 Rust cache paths, transport, and fallback

The Cargo/MBX paths below map generic cache rules for V1 Rust. Future adapters define their own paths and
compatibility fields under the same invariants.

Each path has one owner:

| Data | Owner | Rule |
|---|---|---|
| Mise tools and Rust components | Compiled-in generator catalog, executed by Mise | Embed and invoke exact versions; disable project config, env files, and hooks |
| Cargo registry and Git sources | Velnor source layer | Exclude credentials; separate from MBX |
| Compiler objects and scheduler state | `jdx/mr-boxington-action` when MBX is selected | The action owns the object format (`github-cache-mode: objects`) and the ordinary hosted-job restore/import/export/save lifecycle. Hosted jobs enable `isolate-objects-cache` and suffix the primary key with `github.job`; the action uses a private `RUNNER_TEMP` store and transports its opaque bundle. Velnor does not reimplement the object format |
| Mutable target directory | Matrix job | Reuse sequentially; never share concurrently |
| Successful task result (Gate 6; not active in generated CI) | Mise task cache | Use only for qualified deterministic tasks and complete outputs |

The orchestrator selects the Rust profile, exact MBX version and cache generation, and whether the MBX action
is present. For MBX jobs, Mise installs `mr-boxington@<version>` for the compiler commands. The action
separately installs its own MBX executable through its `version` input for cache restore/import/export. The
renderer passes the same exact catalog version to both; they MUST remain equal. The pinned action owns GitHub
cache restore/import and its post-step export/save. The renderer does not create a second MBX archive path.
Velnor-managed Cargo-source steps use typed workflow IR; task-cache steps remain disabled until Gate 6.

The action builds its own primary key from OS, architecture, effective cache generation, Rust compiler
identity, and revision. Its Rust identity segment is `rust-` plus the first 12 hex characters of SHA-256 over
`rustc -vV` output, or `norust` if no compiler identity is available. With MBX 1.22's directory bundle, the
action adds `-dir` to the configured generation. Ordinary hosted Velnor jobs pass generation
`velnor-mbx-1.22.0` and append `-${{ github.job }}` to the primary key. The action's generated restore prefix
ends after the compiler-identity segment, before revision and job suffix, allowing a compatible older entry
to warm the run. Velnor does not set the action's full `cache-key` or explicit `restore-keys` inputs. The
schema-2 qualification workflow instead uses a run-bound generation containing the action pin, run ID,
attempt, and commit; its writer and reader share that generation and omit the hosted-job suffix.

For ordinary hosted CI, this generated action key is a delegated-action exception to the proposed Velnor key
formula in the [cache and report contract](cache-contract.md). It carries the action's revision and hosted job
suffix, not Velnor's `trust` field, compatibility ID,
or run/matrix closure `snapshot_id`. The qualification's explicit run-bound generation is separate; it also
does not implement Velnor's trust-scoped key. The action's save policy controls writes only, not restore
reads. GitHub cache scope allows pull requests to restore entries from their base/default branch. A read-only
PR key uses its base SHA and the hosted job suffix, so it can even exactly match the base branch's primary key
when the other key inputs match; the compatible restore prefix is the fallback. Velnor delegates object
validity and reuse to MBX's content-addressed object handling and matching-checkout workspace-state rules.
Strict trust-separated MBX read keys are not implemented.

The MBX action does not receive a Velnor trust namespace. It computes save eligibility from event/ref,
default-branch and protected-ref status, same-repository pull-request status, its `save-on-*` inputs, and
`ACTIONS_CACHE_MODE` when the runner supplies it. By default, default-branch pushes may save; protected
non-default branch pushes require `save-on-protected-branch: true`; same-repository pull requests require
both `save-on-pull-request: true` and a `refs/pull/<number>/merge` ref; workflow-dispatch runs require
`save-on-workflow-dispatch: true`. Fork pull requests never save. GitHub's cache scope and runtime access
govern which entries a run can restore or write.
Ordinary hosted CI does not set `ACTIONS_CACHE_MODE` or enable the non-default save opt-ins. Qualification
explicitly sets cache mode to `write` for its protected-main dispatch writer and `read` for its dependent
reader, and opts only the writer into dispatch saves.

For future Velnor-owned task-result caches that implement the proposed trust-scoped contract, a save is allowed
only after its producer succeeded, the current run is trusted for that namespace, and the export has a useful
delta. A task-result cache hit, failed/cancelled task, untrusted PR, empty export, or unavailable producer MUST
NOT trigger a trusted task-result save. Restore and save destinations MUST not overlap active compiler writers.
Current Cargo-source saves follow the separate implemented rule below; the MBX action applies its own policy.

The current V1 generated Rust workflow uses these runner-local paths:

| Data | Path |
|---|---|
| Mise Cargo home and Cargo source inputs | `$RUNNER_TEMP/velnor/cargo` and its selected source-cache subpaths |
| Mutable Cargo target | `$RUNNER_TEMP/velnor/target/<lane_id>`; never an archive path |
| Run and matrix reports | `$RUNNER_TEMP/velnor/<run-key>/<matrix-key>` |
| Mise-installed tools cache | `~/.local/share/mise` |
| MBX private object store and bundle | Private action-managed paths below `RUNNER_TEMP`; Velnor does not set or archive them |

Current generated source-cache steps archive selected paths under the isolated Cargo home: registry
index/cache, Git database, Cargo metadata, and Cargo `bin`. They exclude credentials and `registry/src`.
Separately, the pinned `jdx/mise-action` uses `cache: true` to restore the Mise tools cache at
`~/.local/share/mise`; selected jobs explicitly save that same path with `actions/cache/save` after a
successful push. This tool cache is distinct from Velnor's Cargo source cache.

The implemented Cargo-source transports have two key policies, neither of which uses §1's proposed trust
segment. For MBX/mixed profiles, the shared `actions/cache` key is
`velnor-v1-sources-<target>-<exact-rust-version>-<hashFiles(lockfiles)>`; its restore prefix drops the lockfile
hash. The plan job is the sole writer and crate jobs restore read-only. The save step uses
`if: success() && github.event_name == 'push'`. This workflow's trigger currently accepts pushes only to
`main`, but the condition itself checks only event name, not branch protection or trust identity. For
Cargo-only profiles, the separate pinned `Swatinem/rust-cache` transport uses shared key
`velnor-cargo-<target>-<exact-rust-version>`; the plan job has `save-if: true`, crate-job readers have
`save-if: false`, and targets are excluded. Both transports are implemented exceptions to §1's proposed
generic trust-scoped key and save policy.

Task-artifact paths are reserved for Gate 6. Only after task-result caching is qualified may Velnor set
`MISE_TASK_CACHE_DIR` before Mise starts and archive qualified artifacts from
`$MISE_TASK_CACHE_DIR/task-artifacts/v2` as a separate layer. Current generated CI does not set
`MISE_TASK_CACHE_DIR` or archive `MISE_TASK_ARTIFACTS`.

The MBX action owns its separate portable bundle: its main step restores and imports into its private store.
When eligible, the action's post step exports after compiler writers finish, removes the live store, and
passes only the bundle to the cache service. Hosted MBX jobs use `isolate-objects-cache: true` and
`cache-key-suffix: ${{ github.job }}`. Exact hits skip another save; a recognized empty export (no completed
MBX build) also skips saving. Velnor source and Mise tool-cache steps MUST NOT archive the MBX action's
isolated store or bundle.

The action declares `post-if: success()`: post export and cleanup run only when
the job has remained successful. If an earlier step fails or the job is
cancelled, this action post does not run; the disposable runner's teardown owns
removal of its temporary store. If post runs, malformed restored bundles or
failed imports fail the action instead of being discarded for a cold retry.
Restore-service errors also fail the job. A recognized “no completed MBX
builds” export is a no-cache success; other export, bundle validation, local
storage, and cleanup errors fail the post step. Only save failures explicitly
classified by the pinned action as cache-service reservation, 5xx, or network
transport outages warn and continue with the cache unavailable. Local-storage,
unknown, and unconfirmed save failures fail the action. A failed post makes the
job fail, so the required merger sees its failed job result.

The earlier ordinary-hosted MBX flow set `ACTIONS_CACHE_MODE=read` and used a
Velnor-owned `$RUNNER_TEMP/mbx-single-bundle`; it was retired. Run `37114238559`
describes that former ordinary-hosted path only. The dispatch-only schema-2
qualification workflow remains separate: its run-bound protected-main writer
and dependent read-only reader deliberately set `ACTIONS_CACHE_MODE` and omit
the ordinary hosted-job key suffix. A Cargo-profile job does not invoke the
Mr. Boxington action.

Velnor MUST NOT configure Mise `task.cache.remote_url`, remote namespaces, remote tokens, or OIDC task-cache
credentials in V1. There is no Velnor cache server. If Gate 6 enables task-result caching, its selected
transport is an opaque GitHub archive of the qualified Mise local directory; Mise remains the only authority
for task keys, checksums, output replay, and cache invalidation. No such task-artifact archive is active in
current generated CI.

Task-result caching is disabled pending Gate 6. Current workflows use direct pinned `mise exec` commands and
write no task definition. The proposed temporary TOML delivery shape and invocation with `mise run --file` are
blocked: probes of pinned Mise 2026.9.18 found no `--file` option for `mise run`, so that argv cannot load the
runner-temp task definition. Do not generate it. Gate 6 needs a supported task-definition delivery mechanism,
then Linux backend proof and the full qualification battery, before task-result reuse can be enabled. Any supported definition must carry
fixed command arguments, complete `sources`, explicit `outputs` (including `outputs = []` when appropriate),
and complete `cache.command_inputs`; it must not read, create, or modify project `mise.toml`, `mise.lock`, or
`.mise/tasks`.

If Gate 6 qualifies a supported delivery mechanism, only qualified tasks MAY enable Mise's experimental
artifact cache. Proposed modes are `local-only` by default for local runs, `read-only` for PR and `merge_group`,
`read-write` for protected default-branch pushes, and `off` for release jobs. The exact task TOML cache field
remains subject to Gate 6 fixtures; no task-cache behavior is active before qualification.

For proposed Velnor-owned trust-scoped task-result archives, pull requests and merge groups MAY restore trusted
default-branch archives but MUST NOT write trusted or release archives. They MUST NOT promote PR-produced
executable contents. Fork pull requests are read-only. Protected default-branch pushes MAY write trusted
archives only after the task passes and its report is complete. Release jobs MUST reject PR archives and MUST
NOT restore or save task-result archives; Velnor release workflows do not invoke the MBX action. Release
outputs use a clean or trusted-source-only path. Current Cargo-source cache behavior is documented above;
other MBX uses follow the action policy described above.

For proposed Velnor-owned task-result caches, restore MUST verify compatibility and ownership before use. A missing archive is
`no_entry`; a GitHub restore/save failure is `cache_unavailable`; a failed task-cache read/write is a
Mise-reported miss. Corrupt, incomplete, or mismatched Velnor-owned data MUST be discarded and the task MUST
execute when otherwise eligible. A cache miss MUST never fail a task by itself. A Velnor-managed cache save
failure MUST leave a successful task successful and report `cache_write_disabled` or `cache_unavailable`.
These fail-open rules do not apply to MBX action restore/import/export/save/cleanup errors; those are defined
above.

Future Velnor-owned task-result caches MUST NOT save unchanged data, use an unbounded stable key, globally
prune caches, or restore one cache layer through two mechanisms. MBX objects, Cargo sources, and any future
Mise task artifacts MUST remain independently restorable. The default branch MUST produce warm snapshots
through ordinary CI; no unconditional warm-up workflow is allowed.
