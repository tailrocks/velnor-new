# PR #12 owned-source review

**Decision:** narrow the work to official-tool adoption, real regression coverage, and safe retirement of the staged owned-source system. Do not merge the owned framework as a prerequisite for ordinary CI caching. Do not delete its branches on the strength of an incomplete negative search.

**Assurance level: PARTIAL.** This review used read-only GitHub API/source access. Container Git networking failed; fresh upstream clones, `git cat-file`, local builds, executable action bundles, and hosted qualification were not available. The required all-ref/all-consumer execution graph was not completed. This is a concrete implementation proposal with explicit evidence gates, **not a universal “zero consumers” or deletion-safety certificate**. No PR, push, branch modification, issue comment, or repository code change was performed.

## 1. Frozen identities and coverage

The reviewed PR head was `d94fe9cefa6a0b97f00d235191c2d3142d1af6ff`, branch `perf/cache-selection-qualification`. Current main was `5a946c33cf005777feab2bc91fa4aa8e01dd58f4`. Both were re-read and remained unchanged. The PR body still named `fb9963a45cce9259203c0d435766e6ae0ad76b89`; that body is not the source identity. [PR metadata](https://github.com/tailrocks/velnor-new/pull/12), [main](https://github.com/tailrocks/velnor-new/commit/5a946c33cf005777feab2bc91fa4aa8e01dd58f4), [review head](https://github.com/tailrocks/velnor-new/commit/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff).

The initial probe began at 2026-10-03 20:09:11 UTC / 2026-10-04 04:09:11 Singapore time. Fourteen branches were enumerated. During review, `fix/mbx-hosted-cache-space` advanced from `8480ddb5ee655dfc3f6c6c04360ad6c073980f0b` to `1ecd84d1f6320e9a0209968c2057952b9363c8a4`, and `tar-absolute` advanced from `3aa72b54fe0e822171df9e64e23a6baa6e7e25cd` to `5b309009599ce11173830045e237fef6900d6b42`. Their latest relevant source/patches were read, but neither complete branch was exhaustively searched. All six owned tips were unchanged. [Branch endpoint](https://api.github.com/repos/tailrocks/velnor-new/branches?per_page=100).

`scope.json` names 46 consumer repositories, plus the generator. All 46 were included in one indexed default-branch search for `owned-source`, which returned zero hits. That search does not cover every immutable SHA pin, non-default ref, indirect invocation, or current unindexed update. Six mandatory consumer heads were fetched, along with each `ci.yml` and `.velnor/config.toml`. The inventory records every head and the other 40 explicitly as **not head-fetched**. [Scope at reviewed SHA](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scope.json).

Use `owned-source-inventories.md` for the 15 custom commits, 14 branches, 46 consumers, and all Python decisions. `owned-source-inventory.json` contains machine-readable SHAs, parents, statistics, coverage limitations, and immutable source URLs. Statistics are GitHub REST aggregates, not claimed `git show` results.

### Evidence ranking

1. Exact immutable source and actual API metadata establish what exists and how code is wired.
2. Exact run/job/attempt/log evidence can establish actual hosted execution; no such successful owned-feature execution was demonstrated here.
3. Executable integration-test source establishes a real-code test consumer definition, not proof that its suite ran.
4. Published source-only assets establish publication, not binary qualification or production adoption.
5. PR descriptions, commit messages, and embedded receipts remain claims until independently replayed or matched to immutable execution evidence.
6. Negative indexed search results are leads, not exhaustive absence proofs.

This distinction matters: a test that launches a real binary is not a mock, but its mere existence is not proof of an active CI lane. Likewise a workflow definition declares an execution path, but its registration/dispatchability and actual hosted invocation still require evidence.

## 2. Strongest findings

### F1 — “Merged upstream” does not mean the mise fix is deployed

The no-config core was accepted as **jdx/mise#13926**, merged on **2026-10-03 at 15:37:37 UTC**, merge commit `dfe74a90b41603625ee6aabecb42f14a1f5eb0f6`. The accepted patch is **two files, +37/−1**, including an executable Bash e2e regression. It is much narrower than the staged mise commit's **nine files, +403/−1**. [Accepted PR](https://github.com/jdx/mise/pull/13926), [merge commit](https://github.com/jdx/mise/commit/dfe74a90b41603625ee6aabecb42f14a1f5eb0f6), [owned commit](https://github.com/tailrocks/velnor-new/commit/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96).

The latest release endpoint returned **v2026.10.1**, release `402539535`, published **2026-10-03 14:12:48 UTC**, before that merge. Its peeled source commit is `050ce5a20287a0aafd872b1191699a5fdafff5ac`. Its `src/config/miserc.rs` lacks the newly added no-config guards in `load_global_miserc_settings`, `get`, and `get_global_ignored_config_paths`. Thus neither the release name nor “upstream merged” is sufficient adoption evidence. [Release](https://github.com/jdx/mise/releases/tag/v2026.10.1), [released source](https://github.com/jdx/mise/blob/050ce5a20287a0aafd872b1191699a5fdafff5ac/src/config/miserc.rs).

The reviewed Velnor catalog still pins mise **2026.9.18**. Its process boundary always supplies `--no-config --no-env --no-hooks`; this is a real consumer of the no-config contract. `IsolatedCommand::mise_exec`, `mise_install`, `repo_task`, and `with_cwd` are concrete Velnor-side seams. [Catalog](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/crates/velnor-actions-mise/src/catalog.rs), [process boundary](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/crates/velnor-actions-mise/src/command.rs).

**Disposition:** preserve the no-config requirement; add narrowly scoped Rust real-binary regression coverage; adopt an official fixed release only after source and binary verification. Do not retain the whole owned mise branch, and do not submit the same upstream fix again. A temporary neutral-directory/direct-executable workaround would need exact global/project config-isolation tests; it has not been proven here. Therefore this review does **not** claim “no workaround exists.”

### F2 — Both action snapshots have a v1 parser; the MBX tip emits v2

Both `c3cbe8e56ccb4727624df45022357f49d2953075` and `62ec0713473dffeab46884b7c03906042794e696` contain the same `src/comparison.ts` blob, `1846dc6e6817a5c3459f78ddf72061139fb7ff38`. `comparisonExportResult` requires `version === 1`, Boolean `useful_delta`, matching `exported`, and a 64-hex semantic digest. [Older parser](https://github.com/tailrocks/velnor-new/blob/c3cbe8e56ccb4727624df45022357f49d2953075/src/comparison.ts), [newer parser](https://github.com/tailrocks/velnor-new/blob/62ec0713473dffeab46884b7c03906042794e696/src/comparison.ts).

At MBX `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81`, `cache_export_report.rs` emits **version 2** and **`emitted_bundle_useful_delta`**, plus workspace-usefulness/capture/budget information—not the v1 field contract. The action's `post()` passes comparison export output into that parser and catches errors with `core.setFailed`. [MBX producer](https://github.com/tailrocks/velnor-new/blob/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81/crates/mbx/src/cli/cache_export_report.rs), [action post source](https://github.com/tailrocks/velnor-new/blob/62ec0713473dffeab46884b7c03906042794e696/src/index.ts#L600-L820).

**Explicit ruling:** neither owned action snapshot is an approved comparison-mode pairing with MBX `ac6ceed…`. This is a source-contract incompatibility, not a claim that all ordinary MBX modes fail.

The action actually launches `dist/index.js`. Its newer blob `933dc3e6b4690d33594dd3b02135e6967d3ab451` is **1,327,451 bytes**; the available content and blob endpoints did not return its bytes. Source-to-bundle equivalence and the actual packaged runtime are therefore unverified. Do not turn source analysis into a claimed runtime test. [Entrypoint](https://github.com/tailrocks/velnor-new/blob/62ec0713473dffeab46884b7c03906042794e696/action.yml), [bundle](https://github.com/tailrocks/velnor-new/blob/62ec0713473dffeab46884b7c03906042794e696/dist/index.js).

**Disposition:** do not activate either owned pairing. Retain stock pairing(s) until a separate justified feature passes real producer→packaged-action tests. Do not paper over the mismatch by blindly accepting both report versions: the useful-delta semantics changed, not merely a field spelling.

### F3 — Foundation has a wired source graph, but activation and hosted qualification are unproved

The PR's dispatch-only `foundation-qualification.yml` calls:

```text
.github/workflows/foundation-qualification.yml:21
  uses: tailrocks/velnor-new/foundation-qualification@8758d976a1b25eb387f48aa04ea86f57739b84cf
  → foundation-qualification/action.yml (Node 24)
  → owned-cache/foundation-native-entrypoint.mjs
  → foundation-native-bootstrap.mjs
  → real isolated Python positive/qualification subprocesses
  → qualification outputs / uploaded control evidence
```

The bootstrap inventories actual native paths and launches Python with `-I -S -B`; its receipts explicitly remain non-authoritative. This is neither a prose-only reference nor a call to the PR's 20-script publication pipeline. [Workflow](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/.github/workflows/foundation-qualification.yml), [action](https://github.com/tailrocks/velnor-new/blob/8758d976a1b25eb387f48aa04ea86f57739b84cf/foundation-qualification/action.yml), [bootstrap](https://github.com/tailrocks/velnor-new/blob/8758d976a1b25eb387f48aa04ea86f57739b84cf/owned-cache/foundation-native-bootstrap.mjs).

The Foundation generator gate is specifically `VelnorRepositoryV1`; the consumer path emits no Foundation file. A separate compiled module binds the reviewed repo/ref/commit/tree/parent/source-manifest tuple and regenerates the pin. Therefore “just remove one YAML line” is insufficient. [Policy gate](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/crates/velnor-actions-orchestrator/src/foundation_qualification.rs), [compiled source tuple](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/crates/velnor-actions-orchestrator/src/foundation_qualification_source.rs), [renderer](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/crates/velnor-actions-workflow-renderer/src/foundation_qualification.rs).

The Actions endpoint filtered to PR #12's branch plus `workflow_dispatch` returned **zero runs**. The endpoint filtered to MBX `ac6ceed…` returned **zero runs**. Those are bounded negative observations, not proof that every historical helper or test was never executed. [Branch dispatch query](https://api.github.com/repos/tailrocks/velnor-new/actions/runs?branch=perf%2Fcache-selection-qualification&event=workflow_dispatch&per_page=1), [MBX head query](https://api.github.com/repos/tailrocks/velnor-new/actions/runs?head_sha=ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81&per_page=1).

**Disposition:** STAGED-HOLD of this source dependency until its owning decision is resolved. Recommended target: remove the Foundation-only workflow/renderer/compiled tuple/tests together, preserving historical evidence. Proposed accountable owner: PR author `donbeave`; proposed decision deadline **2026-10-11**. This is a proposed deadline, not an existing commitment. Retaining Foundation instead requires a named actual runner requirement and a separately scoped Velnor-tree design; nothing shown requires embedding it inside an `actions/cache` source fork.

### F4 — The candidate tool pipeline has no demonstrated admissible hosted route

`source_qualification_execution.py` accepts only `.github/workflows/owned-tools.yml` on these specific routes:

| Event / ref | Job |
|---|---|
| `workflow_dispatch`, `refs/heads/main` | `build-native` |
| `push`, `refs/heads/owned-tool-candidates` | `publish-artifacts` |

The main-ref fetch of that workflow returned not found, and `owned-tool-candidates` was absent from both returned 14-branch listings. A different arbitrary workflow or an unbound receipt is not equivalent under this validator. [Route validator](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/source_qualification_execution.py).

`qualify-owned-tool.py` further rejects MBX with **“MBX native qualification is unavailable”**. It admits mise and launches its candidate behavior helper. A Rust translation of this same orchestration would not fill the missing MBX qualification. [Qualifier](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/qualify-owned-tool.py).

There are actual immutable source publications: MBX-action release **402232689** is non-draft, immutable, has **five assets**, and explicitly says behavioral qualification and signed build provenance are absent. Its release metadata `target_commitish` is the Velnor commit `c57c700…`, not the foreign action snapshot. That field does not, by itself, prove the tag's peeled target or preserve the foreign commit's reachability. [Actual release](https://github.com/tailrocks/velnor-new/releases/tag/owned-source-mbx-action-c3cbe8e56ccb4727624df45022357f49d2953075), [release API](https://api.github.com/repos/tailrocks/velnor-new/releases/402232689).

**Disposition:** retire the unactivated publication/build/qualification pipeline rather than porting it wholesale. Preserve the published evidence. G0 must still classify manual and test callers before files are physically removed.

### F5 — The cache fork changes runtime and archive contracts

`owned-cache/tar.js` rejects Windows, rejects compression methods other than `gzip`, validates a Velnor SDK READY record, requires admitted Python/helper identities, and requires quarantine-based extraction. Its `extractCacheToWorkspace` rejects the old extraction path. These are substantive new requirements, not an ordinary cache version bump. [Archive adapter](https://github.com/tailrocks/velnor-new/blob/8758d976a1b25eb387f48aa04ea86f57739b84cf/owned-cache/tar.js).

The whole custom cache commit is **+120,449/−93,453**, with much of the churn in four compiled distribution bundles and substantial newly added native/SDK/Python source. Its compiled tar wrappers and its Foundation action are different execution paths; observing the latter does not prove a consumer of every cache-archive modification. [Custom cache commit](https://github.com/tailrocks/velnor-new/commit/8758d976a1b25eb387f48aa04ea86f57739b84cf).

**Disposition:** do not replace stock `actions/cache` pins globally with `8758d97…`. Conversely, replacing the Foundation subaction pin with stock `actions/cache@55cc…` is invalid: that is a different action/path, not a feature-equivalent fallback.

### F6 — A concrete Velnor-side cache workaround exists in open PR #26, but is not yet a proven replacement

Open **PR #26**, current inspected head `1ecd84d1f6320e9a0209968c2057952b9363c8a4`, uses official MBX/action versions and adds renderer-owned cache save behavior. `mbx_bundle.rs` forces the action to `ACTIONS_CACHE_MODE=read`, exports an MBX directory bundle outside the store under `${{ runner.temp }}/mbx-single-bundle`, and appends an explicit cache save. This is a specific implementation seam, not a hand-wave that “a wrapper might work.” [PR #26](https://github.com/tailrocks/velnor-new/pull/26), [module](https://github.com/tailrocks/velnor-new/blob/1ecd84d1f6320e9a0209968c2057952b9363c8a4/crates/velnor-actions-workflow-renderer/src/mbx_bundle.rs).

**Do not adopt it uncritically.** The reviewed stock action restores `cachePaths = [cacheArchive]`, where `cacheArchive` is below `mbx cache dir`, and imports that path. PR #26's separate save uses the external `runner.temp` path. GitHub's cache version incorporates the cached path as well as compression. Reusing the same primary key does not make these different path lists compatible. This is a source-level round-trip defect/risk that needs an exact packaged-action test, not a claimed observed production failure. [Stock action restore/import](https://github.com/jdx/mr-boxington-action/blob/1687e54eb349cadf61fa38b5813a77875489e8e6/src/index.ts#L355-L480), [cache-version contract](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).

The minimal Velnor-only design is to own **both** restore and save of the same external path, with explicit MBX import/export and a versioned key. Alternatively preserve the action's exact path/version contract. Do not keep action-owned restore and an incompatible wrapper-owned save. Validate corruption rejection, no PR writes, cancellation cleanup, unique per-job directories, and no deletion of a live shared store. The current `*mbx*` shell guard is not a sufficient general ownership proof for recursive deletion.

PR #26's body records prior test results, but its body itself names earlier heads; those are not proof for `1ecd84…`. This report did not run those tests. Preserve this independent stock-tool repair work; do not fold it into the owned forks or declare it complete.

## 3. Snapshot genealogy and breaking surfaces

The upstream mappings below were independently read through upstream tag/Git-object APIs and source parents. **The requested fresh-clone `cat-file` proof remains open.**

| Snapshot tip | True upstream | Original fork point | Later upstream merged into snapshot | Custom commits |
|---|---|---|---|---:|
| `dbbf5b0…` mise | `jdx/mise` | `bc11f90c74eba23bf0d7350efb540e62fb7d9ffd`, v2026.10.0 | none identified | 1 |
| `ac6ceed…` MBX | `jdx/mr-boxington` | `201b9df3d18e8e96831bee631035f6b7c7ae20e0`, v1.21.0 | `a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313`, v1.21.1 | 6, including merge |
| `62ec071…` action | `jdx/mr-boxington-action` | `9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3`, v1.5.0 | `1687e54eb349cadf61fa38b5813a77875489e8e6`, v1.6.0 | 5, including merge |
| `c3cbe8e…` action | same | same | same | first 4 of those 5 |
| `583dddc…` semver | `obi1kenobi/cargo-semver-checks` | `4297e8b5f6306531375ba2ba332171e5792b4c38`, v0.50.0 | none identified | 2 |
| `8758d97…` cache | `actions/cache` | `55cc8345863c7cc4c66a329aec7e433d2d1c52a9`, v6.1.0 | none identified | 1 |

Proof endpoints: [mise annotated tag](https://api.github.com/repos/jdx/mise/git/tags/f2a9db076328ed4c6f93fef665c14bf733b6b7f5); [MBX v1.21.0 tag](https://api.github.com/repos/jdx/mr-boxington/git/tags/5d49803a7e200df12b188329aa796842472bfaa3); [MBX v1.21.1 tag](https://api.github.com/repos/jdx/mr-boxington/git/tags/fb284e2c05308e32b3a35347ebf14321229f7766); [action tags](https://api.github.com/repos/jdx/mr-boxington-action/git/matching-refs/tags/v1.); [semver tag](https://api.github.com/repos/obi1kenobi/cargo-semver-checks/git/ref/tags/v0.50.0); [cache tags](https://api.github.com/repos/actions/cache/git/matching-refs/tags/v6).

### MISE-1: separate four concerns

The owned commit adds a 293-line `owned_cargo_wrapper.rs`, 56-line regression file, and smaller Cargo/config/main/shim/version edits. It binds `MISE_OWNED_CARGO_WRAPPER` to a SHA-256, requires the three isolation environment flags, verifies file/path/mode/digest conditions, dispatches Cargo before normal configuration, and strips the shim directory to avoid recursion. The source explicitly limits its concurrent-file-mutation assumptions; do not equate a hash check with an immutable executable handle. [Wrapper source](https://github.com/tailrocks/velnor-new/blob/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96/src/owned_cargo_wrapper.rs).

The no-config fix is generic and already accepted. Caller-specific dispatch, shim generation and owned banner are separate requirements and have no demonstrated indispensable Velnor consumer here. A direct verified executable invocation at the existing Velnor process boundary is a concrete alternative; its inherited environment and nested Cargo execution require tests. Dropping the foreign wrapper does not mean removing stock `compile_driver = "mbx"` support.

### MBX-1 through MBX-6: not one cache tweak

The custom chain adds snapshot usefulness/retention, root-pair restore, native ownership and compiler-event measurement, pinned Bats source closure, and finally lineage/durable execution state. IPC 12 then IPC 13 and report version 2 are declared by the commits; report v2 is directly visible in the producer. The tip's commit claims extensive local tests but also states that native/workspace/Bats/hosted publication qualification remains incomplete. These claims were not rerun. [Full custom chain and per-commit stat links](owned-source-inventories.md).

The owned version feature changes the banner to `1.21.1-owned-cache-transport`; without that feature the version constant remains ordinary `1.21.1`. Banner identity alone must not be used to infer the entire compiled behavior or protocol. [Version source](https://github.com/tailrocks/velnor-new/blob/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81/crates/mbx/src/version.rs).

The ordinary disk-pressure problem has a stock-version/renderer avenue in PR #26. That does **not** prove stock MBX reproduces every owned lineage or usefulness feature. For those other features the missing premise is a retained consumer requirement; do not assert either equivalence or inevitable upstream rejection.

### ACT-1 through ACT-5: separable action policy and protocol

Executable selection, binary hash verification, cache-save policy and baseline/group binding are distinct changes. Save-policy restrictions can live in Velnor-rendered explicit steps. Binary verification can live beside the existing Rust process boundary. Both are concrete alternatives to carrying a foreign action tree. Semantic comparison remains coupled to the MBX producer contract; it is not safely replaced by renaming a JSON field. The two private action branches share history and should not be treated as two independent supported products. [New action definition](https://github.com/tailrocks/velnor-new/blob/62ec0713473dffeab46884b7c03906042794e696/action.yml).

### SEM-1 / SEM-2: real implementation and real test driver, not a necessary deployed feature

The new 82-line binary exposes `velnor-plan` and `velnor-compare` and reports `cargo-semver-checks-owned 0.50.0+velnor.owned`. The 248-line comparison implementation uses supplied docs/metadata and the upstream lint engine; it is more than an outer shell wrapper. Additional plan/report modules and adapters contribute to the **+867/−3** custom feature. The subsequent **+2/−0** commit only restricts workflow-lint pushes. [CLI](https://github.com/tailrocks/velnor-new/blob/583dddce84706786fc54c41a2c768c28a09c65fd/src/bin/cargo-semver-checks-owned.rs), [comparison](https://github.com/tailrocks/velnor-new/blob/583dddce84706786fc54c41a2c768c28a09c65fd/src/supplied_compare.rs).

`tests/owned_supplied_semantics.py` is a **278-line real-binary driver**: it runs Cargo rustdoc/metadata and the owned executable against synthetic cases. It is not accurately described as a mocked-only test. This review did not prove a workflow that invokes it or run it. [Test driver](https://github.com/tailrocks/velnor-new/blob/583dddce84706786fc54c41a2c768c28a09c65fd/tests/owned_supplied_semantics.py).

A simple metadata wrapper does not necessarily reproduce these lint semantics. Without a retained supplied-document consumer, there is no reason to port or publish that functionality. If a consumer is found, upstream public-API feasibility and maintainer scope must be investigated before a verdict; an invented claim of “unupstreamable” is not acceptable.

### CACHE-1: split upstream archive transport from Velnor foundation ownership

The root archive adapter changes compression, platforms, admission and extraction semantics. The Foundation subaction implements Velnor-specific host/SDK qualification. They have different consumers and retirement conditions. There is no demonstrated reason the latter must live under a foreign upstream source tree; if independently justified it belongs in a small Velnor implementation, not a permanent cache fork. Exact complete per-file custom deltas still require G0's local Git inventory, including separating bundles, vendored source, authored source and deleted upstream CI.

## 4. Upstream-maintainer conclusion

**Zero new KEEP-AND-UPSTREAM proposals survive this review's necessity bar.** That is not a proof that every other change is unupstreamable. It means no new proposal here has the complete chain: indispensable live consumer → no adequate Velnor-side alternative → minimal maintainer-shaped patch.

For mise the strongest maintainer evidence is an actual accepted patch, not speculation. Its e2e test style exercises the CLI with malformed config and validates both flag/environment forms. Preserve that minimal shape; cut wrapper identity, native ownership, publication receipts, custom banners and shim policy. [Accepted diff](https://github.com/jdx/mise/pull/13926/files), [upstream contribution guide at released source](https://github.com/jdx/mise/blob/050ce5a20287a0aafd872b1191699a5fdafff5ac/CONTRIBUTING.md).

No prospective “maintainer will accept” assurance is claimed, and no made-up rejection precedent is used. A newly discovered essential MBX/semver feature would reopen Phase 3 with its actual upstream testing/platform/MSRV/contribution requirements. It would not authorize shipping this entire staged framework.

## 5. Python conclusion and the smallest port

The enumerated review surface is **20 principal scripts, 2 support fixtures and 14 test files**, rather than assuming the prompt's approximate 17. The proposed principal-file end state is **19 DELETE and one selective PORT-TO-RUST**, with no justified KEEP-PYTHON exception. This is a proposed cleanup decision, not a claim that every test or manual helper is uncalled. The full per-file table states which bodies were only partially inspected. [PR changed-file inventory](https://github.com/tailrocks/velnor-new/pull/12/files), [detailed table](owned-source-inventories.md).

The one retained function is **testing the stock mise no-config contract already used by the Rust process boundary**. Extract only those cases from `owned_mise_qualification.py` into `velnor-actions-mise` integration tests. Do not port the owned wrapper, fake MBX fixture, receipts, release uploader, source-capsule engine, or candidate orchestrator. This is Phase 0, before the old tests are removed. Rust can run on the existing CI environments, and the tree already owns subprocess construction; no environment where Rust cannot execute was demonstrated. [Existing Rust boundary](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/crates/velnor-actions-mise/src/command.rs), [selected Python source](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/owned_mise_qualification.py).

Do not widen this to “remove Python from the repository.” Existing `check-freshness.sh` embeds Python; existing Rust freshness modules render/emit the check and are not proof that the HTTP/policy logic already exists in Rust. The moving `tar-absolute` branch adds Python to the runner image for a separate ChainArgos script requirement. That dependency is outside the owned-source cleanup. [Freshness script](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/check-freshness.sh), [runner-image change](https://github.com/tailrocks/velnor-new/commit/5b309009599ce11173830045e237fef6900d6b42).

## 6. Verdict on the five prior claims

| Prior claim | Re-derived result |
|---|---|
| Zero executing consumers on any ref | **Not proved.** An executable Foundation source graph and real-binary semver test source exist; activation is not established. Hosted activation and the full all-ref graph remain unproved. |
| Only hard dependency is one Foundation workflow pin | **Too narrow for deletion.** The same execution chain is also encoded in Rust source tuples, rendering and tests. These are not independent production consumers, but they can regenerate or reject removal of the pin. |
| Mise no-config was the sole upstreamable item and is already done | **Accepted upstream, not fully adopted.** The latest release inspected lacks the fix; blanket “sole upstreamable” was not established. |
| All MBX/semver/cache changes are DROP | **Reasonable provisional target, not unconditional deletion authority.** Keep the evidence and close caller coverage; preserve independent stock-tool repairs. |
| Action and binary tips are incompatible | **Confirmed in source for comparison mode, for both action snapshots.** Packaged bundle execution remains untested. |

## 7. Remaining questions and cheapest probes

| Unknown | Evidence that settles it | Cheapest next probe |
|---|---|---|
| Exact original/upstream ancestry and all custom file stats | Fresh upstream objects, peeled tags, per-parent diffs, complete custom rev-list | G0 bare clones; `cat-file -e`, `rev-parse tag^{commit}`, `rev-list`, `diff --numstat` |
| Every active consumer and indirect test caller | Ref/SHA-bound source call graph plus exact run logs where available | `git grep` every fetched Velnor ref and all 46 consumer workflow/composite/script closures; then expand only positive paths |
| Action dist equals reviewed source | Reproducible bundle rebuild and real producer→packaged-action run | `npm ci`, upstream build/test command, clean dist diff, real MBX export |
| A fixed official mise release is available | Current release tag contains fix and distributed binary passes regression | Query releases again; test source ancestry and actual asset before changing catalog |
| Python tests/manual tools are still useful to retained workflows | Complete imports/invocations and fixture dependency map | Search exact script names/import stems and inspect the positive test bodies; do not assume all mocks |
| PR #26 external bundle can restore | Same path/version/compression/key contract and two fresh hosted jobs with real imports/reuse | Test current generated YAML, then own both external restore/save if path mismatch remains |
| Snapshot refs can be retired without losing reproducibility | Verified Git bundle restore, exact tag/ref reachability, no remaining active string/ref binding | `bundle create/verify`, isolated restore, tag peeling and positive-ref scan |
| Four sample configs' effective defaults under their own pinned generator versions | Version-specific config parse/plan output | Run each pinned generator in scratch; do not infer all six from two explicit `consumer-v1` entries |

**Completion rule:** an implementation agent may complete the source cleanup only after G0 and the Phase 0 regression port. It may report retirement COMPLETE only after exact-head CI, consumer regeneration, release/binary adoption and safe archival/ref closure. Missing any mandatory evidence remains PARTIAL. No negative search result in this report authorizes remote branch deletion by itself.
