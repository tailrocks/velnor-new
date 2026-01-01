# Owned-source retirement and official-tool adoption specification

Version: 1.0 • Review date: 2026-10-04 Singapore • Repository: `tailrocks/velnor-new`

## 0. Authority, evidence and target state

This specification is a **proposal produced by research**, not authorization to mutate repositories. In the current research mode: no pushes, PRs, issue comments, branch changes, workflow dispatches or release changes. Work only in newly created scratch directories. Implementation and remote retirement require an explicit subsequent implementation authorization. No existing keeper checkout may be modified.

The target is a smaller Velnor implementation that uses official tools and stock cache actions, preserves the no-config isolation requirement, and retires unneeded owned-source infrastructure. It is **not** a wholesale rejection of every change in PR #12, a rewrite of all Python in the repository, or a permanent private fork.

### Immutable review anchors

| Item | SHA / identity |
|---|---|
| Current main examined | `5a946c33cf005777feab2bc91fa4aa8e01dd58f4` |
| PR #12 examined | `d94fe9cefa6a0b97f00d235191c2d3142d1af6ff` on `perf/cache-selection-qualification` |
| Owned mise | `dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` |
| Owned MBX | `ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` |
| Owned action, older | `c3cbe8e56ccb4727624df45022357f49d2953075` |
| Owned action, newer | `62ec0713473dffeab46884b7c03906042794e696` |
| Owned semver | `583dddce84706786fc54c41a2c768c28a09c65fd` |
| Owned cache/Foundation | `8758d976a1b25eb387f48aa04ea86f57739b84cf` |
| Accepted upstream mise fix | PR `jdx/mise#13926`; merge `dfe74a90b41603625ee6aabecb42f14a1f5eb0f6` |
| Stock-cache repair under separate review | PR #26, `1ecd84d1f6320e9a0209968c2057952b9363c8a4` |
| Separate runner dependency repair to preserve | `tar-absolute`, `5b309009599ce11173830045e237fef6900d6b42` |

Primary anchors: [PR #12](https://github.com/tailrocks/velnor-new/pull/12), [upstream fix](https://github.com/jdx/mise/pull/13926), [PR #26](https://github.com/tailrocks/velnor-new/pull/26). Every moving identity must be refreshed before implementation; do not use the PR body's stale claimed head.

**Known evidence limitations:** no fresh `git cat-file` proof, no local builds/tests, incomplete all-ref and 46-consumer content coverage, and unread action bundle bytes. These are mandatory Gate G0 work, not assumed passes. No final branch deletion is authorized by this specification alone.

## 1. Scope: what lives, what dies, what remains held

### Live item L1 — Stock mise no-config regression contract, in Rust

**Problem.** The existing `velnor-actions-mise` process boundary emits `--no-config --no-env --no-hooks`, but the official release inspected during research, v2026.10.1 at `050ce5a20287a0aafd872b1191699a5fdafff5ac`, lacks the accepted miserc guards. A cleanup that merely says “upstream merged” would lose the distinction between source acceptance and deployed behavior. [Process boundary](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/crates/velnor-actions-mise/src/command.rs), [released miserc source](https://github.com/jdx/mise/blob/050ce5a20287a0aafd872b1191699a5fdafff5ac/src/config/miserc.rs).

**Minimal design.** Port only the no-config/miserc behavior cases from `scripts/owned_mise_qualification.py` into a proposed `crates/velnor-actions-mise/tests/impl_miserc_isolation.rs`, registered in the existing integration-test harness. Reuse the existing isolated process boundary and fixture conventions. Do not introduce another unrestricted process constructor or weaken environment-policy validation to make tests pass. The test executable must be an actual selected official mise binary, not a script that echoes expected answers.

**Acceptance tests.** Cover at least the eight combinations of flag/environment no-config selection × `--version`/`exec` operation × repository-root/nested working directory. Fixtures must exercise malformed project, global and system miserc layers. Add a no-no-config control showing that malformed config is genuinely reached, and a pre-fix release control that demonstrates at least one regression failure. A “missing executable” skip is not a qualification pass. Tests must record binary identity and source/release identity without depending on the owned publisher or candidate receipts. Run the applicable cases on each retained supported CI platform; unsupported matrix members remain explicit gaps.

**Not included.** Owned Cargo wrapper dispatch, custom version banners, shim generation, fake MBX fixtures, source-capsule signatures, SDK admission, native MBX qualification, release publication, or a generic Python-to-Rust port.

**Slice / estimated size.** One independent Velnor PR or logical commit: approximately 120–220 authored Rust test/helper lines plus minimal harness registration. Estimate is a scope budget, not a measured existing diff. Title shape: `test(mise): verify no-config isolation with real binaries`.

**Rollout.** This is Phase 0 and must land or be present in the candidate before deleting the Python cases. A explicitly named qualification target may temporarily document a known failing old release; it must not silently pass or cause ordinary unrelated unit tests to be mislabeled qualified.

**Rollback.** Revert the new test integration if defective, preserving its fixture evidence. Do not restore the entire owned pipeline as a testing dependency.

### Live item L2 — Adopt the accepted fix through an official distribution

**Design.** Find the first available official release whose source includes `dfe74a90…` or an independently verified equivalent backport. Verify the tag/commit and released binary with L1. Only then update `MISE_VERSION` and the actual associated bootstrap/action/version/checksum/freshness authorities; regenerate affected workflow fixtures. Do not invent a future version number or change only prose. [Version authority at review](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/crates/velnor-actions-mise/src/catalog.rs).

**No new upstream PR.** The accepted core is already upstream as a two-file +37/−1 change. No private mise branch is the end state. Retire the larger owned wrapper/shim/banner changes once their caller search is closed.

**Unavailable-release handling.** If no fixed official binary exists, L2 remains PARTIAL. Continue independent source cleanup and evidence work, but do not claim the isolation fix deployed. A bounded Velnor-side workaround may be qualified at `IsolatedCommand::mise_exec` / `mise_install` / `with_cwd` / `repo_task`: run discovery from a trusted neutral directory, isolate all global/system config roots, and invoke the actual build command in its intended repository directory only after tool selection. This is an exact seam, not a proven workaround. It must pass the same malformed-config tests and preserve environment/tool-version policy. Do not ship it merely on the assumption that changing CWD is sufficient.

**Slice / estimate.** A separate small adoption change: approximately 20–80 authored lines excluding generated workflow churn and upstream release metadata. Existing release/bootstrap infrastructure must be reused; no owned-tool publisher is added.

**Rollout and rollback.** Publish a new immutable generator artifact through existing approved release machinery, then regenerate consumers from that exact artifact. Do not overwrite `v0.1.0` or another used release. Reverting to a known-broken mise version must not silently restore a claimed security/isolation guarantee: fail the affected qualification or use an independently qualified workaround.

### Live item L3 — Remove the unneeded owned-source apparatus, retaining passive evidence

**Design.** Remove the 19 unneeded principal scripts, the non-retained portions of the selectively ported file, and support/test files only after their full caller/import graph is closed. Preserve historical outputs as passive evidence with their original status, exact source identities and limitations. Do not retain executable publishers merely to maintain historical JSON.

Remove Foundation's generated workflow, dedicated renderer/template, compiled reviewed-source tuple, dedicated registration/CLI plumbing and associated tests as one coherent feature-removal change. Retain generic native/process/renderer capabilities used elsewhere. A test proving this dead feature stays absent from both relevant generated-policy surfaces is appropriate; a blanket test forbidding all future native features is not.

**Hold condition.** Foundation is STAGED-HOLD because PR #12 contains a real executable source chain whose activation is unproved. Proposed owner `donbeave`, proposed resolution deadline **2026-10-11**. Resolve by either accepting its removal or providing a named retained runtime requirement, actual invocation/run evidence and a separately minimal Velnor-tree design. The deadline is a proposed decision date, not permission for automatic deletion. A newly demonstrated consumer suspends only the affected deletion slice.

**Slices / estimates.** (A) Foundation detachment: at most roughly 50 new authored lines plus deletion of the dedicated feature closure and regenerated outputs. (B) Python/source-publication retirement: preferably no new runtime code; delete by exact inventory/import ownership rather than `rm scripts/*.py`. (C) passive evidence/retirement manifest: approximately 100–200 documentation lines plus structured inventory. Keep these separately reviewable so a consumer discovery can revert one slice without restoring everything.

**Do not delete:** unrelated PR #12 correctness improvements, stock MBX support, existing freshness/OCI/release machinery, historical immutable release assets, or Python required to execute consumer workloads. The independent runner-image change at `5b309009…` is outside this scope.

**Rollback.** Until G5, retain all snapshot refs. After G5, rollback must be possible from a verified Git bundle and the cleanup commits. Source-only tar archives alone are not sufficient Git-history recovery proof.

### Live item L4 — Coordinate, do not duplicate, the official MBX cache repair

**Problem/seam.** PR #26 at `1ecd84d1…` already implements an official MBX/action renderer route in `mbx_bundle.rs`. The source inspected saves an external bundle but leaves the stock action restoring its internal cache path. Same key does not imply same cache version or restored location. This is a concrete Velnor-side alternative to a private fork, but not a qualified replacement. [Wrapper](https://github.com/tailrocks/velnor-new/blob/1ecd84d1f6320e9a0209968c2057952b9363c8a4/crates/velnor-actions-workflow-renderer/src/mbx_bundle.rs), [stock restore contract](https://github.com/jdx/mr-boxington-action/blob/1687e54eb349cadf61fa38b5813a77875489e8e6/src/index.ts#L355-L480).

**Minimal coherent design.** Use official MBX v1.21.1 (`a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313`) and official action v1.6.0 (`1687e54eb349cadf61fa38b5813a77875489e8e6`) where the action is retained. Either preserve that action's complete restore/save contract or let the Velnor renderer own BOTH stock `actions/cache/restore` and `actions/cache/save` of the identical external path, with explicit official MBX import/export. Use stock cache `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` unless separately verified release policy calls for a newer official pin. No owned report v1/v2 adapter is needed for this stock route.

Path/key/version/format semantics must be specified together. Keep cache-path identity stable across writer/reader runs; put qualification run/attempt/SHA nonces in keys, not into a path whose hash would make every subsequent restore a different cache version. Job isolation must prevent one job deleting another job's store. Do not use a substring such as `*mbx*` as deletion authorization. Prefer no recursive deletion until canonical containment, exclusive ownership, stopped consumers and rollback are proved.

**Tests.** Parse actual generated YAML; assert exact official pins, identical restore/save path sets, matching compression/format and explicit MBX import. Test no writes on PR/fork/dispatch paths unless separately authorized, no save after empty/failed export, corrupt input rejection, cancellation, shared-store refusal and legacy cache isolation. Perform two fresh hosted jobs with real cache service, prefix restore and observable imported-object/compiler reuse. A mock cache, same-process local round trip or `cache-hit` output alone does not qualify it. Measure the affected ChainArgos workload and peak bytes/inodes separately from the small probe crate.

**Coordination / estimate.** Reuse PR #26's owner's work; do not open a competing implementation. A necessary correction should be a separately reviewable slice, roughly 100–250 Rust/fixture lines excluding generated churn. This scope budget is not approval of the current patch. Existing active head test claims must be refreshed; earlier-head tests are not inherited.

**Rollback.** Revert the specific generator cache change and select a fresh isolated cache namespace. Preserve working stock compilation. Do not fall back to the incompatible owned action/binary pairing.

## 2. Disposition of foreign changes

| Change group | End state | Important qualification |
|---|---|---|
| Mise `dbbf5b0…`: no-config core | Adopt accepted official upstream implementation; Rust consumer regression test | Not the entire custom commit; no second upstream PR |
| Mise `dbbf5b0…`: wrapper/shim/banner remainder | Provisional DROP after G0 | Direct Velnor process boundary is a concrete alternative; nested execution must be tested |
| MBX `1ca12e…`, `ee250a…`, `e07c07…`, `29865b…`, `ac6ceed…` | Provisional DROP after G0 | No demonstrated indispensable owned-feature consumer; do not claim stock cache policy reproduces all lineage/usefulness semantics |
| MBX private upstream merge `2f324e…` | DROP as distribution mechanism | Preserve official v1.21.1 adoption independently |
| Action custom `f053f2…`, `06f353…`, `c3cbe8…`, `62ec07…` | Provisional DROP after G0 | Save policy and executable validation can be Velnor-owned; no approved owned comparison pairing |
| Action private upstream merge `198f0d…` | DROP as distribution mechanism | Official v1.6.0 remains available |
| Semver `d73a5d…`, `583ddd…` | Provisional DROP after G0 | Real-binary test driver is preserved as evidence; metadata wrapper equivalence is not assumed |
| Cache `8758d97…` archive/SDK framework | Provisional DROP after G0 | Not a drop-in stock-cache replacement |
| Cache `8758d97…` Foundation subaction and Velnor pin | STAGED-HOLD → recommended removal through L3 | Actual source call chain and compiled regeneration dependencies must be detached first |

Full SHAs, exact dates, parent lists and per-commit aggregate stats are in `owned-source-inventory.json`. There are 15 unique custom commits; the older action branch is a prefix of the newer one's five-commit custom chain. No permanent KEEP-OWNED foreign fork and no new KEEP-AND-UPSTREAM proposal is authorized by this specification. Discovery of a real indispensable consumer reopens only its necessity/maintainer analysis.

## 3. Branch endgame

Every row below is **KEEP UNTIL**, not “delete now.” Proposed accountable owner is `donbeave` or an explicitly delegated repository maintainer. Proposed decision checkpoint is 2026-10-11. The resolving ref must be recorded as the **actual full merged cleanup/adoption SHA**, never a fabricated future commit or a moving `main` label.

| Exact branch suffix under `refs/heads/owned-source/` | Retirement event | What could break if removed early | Exact currently binding/ref evidence |
|---|---|---|---|
| `mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96` | G0 coverage + selected regression extraction + source-publication reference retirement + verified archive | Source staging/publication or undiscovered manual qualification; no proven deployed binary caller | PR #12 `d94fe9…` source catalog/scripts; staged tuple, plus source-only publication records |
| `mbx/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81` | G0 + explicit incompatible-pair prohibition + archive + affected receipts retired | Staging/tests/source reconstruction; no proven active binary delivery | PR #12 source/behavior files and exact MBX tip |
| `mbx-action/62ec0713473dffeab46884b7c03906042794e696` | G0 + all action/source pins detached + archive | Source-based tests or unsearched ref consumers | PR #12 publication/source catalog; comparison parser at exact tip |
| `mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075` | G0 + source-ref readers retired + archive of release/history bindings | Older source-publication readers; it is an ancestor of newer action tip, but ref-name contracts may still matter | Immutable source release 402232689; exact older branch and ACT-5 parent |
| `semver-checker/583dddce84706786fc54c41a2c768c28a09c65fd` | G0 + capsule/test consumers retired or transferred + archive | Supplied-document real-binary test/capsule workflows if found | PR #12 `source_semver_capsule.py`; snapshot `tests/owned_supplied_semantics.py` |
| `cache-action/8758d976a1b25eb387f48aa04ea86f57739b84cf` | G0 + L3 Foundation detachment merged + no surviving runtime pins + archive | Foundation action resolution/reproducibility; generator reintroduces pin unless compiled tuple/template removed | PR #12 workflow line 21 and `foundation_qualification_source.rs` at `d94fe9…` |

Deleting a branch is not identical to immediately deleting its commit object; do not assert instant SHA breakage. Conversely, undocumented GitHub object retention is not a durability guarantee. Preserve immutable release assets and prove recoverability before any separately authorized remote deletion. Do not assume a release's `target_commitish` is its peeled tag target or that a Velnor-targeted tag preserves a foreign snapshot commit.

## 4. Gates and command plan

### G0 — Evidence closure, before destructive editing

Use new scratch bare clones. Record start/end timestamps, all heads, every open PR head, and any changes during the sweep. A valid skeleton is:

```bash
set -euo pipefail
scratch=$(mktemp -d)
git clone --mirror https://github.com/tailrocks/velnor-new.git "$scratch/velnor.git"
git -C "$scratch/velnor.git" fetch origin \
  refs/pull/12/head:refs/review/pr12
git -C "$scratch/velnor.git" rev-parse refs/heads/main refs/review/pr12
git -C "$scratch/velnor.git" for-each-ref \
  --format='%(refname) %(objectname)' refs/heads refs/review > "$scratch/refs.txt"
# No push, branch checkout, workflow dispatch or remote write belongs in G0.
```

For each of the five true upstream repositories, first create a fresh bare clone and prove its tag before importing any foreign object. Example:

```bash
git clone --bare --filter=blob:none https://github.com/jdx/mise.git "$scratch/mise.git"
git -C "$scratch/mise.git" cat-file -e bc11f90c74eba23bf0d7350efb540e62fb7d9ffd^{commit}
test "$(git -C "$scratch/mise.git" rev-parse refs/tags/v2026.10.0^{commit})" = \
  bc11f90c74eba23bf0d7350efb540e62fb7d9ffd
git -C "$scratch/mise.git" fetch https://github.com/tailrocks/velnor-new.git \
  refs/heads/owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96:refs/review/owned
# refs/review/owned is local evidence, not an upstream branch modification.
git -C "$scratch/mise.git" rev-list --reverse --topo-order \
  refs/review/owned --not --branches --tags
```

Repeat with the original and later merged upstream pairs: MBX `201b9df…`/v1.21.0 and `a0a44c…`/v1.21.1; action `9df1d4…`/v1.5.0 and `1687e5…`/v1.6.0; semver `4297e8…`/v0.50.0; cache `55cc83…`/v6.1.0. Record complete per-file `git diff parent commit --numstat` and `--stat`, with first-parent merge imports clearly separated from authored changes. Check tags and object types with `cat-file`; never infer the fork point from the source catalog alone.

Search every Velnor ref and all 46 consumers from the pinned `scope.json`. Include full six SHAs, branch names, `MISE_OWNED_CARGO_WRAPPER`, `owned-cache-transport`, `comparison-state`, `expected-binary-sha256`, `velnor-plan`, `velnor-compare`, `foundation-qualification`, SDK/quarantine variables, and every Python filename/import stem. Use `git grep -n -I` on immutable refs and follow positives through composite actions, reusable workflows, build scripts, Rust tests and generated code. Bound concurrent repository probes; do not run unbounded filesystem searches or execute fetched scripts merely to discover references. Record every repo/ref checked, errors, hit counts and unresolved edges. Missing repositories are not zero-hit repositories.

For candidate real consumers, bind run ID, attempt, head/merge SHA, job and exact command; inspect logs rather than trusting a status badge. Classify prose, fixtures/mocks, real-binary test source, declared staged workflow graphs and observed hosted execution separately. No consumer-found lane may be suppressed because it conflicts with this review's recommended DROP.

### Phase 0 / G1 — Rust port and official binary qualification

Implement L1 before deleting its Python source. Verify a known affected stock release actually fails the intended control, and the selected fixed binary passes. Missing executables, inaccessible platforms and unexecuted test selections remain failures/gaps, not skips counted as success. Do not qualify MBX by running a fake shell executable.

Use the repository's pinned toolchain and current approved commands, including the equivalents of:

```bash
mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo fmt --all -- --check
mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo clippy --workspace --all-targets --locked -- -D warnings
mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo test -p velnor-actions-mise --locked
```

First re-read current repository instructions and actual test harness registration; these commands are proposed gates, not a claim they ran in this review. Run targeted tests, then the repository's full required Nextest/format/lint/dependency/policy/parity gates. Do not disable freshness or security tests to obtain green output.

### G2 — Source cleanup and generated-output closure

Apply L3 by exact ownership. No active `.github/workflows`, composite action, renderer tuple, build script or runtime dependency may retain an owned snapshot pin. Historical evidence references may remain under explicitly passive documentation. Regenerate Velnor's repository-policy output and consumer-policy fixtures with the same candidate generator; generate twice and require byte-identical output and no second-run diff. Verify the Foundation file is removed by generator output, not simply hand-deleted and later regenerated.

### G3 — Cache workaround and integration

Coordinate with current PR #26. Verify the exact current stock versions and packaged action; do not use claims from an older head. Test one coherent restore/import/build/export/save graph and a separate hosted reader. Record key/version/path/format identity, source/binary identity, imported object counts, reused compilation and byte/inode peaks. Report cache-service absence/cancellation as incomplete qualification. The main source cleanup need not import a large cache redesign merely to close G2; keep the independent repair slice separate.

### G4 — Consumer regeneration and rollout

Mandatory first cohort: `jackin-project/jackin`, `tailrocks/velnor`, `tailrocks/parallax`, `ChainArgos/java-monorepo`, `tailrocks/github-terraform`, `tailrocks/velnor-actions-fixture`. Then cover all 46 from the recorded scope. For each record the repository SHA, effective policy, generator artifact SHA/digest, before/after pins, generated-file diff, validation result and exact CI run/attempt. Omitted config policy must be evaluated with that consumer's actual generator version, not assumed from another head.

No downstream push occurs in research mode. In authorized implementation mode use the existing release/consumer migration mechanism, preserve unrelated generated workflows and use small per-repo changes. Both PR and merged-main CI must be green at the actual new commits. A cached old success is not a pass for the new pin.

### G5 — Archive, retire, recheck

Create a Git bundle preserving each snapshot's exact source history/ref in scratch; verify it and restore it into a separate bare repository, then `cat-file` the expected commits and trees. Peel relevant remote tags and inventory release asset identities. Record a passive retirement manifest with actual resolving merge SHA, owner, archived artifact digest and final zero-live-reference evidence. Re-fetch immediately before any separately authorized branch deletion. Never delete `main`, unrelated branches, existing generator releases, or immutable evidence assets.

## 5. COMPLETE versus PARTIAL

**Source cleanup COMPLETE:** G0 caller closure, L1 selective port and its real tests, and G2 source/generated-output retirement all pass on the exact candidate. If no fixed official binary is available, explicitly report adoption PARTIAL rather than stretching this label.

**Overall COMPLETE:** G0–G5 pass; L2's official fixed binary is deployed through the recorded consumer cohort; the retained cache path is independently qualified; actual PR and merged-main CI pass; all six snapshot branches have approved, auditable end states and recoverable archives.

**PARTIAL:** any required repository/ref was inaccessible or unsearched; bundle bytes were unverified where still used; a mandatory test/platform was skipped; release adoption is pending; a live consumer lacks a replacement; exact-head hosted evidence is missing; or branch-retirement authorization/recoverability is absent. List the failing gate, owner, required evidence and cheapest next probe. Never substitute “probably unused” for G0.

## 6. Python verdict table

The following target-state table is deliberately narrower than a repository-wide language migration. Principal source files were read fully or in bounded ranges; the support-test bodies and complete caller closure remain G0 work. DELETE is a proposed disposition, not authorization to remove an uninspected surviving caller. No KEEP-PYTHON exception was demonstrated.

| Principal file | Verdict | Replacement / reason |
|---|---|---|
| [`scripts/analyze-ci-performance.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/analyze-ci-performance.py) | **DELETE** | Freeze historical JSON/Markdown results first. No retained automation caller proved; do not translate one-off analysis merely because Rust is preferred. |
| [`scripts/collect-ci-performance.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/collect-ci-performance.py) | **DELETE** | Use read-only gh evidence export for this cleanup; a reusable runtime collector is not established. Reopen only with a named retained caller. |
| [`scripts/bootstrap-owned-tool-builder.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/bootstrap-owned-tool-builder.py) | **DELETE** | The private tool publication pipeline is not retained. Existing CI already provisions Rust; no new bootstrap service is required. |
| [`scripts/build-owned-tool.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/build-owned-tool.py) | **DELETE** | No retained owned candidate release. Do not port a fork builder that is being retired. |
| [`scripts/download-owned-tool-candidate.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/download-owned-tool-candidate.py) | **DELETE** | Candidate publication/qualification is not activated; no retained download protocol. |
| [`scripts/immutable_publication.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/immutable_publication.py) | **DELETE** | Preserve existing immutable releases as historical evidence; do not retain a mutator framework solely to retire it. |
| [`scripts/owned_mise_qualification.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/owned_mise_qualification.py) | **PORT-TO-RUST** | Extract ONLY stock-mise no-config/miserc regression cases into velnor-actions-mise integration tests as Phase 0. Delete owned-wrapper, fake-MBX, candidate-receipt and publication cases. |
| [`scripts/owned_tool_behavior.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/owned_tool_behavior.py) | **DELETE** | Private candidate behavior schema retires; selected no-config expectations become typed Rust test cases, not a receipt engine. |
| [`scripts/owned_tool_qualification_evidence.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/owned_tool_qualification_evidence.py) | **DELETE** | No retained candidate promotion path. Do not weaken unrelated existing provenance checks. |
| [`scripts/owned_tool_source.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/owned_tool_source.py) | **DELETE** | Replace executable source framework with passive audit manifest and preserved bundles. |
| [`scripts/publish-owned-tool-artifacts.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/publish-owned-tool-artifacts.py) | **DELETE** | No owned binary distribution remains in the target scope. |
| [`scripts/publish_owned_source.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/publish_owned_source.py) | **DELETE** | Historical publications remain immutable; retirement does not require a general publisher. |
| [`scripts/qualify-owned-tool.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/qualify-owned-tool.py) | **DELETE** | Only mise is admitted; MBX qualification explicitly unavailable. Selected regression tests move without porting this orchestrator. |
| [`scripts/source_proof_capsule.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/source_proof_capsule.py) | **DELETE** | Passive evidence is retained, not executable authority or a new proof-capsule service. |
| [`scripts/source_publication.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/source_publication.py) | **DELETE** | Replace with the passive inventory in this package; do not keep a compiler/runtime dependency on retired snapshots. |
| [`scripts/source_publication_records.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/source_publication_records.py) | **DELETE** | Freeze outputs with provenance; regeneration is not an ongoing runtime need. |
| [`scripts/source_qualification_execution.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/source_qualification_execution.py) | **DELETE** | owned-tools.yml on main and owned-tool-candidates route are not established; delete with the retired pipeline, not independently of any newly found caller. |
| [`scripts/source_release_policy.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/source_release_policy.py) | **DELETE** | No new private source releases are planned; retain preexisting generic release safety outside this module. |
| [`scripts/source_semver_capsule.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/source_semver_capsule.py) | **DELETE** | No retained Velnor supplied-document caller; real semver behavioral fixture remains archived, not mislabeled as mock-only. |
| [`scripts/stage-owned-tool-source.py`](https://github.com/tailrocks/velnor-new/blob/d94fe9cefa6a0b97f00d235191c2d3142d1af6ff/scripts/stage-owned-tool-source.py) | **DELETE** | No retained owned-source build pipeline. |

Support fixture/test deletion candidates, each subject to exact import/caller closure:

```text
scripts/owned_tool_execution_test_fixtures.py
scripts/owned_tool_publication_test_fixtures.py
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
```

`test_git_optional_locks.py` requires a separate surviving-Git-policy check. Extract any still-needed regression into the appropriate existing Rust Git/process tests before deletion; discovery of that need expands Phase 0 rather than deferring a necessary port. The same rule applies to any other newly proved retained test consumer. Do not claim every listed test is mocked.

## 7. Consumer scope (46)

Authoritative input is the pinned PR #12 `scope.json`; refresh it and record any change. The research pass did not fully inspect all these repositories.

```text
jackin-project/jackin
jackin-project/jackin-agent-smith
jackin-project/homebrew-tap
jackin-project/jackin-role-action
jackin-project/jackin-sentinel
jackin-project/jackin-dev
jackin-project/jackin-github-terraform
jackin-project/jackin-the-architect
tailrocks/github-terraform
tailrocks/termpane
tailrocks/tui-snap
tailrocks/velnor
tailrocks/termrock
tailrocks/parallax
tailrocks/terminal-components-claude
tailrocks/tailrocks-repository-skills
tailrocks/tailrocks-skills
tailrocks/tailrocks-pull-request-skills
tailrocks/homebrew-velnor
tailrocks/velnor-apt
tailrocks/parallax-telemetry-playground
tailrocks/velnor-actions-fixture
tailrocks/holla
tailrocks/tracing-request-level
tailrocks/pg-bigdecimal
tailrocks/ruxel
tailrocks/schemalane
tailrocks/holla-apt
tailrocks/homebrew-parallax
tailrocks/homebrew-ruxel
tailrocks/homebrew-tablerock
tailrocks/homebrew-holla
tailrocks/tablerock
tailrocks/cloudflare-tofu
tailrocks/tailrocks-typescript-skills
tailrocks/tailrocks-skill-authoring-skills
tailrocks/tailrocks-rust-skills
tailrocks/tailrocks-roadmap-skills
tailrocks/tailrocks-open-source-skills
tailrocks/tailrocks-macos-skills
tailrocks/tailrocks-code-quality-skills
ChainArgos/blockchain-nodes
ChainArgos/java-monorepo
ChainArgos/jackin-agent-brown
ChainArgos/cloudflare-tofu
ChainArgos/github-terraform
```

## 8. Full custom commit identifiers

Aggregate statistics and exact parent lists are in the accompanying inventory; these are the exact commits whose disposition must be closed.

| ID | Full SHA | Target disposition |
|---|---|---|
| MISE-1 | [`dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96`](https://github.com/tailrocks/velnor-new/commit/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96) | Split: adopt already-merged no-config core through official distribution; drop owned Cargo-wrapper/banner/shim additions after G0. |
| MBX-1 | [`1ca12eb48391061a75e97d32e5b06fedf8a6253d`](https://github.com/tailrocks/velnor-new/commit/1ca12eb48391061a75e97d32e5b06fedf8a6253d) | Provisional DROP: semantic comparison/retention has no demonstrated production caller. Ordinary cache save policy is not equivalent semantic usefulness. |
| MBX-2 | [`2f324e89af509a1c61d802cf23741a04d7510b57`](https://github.com/tailrocks/velnor-new/commit/2f324e89af509a1c61d802cf23741a04d7510b57) | DROP private merge as delivery mechanism; preserve official v1.21.1 upgrade independently via PR #26. |
| MBX-3 | [`ee250ac37654a4cfbb55b6cd470f2a257204bbe9`](https://github.com/tailrocks/velnor-new/commit/ee250ac37654a4cfbb55b6cd470f2a257204bbe9) | Provisional DROP: no demonstrated owned root-pair consumer; do not claim ordinary target caching reproduces these semantics. |
| MBX-4 | [`e07c07cfec773897e5c439043901e661ad3f7b45`](https://github.com/tailrocks/velnor-new/commit/e07c07cfec773897e5c439043901e661ad3f7b45) | Provisional DROP: native ownership/compiler-event protocol not required by sampled stock consumers; retain reproducible defect evidence, not this fork. |
| MBX-5 | [`29865b5a18e3414084ff5db27d1da455772b4c34`](https://github.com/tailrocks/velnor-new/commit/29865b5a18e3414084ff5db27d1da455772b4c34) | Provisional DROP with abandoned fork; vendored Bats closure is not evidence its tests ran or passed. |
| MBX-6 | [`ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81`](https://github.com/tailrocks/velnor-new/commit/ac6ceed10df0c6cb0af1bc6d1a69d0de4d392f81) | Provisional DROP; explicitly incompatible with reviewed action comparison-report v1 parser; IPC 13/export v2 need coordinated consumers. |
| ACT-1 | [`f053f215866af0ddb6d2f32ecc19d45a5d25edc2`](https://github.com/tailrocks/velnor-new/commit/f053f215866af0ddb6d2f32ecc19d45a5d25edc2) | Provisional DROP: caller-bound executable and comparison state have no demonstrated production caller; Velnor can own verification and CLI invocation. |
| ACT-2 | [`198f0d1a538d34a91d7692f8f302259643c0f737`](https://github.com/tailrocks/velnor-new/commit/198f0d1a538d34a91d7692f8f302259643c0f737) | DROP private merge as distribution; stock action v1.6.0 is independently available. |
| ACT-3 | [`06f353d41002af758d27490164f53c82e2165637`](https://github.com/tailrocks/velnor-new/commit/06f353d41002af758d27490164f53c82e2165637) | Provisional DROP foreign patch; verify absolute binary/hash at Velnor process boundary if that requirement becomes live. |
| ACT-4 | [`c3cbe8e56ccb4727624df45022357f49d2953075`](https://github.com/tailrocks/velnor-new/commit/c3cbe8e56ccb4727624df45022357f49d2953075) | DROP as foreign dependency; enforce protected/default/event conditions in renderer-owned explicit save steps. |
| ACT-5 | [`62ec0713473dffeab46884b7c03906042794e696`](https://github.com/tailrocks/velnor-new/commit/62ec0713473dffeab46884b7c03906042794e696) | Provisional DROP: late baseline/export-group path is not activated in sampled consumers; strict report parser rejects MBX v2. |
| SEM-1 | [`d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a`](https://github.com/tailrocks/velnor-new/commit/d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a) | Provisional DROP; real-binary test driver exists, but no proved live Velnor supplied-document requirement. No claim a wrapper reproduces private lint semantics. |
| SEM-2 | [`583dddce84706786fc54c41a2c768c28a09c65fd`](https://github.com/tailrocks/velnor-new/commit/583dddce84706786fc54c41a2c768c28a09c65fd) | DROP with snapshot branch; this is fork CI policy, not a runtime upstream dependency. |
| CACHE-1 | [`8758d976a1b25eb387f48aa04ea86f57739b84cf`](https://github.com/tailrocks/velnor-new/commit/8758d976a1b25eb387f48aa04ea86f57739b84cf) | Split: provisional DROP archive/SDK framework; Foundation remains STAGED-HOLD until its generated/compiled pin closure is removed or a real owner accepts a separately scoped Velnor implementation. |
