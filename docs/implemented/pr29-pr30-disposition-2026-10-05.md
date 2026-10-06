# PR29 and PR30 disposition (2026-10-05)

**Status:** Read-only lineage disposition. This record does not merge, close,
reply to, or resolve either pull request. It does not claim MBX qualification,
runtime cleanup, or release runtime proof.

The comparison base is protected `main` at
`4fffbc22ce159305c62ae039668da2a14e2e3366`. The exact PR heads, run IDs,
review feedback, and changed-path inventories below were rechecked on
2026-10-05. The decisions refer to the source at those immutable PR heads, not
to later branch movement.

## Current PR state and unique scope

| PR | Remote state at snapshot | Unique behavior | Disposition |
| --- | --- | --- | --- |
| [#29](https://github.com/tailrocks/velnor-new/pull/29) | Open, ready for review; head `4ad1e34a1e20589386ddefdc25eaceb067341f0b`, base `47815c83b9eeadbaf84b741918fffa7ea550da89`, 99 changed paths; a fresh GitHub API check on 2026-10-05 reports merge state `DIRTY`. [Run `37183113924`](https://github.com/tailrocks/velnor-new/actions/runs/37183113924) passes Plan, Required, all Rust/static checks, and DCO; Publish baseline is skipped. No exact-head protected MBX writer/fresh-reader dispatch is recorded. | Moves hosted MBX object-cache save/restore and post lifecycle to the action with per-job cache-key separation; keeps the Scale Set single-bundle path; adds LCS factoring for shared hosted/Scale Set job steps. | Keep open pending normal current-main sync, exact-source/action-pin reconciliation, and actual cache qualification. Green generator CI is not a protected writer/consumer round trip. |
| [#30](https://github.com/tailrocks/velnor-new/pull/30) | Open, ready for review; head `fe0064cbb82d861653439faaf4a088186571dec6`, base `96b08aa236a6b5afcbd6c768bfdc83e6fa7b7fe8`, 46 changed paths; a fresh GitHub API check on 2026-10-05 reports merge state `DIRTY`. Current-head [run `37227552789`](https://github.com/tailrocks/velnor-new/actions/runs/37227552789) fails Plan, Alint, and Required; Rust jobs are skipped. Actionlint, Cargo Deny, Cargo Machete, Zizmor, and DCO pass; Publish baseline is skipped. | Keeps MBX at `1.21.1` while selecting action `v1.7.1`/`d082` and separates job keys. Hosted Linux sets `MBX_GC_AUTO=0` so the action owns its post-export object lifecycle; explicit `mbx gc` before bundle export is confined to the Scale Set route. It retains that single-bundle route and adds the separate hosted Cargo-source-pruning algorithm described below. An earlier action-store cleanup workaround and its rendered-command regression were introduced by [commit `b8de2fb`](https://github.com/tailrocks/velnor-new/commit/b8de2fb2b875bd1c67ec7b44b1153e2401566efb) and removed by [commit `422fcd4`](https://github.com/tailrocks/velnor-new/commit/422fcd405325d4a0e60b53e8984bde5dac0ec0e8); neither is behavior of this final head. | Do not merge this head. Plan reports that generated `.github/workflows/ci.yml` differs from the checked-in file; Alint reports Rust source over the §5 400-line limit, and Required then lacks its Plan artifact. The earlier green [run `37202815647`](https://github.com/tailrocks/velnor-new/actions/runs/37202815647) was for old SHA `0d8912dc`, not this head. Keep the Cargo-prune algorithm as unimplemented design evidence for a future adopted non-V1 runtime owner. |

The PR29 base and PR30 base reported by GitHub are older than the comparison
`main` SHA above. Their status checks therefore describe their recorded source
and base, not a candidate after current-main integration. PR29 has one
`COMMENTED` Codex review on older SHA `89cdc772cb` and two resolved inline
threads without replies: [lane factoring](https://github.com/tailrocks/velnor-new/pull/29#discussion_r4176060915)
and [lifecycle contract alignment](https://github.com/tailrocks/velnor-new/pull/29#discussion_r4176060919).
PR30 has no formal reviews or inline findings; its only issue comment is an
informational Codex summary.

PR29's body claims MBX `1.22.0` and released action `v1.7.1` at `d082`. The
recorded PR29 source instead selects MBX `1.22.0` and candidate action commit
`ec3ebbfbc1fdaffa59d476e87e4f386fdc60d533` in version policy/workflow
generation ([policy at the PR head](https://github.com/tailrocks/velnor-new/blob/4ad1e34a1e20589386ddefdc25eaceb067341f0b/.velnor/version-policy.toml)).
That is not the production authority on current `main`: the
[cache contract](../proposed/cache-contract.md) retains MBX `1.21.1` and action
v1.6 pending source, lifecycle, disk, and input qualification. Resolve this
source/body/authority mismatch and bind any qualification to the exact source
and action bytes before describing the candidate as qualified.

PR30's action-store cleanup workaround and Cargo-source pruning are separate
historical effects. Commit [`b8de2fb`](https://github.com/tailrocks/velnor-new/commit/b8de2fb2b875bd1c67ec7b44b1153e2401566efb) introduced the workaround for making
real read-only directories in isolated action-owned `store/out-dirs` removable
before the action's post step; its regression created 101 nested read-only directories
under a 64-file-descriptor limit and checked that symlink targets, the sibling bundle,
files, and `MBX_CACHE_DIR` remained untouched. Commit [`422fcd4`](https://github.com/tailrocks/velnor-new/commit/422fcd405325d4a0e60b53e8984bde5dac0ec0e8) removed both the workaround and
its regression and changed hosted configuration to `MBX_SHARE_OUT_DIR=0`. The final PR30 head
does not contain that cleanup behavior or test. Its separate
[Cargo-source pruning script](https://github.com/tailrocks/velnor-new/blob/fe0064cbb82d861653439faaf4a088186571dec6/scripts/prune_hosted_cargo_sources.py) removes Cargo registry and Git source trees
after task and cache-save work. The PR30 [cache contract at its exact head](https://github.com/tailrocks/velnor-new/blob/fe0064cbb82d861653439faaf4a088186571dec6/docs/proposed/cache-contract.md#L141)
sets hosted `MBX_GC_AUTO=0` and leaves hosted object lifecycle to the pinned
action's post export. The `mbx gc` command before external-bundle export is
Scale Set-only. Neither source change establishes a protected cache round trip
or an ENOSPC fix.

## PR30 Cargo-source cleanup design evidence

The exact PR30 tree contains the cleanup script and the
[focused regression suite](https://github.com/tailrocks/velnor-new/blob/fe0064cbb82d861653439faaf4a088186571dec6/scripts/test_prune_hosted_cargo_sources.py).
Its generated-workflow hook is
`crates/services/velnor-actions-workflow-renderer/src/mbx_bundle.rs`; it runs only after
success on a protected default-branch push on GitHub-hosted Linux and targets
`$RUNNER_TEMP/velnor/cargo`. Current `main` has no pruning script or cleanup
hook; generated jobs still set `CARGO_HOME` to that path, as the [Mise ToolHomes constructor and env](https://github.com/tailrocks/velnor-new/blob/ccd4642ec382c7f67ec0ea7247f42de840e4cf2e/crates/velnor-actions-mise/src/steps.rs#L34-L44) and the [crate-job tool-step constructor](https://github.com/tailrocks/velnor-new/blob/ccd4642ec382c7f67ec0ea7247f42de840e4cf2e/crates/velnor-actions-orchestrator/src/matrix_tools.rs#L117-L137) and [obligation-step environment](https://github.com/tailrocks/velnor-new/blob/ccd4642ec382c7f67ec0ea7247f42de840e4cf2e/crates/velnor-actions-orchestrator/src/matrix_step.rs#L130-L139)
and [orchestrator source-cache owner](https://github.com/tailrocks/velnor-new/blob/ccd4642ec382c7f67ec0ea7247f42de840e4cf2e/crates/velnor-actions-orchestrator/src/source_cache.rs) show; the latter builds
source restore/save paths but does not delete the live trees.

The algorithm's safety properties are worth preserving for a future, explicitly
adopted runtime owner:

- It validates canonical, non-root absolute paths and opens every path
  component relative to directory descriptors with no-follow flags. It
  requires `CARGO_HOME == RUNNER_TEMP/velnor/cargo`.
- It reads mount IDs from `/proc/self/fdinfo/<fd>`, fails closed if the ID
  cannot be read, and rejects nested mounts. Directory traversal uses
  descriptor-relative opens; non-directory entries are inspected with
  `O_PATH`.
- Before the first deletion it fully inventories both `registry/` and `git/`.
  It then deletes only those roots relative to pinned directory descriptors,
  rechecking inode identity, and records allocated bytes, inode counts, free
  bytes, and free inodes before and after.
- It preserves Cargo `bin/` and config, Velnor target and MBX paths, reports,
  and external sentinels. It unlinks an in-tree symlink without traversing its
  target. It runs only where Linux `O_PATH`, `O_NOFOLLOW`, and mount IDs exist.

The PR30 regression suite covers the corresponding threat boundaries: symlinked
ancestors/roots fail before mutation; either root's nested mount or unavailable
mount ID fails before either root is deleted; mount changes between preflight
and deletion and mounted files are rejected; noncanonical paths and a Cargo
home outside the exact runner-temp location are rejected; and a successful
case removes only `registry/` and `git/` while preserving neighboring state.
The wiring tests constrain the successful-task/cache-save ordering and require
the Scale Set lane to omit this hosted-only step. These are source/test cases on
the open PR30 branch, not evidence that the behavior is present or qualified
on current `main`.

The helper's safety model assumes no concurrent same-UID mutator. Preflighting
both trees prevents known mount/symlink hazards from being entered, but deletion
is not transactional if a later operation fails. The algorithm is Linux-only.
A future owner must retain the descriptor-relative/no-follow/mount-aware
properties and prove a safe implementation under the repository's `unsafe`
ban; a path-based recursive delete is not an equivalent port. The old helper hard-codes `$RUNNER_TEMP/velnor/cargo`; the proposed
[cache contract](../proposed/cache-contract.md) defines `VELNOR_CACHE_ROOT=$RUNNER_TEMP/velnor/cache`
and `CARGO_HOME=$VELNOR_CACHE_ROOT/cargo`, while current generated jobs still set
`CARGO_HOME` to `$RUNNER_TEMP/velnor/cargo` ([Mise ToolHomes constructor and env](https://github.com/tailrocks/velnor-new/blob/ccd4642ec382c7f67ec0ea7247f42de840e4cf2e/crates/velnor-actions-mise/src/steps.rs#L34-L44),
[orchestrator source-cache owner](https://github.com/tailrocks/velnor-new/blob/ccd4642ec382c7f67ec0ea7247f42de840e4cf2e/crates/velnor-actions-orchestrator/src/source_cache.rs)). A future owner must reconcile
this proposed/effective path mismatch before adoption and bind pruning to
actual `CARGO_HOME`. The old inventory also lacks explicit depth, entry-count,
and total-work bounds; a future owner must set and test finite limits before
retaining or porting the algorithm.

## Why V1 does not own this runtime deletion

The [V1 boundary in `AGENTS.md`](../../AGENTS.md) defines V1 as a workflow
generator, not a runner, interpreter, or second task graph. The adopted
[architecture](../proposed/architecture.md) assigns the CLI to typed
`init`/`plan`/`generate` dispatch; the orchestrator coordinates planning and
generation but must not own OS process details or process creation; the Mise
adapter owns fixed, pinned tool subprocesses; and the workflow renderer emits
generic workflow YAML from typed IR, not filesystem behavior. The [CLI
contract](../proposed/cli-contract.md) says `plan` is analysis-only and task
execution occurs in generated workflows. Its private typed operations are
`write-request-v1`, `plan-v1`, `merge-v1`, `fetch-reports-v1`, and
`write-task-report-v1`; there is no cleanup operation. None of these contracts
adopts a V1 runtime owner for recursive hosted-runner cleanup.

The nested `crates/velnor-runner` owner is the separate macOS Scale Set runner
implementation. It does not own the filesystem lifecycle of a GitHub-hosted
Linux Cargo job. The V1 Cargo-source cache owner restores and saves source
archives; that ownership does not silently grant it authority to mutate the
live Cargo home. This is a missing adopted runtime boundary, not a request to
add deletion to the renderer, orchestrator, CLI, or Mise adapter.

The proposed PR30 hook runs after task processes and cache saves. It can reclaim
space after those operations, but it cannot prevent a peak disk/inode
exhaustion that occurs during compilation, testing, or cache export earlier in
the job. Its before/after accounting is not a peak measurement. Do not describe
it as an ENOSPC mitigation proven by this algorithm or its synthetic tests.
Retain the algorithm above as design evidence until a non-V1 runtime owner is
explicitly adopted with a lifecycle, threat model, dependency/API boundary, and
peak-capacity qualification plan.

## PR65 receipt mapping to the canonical release graph

The narrow PR65 behavior is represented in the pending canonical PR46 source
at `48bd8406e541729d48ae04c9e48afc0206fd0024`, principally in
[`schema2_generator_release_manifest_publish.rs`](https://github.com/tailrocks/velnor-new/blob/48bd8406e541729d48ae04c9e48afc0206fd0024/crates/velnor-actions-workflow-renderer/src/schema2_generator_release_manifest_publish.rs)
and its publish tests/fake GitHub commands. The compared PR65 source SHA is
`bc7f784a51233bd29676353392c9fc07c0ad01a7`; a read-only comparison of PR65
and PR46 is summarized here; its supporting review packet is retained off-repository.
This is a selective replacement in the typed three-target release graph; it
does not make PR65's separate legacy publisher authoritative, and it is not
yet part of `main` at the snapshot SHA.

The canonical publisher resolves one positive release ID from the new draft,
requires its exact tag and `draft=true`, and uses that ID for subsequent API
reads. It verifies tag-to-source binding, the same release ID and tag,
canonical API/HTML/browser URLs, draft/prerelease/immutable state, the exact
20-asset canonical inventory, uploaded state, byte size, and `sha256:` digest
against the same-run inventory. The three targets are Linux x86_64, macOS
arm64, and macOS x86_64; PR65's two-target inventory is not carried. PR46
preserves the exact candidate manifest bytes rather than rebuilding a
manifest from draft API evidence. It rechecks latest-main and the exact-source `Required` check
immediately before the draft-to-published mutation, reads back the same
release ID, and emits accepted metadata only after the immutable postflight.
The receipt binds source SHA, version/tag, workflow authority, run/attempt,
release ID, every asset's name/URL/digest/size, and the byte-identical
canonical manifest. PR46's receipt format and upload mechanics differ from
PR65's atomic staging-directory receipt (and do not carry its checksum file);
the comparison preserves the immutable API-observation gate, not byte-for-byte
receipt-schema or filesystem-atomicity equivalence.

Those mocked API/source tests establish generated-script behavior only. There
has been no live release dispatch, release publication, immutable GitHub
readback, protected-environment verification, or authentic same-run
three-target candidate qualification. Do not count the PR65 receipt mapping as
runtime release proof.

## Proposed PR dispositions (not posted)

The text below is a drafting aid only. No GitHub reply has been posted and no
review thread has been changed by this record.

### PR29 inline thread: shared lane factoring

> The current PR29 head adds LCS-based common-step factoring for the hosted and
> Scale Set lanes ([PR29 head commit](https://github.com/tailrocks/velnor-new/commit/4ad1e34a1e20589386ddefdc25eaceb067341f0b)),
> with renderer coverage for the shared-lane output. The earlier current-main
> lane-cap fix is [commit `2ca2fbd`](https://github.com/tailrocks/velnor-new/commit/2ca2fbd63c2650656ecb4ec7974fd8f7630e7a7f). The exact PR29 head
> Plan/Required run passes. This addresses the workflow-size concern at source
> level; it does not establish MBX runtime qualification. Please assess again
> after normal current-main synchronization and fresh exact-head checks.

### PR29 inline thread: action-owned lifecycle contract

> The PR29 source implements action-owned hosted MBX lifecycle
> ([implementation](https://github.com/tailrocks/velnor-new/commit/d83a93c345b6555d3e00b82cbd585e45ca437962)
> and [contract update](https://github.com/tailrocks/velnor-new/commit/5b25a0591ddf419959d6698e26671f0d607556f6))
> while keeping the Scale Set bundle route separate. The candidate remains
> held: its source selects MBX `1.22.0` and action commit
> `ec3ebbfbc1fdaffa59d476e87e4f386fdc60d533`, while the PR body names
> `v1.7.1`/`d082`; there is no exact-head protected writer/fresh-reader
> qualification. Current `main` restored the held production pin in
> [commit `5a3f534`](https://github.com/tailrocks/velnor-new/commit/5a3f5348c86d733d8705dadd7c2b36420a2fc0bd)
> and [commit `2d9bca8`](https://github.com/tailrocks/velnor-new/commit/2d9bca8a37b0440e29a3520aa03e77752079f400).
> Keep production on the held `1.21.1`/v1.6 pair until the candidate
> source, policy, and measured lifecycle evidence agree.

### PR30 overall disposition

> The current PR30 head is not ready to merge: run `37227552789` fails Plan on
> generated-workflow byte drift and fails Alint's 400-line Rust source gate;
> Required fails downstream because Plan produced no artifact. The earlier
> green run was for an older source SHA. The fd-relative Cargo-source cleanup
> is retained as future runtime-owner design evidence, but current V1 has no
> adopted owner for that host mutation and the post-task timing cannot prevent
> a peak ENOSPC event. No current-main runtime behavior or qualification is
> claimed here.

Do not close either PR based solely on this record. The coordinator should make
any later supersession decision only after unique behavior has an independently
reviewed replacement on protected `main`, and after the PR's current feedback
and status are checked again.
