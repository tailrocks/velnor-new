# PR #12 current disposition and historical evidence

## Current exact-ref snapshot — PR `updated_at` 2026-10-04T18:34:30Z (2026-10-05 01:34:30 +07:00); observation clock unrecorded

Paired PR API and `refs/pull/12/head` reads bind open draft PR #12 to
`f4f1312124cfcb4f9f531557ff5c229eb691fc58`, base
`47815c83b9eeadbaf84b741918fffa7ea550da89`. PR `updated_at` was
`2026-10-04T18:34:30Z`; the paired API/ref observation clock was not retained.
The Files API path set matches the exact Git tree: 461 changed paths, including
100 `.py` paths. The ledger edit cutoff is recorded in the signed freeze record
in the linked addendum, with UTC and `Asia/Ho_Chi_Minh` times. Snapshot hashes,
the 225-path delta from `7c52bbda`, and path dispositions are in
[`pr-12-delta-7c52-to-f4f131-2026-10-05.md`](pr-12-delta-7c52-to-f4f131-2026-10-05.md).
The old 291-path/79-Python ledgers and 54-script matrix remain intact as
historical evidence; no previous receipt was overwritten.

The exact feedback/check snapshot most recently verified was head `dc492b4`,
before current `f4f131`: comments, reviews, and review threads were zero; DCO
was the only successful check and no status contexts existed. Re-fetch feedback
and checks at `f4f131` before any merge or closure. DCO alone is not CI-green.
The source delta dispositions remain scoped and `PARTIAL`; main merge, hosted
qualification, release, consumer adoption, and archive/ref gates are open.

## Superseded 2a2 snapshot — document updated 2026-10-04 07:27 UTC

GitHub REST snapshot cutoff: `2026-10-04T07:27:54Z`.

This section supersedes every status below the “Historical snapshot” heading.
It is bound to PR #12 head `2a2dab0e04a7da18975ca634e505eb9d72dc2e58`, base
`47815c83b9eeadbaf84b741918fffa7ea550da89`, and integration candidate
`df0c4df8a12e9bc66c7356f414de4f6f1f751148`. PR #12 is still open and draft.
At this cutoff, the paginated GitHub Files API returned 280 paths; exact REST
counts were 0 issue comments, 0 reviews, and 0 review comments/threads. The
exact 54 added `scripts/**` paths (excluding modified
`scripts/check-freshness.sh`) are classified in
[`pr-12-script-disposition-2026-10-04.md`](pr-12-script-disposition-2026-10-04.md):
29 `NOT-CARRIED`, 25 `ALREADY-ABSENT`, with no unresolved path-level hold.
These are candidate-branch source dispositions; they do not mean changes are
merged to `main`, released, or adopted by consumers.

### Current latest-head review status

An independent source-delta review compared PR #12 head `2a2dab0e` with its
previous head `0efb5565` and found one new commit adding 18 paths, all confined
to a Rust observer/registry fixture; it found no product caller, registered
generator source, workflow, or golden change. This is the latest-head delta,
not a PR-versus-integration comparison. The immutable compare endpoint is
[`0efb5565...2a2dab0e`](https://api.github.com/repos/tailrocks/velnor-new/compare/0efb5565dd33a70b67fc2198d032153f6c5bee9f...2a2dab0e04a7da18975ca634e505eb9d72dc2e58);
the captured response SHA-256 is
`186cce20df223de8036e80ef3a88cd93a043cbe0196fd6fdc18a578f50432d32`. The
separate exact per-file audit is bound to the same PR and integration SHAs
above. The 0ef/262-path partition and its 37-script matrix below are historical
evidence only. The immutable compare response records only the 0ef-to-2a2 delta;
it is not used as the current full path inventory.

### Superseded 0ef snapshot: complete changed-path partition (262 paths)

The independent exact-head audit accounted for all 262 unique PR paths. The
following disjoint selectors define the file-level status partition; the
remaining 146 are the PR file set after subtracting the first five selectors.

| Paths | Count | Disposition |
|---|---:|---|
| `crates/velnor-actions-native/**` | 22 | `NOT-CARRIED`; no adopted native recipe or production caller. Preserve independent image-release, runner, DinD, tar-shim, ShellCheck, and freshness owners. |
| Python files under `crates/**` outside `velnor-actions-native` | 7 | `NOT-CARRIED`; unadopted source-proof capsules. Rust asset-manifest validation does not replace their Git-tree, ZIP, or Cargo-read semantics. |
| `scripts/**`, excluding `scripts/check-freshness.sh` | 37 | `TEMPORARY-HOLD`; private builder, qualification, publication, fixture, and Python-test paths need an explicit owned-source disposition. |
| `docs/**`, plus `repositories.txt`, `repository-evidence.csv`, `scope.json`, and `velnor-actions-ci-performance-{goal,spec}.md` | 44 | `PARTIAL`; passive proposals/evidence/inventory, not executable or rollout proof. |
| `fixtures/**` | 6 | `PARTIAL`; output fixtures alone do not establish implementation. Keep only fixtures whose producer/consumer maps to retained code. |
| All other paths in the exact 262-file PR response | 146 | Feature-by-feature decisions below; includes integrated source fixes and explicit partial/hold deltas. |

The five selectors are disjoint. Their counts plus the residual set equal the
262 unique paths returned by the API at this historical cutoff. The live
[PR #12 Files page](https://github.com/tailrocks/velnor-new/pull/12/files) is
mutable navigation; this historical section does not preserve its path list.

### Fixes already present in the integration candidate

`LANDED IN INTEGRATION` below means the listed change is in candidate
`6ea09f2cb2beb595995bb2987b622791b0b7729d`; it is not merged into current
`main` (`47815c83b9eeadbaf84b741918fffa7ea550da89`). None of these source
fixes proves hosted qualification or consumer adoption.

| Feature | Integration source commit(s) | Current disposition |
|---|---|---|
| Crate-obligation self-gates | `87bc5c8041fb066f407017daa664a29dd8bb194e` | `LANDED IN INTEGRATION`; self-gate, strict-prior-gate, and duplicate-precedence regressions retained. |
| Canonical release-tool URL | `a97797e738df4e90b50142d7e44ae2b048ba659f`, `8a5f6c87008fe8cf0578253c1a7bd8462fbb39e9` | `LANDED IN INTEGRATION`; strict URL validation and caller closure. |
| Git optional-lock isolation | `d32139e95d2a88795f065e7cf634ba49e8a9a39d`, `8de8a5ec869b731d7e08e452eb19d290c8216803` | `LANDED IN INTEGRATION`; lock control, config isolation, and Git regressions. |
| Tools-only bootstrap | `dd995b5df61b1573cd75386ae558bfdb7062acb9` | `LANDED IN INTEGRATION`; Rust installation remains absent when the suite owns no Rust tools. |
| Attempt-bound artifact retrieval | `d20226ebc052af03199c4ed2f37a17ae48043520` | `LANDED IN INTEGRATION`; baseline, planned, shard, retry, and race paths. |
| Full Git-ref validation | `f7d38268ce45c23216d9bbdb20184177fc9888ba` | `LANDED IN INTEGRATION`; Git-oracle cases include valid intermediate-dot components. |
| Branch shorthand validation | `77dc7631bde8d0bc20db8c91ea5327243a3aa7d1`, `115781a7e` | `LANDED IN INTEGRATION`; validation is carried through current callers. |
| Generated `.github` preservation | `6fb3229af`, `71f7d0bd9`, `1ddefb071` | `LANDED IN INTEGRATION`; transactional generation, ownership, and symlink-mode handling. |
| Typed suite/tool ownership | `6ee09251a7c8e6fa9c2a19135a73e9eba6d91943` | `LANDED IN INTEGRATION` in `matrix_tools.rs`; PR `matrix_suite.rs` classification remains a separate delta. |
| Nextest binary preparation | `f9743505c`, `075c76ab8` | `LANDED IN INTEGRATION`; argv/bootstrap coverage and test split. |
| Tofu root identity and caller closure | `0ed1882c5`, `731a161b4`, `b901a8603` | `LANDED IN INTEGRATION`; typed identity, root obligations, and fallible caller propagation. |
| Bounded provenance carry | `93371bb48`, `086a257fe`, `b901a8603` | `LANDED IN INTEGRATION`; baseline carries original proof run and distinct carrying-run evidence with parent/digest bounds. |
| MBX external-bundle lifecycle and 1.22.0 fixtures | `9a249e9ab`, `47815c83b`, `c447a2e35`, `b4c5f38e6`, `3da61ae9f` | `LANDED IN INTEGRATION`; production uses the reviewed local backend + explicit bundle route. Hosted workload/disk-pressure qualification remains `PARTIAL`. |
| Release-manifest validation/publication source | `385a772bc`, `03dc45e9a`, `35f52bf18`, `fccd0beb9`, `4b9e30f06` | `LANDED IN INTEGRATION`; version 0.1.1, canonical manifest name, source/freshness/CI checks, and attestation validation are source changes, not release receipts. |
| Consumer manifest policy boundary | `7d274f201451a4575835998ed889c49e551006cf` | `LANDED IN INTEGRATION`; consumer manifest reads are policy-scoped, malformed producer `generator.lock` remains fail-closed, and CLI warning coverage is retained. Not merged to `main` or a release receipt. |
| Cross-platform symlink mode preservation | `6ea09f2cb2beb595995bb2987b622791b0b7729d` | `LANDED IN INTEGRATION`; checked conversion targets Rustix's platform `RawMode`, retaining Apple narrowing checks and Linux `u32` behavior. Exact-head hosted PR #28 run `37183271616` passed; this does not establish release or consumer qualification. |
| Version pins and regenerated outputs | `95f82d3b6`, `5b42f908d`, `4c90b851e`, `47a5b50e4`, `5b3f4c8e1`, `140faf330` | `LANDED IN INTEGRATION`; generator 0.1.1 and MBX 1.22.0 source/fixtures. Nested runner 0.1.0 and historical immutable v0.1.0 artifacts remain intentional. |

### Superseded 0ef delta and dispositions (historical only)

At PR head `0efb5565dd33a70b67fc2198d032153f6c5bee9f`, since prior head
`9d7fc047565d97a7635133f590f967d16dcbddef`, the PR had 33 changed blobs, with
five paths newly added. That tree comparison found 23 byte-identical paths,
112 paths present with different blobs, and 127 PR paths absent from the then
current integration candidate. These counts do not describe head `2a2dab0e`.

- **Consumer manifest policy — `LANDED IN INTEGRATION`, not merged.** Commit
  `7d274f201451a4575835998ed889c49e551006cf` scopes manifest reads to
  `ConsumerV1`; `VelnorRepositoryV1` ignores malformed, invalid-UTF-8,
  directory, and symlink consumer-only manifest inputs. A paired regression
  proves malformed producer-owned `.velnor/generator.lock` still fails closed
  through `finalized::owned_preparation` and
  `verify_velnor_repository_files`/`parse_generator_lock`; version-policy
  catalog mismatch validation remains. Consumer CLI warning coverage remains
  enabled while producer generation does not emit that consumer-only warning.
  This source slice passed exact-head review; final integrated gates and main
  merge remain separate.
- **Generation and workflow-context extraction — `PARTIAL`.** PR adds
  `generate_output_commit.rs` and `workflow_context.rs`; transactional output
  behavior already exists in integration. Treat these as implementation
  structure, not a missing-behavior claim, unless a caller-level regression
  is demonstrated.
- **Suite ownership — `PARTIAL`.** PR adds runner classifications to
  `matrix_suite.rs`. Integration uses the typed `matrix_tools.rs` registry.
  Reconcile the registries without adopting native recipes or putting runner
  behavior into V1.
- **Foundation additions — `NOT-CARRIED` for the executable proposal.** PR
  adds CLI arguments, smoke/validator paths, staging, and Foundation workflow
  behavior. Current integration rejects the retired Foundation flag and has
  detachment regression `7bdc4d147`; keep that explicit decision and its
  tests. Do not label PR-only Foundation code as landed.
- **T32 identity — `LANDED IN INTEGRATION`.** The current path is
  byte-identical and uses `dir-` identity.
- **Freshness timestamp refreshes — `PARTIAL`.** Timestamp-only edits do not
  qualify tools or prove current-source freshness.
- **Private owned-source files at 0ef — historical only.** The 37-path matrix
  belongs to that old head. The latest 2a2 head's exact 54 added script paths
  are classified in [`pr-12-script-disposition-2026-10-04.md`](pr-12-script-disposition-2026-10-04.md).
  No private qualification or publication is inferred.

The independent audit specifically accounted for all seven embedded Python
capsule paths:

```text
crates/velnor-actions-orchestrator/src/release_admission.py
crates/velnor-actions-orchestrator/src/release_source_snapshot.py
crates/velnor-actions-orchestrator/src/release_source_tree.py
crates/velnor-actions-orchestrator/tests/release_source_snapshot_test.py
crates/velnor-actions-orchestrator/tests/release_source_tree_test.py
crates/velnor-actions-rust/src/release_source_intent_guard.py
crates/velnor-actions-rust/tests/release_source_intent_guard_test.py
```

These are `NOT-CARRIED`: no production caller is registered. This is not a
claim that the Rust asset manifest replaces source-tree authentication, ZIP
comparison, or Cargo read-closure behavior. Likewise, the 22 native paths are
`NOT-CARRIED` because no adopted recipe/caller uses them. This does not remove
the separate image-release, runner, DinD, tar-shim, ShellCheck, or freshness
owners.

### Current validation and rollout limits

- The earlier source candidate `140faf` passed pinned local workspace gates:
  Nextest 2,761 passed / 1 skipped; strict workspace/all-target Clippy, fmt,
  Alint, cargo-deny, and freshness passed. That receipt does not cover later
  changes. Local freshness did not run the optional live advisory scan; CI
  Cargo Deny owns that scan.
- PR #28 run `37182791818` at `fdc731e3` was cancelled after its CLI and
  orchestrator Rust jobs failed strict Clippy on Linux's `RawMode = u32`.
  The target-aware checked conversion is in `6ea09f2`. Exact-head run
  [`37183271616`](https://github.com/tailrocks/velnor-new/actions/runs/37183271616)
  completed successfully at `2026-10-04T06:50:41Z`; all checks passed and
  Publish baseline was skipped. Refresh PR feedback/reviews before marking it
  ready or merging.
- PR #12 current-head run `37183335691` at `2a2dab0e` failed `Alint`, `Plan`,
  and aggregate `Required`. Alint rejects the new nested fixture's stray
  `scripts/qualification/mbx-synchronous/registry-fixture/Cargo.lock`; Plan
  rejects its unclassified suite `mbx-synchronous-registry-fixture`. These
  are PR-only fixture checks, not integration-candidate failures. Do not add
  a nested workspace lock or weaken the lock allowlist.
- Historical PR #12 run [`37178556819`](https://github.com/tailrocks/velnor-new/actions/runs/37178556819)
  at head `0efb5565` failed `impl_cli_verify_local::verify_local_repo_policy_stage_executes`:
  `check-freshness.sh` reported `uv` evidence checked at
  `2026-10-02T21:54:08Z`, 31.1 hours old against the 24-hour interval; the
  CLI job and aggregate `Required` failed. The orchestrator job passed. This is
  a historical failure at the earlier head, not the current run's failure.
- Official Mise adoption is `PARTIAL`: latest published stable 2026.10.1 is
  still the known-broken pre-fix binary. Do not claim a fixed official
  distribution from the source-built auxiliary binary.
  Fresh public release/API review at `2026-10-04T06:53:09Z` found no newer
  stable release: [v2026.10.1 release `402539535`](https://api.github.com/repos/jdx/mise/releases/402539535)
  remains immutable, and its tag peels to
  `050ce5a20287a0aafd872b1191699a5fdafff5ac`, eight commits before accepted
  fix `dfe74a90b41603625ee6aabecb42f14a1f5eb0f6` ([compare API](https://api.github.com/repos/jdx/mise/compare/v2026.10.1...dfe74a90b41603625ee6aabecb42f14a1f5eb0f6)).
  Its [macOS ARM64 asset](https://api.github.com/repos/jdx/mise/releases/assets/607914737)
  SHA-256 remains `d225d1c8ef2934a86be93692a19365fb1df1cd6958a0af7b85909514e0c608a7`;
  [Linux x64](https://api.github.com/repos/jdx/mise/releases/assets/607914627)
  remains `31e6859cf639ed4594906da3fcd0fe2055e9daddae75e9786dbe50b3fb3c0f4a`.
- MBX hosted writer/reader, cache-version/path reuse, cancellation, corrupt
  import, parallel writer, ChainArgos workload peak bytes/inodes, and disk
  pressure remain `PARTIAL`.
- Earlier 2026-10-04 R13 snapshot: Adopted R13 requires an explicit same-repository PR cache opt-in
  in a separate PR namespace while forks remain read-only and trusted production keys remain push-only.
  The typed `SameRepositoryScoped` config and trusted push-only authorization cleanup are integrated.
  The renderer does not yet consume the config or enable the PR-scoped namespace; that wiring and repeat
  same-repository PR/fork hosted proof remain pending. Do not treat a same-repository PR save as trusted-cache authorization.
- 2026-10-04 cache-contract delta: renderer consumes `SameRepositoryScoped`, validates same-repository,
  non-fork PR identity, binds PR number and head SHA into a separate namespace, and gates export/save
  on effective policy. Wiring is implemented; repeated same-repository PR/fork hosted proof remains pending; PR writes do not authorize trusted-cache writes.
- Published archive recovery is complete: immutable release `402793692` has
  fresh-download proof with original JSON digest
  `07b853c18389042bb8adde7051b4a188f562f5ba461eb1112e4ac8afeba7c50f`; the
  183-commit enumeration addendum digest is
  `68f84758234395a049e83c17070403915d07ada1a92bbe9c4b6785ebb21cada0`. The
  [archive ledger](archive-proof-2026-10-04/snapshot-retirement-ledger.md)
  records six protected anchors. These proofs do not establish historical
  Action SHA resolution or authorize deletion.
- Consumer migration and six-ref retirement remain `TEMPORARY-HOLD`. The
  audited 46-default + relevant-PR union found no snapshot SHAs and recorded
  three owner holds; 45 workflows still use 0.1.0 and ChainArgos generator
  migration remains held. Recheck live consumers, preserve action-SHA history,
  complete owner checkpoints, then run a separate exact-ref deletion review.
  Do not delete the three out-of-scope MBX refs.
- Release publication remains unqualified. Environment `generator-release`
  (`23405613232`) has a required reviewer, self-review prevention, and
  protected-branch-only policy, but GitHub reports `can_admins_bypass=true`.
  The REST schema does not expose that setting; the bounded UI attempt failed
  because no browser surface was available (`cgWindowNotFound`). No setting
  changed. Owner `@donbeave` must disable administrator bypass through the
  logged-in GitHub environment settings and verify readback. BOOT-4.2 still
  lacks two distinct administrator approvals and an independent reproducible
  rebuild receipt; environment setup does not satisfy either receipt.

### Required final refresh before PR #12 closure

At final integration head, fetch the PR head/base, every paginated changed
path, all issue/review/inline comments and thread resolution state, and all
required check runs. Recompute the latest-head changed-path partition (280
paths at the REST cutoff above) and current-tree blob comparison; update this
record if the PR moves. Preserve the exact paginated path inventory with the
result. The mutable [PR #12 Files page](https://github.com/tailrocks/velnor-new/pull/12/files)
is live navigation, not the inventory snapshot. Reply to every accepted or
rejected review item with a fixing commit URL or evidence before resolving.
Keep PR #12 open until the consumer-manifest policy fix and every other
applicable disposition are complete; then close or supersede it only after a
fresh zero-feedback review.

---

## Historical snapshot — superseded

The following snapshot was written against PR head `c694d8029` / 252 paths and
integration base `f7d38268`. It is retained as historical evidence only. Its
status claims, especially its archive state and `TEMPORARY-HOLD` rows for work
now landed in `140faf`, are not current.

Snapshot: 2026-10-04. This is a passive review record against integration base
`f7d38268ce45c23216d9bbdb20184177fc9888ba`.

## Evidence boundary

- PR #12 is open at exact head [`c694d8029eb880db639fa89b0589090dd2b15364`](https://github.com/tailrocks/velnor-new/commit/c694d8029eb880db639fa89b0589090dd2b15364). Its recorded base is `c57c700459bbe1549fe7eedcb7d8689585c38986`; live `main` is `47815c83b9eeadbaf84b741918fffa7ea550da89`. Integration base `f7d38268...` descends from that live main.
- Refresh on 2026-10-04 returned the same PR head and base, with 252 changed paths across the paginated files endpoint; there is no new diff since this historical snapshot. GitHub reports only the DCO check complete/successful and no submitted reviews. No hosted qualification or consumer result is inferred. The [PR #12 Files page](https://github.com/tailrocks/velnor-new/pull/12/files) is mutable live navigation, not an immutable inventory snapshot.
- `LANDED` means the named source fix is an ancestor of `f7d38268`; it does not mean runtime or rollout qualification. All missing or unresolved work stays `PARTIAL` or `TEMPORARY-HOLD`.
- Direct tree checks at `f7d38268` find the private Python tools, Foundation qualification, `velnor-actions-native`, `root_identity.rs`, and typed `matrix_suite.rs` absent. Keep current freshness, release, OCI, workflow, and cache code intact; this document changes none of it.

## Feature dispositions

| Feature | Status at `f7d38268` | File-level disposition / evidence |
|---|---|---|
| Self-gated crate obligations | LANDED | `crate_job.rs` and its tests; fix [`87bc5c8041fb066f407017daa664a29dd8bb194e`](https://github.com/tailrocks/velnor-new/commit/87bc5c8041fb066f407017daa664a29dd8bb194e). |
| Canonical release asset URL validation | LANDED | `crates/velnor-actions-contract/src/tooling.rs`; fix [`a97797e738df4e90b50142d7e44ae2b048ba659f`](https://github.com/tailrocks/velnor-new/commit/a97797e738df4e90b50142d7e44ae2b048ba659f). |
| Optional Git lock isolation | LANDED | `crates/velnor-actions-mise/src/command_git.rs`, command and Git tests; fix [`d32139e95d2a88795f065e7cf634ba49e8a9a39d`](https://github.com/tailrocks/velnor-new/commit/d32139e95d2a88795f065e7cf634ba49e8a9a39d). |
| Tools-only no-Rust bootstrap | LANDED | Generator/bootstrap workflow paths; fix [`dd995b5df61b1573cd75386ae558bfdb7062acb9`](https://github.com/tailrocks/velnor-new/commit/dd995b5df61b1573cd75386ae558bfdb7062acb9). |
| Attempt-bound artifact retrieval | LANDED | `retrieve_baseline.rs`, planned retrieval, retry fixtures/tests; base fix [`d20226ebc052af03199c4ed2f37a17ae48043520`](https://github.com/tailrocks/velnor-new/commit/d20226ebc052af03199c4ed2f37a17ae48043520). This does not close the separate provenance-carry hold below. |
| Full Git-ref validation | LANDED | `crates/velnor-actions-contract/src/vcs.rs`; fix [`f7d38268ce45c23216d9bbdb20184177fc9888ba`](https://github.com/tailrocks/velnor-new/commit/f7d38268ce45c23216d9bbdb20184177fc9888ba). |
| Branch shorthand grammar | PARTIAL | PR change [`6209c06d87c2f7e41ff162b77cc51e0c99989eec`](https://github.com/tailrocks/velnor-new/commit/6209c06d87c2f7e41ff162b77cc51e0c99989eec) updates `branch.rs`, `cover_baseline.rs`, and `internal_request.rs`; it is not in `f7d38268`. Full-ref validation above does not establish shorthand parity. |
| Provenance carry and closed source proof | PARTIAL | Retrieval is landed, but PR-only `provenance_lineage.rs`, `provenance_lineage_tests.rs`, provenance callers, and source proof capsule work are absent from the base. PR commits [`5280a5a1a7858e41842d77632e426b63ec9655b2`](https://github.com/tailrocks/velnor-new/commit/5280a5a1a7858e41842d77632e426b63ec9655b2) and [`1dcd0791e90abb0e5b758556f0746d0f061c994e`](https://github.com/tailrocks/velnor-new/commit/1dcd0791e90abb0e5b758556f0746d0f061c994e) remain candidates; no caller evidence is supplied here. |
| Tofu root identity and caller migration | TEMPORARY-HOLD | PR additions `root_identity.rs` and root-key caller/test updates from [`49441b888679ba08f6cbb3d0c33809958fd20f31`](https://github.com/tailrocks/velnor-new/commit/49441b888679ba08f6cbb3d0c33809958fd20f31) and [`5b4f9be958e97acb1ec8034ada252fc54747e9ac`](https://github.com/tailrocks/velnor-new/commit/5b4f9be958e97acb1ec8034ada252fc54747e9ac) are absent at the base. No caller migration is inferred. |
| Typed repository suite ownership | TEMPORARY-HOLD | PR `matrix_suite.rs`, tests, and crate-job/tool wiring from [`6c110a9050b7f8af0fef72b2be85ed24e762146c`](https://github.com/tailrocks/velnor-new/commit/6c110a9050b7f8af0fef72b2be85ed24e762146c) are absent at the base. |
| Nextest binary preparation | TEMPORARY-HOLD | PR Rust argv/workflow-tool tests from [`8a75f54847d636fe278d302310abce060e236120`](https://github.com/tailrocks/velnor-new/commit/8a75f54847d636fe278d302310abce060e236120) and regenerated `.github/workflows/ci.yml` from [`ce3c157a444e00db3148ca1885f533f91e700d1b`](https://github.com/tailrocks/velnor-new/commit/ce3c157a444e00db3148ca1885f533f91e700d1b) are not in the base. Keep the current workflow until matching source is integrated and regenerated. |
| `.github` output preservation | TEMPORARY-HOLD | PR `generate_preserve.rs` and its tests from [`f3c937acc2df90fada059f3f427f7157ce08461c`](https://github.com/tailrocks/velnor-new/commit/f3c937acc2df90fada059f3f427f7157ce08461c) are absent. Preserve the integration tree until this behavior and its complete ownership boundary are integrated. |
| Foundation executable closure | TEMPORARY-HOLD | PR CLI dispatch, orchestrator source/tests, renderer-owned workflow source/tests, and `.github/workflows/foundation-qualification.yml` from [`57dc370d8e84e6d1edc34a273a5ccc313cb03dd0`](https://github.com/tailrocks/velnor-new/commit/57dc370d8e84e6d1edc34a273a5ccc313cb03dd0) / [`d94fe9cefa6a0b97f00d235191c2d3142d1af6ff`](https://github.com/tailrocks/velnor-new/commit/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff) are absent. Source files and fixtures alone do not establish executable closure. |
| Owned-tool source, qualification, and publication scripts | TEMPORARY-HOLD | The 19 principal scripts, `owned_mise_qualification.py`, two support fixtures, and 14 script tests plus pinned requirements are listed below. They are absent from the base; source inspection or test fixtures do not qualify a caller or published tool. |
| Independent OCI / Ruby / shell / REUSE owners | TEMPORARY-HOLD | `crates/velnor-actions-native/**`, including OCI codecs/tests and Ruby, shell, and REUSE facades; native ownership commit [`92fee7465294af8f721319ccaec6d1308832d9df`](https://github.com/tailrocks/velnor-new/commit/92fee7465294af8f721319ccaec6d1308832d9df), OCI addition [`8cba870d41f00be145ff6ddb3420926078d85451`](https://github.com/tailrocks/velnor-new/commit/8cba870d41f00be145ff6ddb3420926078d85451). The crate is absent at the base; no native recipe adoption is inferred. |
| MBX external-bundle lifecycle | LANDED | The base already saves hosted MBX as one external bundle and restores it before the build in `mbx_bundle.rs` and `cache_steps.rs`; fixes [`9a249e9ab99a019ab7ba0e6f835dee406827db20`](https://github.com/tailrocks/velnor-new/commit/9a249e9ab99a019ab7ba0e6f835dee406827db20) and [`47815c83b9eeadbaf84b741918fffa7ea550da89`](https://github.com/tailrocks/velnor-new/commit/47815c83b9eeadbaf84b741918fffa7ea550da89) are in the base ancestry. Preserve this current lifecycle. |
| MBX parity / fixture evidence | TEMPORARY-HOLD | Separate candidate [`1ecd84d1f6320e9a0209968c2057952b9363c8a4`](https://github.com/tailrocks/velnor-new/commit/1ecd84d1f6320e9a0209968c2057952b9363c8a4) is not in PR #12 or `f7d38268`. Hold only reconciliation of its renderer and fixture parity evidence with the already-landed lifecycle; it is not the source of that current behavior. |

## Python source and test file dispositions

All paths in this section are absent at `f7d38268` and remain `TEMPORARY-HOLD`.

19 principal private source scripts:

```text
scripts/analyze-ci-performance.py
scripts/bootstrap-owned-tool-builder.py
scripts/build-owned-tool.py
scripts/collect-ci-performance.py
scripts/download-owned-tool-candidate.py
scripts/immutable_publication.py
scripts/owned_tool_behavior.py
scripts/owned_tool_qualification_evidence.py
scripts/owned_tool_source.py
scripts/publish-owned-tool-artifacts.py
scripts/publish_owned_source.py
scripts/qualify-owned-tool.py
scripts/source_proof_capsule.py
scripts/source_publication.py
scripts/source_publication_records.py
scripts/source_qualification_execution.py
scripts/source_release_policy.py
scripts/source_semver_capsule.py
scripts/stage-owned-tool-source.py
```

Separate qualification module and two support fixtures:

```text
scripts/owned_mise_qualification.py
scripts/owned_tool_execution_test_fixtures.py
scripts/owned_tool_publication_test_fixtures.py
```

14 script tests and pinned requirements:

```text
scripts/test_ci_performance_analysis.py
scripts/test_download_owned_tool_candidate.py
scripts/test_git_optional_locks.py
scripts/test_owned_tool_build.py
scripts/test_owned_tool_execution_evidence.py
scripts/test_owned_tool_qualification_evidence.py
scripts/test_publish_owned_tool_artifacts.py
scripts/test_qualify_owned_tool.py
scripts/test_source_proof_capsule.py
scripts/test_source_publication.py
scripts/test_source_publication_records.py
scripts/test_source_qualification_execution.py
scripts/test_source_release_transaction.py
scripts/test_source_semver_capsule.py
scripts/ci-performance-analysis-requirements.txt
```

## Remaining changed-path families

| Changed paths in PR #12 | Status / disposition |
|---|---|
| `.github/workflows/ci.yml`, `.github/workflows/foundation-qualification.yml`, `Cargo.toml`, `Cargo.lock` | `PARTIAL` / `TEMPORARY-HOLD`: workflow and dependency changes travel with the exact generator/feature that owns them; retain the base workflow and cache behavior meanwhile. |
| `.velnor/freshness-inventory.json`, `.velnor/version-policy.toml`, `scripts/check-freshness.sh` | `PARTIAL`: preserve the existing freshness/pin gates; PR edits are not evidence those gates pass on the integration source. |
| `crates/velnor-actions-mise/src/catalog.rs`, `catalog_versions.rs`, and catalog tests | `PARTIAL`: keep catalog/version changes with their pin and freshness evidence; no new tool version is adopted by this ledger. |
| `crates/velnor-actions-orchestrator/src/release_admission.py`, `release_source_snapshot.py`, `release_source_tree.py`; `crates/velnor-actions-rust/src/release_source_intent_guard.py` and tests | `TEMPORARY-HOLD`: PR-only embedded release guards and source readers; absent at the base and unqualified as callers. Keep with source-bound publication work. |
| `crates/test_support/**`, `crates/velnor-actions-cli/tests/fixtures/p12_*`, `crates/velnor-actions-*/tests/**`, `fixtures/parity/**` | `PARTIAL`: test helpers, P12 fixtures, and regenerated expected outputs remain attached to their held source changes; a golden update is not implementation proof. |
| `docs/proposed/{architecture.md,rust-quality-contract.md,workflow-contract.md,workflow-matrix-contract.md}` | `PARTIAL`: contract proposals only; they do not close implementation, caller, or acceptance obligations. |
| `docs/reviews/ci-performance-*`, `docs/reviews/ci-performance-nextest-evidence/**`, `docs/reviews/python-uv-upgrade-evidence.json`, `repositories.txt`, `repository-evidence.csv`, `scope.json`, `velnor-actions-ci-performance-{goal,spec}.md` | `PARTIAL`: retain as audit/evidence records. PR source records all 47 performance rows `INCOMPLETE`; no runtime, performance, archive, or consumer rollout is granted. |
| Remaining edits under `crates/velnor-actions-{cli,contract,mise,orchestrator,rust,tofu,workflow-renderer}/**` | Follow the feature rows above: landed base fixes stay; PR-only source, tests, and callers remain `PARTIAL` or `TEMPORARY-HOLD`. This historical snapshot does not preserve an exact path inventory; the live PR Files page is mutable navigation. |

## Separate archive and consumer holds

**Archive status: TEMPORARY-HOLD.** The local candidate at
`/private/tmp/velnor-owned-source-archive-candidate-20261004` is unpublished and
not durable. A live `ls-remote` on 2026-10-04 returned all eight owned-source
refs, including these six in-scope tips; none was deleted.

Each row is a Phase G hold owned by `@donbeave` through checkpoint
**2026-10-11** (seven calendar days from the 2026-10-04 start). For every row,
next event is: publish a fresh immutable archive tag, independently download,
checksum, and restore it in a clean repository, then recheck the current ref
and consumers and record the retained action-SHA plan. This depends on final
caller migration and release CI. Keep the archive `TEMPORARY-HOLD` until the
published bytes pass that proof; keep every ref until then.

| Ref | Tip SHA | Owner | Checkpoint | Status |
|---|---|---|---|---|
| `refs/heads/owned-source/cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf` | `8758d976a1b25eb387f48aa04ea86f57739b84cf` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/mbx-action/62ec0713473dffeab46884b7c03906042794e696` | `62ec0713473dffeab46884b7c03906042794e696` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075` | `c3cbe8e56ccb4727624df45022357f49d2953075` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |
| `refs/heads/owned-source/semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd` | `583dddce84706786fc54c41a2c768c28a09c65fd` | @donbeave | 2026-10-11 | TEMPORARY-HOLD |

**Consumer status: PARTIAL.** The PR's source inventory and local changes do not
prove final caller migration, current consumer pins, release CI, or adoption.
Recheck current consumers and action SHAs after the migration and release CI;
do not infer adoption from the archive candidate or from PR source/test files.
