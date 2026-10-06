# /goal — close PR #12 owned-source evidence gaps and retire only what is unnecessary

## Operating authority

Default mode is **RESEARCH_ONLY**. Do not push, open PRs, modify issues/releases, dispatch workflows, change remote branches or mutate keeper checkouts. Use new scratch bare clones and immutable file reads. Produce evidence, a final disposition and reviewable proposed patches/specification only. The current research request is not permission to implement or retire remote refs.

When the owner explicitly initiates **IMPLEMENTATION** using this goal, execute the implementation sections below. Reuse the existing appropriate branch; use small logical commits, merge rather than rebase shared work, and avoid competing branches or duplicated PRs. Remote publication/deletion still requires the scope of that authorization. Never treat an unresolved evidence gate as permission to act destructively.

Work autonomously within that authority. Use independent lanes for ancestry/inventory, consumers, workaround/upstream necessity, Rust tests, and security/deletion review where agents are available. Do not claim a subagent, command or test ran when it did not. Resolve disagreements by immutable source and actual execution evidence, not confidence. Missing access is a recorded gap, never an empty result.

## Objective

Finish the minimum necessary Velnor work left by the PR #12 review: preserve the stock mise no-config guarantee, adopt its already-merged official fix, retain independent stock-tool cache repairs, and retire unneeded private source/publication infrastructure safely. Do not port the entire Python system. Do not remove unrelated PR #12 fixes, ordinary MBX support, existing freshness/OCI/release tooling, or Python required by consumer workloads. No permanent private fork is an acceptable end state.

Primary repository: https://github.com/tailrocks/velnor-new
PR: https://github.com/tailrocks/velnor-new/pull/12

Review anchors, which must be refreshed before work:

```text
main                     5a946c33cf005777feab2bc91fa4aa8e01dd58f4
PR #12 head              d94fe9cefa6a0b97f00d235191c2d3142d1af6ff
PR #12 branch            perf/cache-selection-qualification
owned mise               dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96
owned MBX                ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81
owned action older       c3cbe8e56ccb4727624df45022357f49d2953075
owned action newer       62ec0713473dffeab46884b7c03906042794e696
owned semver             583dddce84706786fc54c41a2c768c28a09c65fd
owned cache/Foundation   8758d976a1b25eb387f48aa04ea86f57739b84cf
accepted mise fix        dfe74a90b41603625ee6aabecb42f14a1f5eb0f6 (jdx/mise#13926)
separate stock fix PR26  1ecd84d1f6320e9a0209968c2057952b9363c8a4
separate tar-absolute    5b309009599ce11173830045e237fef6900d6b42
```

The prior review is PARTIAL: API object/tag evidence was available, but fresh `cat-file`, full all-ref/46-consumer coverage, real test execution and action bundle inspection were missing. Do not promote that review's provisional DROP targets to universal no-consumer facts.

## Phase A — close the evidence before deciding what can be deleted

Refresh main, `refs/pull/12/head`, all repository branches/tags and all open PR heads. Record every SHA, timestamps and changes at the end of the sweep. Do not trust PR #12's stale body head `fb9963a…`. The prior sweep found 14 branches; that is a historical observation, not a fixed current count.

Clone each true upstream into a NEW bare scratch repository and verify the upstream tag/object before importing any foreign snapshot:

```text
jdx/mise:
  original bc11f90c74eba23bf0d7350efb540e62fb7d9ffd  v2026.10.0
jdx/mr-boxington:
  original 201b9df3d18e8e96831bee631035f6b7c7ae20e0  v1.21.0
  merged   a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313  v1.21.1
jdx/mr-boxington-action:
  original 9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3  v1.5.0
  merged   1687e54eb349cadf61fa38b5813a77875489e8e6  v1.6.0
obi1kenobi/cargo-semver-checks:
  original 4297e8b5f6306531375ba2ba332171e5792b4c38  v0.50.0
actions/cache:
  original 55cc8345863c7cc4c66a329aec7e433d2d1c52a9  v6.1.0
```

Use `git cat-file -e SHA^{commit}`, `git rev-parse refs/tags/TAG^{commit}`, full parent inspection, custom `rev-list`, and per-parent `diff --stat --numstat`. Import owned commits under local `refs/review/` only. Distinguish original fork points from later upstream merges. The reviewed inventory has 15 unique custom commits; regenerate and investigate differences. Separate authored code, generated bundles, vendored tests and removed upstream CI when reporting sizes.

Load the exact current `scope.json`; at reviewed PR head it named 46 consumers. Search every Velnor ref plus every consumer's generated workflow/composite/script graph. Required detailed first cohort:

```text
jackin-project/jackin
tailrocks/velnor
tailrocks/parallax
ChainArgos/java-monorepo
tailrocks/github-terraform
tailrocks/velnor-actions-fixture
```

Search all six full SHAs, owned branch names, `MISE_OWNED_CARGO_WRAPPER`, `owned-cache-transport`, `comparison-state`, `expected-binary-sha256`, `velnor-plan`, `velnor-compare`, Foundation, SDK/quarantine names, every pipeline script name and import stem. Use bounded `git grep` and follow actual positive edges. An indexed default-branch search alone is insufficient. Inspect real tests and generated code, not just workflow `uses:` lines. Resolve omitted policy with the consumer's actual pinned generator.

Classify evidence separately: prose; mock fixture; real-code test definition; staged workflow/source graph; actual hosted execution. For runtime claims record run ID, attempt, head/merge SHA, job, command and log. Do not dismiss `tests/owned_supplied_semantics.py` as mocked: at semver `583ddd…` it launches Cargo and the actual owned executable. Determine whether anything actually invokes it.

If a live consumer was missed, lead the report with its complete execution chain and suspend only the affected deletion. Exhaust specific Velnor seams before any upstream proposal. No foreign feature survives merely because it is sophisticated or locally tested. No feature is declared unupstreamable without maintainer evidence.

## Phase 0 — selective Rust port before deleting Python coverage

Port only stock mise no-config/miserc cases from `scripts/owned_mise_qualification.py` to `crates/velnor-actions-mise/tests/impl_miserc_isolation.rs` or the exact existing harness-equivalent. Reuse `IsolatedCommand` and its approved environment/process boundary. Do not port owned Cargo-wrapper dispatch, fake MBX, publication receipts, source capsules or candidate orchestration.

Use a real official binary. Test flag/environment selection × version/exec × root/nested CWD, malformed project/global/system miserc layers, a no-no-config control and a known affected-release control. Preserve exact binary/release identity. Missing binaries or unrun platforms are not successful qualification. Keep the change small, approximately 120–220 authored test/helper lines, independently reviewable before deletion.

The accepted upstream fix is already `jdx/mise#13926`, merged 2026-10-03 15:37:37 UTC. The latest release inspected in the review was v2026.10.1 at `050ce5a20287a0aafd872b1191699a5fdafff5ac`, and its source did not contain the guards. Re-query releases now. Select an official fixed release by ancestry/equivalent-backport evidence AND actual binary tests; do not invent a version or assume merge equals deployment. No duplicate upstream PR.

Then update the actual `MISE_VERSION`/bootstrap/checksum/freshness authorities and regenerate outputs. If no fixed binary exists, report adoption PARTIAL and continue independent cleanup. A temporary neutral-CWD/direct-executable workaround must be proved at `IsolatedCommand::mise_exec`, `mise_install`, `with_cwd` and `repo_task`, including global/system isolation; merely changing CWD is not proof.

Any newly discovered retained Python test consumer must be ported in this Phase 0 to its existing owning Rust crate before deletion. Do not defer a needed port to an unspecified later task.

## Phase B — retire the unnecessary pipeline and Foundation closure

Target principal-script disposition:

```text
PORT selected stock-mise regression cases only:
  scripts/owned_mise_qualification.py

DELETE after exact caller closure:
  scripts/analyze-ci-performance.py
  scripts/collect-ci-performance.py
  scripts/bootstrap-owned-tool-builder.py
  scripts/build-owned-tool.py
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

Remove corresponding support fixtures/tests only with their imports/callers. Specifically check `test_git_optional_locks.py` for surviving generic Git-policy coverage before deciding it belongs to this retirement. Preserve historical performance/publication outputs as passive evidence. Do not use `rm scripts/*.py`.

The reviewed candidate qualifier admits only `owned-tools.yml` via `workflow_dispatch/main/build-native` or `push/owned-tool-candidates/publish-artifacts`; main's workflow was not retrievable, the candidate branch was absent, and MBX qualification explicitly returned unavailable. Recheck that evidence; do not build a new publisher just to make staged code look useful.

Foundation at PR #12 workflow line 21 references the cache snapshot. Trace and remove, as one coherent change if still unnecessary:

```text
.github/workflows/foundation-qualification.yml
crates/velnor-actions-orchestrator/src/foundation_qualification.rs
crates/velnor-actions-orchestrator/src/foundation_qualification_source.rs
crates/velnor-actions-workflow-renderer/src/foundation_qualification.rs
their dedicated template, registration/CLI plumbing and tests
```

The exact template/test paths must come from the current tree, not guessing. The source tuple binds more than a YAML pin. Do not replace the Foundation subaction with stock cache; stock cache is not that action.

Foundation is STAGED-HOLD pending the owning decision. Proposed owner `donbeave`, proposed decision checkpoint 2026-10-11. Recommended outcome is removal. Retention requires a named actual runner requirement, execution evidence and a minimal Velnor-tree implementation, not a permanent `actions/cache` fork. A deadline never substitutes for deletion authorization.

The owned MBX/action comparison pairing is prohibited: both action tips share v1 `comparisonExportResult`, while MBX `ac6ceed…` produces v2 `emitted_bundle_useful_delta`. Do not blindly accept both protocols or claim all ordinary modes are broken. If a necessary comparison consumer survives, rebuild and inspect committed `dist/index.js` and run real producer→packaged-action tests before designing one coherent supported pairing. Otherwise retire the feature, not repair an unused protocol.

## Phase C — preserve and qualify the stock cache repair, without duplicating PR #26

Refresh PR #26 and coordinate with its current owner. `mbx_bundle.rs` at inspected `1ecd84d1…` restores through stock action but separately saves `${{ runner.temp }}/mbx-single-bundle`; the stock action restores/imports its own store-internal archive path. Cache version includes path/compression, so sharing only the key is insufficient.

Choose one coherent contract: preserve the action's full restore/save behavior, OR have Velnor render BOTH stock restore and stock save of an identical external bundle path, plus explicit official MBX import/export. Keep path identity stable across writer/reader runs and put qualification freshness in keys rather than random cache paths. Preserve PR read-only policy. Do not delete a store unless ownership, canonical containment, exclusive job access and stopped consumers are proved; `*mbx*` is not an ownership check.

Use official MBX/action versions (`a0a44c…` / `1687e5…` as the reviewed baseline), no owned-report protocol. Add real two-job cache-service qualification with prefix restore, imported objects and compilation reuse; include corruption, empty export, cancellation and shared-store refusal. Measure the affected ChainArgos workload separately. Earlier-head PR test claims and a small probe are not proof for the current large workload.

Keep this repair independently mergeable; do not block narrow source retirement by inventing a larger cache architecture project or silently inherit a broken workaround.

## Phase D — verification, rollout and safe branch endgame

Run current pinned repository gates, including formatting, Clippy, targeted/full Rust tests, existing Nextest, policy/security/dependency checks, generator parity and freshness. Read current repository instructions first. Do not weaken or skip gates to get a green badge.

Regenerate repository-policy and consumer-policy output from the same candidate twice; demand byte-identical output and no second-run changes. No active generated workflow, renderer tuple, script, build/test runner or runtime dependency may retain one of the six owned pins after retirement. Passive history may retain clearly labeled references.

In authorized implementation mode publish through the existing immutable generator release mechanism and regenerate the six initial consumers, then all 46. Record repo SHA, effective policy, generator artifact digest, changed pins, validation and actual PR/main run IDs and attempts. Never overwrite a used `v0.1.0` release. Source acceptance, release creation and consumer deployment are distinct events.

For EACH owned snapshot branch: retain until all-reference closure, transferred/removed consumers, archived Git history, and actual merged resolving SHA are recorded. Create and verify a Git bundle, restore it in an independent bare scratch repo, and `cat-file` expected commit/tree objects. Preserve immutable source-only releases; peel tags rather than trusting `target_commitish`. Branch deletion does not immediately delete a commit, but undocumented object retention is not a durability guarantee.

No branch is “delete now” under the review's incomplete evidence. Before any separately authorized deletion, re-fetch and recheck current pins, record owner, archive digest, exact resolving ref, event and rollback. Never delete main, unrelated refs, or generator releases.

## Required final output

Produce the final spec/goal, complete per-change verdict and Python tables, exact ref/consumer coverage matrix, ancestry/file-stat evidence, real run/test evidence, consumer migration/rollback ledger and per-branch endgame. Every factual claim needs immutable file:line or SHA; estimates and proposals must be labeled.

Report **COMPLETE** only when all mandatory evidence, selective port, official fixed-binary adoption, retained cache qualification, consumer regeneration/CI and approved archival/ref endgames are closed. Otherwise report **PARTIAL**, with each unknown, what settles it, its owner and cheapest next probe. Do not claim “no consumers anywhere,” a test pass, an upstream rejection, a fresh clone or a remote action without evidence.
