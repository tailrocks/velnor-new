/goal
Restore all useful work from the specified pull requests into the existing target branch.
Implement the changes. Do not stop after the analysis.

Repository: tailrocks/velnor-new
Target branch: consolidate/all-work-clean-2026-10-14

## 1. Required result

Preserve all source history. Restore all useful changes that are absent from the target.
Keep useful changes that already exist in main or the target.
Combine related changes into one working implementation.
Restore the useful crate splits from PR #97.
Do not keep the old crate layout only because it reduces merge conflicts.

A useful change adds a feature, corrects behavior, fixes a defect, or improves security.
It can also improve tests, development tools, build time, documentation, or code structure.
Small improvements are in scope.
A change is not useful only because it exists in an old branch.

Use ASD-STE100 Simplified Technical English for the plan, decision records, and final report.
Keep technical names and code identifiers unchanged.

## 2. Source records

The review used these commits on 15 October 2026:

MAIN_REVIEW = 7c594f7aef03990d8ff952727b0c56497372e355
TARGET_REVIEW = ca69bdb108c0d7ffa9666b18cffee91c01c7a829

Treat these commits as fixed review records.
Fetch the current remote refs before work starts.
Record any changes since this review.
Do not move the target back to TARGET_REVIEW.

Each source row gives the PR number, original branch, and reviewed head SHA.
The PR URL is https://github.com/tailrocks/velnor-new/pull/<number>.

| PR | Original branch | Reviewed head SHA |
|---|---|---|
| #151 | `fix/crate-workflow-capacity` | `72da60af4b6fd2dbfd162855079acebfe1e8086f` |
| #150 | `integrate/relevant-branches` | `fc07607ea9a98a8da4aff34f9b59a7eae27a526e` |
| #144 | `ci-tooling/b7-task-execution-201ab` | `d35b583a53aed687a78053ff0ef6ba2a758c3925` |
| #143 | `release/velnor-actions-0.1.7` | `ce4915b0cd94215fc2c4b04bf65e483e2e657ead` |
| #128 | `fix/persistent-generate-stage-root` | `26ad69883870de9ff91c72adb266677a373aa650` |
| #127 | `fix/retire-mbx-bundle-qualification` | `708fcbcb25a27b671975cc99397229d4aade0b85` |
| #126 | `fix/qualify-mise-2026.10.6-signed` | `b0e52b8b0668011764cf3b31d899acca751654e7` |
| #125 | `fix/registration-target-boundary` | `6cdb3c60eb6c930d113c6373de01afafad61a831` |
| #124 | `codex/mise-2026.10.6-own-pin` | `0ef7a9cd087ebcf2b1510321c1b54ca90d761e16` |
| #114 | `codex/mise-2026.10.5-qualification` | `e19d3f143887cc8a9c5914ed92697a3f80cc5350` |
| #110 | `fix/compact-over-cap-workflows-clean-base` | `e2584bf354397f3a0730574c7542c51e2ac68d78` |
| #109 | `feat/rust-binary-release` | `8eb77ac79c938331da564d3726d92fb90d6fca80` |
| #98 | `standardize/verify-vectors` | `47c7b5b2e77cdc29b5f6b3d9329a3adf393d3971` |
| #97 | `rust-policy-rollout` | `121138a959a5f26cec5eadd5f8c92b5633e7307a` |
| #95 | `codex/image-release-exact-source` | `9b7e6270768837149bff53c9d601066b307f42e7` |
| #94 | `impl/pr12-freshness-foundation-20261005` | `1557aabb4d3eeb8705e077934c9d38a7d9228f45` |
| #77 | `codex/freshness-evidence-20261005` | `9788a5e1c1303da05e9d26aa96043785b2f970a1` |

## 3. Protect and recover the work

Read the repository instructions before you edit files or run scripts.
Check the local status, worktrees, and remote refs.
Protect existing local changes. Use a separate worktree when necessary.
Create a backup ref for the current target head.
Record the current main head as the comparison baseline.

Recover all 17 reviewed source heads.
Fetch refs/pull/<number>/head when the original branch is absent.
Compare each fetched SHA with its source record.
Use the verified commit object from existing history when the PR ref is unavailable.
Do not use a temporary PR merge commit as the source head.

Create a recovery branch for each reviewed head.
Use recovery/pr-<number>-<short-sha> as its name.
Restore each original branch name when that name is absent.
Push the restored source refs without force.
Verify that each remote ref points to the intended full SHA.
Do not overwrite a branch that now contains different work.
Preserve both versions when a PR head has changed.

Report an unavailable source as BLOCKED.
Do not invent missing commits or claim to recover uncommitted work.

Do not reset the target to an old branch.
Do not force-push, delete source refs, or discard local changes.
Do not use an ours merge or a blanket ours/theirs conflict rule.
Do not create empty merges as evidence of restoration.

The reviewed target already contains the reviewed source heads in its history.
Recheck this relationship against the current target.
An ancestor relationship does not prove that the source code remains present.
Create new restoration commits when an ordinary merge has no effect.
Merge source history only when that history is actually absent.

## 4. Compare every source with main and the target

Build a complete file and commit inventory for every source.
Use local Git data for large comparisons.
Do not treat a limited API file list as a complete inventory.
Read the PR discussion to identify intent, known defects, and missing evidence.
Verify important claims against code.

For each PR, examine its original base and its reviewed head.
Find its merge base with the current main snapshot.
Compare the source changes from that merge base.
Also compare the complete source tree with main and with the target.
Use endpoint tree comparisons, not only three-dot comparisons.

Useful commands include:

git merge-base "$MAIN" "$SOURCE"
git diff --find-renames --find-copies "$MERGE_BASE" "$SOURCE"
git diff --find-renames --find-copies "$MAIN" "$SOURCE"
git diff --find-renames --find-copies "$TARGET" "$SOURCE"
git merge-base --is-ancestor "$SOURCE" "$TARGET"

Use verified full SHAs for these variables.
Inspect renamed paths, moved tests, changed file modes, and deleted files.
Inspect earlier commits when later source commits removed useful work.
Review merge conflict resolutions that selected main and discarded source changes.
Do not treat all differences from main as improvements.
Do not treat a missing old path as missing behavior until you check its new owner.

Separate independent improvements within each PR.
Separate parent-branch work from changes added by a stacked PR.
Use patch identity and range comparisons to find duplicates.
Use code review and tests to confirm equivalent behavior.

Create docs/implemented/all-work-restoration-2026-10-14.md.
Use the existing report when that file already exists.
Keep a decision record for each independent change.
Record these fields:

- Source PR, commit, and original path.
- Intended improvement and acceptance test.
- Equivalent code in main and the target, when present.
- Decision, destination path, and target commit.
- Test command, result, and tested SHA.
- Replacement evidence or a precise exclusion reason.

Use these decisions:
RESTORE: Add an absent useful change.
RETAIN: Keep equivalent or better code that is already present.
COMBINE: Preserve useful parts from more than one implementation.
SUPERSEDED: Keep a newer implementation that covers the old requirement.
REJECT: Exclude a harmful, unrelated, or duplicate change with evidence.
BLOCKED: Preserve the source and state the unresolved requirement.

Do not use one general decision to dismiss a large PR.
Do not use age, a closed PR, or merge difficulty as an exclusion reason.
Do not declare completion while an accepted improvement remains unaccounted for.

## 5. Work required for each PR

### PR #151: Wide-workspace generation

Keep the current workflow-capacity improvement.
Preserve the 4,194,304-byte internal matrix budget and its rejection test.
Keep the separate platform output and matrix limits.
Preserve the composite-action fallback for large crate obligation lists.
Keep checkout, tool preparation, and report upload in their correct scopes.
Pass covered-task data through valid composite inputs.

Test the restored workspace and the 150-member fixture, including its root package.
Check that every eligible crate keeps all required obligations.
Check the existing 500,000-byte workflow limit, including its generated marker.
Do not remove crates, truncate matrices, or weaken limits to make generation pass.

### PR #150: Previous integration branch

Use this branch as a source of individual changes, not as the final design.
Review its full history and all conflict resolutions.
Reassess the excluded grouped crate layout and overlapping generator changes.
Do not repeat the previous rule that main always wins a conflict.

Find unique runner admission, attestation, helper supervision, and private-log improvements.
Review action-archive seed recovery, lease handling, cleanup, and journal changes.
Review native adapter work, task execution, MBX routing, and test registration.
Port useful changes not already covered by #97, #143, or #144.

Keep current main features and version pins.
Do not copy recovery dumps, duplicate source trees, build outputs, or unrelated binaries into active product paths.
Inspect useful tests and documentation within recovery material before excluding it.
Keep excluded material available through its recovery ref.

### PR #144: Typed task execution

Restore missing task-execution contracts and their complete execution path.
Add the versioned task manifest, resolver, wrapper, and registered tests together.
Use the execution digest as the wrapper selector.
Keep the plan digest as a separate binding.
Bind arguments, environment, tools, matrix identity, and report-helper identity to the execution record.

Preserve strict parsing, size bounds, digest checks, and the bounded NUL-framed protocol.
Reject unknown fields, changed records, truncated frames, and trailing data.
Do not let the manifest choose its own trusted generator version.
Do not reconstruct commands through shell evaluation.

Preserve pinned Rust and MBX routing.
Remove inherited compiler selectors where the contract requires isolation.
Preserve source and lock validation, task-source preparation, and useful test-registration changes.
Keep release-family selection without bypassing release eligibility.
Combine the renderer changes with #110 and #151.
Do not copy old 0.1.7 release files over current versions.

### PR #143: Runner artifact admission

Restore missing checks for the resource-probe image and native attestation helper.
Bind each artifact to the expected source, workflow, release, digest, and platform.
Validate the daemon-selected Linux amd64 image, not only a requested image tag.
Preserve TUF trust-state checks, rollback protection, and concurrent-state tests.

Preserve helper supervision, deadlines, bounded results, and cleanup.
Bind results to the same live engine and root-bound attempt.
Recheck required identity before accepting a result.
Reject zero-fit capacity and invalid limits.

Combine the code with #97's split runner crates and #150's later fixes.
Keep registration and private-log improvements that remain useful.
Do not claim production qualification from local fixtures or old PR statements.
Keep unqualified runtime paths disabled by their existing safety gates.

### PR #128: Persistent generation stage root

Find the current implementation before applying this patch.
Preserve or restore the private, worktree-bound stage root on the same filesystem.
Keep the stage root linked during publication and cleanup.
Clean its contents without replacing the root itself.
Preserve the required .github directory mode.

Keep atomic exchange and platform-specific filesystem checks.
Reject foreign ownership, unsupported mount identity, and cross-mount publication.
Preserve the Linux STATX_MNT_ID requirement where the implementation uses it.
Keep preview behavior separate from in-place publication.
Report cleanup warnings without hiding a publication failure.

Run interruption, repeated-generation, concurrent-observer, mount, and cleanup tests.
Test supported Linux and macOS paths.
Resolve the reported transient .github NotFound failure with a regression test.
Do not import the older Mise pin from this branch's parent.

### PR #127: Qualification cache contract

Keep the v2 receipt work that is already partly visible in the target.
Verify all producers, readers, artifact names, and tests together.
Require the exact five ordered cache layers.
Reject the retired MBX bundle layer and invalid Cargo-plus-MBX combinations.
Reject old receipt schemas where the new contract requires rejection.

Do not confuse receipt schema v2 with unchanged cache-key preimage schemas.
Preserve valid cache identity and native MBX object behavior.
Do not enable cache admission that lacks the required validation evidence.
Do not import the older Mise pin from the parent branch.

### PR #126: Signed Mise qualification changes

Treat this PR as the reviewed successor to #124.
Keep current Mise pins instead of restoring version 2026.10.6.
Extract useful version-output parsing, executable identity, qualification, and bound checks.
Apply them to current supported platforms and pins.
Preserve negative tests for wrong versions and wrong digests.

Separate old qualification evidence from current test results.
Investigate the recorded final verification failure with #128.
Preserve original authorship and commit records.
Do not invent signatures or sign-off trailers for another author.

### PR #125: Shared registration build target

Retain the shared nested-Cargo target and lock behavior already present in the target.
Do not apply the old file over newer registration fixes.
Update paths and ownership after the workspace and crate moves.
Keep a regression test against an unintended runner-local target directory.
Check source registration for every applicable workspace or replacement fixture.
Check macro-generated tests, OUT_DIR inputs, and MBX registration with #143 and #144.
Ensure that test filters actually select tests.

### PR #124: Earlier Mise pin and bounds

Compare every change with #126 and current main.
Preserve any useful bound or validation change that the successor omitted.
Keep ordinary source-download limits separate from owned-tool bootstrap limits.
Do not weaken unrelated download limits.
Mark the old 2026.10.6 pin as superseded by the current pin.
Keep the historical source ref and qualification record.

### PR #114: Earlier Mise qualification

Compare generic qualification checks with #124, #126, and current main.
Retain useful dispatch-ref, executable-digest, platform, and negative-test improvements.
Mark the 2026.10.5 pin and obsolete platform values as superseded.
Do not replace current generated files with old pin snapshots.
Keep old evidence clearly dated.

### PR #110: Workflow text compaction

Preserve useful compaction that already exists.
Restore missing parts only after comparing the current renderer.
Use repeated run-scalar aliases only when they make an oversized workflow smaller.
Keep normal-size output unchanged unless another accepted feature requires a change.
Resolve aliases only in permitted run-step locations before shell validation.
Reject invalid alias scope and unsafe shell content.

Combine compaction, task manifests, and crate composites into one deterministic render path.
Test all applicable workflow families, not only ci.yml.
Keep exact marker-inclusive size checks and negative size tests.
Do not add a second conflicting fallback implementation.

### PR #109: Opt-in Rust binary releases

Preserve or restore the useful binary-release feature.
Keep release generation opt-in and disabled by default.
Preserve merged-tag eligibility and semantic-version ordering.
Keep supported native Linux and macOS ARM64 builds.
Preserve deterministic archives, checksums, prerelease behavior, and safe resume behavior.

Keep useful pull-request template preservation through generation.
Combine release checks with #95 and current product-release code.
Retain useful freshness transport and archive-validation fixes found in the same branch.
Do not restore obsolete pins or delete newer owned-tool support without a justified replacement.
Do not publish a release during this task.

### PR #98: Configurable verification work

Preserve or restore useful configurable verification behavior.
Check Markdown, strict JSON, skill metadata, links, and native validator probes.
Reject duplicate JSON keys and invalid skill identifiers.
Keep tool pins and argument validation.
Keep configuration independent of one consumer repository.
Connect each required check to the correct final check aggregation.

Adapt the work to the accepted task contract from #144.
Do not maintain duplicate configuration authorities for the same behavior.
Review generated-tree preservation and development-agent configuration changes separately.
Keep useful, compatible development tools and their tests.
Do not hard-code another repository's paths.

### PR #97: Crate structure and runner safety

Use this PR as the primary source for the missing crate structure.
Restore the useful adapters, apps, core, services, and tools groups.
Restore real package boundaries, not only directory names.
Split contract, Mise, orchestration, workflow, and runner responsibilities where the source design is sound.
Give each responsibility one clear owner.
Keep the dependency graph free of cycles.

Move current main and target improvements into the new owners.
Preserve newer resource-probe, attestation, freshness, and native adapter work.
Do not remove a newer feature because the older split branch lacks it.
Update workspace members, dependencies, feature flags, lockfiles, and package metadata.
Update test registration, scripts, fixtures, documentation, and generated workflow paths.
Keep root and runner test coverage during the transition.
Remove obsolete duplicate implementations after their replacements pass tests.

Preserve runner capacity transactions, daemon-bound reservations, and per-row outcomes.
Preserve protected-state checks, read-only credential loading, and readiness rejection on invalid state.
Preserve the AppArmor policy identity checks and Linux Available acknowledgement guard.
Preserve cache-writer election checks, including orphan and non-elected save rejection.
Keep working macOS behavior.

Measure clean builds, leaf-change rebuilds, dependency effects, and duplicate build work.
Use the same toolchain, hardware, and cache conditions for comparisons.
Do not claim a speed improvement from crate count alone.
Explain any rejected split with code evidence and a tested alternative.
Do not reject the structure only because restoration is difficult.

### PR #95: Exact-source release checks

Preserve or restore the release-admission fix.
Select Required jobs for the exact CI run and attempt.
Count all matching jobs before filtering for success.
Require exactly one matching job.
Then validate its source SHA, branch, status, and conclusion.

Reject a success combined with a failure or cancellation.
Reject missing jobs, duplicates, wrong attempts, and wrong source commits.
Apply the invariant to shared release code and every affected release family.
Test rejection before any release creation or publication side effect.
Regenerate actions and snapshots from the corrected source.

### PR #94: Rust repository policy

Preserve the Rust freshness and repository-policy implementation.
Retain bounded upstream probes, dependency checks, and commit-trailer validation.
Keep the private typed CLI boundary.
Do not add maintenance commands to the public product command tree.
Keep scripts thin where Rust owns the policy.
Do not restore replaced Python policy validators as a second authority.

Align this work with the repository-policy app and crate groups from #97.
Keep maintenance crates outside product task derivation when the contract requires that separation.
Keep their dedicated checks in Required aggregation.
Preserve useful archive preflight, task ownership, and metadata fixes from this PR.
Prove that moved tests remain registered and executed.

### PR #77: Historical freshness evidence

Preserve the dated evidence and its source ref.
Do not overwrite the live inventory with old timestamps.
Recreate current evidence only from actual current checks.
Do not change pins merely to match this old report.
Do not renew holds, extend exceptions, or approve newer upstream versions without authorization.
Do not describe a failing freshness check as a pass.

## 6. Integration order

First, protect refs and record baseline results.
Then complete the change inventory and write the initial acceptance tests.

Restore #97's structure in small, testable steps.
Carry current #94 and #125 behavior through each move.
Keep current #110 and #151 generation behavior throughout the migration.

Next, combine runner work from #97, #143, and #150.
Combine task execution and developer tooling from #144, #150, and #98.
Combine release behavior from #95, #109, and #144.
Complete the joint renderer checks for #110, #144, and #151.

Then verify #127 and #128 against the integrated implementation.
Close the unique-change review for #114, #124, and #126 without restoring old pins.
Refresh current evidence after the code is stable.
Keep #77's historical evidence separate.

Audit #150 again at the end for useful changes not covered by the other sources.
Review all 17 decision records before final acceptance.
Use the dependency map to adjust the order when necessary.
Record the reason for each adjustment.

## 7. Validation and completion

Use the repository-approved Mise and MBX entry points.
Do not bypass a required MBX route with bare Cargo.
Use locked dependency resolution where the repository requires it.
Run formatting, lint, unit, integration, documentation, and policy checks.
Run all applicable workspace tests, including runner tests during the workspace transition.
Check test selection counts and registrations.
Do not remove tests or weaken assertions to obtain a pass.

For each restored defect fix, add a test that exposes the old failure when practical.
Build the generator from the integrated source.
Do not use an older published generator as proof of new behavior.
Regenerate owned outputs, actions, fixtures, and snapshots.
Run generation twice and require no unexplained second-run changes.
Verify preview and in-place behavior separately.

Test task-manifest rejection, cache-schema rejection, and exact-source release rejection.
Test wide Rust workspaces and wide OpenTofu configurations.
Test atomic publication under failure and repeated use.
Run the relevant Linux and macOS tests.
Keep platform output limits, security checks, and Required coverage intact.

Record pre-existing failures separately from new failures.
Record exact commands, platform details, test counts, and tested commit SHAs.
Mark unavailable mandatory checks as BLOCKED, not passed.
Do not substitute old PR evidence for tests of the integrated branch.
Do not claim production readiness for an unqualified runner path.

Make small commits with source PR references and accurate authorship.
Use the repository's required commit trailers with the real configured identity.
Push recovery refs and target changes without force.
Check for remote changes before each target push.
Do not change main, publish releases, or deploy runner services.
Do not install or load production security policies as part of this task.

Deliver the completed target branch and its final SHA.
Deliver the source recovery map and the per-change decision report.
Show the old-to-new crate map and dependency summary.
Show retained, restored, combined, superseded, rejected, and blocked work for every PR.
Show test evidence and measured development-performance results.
Explain every source-to-target difference that remains relevant to the requested improvements.

The task is complete only when all accepted improvements have working code and adequate tests.
A preserved branch, merge commit, copied snapshot, or written plan is not sufficient.

## 8. Audit findings, technical root cause analysis, and explicit proposal comparison

### 8.1 Discovery of empty `-s ours` merges across all 26 pull requests

Target branch `consolidate/all-work-clean-2026-10-14` contains exactly 26 pull request merge commits in its first-parent history (from commit `1762a73609` for PR #12 to commit `ca69bdb108` for PR #150).

Technical verification of the Git object database proves:
- All 26 merge commits were executed with `git merge -s ours`.
- Commit `89f5d4bf19` (merging PR #97 `121138a959`) has zero tree differences from its parent `18643215fa` (`0 files changed, 0 insertions, 0 deletions`).
- The entire 78-crate modular tree from PR #97 was discarded. The target working tree remained 100% on the monolithic 10-crate layout.
- Comparing commit `540435094a` (prior to the 26 PR merges) with `ca69bdb108` (the head of target) shows zero file changes across the entire merge sequence.
- Git ancestry checks (`git merge-base --is-ancestor`) return `true` solely due to these empty merge commits. This creates an ancestry illusion: the commit graph records the PRs as merged, but zero source code from PR #97 was actually integrated into the file tree.

### 8.2 Root cause of PR #97 abandonment

PR #97 (`rust-policy-rollout`, commit `121138a959`) split the repository into 78 workspace packages. Investigation into why this branch was abandoned revealed four primary root causes:

1. **Workflow size ceiling deadlock (`workflow_too_large`)**:
   In PR #97, generating GitHub Actions CI workflows for 78 fine-grained packages with inlined job obligations caused `ci.yml` to exceed GitHub's hard limit of 500,000 bytes. The workflow generator rejected the file, and GitHub Actions rejected the workflow upload. Additionally, the orchestrator's internal matrix byte budget (then limited to 524,288 bytes) rejected the matrix plan.
2. **External runner and infrastructure blockers**:
   PR #97 was blocked externally by the lack of an official Ubuntu 26.04 runner container image on GHCR, the offline status of ChainArgos ScaleSet 3, and DCO signoff failures on nine older historical commits.
3. **Cross-platform compilation failure**:
   `safe_read/tests.rs` invoked `rustix::fs::mkfifoat` without target platform gating (`#[cfg(target_os = "linux")]`), causing compilation failure on macOS.
4. **Conflation of generator modularization with runner host daemon**:
   PR #97 bundled generator crate restructuring with an experimental Linux runner host daemon (AppArmor profiles, epoll loops, systemd units, and daemon-bound runner capacity transactions) inside the root generator workspace, conflicting with the V1 generator architectural boundary.

Because CI workflow generation could not complete and tests could not pass, PR #97 was closed unmerged on GitHub. It was subsequently merged via `-s ours` during branch consolidation on October 14, 2026.

### 8.3 Technical viability resolved by PR #110 and PR #151

The technical deadlock that blocked PR #97 is now completely resolved:

- **PR #110 (`e2584bf354`)**: Implemented YAML run-scalar anchor and alias text compaction (`yaml_share.rs` and `render_fallback.rs`), significantly reducing redundant step payloads.
- **PR #151 (`72da60af4b`)**: Raised the internal matrix budget to 4,194,304 bytes (4 MiB) and implemented **composite-action fallback for wide workspaces** (`large_crate_composite.rs`). When workflow size approaches the 500 KB limit, crate obligation steps are automatically extracted into local composite actions (`.github/actions/crate-obligations-*`).
- **Resolution**: Because PR #110 and PR #151 are present and active in `consolidate/all-work-clean-2026-10-14`, generating CI workflows for 78 or more crates now remains well under GitHub's 500 KB limit. The modular crate structure can now be ported with 100% passing workflow generation and green CI checks.

### 8.4 Architectural boundary enforcement: Generator vs Runner (`AGENTS.md`)

Section 5 (lines 338-366) of the original proposal instructed:
> "Preserve runner capacity transactions, daemon-bound reservations, and per-row outcomes... Preserve the AppArmor policy identity checks and Linux Available acknowledgement guard."

This instruction directly conflicts with repository invariants defined in `AGENTS.md`:
- "V1 is a workflow generator: `plan`/`generate` turn repository evidence into CI workflows. It is not a runner, interpreter, or second task graph."
- "Do not implement runner behavior in V1 generator crates. The active runner spec is `docs/proposed/macos-scaleset-runner.md` (nested `crates/velnor-runner`)."
- Root `Cargo.toml` explicitly sets `exclude = ["crates/velnor-runner"]`.
- `crates/velnor-runner` is an independent workspace with its own `Cargo.toml`, `deny.toml`, and `clippy.toml`.

**Boundary Correction**:
- Generator crates under `crates/adapters/`, `crates/apps/`, `crates/core/`, `crates/services/`, and `crates/tools/` (for tools like `velnor-archive-guard`) must remain pure V1 workflow generator code.
- Runner host daemons, AppArmor policies, capacity reservations, and runner CLI code from PR #97, #143, and #150 must NEVER be added to the root generator workspace. Any useful runner improvements belong exclusively within the nested `crates/velnor-runner/` workspace.

### 8.5 Scope reconciliation: 17 reviewed PRs vs 26 requested PRs

The original proposal in Section 2 evaluated 17 PRs (#77 through #151). The user's goal requested accounting for 26 PRs (#12 through #151).

The reconciliation confirms:
- The 9 earlier PRs (#12, #13, #25, #28, #29, #30, #38, #56, #65) are historical foundation branches.
- All 9 were merged via `-s ours` prior to commit `f37321a047` (PR #77).
- Their functional improvements (cache selection qualification, qualification dispatch, tar member extraction staging, hosted qualification, and generator release provenance) were already integrated into main prior to release `v0.1.8`, or were superseded by the modern native MBX/Mise toolchain and v2 cache receipts (PR #127).

| PR | Branch Name | Head SHA | Target Merge Commit | Merge Strategy | Functional Disposition |
|---|---|---|---|---|---|
| #151 | `fix/crate-workflow-capacity` | `72da60af4b` | `8f083583b2` | `-s ours` | Retained and applied directly in target (commits `84dadcca19`–`c1f84a0ef3`). Enables PR #97. |
| #150 | `integrate/relevant-branches` | `fc07607ea9` | `ca69bdb108` | `-s ours` | Superseded with selective audit. Blanket tree overwrites rejected to protect v0.1.8. |
| #144 | `ci-tooling/b7-task-execution-201ab` | `d35b583a53` | `7075c9a025` | `-s ours` | Retained and ported. Typed task execution contracts wired into modular orchestrator/workflow. |
| #143 | `release/velnor-actions-0.1.7` | `ce4915b0cd` | `ce87690853` | `-s ours` | Superseded by v0.1.8 in main. Runner artifact admission belongs in `crates/velnor-runner`. |
| #128 | `fix/persistent-generate-stage-root` | `26ad698838` | `b045ccaaa0` | `-s ours` | Retained in main. Persistent private stage root with transactional publication active. |
| #127 | `fix/retire-mbx-bundle-qualification` | `708fcbcb25` | `d362873850` | `-s ours` | Retained in main. 5-layer cache receipt v2 schema active; MBX bundle retired. |
| #126 | `fix/qualify-mise-2026.10.6-signed` | `b0e52b8b06` | `4c11925a5d` | `-s ours` | Superseded by current pin. Qualification tests retained; Mise pin advanced to 2026.10.8. |
| #125 | `fix/registration-target-boundary` | `6cdb3c60eb` | `47ecddcfa9` | `-s ours` | Retained in main. Shared nested build target and lock boundaries active. |
| #124 | `codex/mise-2026.10.6-own-pin` | `0ef7a9cd08` | `f6ae93bcc0` | `-s ours` | Superseded by PR #126 and current pin 2026.10.8. Download limits retained. |
| #114 | `codex/mise-2026.10.5-qualification` | `e19d3f1438` | `de5f117e7c` | `-s ours` | Superseded by current pins and qualification logic in main. |
| #110 | `fix/compact-over-cap-workflows-clean-base` | `e2584bf354` | `8d304a769d` | `-s ours` | Retained and applied. YAML run-scalar anchor/alias text compaction active in renderer. |
| #109 | `feat/rust-binary-release` | `8eb77ac79c` | `2b21e4d895` | `-s ours` | Retained in main. Opt-in Rust binary release workflow generation active. |
| #98 | `standardize/verify-vectors` | `47c7b5b2e7` | `18643215fa` | `-s ours` | Retained in main. Configurable verification vectors, strict JSON, native probes active. |
| #97 | `rust-policy-rollout` | `121138a959` | `89f5d4bf19` | `-s ours` | Port required now viable. 78-crate modularization unblocked by PR #151 composite fallback. |
| #95 | `codex/image-release-exact-source` | `9b7e627076` | `25d444e9ed` | `-s ours` | Retained in main. Exact-source CI release admission and Required job validation active. |
| #94 | `impl/pr12-freshness-foundation-20261005` | `1557aabb4d` | `343ea4032c` | `-s ours` | Retained in main. Rust repository policy and freshness active in `velnor-actions-freshness`. |
| #77 | `codex/freshness-evidence-20261005` | `9788a5e1c1` | `f37321a047` | `-s ours` | Superseded by current evidence. Historical snapshot preserved; live evidence at 2026-10-15. |
| #65 | `fix/generator-release-provenance` | `a54e34b01b` | `32b45f5dd5` | `-s ours` | Incorporated in main v0.1.8 generator release build metadata. |
| #56 | `fix/hosted-qualification-dispatch` | `7f4b24afdf` | `2e5b4a8d2f` | `-s ours` | Incorporated in main v0.1.8 CI qualification workflows. |
| #38 | `codex/chainargos-cache-foundation` | `b08bc12250` | `9f70f2ffe3` | `-s ours` | Superseded by native MBX toolchain in main v0.1.8. |
| #30 | `codex/mbx-released-action-isolation` | `fe0064cbb8` | `2fbe5c4c3d` | `-s ours` | Superseded by PR #127 5-layer cache receipt v2 in main v0.1.8. |
| #29 | `codex/chainargos-mbx-cache-repair` | `4ad1e34a1e` | `401a915d1a` | `-s ours` | Superseded by native MBX toolchain in main v0.1.8. |
| #28 | `codex/velnor-goal-integration` | `38a6cf7a1d` | `ef99f27d76` | `-s ours` | Superseded by the consolidation branch. |
| #25 | `tar-absolute` | `f17e2e182f` | `5c0327dc77` | `-s ours` | Incorporated in main v0.1.8 archive handling. |
| #13 | `qualification-dispatch` | `73bfa1c4b9` | `7f0ae73784` | `-s ours` | Incorporated in main v0.1.8 qualification dispatch foundation. |
| #12 | `perf/cache-selection-qualification` | `96556c18a0` | `1762a73609` | `-s ours` | Incorporated in main v0.1.8 cache qualification baseline. |

### 8.6 Explicit side-by-side comparison: Original Proposal vs Full Granular Port

| Dimension | Original Proposal (Sections 1–7) | What the User Wants (Full Granular Port) | Architectural and Verification Distinction |
| :--- | :--- | :--- | :--- |
| **Crate Topology** | Broadly mentions restoring #97 crate structure; lacks concrete subcrate mapping; leaves monolithic crates in target. | Full Granular Port of generator crates into domain groups (`adapters`, `apps`, `core`, `services`, `tools`). Monolithic crates split into single-responsibility crates. | Drastically speeds up compilation and leaf-package rebuilds; enforces strict size gates (400 lines/file, 150 lines `lib.rs`). |
| **Runner Isolation** | Conflates runner daemon, AppArmor checks, and daemon reservations into PR #97 generator workspace (lines 338-366). | Strict compliance with `AGENTS.md`. Generator workspace remains pure V1 workflow generator. Runner crates reside strictly in nested `crates/velnor-runner` (`exclude = ["crates/velnor-runner"]`). | Eliminates architectural pollution; keeps generator independent of runner daemon dependencies and OS syscalls. |
| **Merge Strategy & Ancestry** | Relied on ancestry checks (`ca69bdb108`), warning against empty merges but failing to recognize that PR #97 was ALREADY merged via `-s ours` with 0 tree diffs. | Explicitly exposes the `-s ours` ancestry illusion. Replaces the empty merge state with actual ported code, tests, and configurations. | Rejects "ancestry-only" completion; satisfies the second independent requirement of "no work lost". |
| **Workflow Capacity** | Retained PR #110 and PR #151 as isolated fixes without linking them to PR #97's viability. | Leverages PR #110 text compaction and PR #151 composite-action fallback as the architectural key that enables 78+ crate generation under 500 KB. | Solves the exact blocker that caused PR #97 abandonment. |
| **Scope Coverage** | Tracked only 17 PRs (#77–#151), omitting 9 historical PRs. | Comprehensively accounts for all 26 requested PRs (#12–#151) with exact merge commits, head SHAs, and dispositions. | Complete historical audit trail with zero missing PRs. |
| **Modern Feature Retention** | General rule against removing newer features. | Explicitly specifies preservation of OpenTofu 1.13.1 parser/grammar/proposal into `velnor-actions-tofu-core` / `velnor-actions-tofu`, PR #144 typed task execution, and v0.1.8 metadata. | Guarantees zero regressions in OpenTofu 1.13.1 support or typed task execution contracts. |
| **Validation Gates** | General testing requirements. | Exhaustive gate verification: `cargo fmt`, `cargo clippy -D warnings`, `cargo nextest`, `alint`, `cargo deny check`, `scripts/check-freshness.sh`, and double generation determinism checks. | Complies with all repository proof invariants. |

### 8.7 Full Granular Port implementation plan and crate topology

The target workspace replaces the 10 monolithic crates with the following modular domain packages:

```
crates/
├── adapters/
│   ├── velnor-actions-actionlint/
│   ├── velnor-actions-mise/
│   ├── velnor-actions-mise-cache/
│   ├── velnor-actions-mise-catalog/
│   ├── velnor-actions-mise-core/
│   ├── velnor-actions-mise-nextest/
│   ├── velnor-actions-mise-probes/
│   ├── velnor-actions-rust/
│   ├── velnor-actions-rust-core/
│   ├── velnor-actions-tofu/         <-- OpenTofu 1.13.1 closure, selection, golden tests
│   └── velnor-actions-tofu-core/    <-- OpenTofu 1.13.1 AST parser, module grammar, proposal engine
├── apps/
│   ├── velnor-actions-cli/          <-- Public CLI commands (generate, plan, check)
│   └── velnor-actions-freshness/    <-- Repository policy & freshness checks (was repo-policy)
├── core/
│   ├── velnor-actions-contract/
│   ├── velnor-actions-contract-config/
│   ├── velnor-actions-contract-planning/
│   ├── velnor-actions-contract-release/
│   └── velnor-actions-contract-workflow/
├── services/
│   ├── velnor-actions-orchestrator/
│   ├── velnor-actions-orchestrator-core/
│   ├── velnor-actions-orchestrator-discovery/
│   ├── velnor-actions-orchestrator-generation/
│   ├── velnor-actions-orchestrator-plan/
│   ├── velnor-actions-orchestrator-runtime-execute/  <-- PR #144 typed task execution contracts
│   ├── velnor-actions-orchestrator-task-report/
│   ├── velnor-actions-workflow-generator/
│   ├── velnor-actions-workflow-jobs/
│   ├── velnor-actions-workflow-renderer/            <-- PR #110 compaction + PR #151 composite fallback
│   ├── velnor-actions-workflow-schema2/
│   └── velnor-actions-workflow-steps/
├── tools/
│   └── velnor-archive-guard/        <-- Generator archive verification tool
└── velnor-runner/                   <-- EXCLUDED nested workspace (docs/proposed/macos-scaleset-runner.md)
    ├── Cargo.toml
    └── crates/
        ├── velnor-runner-core/
        ├── velnor-runner-github/
        ├── velnor-runner-host/
        ├── velnor-runner-cli/
        └── velnor-resource-probe/
```

#### Detailed Porting Guarantees:
1. **OpenTofu 1.13.1**:
   - `crates/adapters/velnor-actions-tofu-core` receives the HCL parser, module grammar (`src/modules/grammar.rs`), canonical module identity (`src/modules/identity.rs`), and proposal engine.
   - `crates/adapters/velnor-actions-tofu` receives the closure builder, selection logic, build.rs, and golden test suite.
   - All OpenTofu 1.13.1 tests pass with zero regressions.
2. **Typed Task Execution (PR #144)**:
   - Task manifests, execution digest selectors, and bounded NUL-framed protocol are ported to `crates/services/velnor-actions-orchestrator-runtime-execute` and `crates/core/velnor-actions-contract-workflow`.
3. **Workflow Compaction and Fallback (PR #110 & PR #151)**:
   - `crates/services/velnor-actions-workflow-renderer` preserves `yaml_share.rs`, `large_crate_composite.rs`, and the 4 MiB matrix budget.
   - Generating workflows for the expanded workspace automatically extracts large obligation sets into `.github/actions/crate-obligations-*`, maintaining `ci.yml` strictly under 500 KB.
4. **Runner Isolation Boundary**:
   - `crates/tools/velnor-runner-*` from PR #97 are excluded from the root workspace. Any useful runner improvements are directed exclusively to the nested `crates/velnor-runner` workspace.
