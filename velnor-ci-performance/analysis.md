# CI performance review: ChainArgos PR #2085

**Decision:** Improve the existing runner and generator. Release small fixes in separate waves. Do not wait for the large integration branches.

Review date: 4 October 2026. See `evidence-index.md` for fixed source identities and evidence links. Evidence labels such as [E02] refer to that file.

## 1. Scope and limits

The measured run is `37178675286`, attempt 1. Its PR head is `7cbe1db…`. Its tested merge is `364d8b2…`. The inspected logs use generator `47815c8…`, not the older generator named in the PR description. Velnor PRs #26 and #27 were already merged at the reviewed main. [E01–E04]

The snapshot contains 43 job records. They include 20 Rust pairs, an Actionlint pair, and Plan. It contains 36 successes, two cancellations, three queued jobs, and two running jobs. Do not call this a completed paired qualification. Do not calculate a final workflow duration from this snapshot. The observations concern this CI workload. They do not prove the state of all Java, frontend, deployment, or release workflows. [E01]

Three complete logs received close phase analysis: hosted AMQ, Scale Set AMQ, and hosted eth-processor-app. Job and step records cover the wider run. The review did not inspect the actual host daemon, Docker virtual machine, or running image digests. Hardware, emulator, network, and scheduler explanations remain candidates where host evidence is missing. [E01–E03]

## 2. Measure three different delays

Use these definitions:

- **Queue time:** actual job start minus job creation.
- **Job time:** job completion minus actual job start. This includes setup, post actions, and API completion delay.
- **Setup time:** duration of the GitHub `Set up job` step.

A queued job can have a placeholder start timestamp. Do not treat that value as an actual start. Keep an unknown value empty. Exclude cancelled and nonterminal jobs from successful execution ratios. [E01]

| Workload | Hosted job | Scale Set queue | Scale Set job | Hosted setup | Scale Set setup |
| --- | ---: | ---: | ---: | ---: | ---: |
| Actionlint | 0:51 | 0:21 | 2:23 | 0:31 | 1:37 |
| amq-protocol-types | 1:47 | 15:18 | 4:24 | 0:19 | 1:56 |
| bitcoin-grpc-server | 6:56 | 1:52 | 11:58 | 0:12 | 3:19 |
| bitcoin-migration | 9:07 | 0:21 | 14:28 | 0:20 | 2:00 |
| bitcoin-processor-app | 14:05 | 1:23:44 | 28:45 | 0:18 | 5:24 |
| processor-rpc-binding | 1:22 | 3:16:09 | 9:03 | 0:18 | 5:02 |
| chainargos-scripts | 13:19 | 37:07 | 13:09 | 0:19 | 2:16 |
| tron-processor-app | 16:03 | 20:17 | 16:07 | 0:22 | 1:57 |

These values come from one attempt, not a controlled hardware benchmark. The two final rows prevent an incorrect conclusion: local execution is not slower by one fixed multiplier. The long queue and setup still make local results arrive much later. [E01]

The hosted eth-processor-app job took 18:44. Its local counterpart was cancelled after 31:40 of job time. The cause of cancellation was not established. Do not use that cancellation as a completed runtime comparison. Two local jobs started after more than 3:36 in the queue. This proves a major end-to-end delay. It does not identify whether all that delay came from insufficient capacity, stale capacity, unrelated host demand, or listener faults. [E01]

## 3. F01 — A generated shared action causes another monorepo download

The AMQ local runner resolves a generated shared action from the consumer repository. It prepares the full repository archive at the tested merge. It then runs the nested checkout step. The log thus contains two different source transfers: an action archive and a Git checkout. [E02]

The local action-repository interval is 102.150 seconds. The hosted interval is 17.227 seconds. This interval includes download, extraction, and preparation. Network speed alone does not explain all of it. The local setup step takes 116 seconds. The hosted setup takes 19 seconds. The 97-second setup difference accounts for about 62% of the 157-second total job difference. These are arithmetic comparisons, not predicted gains. [E02]

**Primary repair:** Generate one outer checkout. Then use `./.github/actions/<generated-action>` from that checkout. Remove the inner checkout. Preserve the generated composite files. Preserve action permissions and task identities. Verify that the workflow remains below the platform size limit that motivated the earlier composite work. [E04; Proposed]

The earlier move to `$/` removed a checkout prerequisite but exposed the full source archive as an action download. Changing to a local action is not complete unless the generator also provides its checkout prerequisite. Merely moving 102 seconds into another step does not improve wall time. Measure the complete job. [E02, E04; Proposed]

## 4. F02 — Tool cache restore and save do not form a complete contract

Both AMQ lanes miss the Mise tool cache. Both install the Rust toolchain and other tools. Current Velnor saves only the Mise data directory. The log places Rustup outside that directory. The built-in action restore and explicit save also use different path expressions. GitHub cache versioning includes the path list. Matching visible key strings do not prove matching cache identities. [E02, E11, E14]

The repair must define one cache owner and a complete payload. Include the selected Mise installations, Rustup home, and required executable links. Restore and save the same normalized path list. Apply the same environment to installation, version probes, compilation, and cache save. Test archive extraction, symlinks, permissions, and executable mode. Avoid caching credentials, unrelated global tools, and volatile state. [Proposed]

Do not set `cache_save: true` and assume the problem is solved. The inspected Mise action registers its built-in save in the install path. These workflows use `install: false`. Either use its complete verified lifecycle or retain explicit restore/save ownership. Do not mix two incomplete lifecycles. [E11]

## 5. F03 — Cargo sources hit on hosted but miss on local

The hosted AMQ job restores 147,446,399 bytes of Cargo source cache. The local job misses the corresponding source key and fetches dependencies. The paths differ between hosted `work/_temp` and local `_work/_temp`. Path-dependent cache versions are therefore a strong source-derived explanation. Confirm it with the cache API; do not declare this the only possible cause without scope, version, and archive evidence. [E02, E14]

Separate immutable source data from tool executables. A Cargo registry cache is not a compiler object cache. A source-cache hit does not imply zero Rust compilation. Resolve the selected manifest and target closure. The observed AMQ job also fetches the unrelated logo workspace. Avoid work that the selected task cannot use. For offline builds, verify completeness before removing the online fetch. [E02; Proposed]

## 6. F04 — MBX has duplicate setup and competing lifecycle ownership

The measured jobs install MBX through Mise, then install it again through the MBX action. They also run the native action and a manual single-bundle restore/import sequence. Both MBX routes miss in the inspected AMQ pair. PR saves are deliberately off. Repeating this PR does not automatically create a trusted cache producer. [E02–E03]

#29 is the best first integration candidate. Its code moves lifecycle ownership to the native action and removes the manual bundle path. It adds hosted object-cache isolation and a job suffix. The competing `codex/mbx-lifecycle-fix` branch has diverged. Compare its unique installer and validation changes. Preserve useful changes without bringing back the obsolete lifecycle. [E05–E06]

The newer action supports an explicit Rust toolchain and use of an existing MBX binary. Use those capabilities with strict preflight. Otherwise an absent binary can lead to a latest-version installation. A toolchain-specific cache must use the actual build toolchain, not ambient `rustc`. Verify main and post actions use the same binary and cache root. [E13; Proposed]

Per-job isolation protects namespaces. It can also duplicate large dependency sets. Start with a safe implementation. Then measure whether compatible compile groups can share immutable objects within the same trust scope. Never broaden restore prefixes across incompatible targets, formats, permissions, or toolchains. [Proposed]

## 7. F05 — The heavy job repeatedly compiles related work

The eth-processor-app hosted log has these phases: [E03]

| Phase | Time | Interpretation |
| --- | ---: | --- |
| Clippy | 238.084 s | Compile and lint work |
| Step named `Build test executables` | 188.713 s | The command actually runs a development build |
| Nextest step | 538.881 s | Includes another test compilation and test execution |
| Test compilation inside Nextest | about 307 s | Reported as 5m07s; not an additional phase to add again |
| Actual test execution inside Nextest | 229.887 s | 318 tests, 18 binaries, three skipped tests |
| Documentation | 65.138 s | Separate documentation build |

The performance branch has a concrete repair. It prepares Nextest binaries with `nextest list --list-type binaries-only`. The Cargo route uses `test --no-run`. Reuse that change, with its tests, in a small release. Keep a separate product-build task only when the product contract requires one. [E09]

Do not promise that deleting the development build saves all 188.713 seconds. It produces some objects that later phases reuse. The saved wall time must come from a controlled before/after test. [E03; Inference]

The log also shows legitimate local MBX hits between commands. Caching is not universally inactive. Clippy and test compilation have different inputs. Preserve separate identities where flags, features, profiles, or dependency graphs differ. Measure misses by cause. Do not force incompatible compilations into one identity to inflate a hit counter. [E03; Proposed]

## 8. F06 — Hosted disk pressure remains after the earlier GC fix

The heavy hosted job begins with roughly 16.3 GB available on the measured filesystem. The log later reports 5.0 GiB, then 3.6 GiB free. MBX collection removes 5,619 shared-cache objects and 1,568 action results, with about 1.3 GiB of logical data reported freed. The current GC setting therefore runs, but the overall working set remains expensive. [E03]

Measure physical usage of compiler outputs, MBX objects, export staging, compressed archives, tool installations, and container images. Account for hardlinks and copy-on-write sharing correctly. Avoid two exports of the same objects. Bound the export set and restore set. Preserve objects held by active compilation or export leases. A routine cache export must not fill the disk. [Proposed]

Do not disable GC to improve a benchmark. Do not convert archive integrity failures into success. Treat an ordinary cache miss as a cold build, with a recorded cause. [Proposed]

## 9. F07 — Queue progress and capacity need host evidence

#25 now contains worker-release and admission changes. It is no longer only a tar workaround. Release of exited workers, journal state, occupied slots, and fresh demand belong in the critical path of the review. Confirm the deployed daemon contains the chosen fixes. A merged commit is not deployment evidence. [E07]

The capacity branch provides a useful starting point, not a ready policy for every host. It is based on older #25 code. It samples the Mac host and `/`, not necessarily the Docker virtual machine and its storage. A one-slot startup combined with missing telemetry can restrict progress. Synchronous host probes can also need bounded execution. These are candidate-source risks, not proven causes of the measured run. [E08]

Record demand receipt, eligible demand, admission, image preparation, container start, runner registration, assignment, completion, and cleanup. Identify intervals with free capacity and waiting eligible work. Also record other repositories that use the host. A worker limit alone is not enough. CPU pressure, memory, storage, and per-job test concurrency must fit the actual execution environment. [Proposed]

## 10. F08 — Preload official action archives before runner startup

The deployed official runner version already has `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`. Use it before considering a runner fork. Populate exact resolved-commit archives in a host-owned store. Expose only each job's authorized subset. Mount it read-only before the runner process starts. A cache step inside a workflow is too late to improve that workflow's `Set up job`. [E12; Proposed]

The normal cache path still copies and extracts the archive. First measure the network gain. Then measure extraction and filesystem cost. Consider immutable unpacked trees or private copy-on-write clones only after compatibility tests. Some actions write beside their own files. A globally writable shared `_actions` directory would break isolation. [E12; Proposed]

The preloader must not execute action code. It must resolve authorization before private cache access. Verify the archive digest and structure before publication. Use temporary files and atomic publication. Quarantine corrupt entries. A missing entry may use normal authorized download; an authorization failure must not become a cache bypass. [Proposed]

## 11. F09 — Git seeds are different from action archives

Keep a host-owned Git object store for each repository and authorization scope. Create a fresh private worktree for every worker. Seed only the needed commit, parents, submodules, and LFS objects. The tested PR merge is a distinct commit from the branch head. Preserve it. Refresh changed PR merge refs by SHA, not by a stale branch name. [E01, E15; Proposed]

Prototype the seed with the pinned official checkout action. Verify that origin validation, cleanup, shallow fetch, and credential removal preserve it. Git reference clones are an implementation option; actions/checkout does not automatically offer a `reference` input. Dissociate references or hold an immutable lease for the entire job. Never expose the host token, mutable mirror metadata, or another repository's objects. [E15; Proposed]

For macOS Docker providers, compare a Linux-managed volume with a Mac-shared directory. Place metadata-heavy cache data on the measured faster storage. Do not assume the provider, CPU architecture, memory allocation, or emulator from `linux/amd64` alone. [E16; Proposed]

## 12. F10 — Preload tools and per-job DinD images

A warm worker must use a verified tool bundle before it installs tools from origin. Key the bundle by exact tool versions, target, installation layout, and required components. Keep worker writes private. Preserve fresh runner registration and one-job worker lifetime. [Proposed]

An image in the outer host daemon is not automatically an image in each inner DinD daemon. Seed each per-job daemon through a verified image archive, approved local registry, or qualified storage snapshot. Populate service images before the runner initializes services. Do not solve image pulls by exposing the host Docker socket or sharing a mutable inner Docker root between jobs. [Proposed]

## 13. F11 — Avoid unnecessary jobs without weakening Required

The generator should select affected tasks before it allocates an expensive worker. Review the performance branch's selection changes against the current source. Include both old and new dependency graphs, deleted files, changed build scripts, workspace settings, feature sets, and toolchain files. An uncertain input expands work; it must not silently omit work. [E09–E10; Proposed]

Keep full paired qualification separate from ordinary affected-work CI. During paired qualification, both lanes must execute each required obligation. Do not let a result produced on hosted infrastructure stand in for local compilation and execution. Safe source and compiler-object reuse is different from accepting another lane's test result. [Proposed]

Each report needs lane, job, run, attempt, tested commit, task identity, and input identity. The inspected AMQ reports use the same display artifact name for both lanes. Both uploads succeeded with different artifact IDs. That is not proof of an upload collision. It is a reason to verify consumer selection and ambiguity rejection. [E02; Candidate]

## 14. F12 — Tests and later maintenance still matter

The heavy Ethereum tests themselves take almost four minutes. Trace service image pulls, container startup, database migration, fixture creation, test execution, and teardown. Reuse fixtures only within a scope that preserves isolation. Use unique database schemas or independent databases where required. Bound test parallelism across all active jobs. Do not remove assertions, tests, doctests, or negative tests for speed. [E03; Proposed]

Review source-only branches, stale qualification records, diagnostics, release gates, and the remaining deployment workflows after the first releases. Reuse useful performance collectors. Do not add a new runtime language merely to collect timing data. Keep essential runner and generator changes in Rust. [E04, E10; Proposed]

## 15. Release order

1. **Wave A:** Integrate the reviewed #29 cache lifecycle and unique companion fixes. Publish a new generator. Regenerate the full consumer tree. Prove an authorized cache write and a subsequent restore.
2. **Wave B:** Release checkout-once generation, complete tool-cache ownership, explicit MBX toolchain selection, and correct test-binary preparation. Keep patches independently reviewable.
3. **Wave C:** Release worker-progress fixes from #25 and the adapted capacity change. Publish and install exact daemon and image products. Prove queue progress and cleanup.
4. **Wave D:** Add action, Git, tool, source, and DinD seeds. Prove cold fallback, private isolation, and reduced origin bytes.
5. **Wave E:** Finish selection, compatible compile-group reuse, heavy-test fixture work, repeated measurements, and remaining branch disposition.

Run these tracks in parallel where files and runtime resources do not conflict. Do not wait for one large branch to contain every fix. Do not skip a correctness or authorization test to achieve a fast merge. [Proposed]

## 16. Completion

The supplied specification defines finite gates. Functional, isolation, provenance, and required-result gates are mandatory. Performance budgets are explicit proposed acceptance targets, not achieved results. When a budget fails, retain the failed item and investigate the largest measured cause. Finish only when every mandatory checklist item has evidence. Do not use an endless optimization loop as a substitute for a complete release. [Proposed]
