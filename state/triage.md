# Branch triage ledger (read-only, 2026-11-16)

Source: one `gh pr list --state all --limit 600` (837..55, so every recent PR is covered), `git ls-remote --heads`, and `git fetch`. Ahead/behind is relative to origin/main. "Unique commits" = `git cherry origin/main origin/<b>` lines starting with +. Batch-ancestor checks: `merge-base --is-ancestor origin/<b> origin/fast/batch-N`.

| Branch | Last commit | Behind/ahead main | Unique commits | PR (newest) | PR state | PR base | Issue | Verdict | Notes |
|---|---|---|---|---|---|---|---|---|---|
| fast/597-main-and-scheduled | 2026-11-15 | behind 3 / ahead 50 | 38 | #824 | CLOSED | fast/808-plan-from-git | Refs #597 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/600-build-tests-once | 2026-11-15 | behind 3 / ahead 44 | 33 | #828 | CLOSED | fast/602-setup-prepare-restored-run | Refs #600 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/602-setup-prepare-restored-run | 2026-11-15 | behind 2 / ahead 54 | 42 | #798 | CLOSED | main | Refs #602 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/603-events-concurrency-policy | 2026-11-15 | behind 1 / ahead 5 | 4 | #776 | CLOSED | main | Refs #603 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/605-cache-families | 2026-11-15 | behind 3 / ahead 48 | 36 | #825 | CLOSED | fast/602-setup-prepare-restored-run | Refs #605 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/653-dogfood-helper-source | 2026-11-15 | behind 3 / ahead 45 | 34 | #823 | CLOSED | fast/602-setup-prepare-restored-run | Refs #653 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/705-gen-timings | 2026-11-15 | behind 3 / ahead 7 | 5 | #766 | CLOSED | main | Closes #705 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/712-sound-doc-steps | 2026-11-15 | behind 3 / ahead 12 | 11 | #765 | CLOSED | main | Refs #712 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/713-full-toolchain-profile | 2026-11-15 | behind 2 / ahead 61 | 47 | #804 | CLOSED | main | Closes #713 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/717-setup-velnor | 2026-11-15 | behind 2 / ahead 23 | 17 | #755 | CLOSED | main | Closes #717 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/718-custom-harness | 2026-11-15 | behind 1 / ahead 11 | 8 | #767 | CLOSED | main | Closes #718 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/722-engine-leaves-velnor-gen | 2026-11-15 | behind 1 / ahead 14 | 6 | #738 | CLOSED | main | Closes #722 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/726-release-plan | 2026-11-15 | behind 3 / ahead 13 | 11 | #763 | CLOSED | main | Closes #726 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/727-render-release-yml | 2026-11-15 | behind 3 / ahead 50 | 35 | #810 | CLOSED | main | Closes #727 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/728-release-build-publish | 2026-11-15 | behind 2 / ahead 59 | 46 | #815 | OPEN | main | Closes #728, Closes #728 | batch-3 | Open PR in planned Batch 3 |
| fast/736-socket-path-length | 2026-11-15 | behind 3 / ahead 2 | 2 | #773 | CLOSED | main | Closes #736 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/739-generator-tests-no-external | 2026-11-15 | behind 3 / ahead 19 | 12 | #771 | CLOSED | main | Closes #739 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/740-doc-scope | 2026-11-15 | behind 3 / ahead 5 | 5 | #792 | CLOSED | main | Refs #740 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/742-local-mode-pair | 2026-11-15 | behind 3 / ahead 4 | 4 | #787 | OPEN | main | Closes #742 | batch-3 | Open PR in planned Batch 3 |
| fast/743-local-authority | 2026-11-15 | behind 3 / ahead 14 | 10 | #809 | CLOSED | main | Closes #743 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/744-local-host | 2026-11-16 | behind 3 / ahead 23 | 16 | #821 | OPEN | main | Closes #744 | batch-3 | Open PR in planned Batch 3 |
| fast/745-local-runner-pair | 2026-11-15 | behind 3 / ahead 17 | 12 | - | - | - | #745 (named in commits) | NO PR - human decision | 4 unique commits for #745 stacked on fast/744-local-host; no PR was ever opened from this head; not an ancestor of batch-1/2 or of the #821/#787 heads |
| fast/748-pair-images-jit | 2026-11-15 | behind 1 / ahead 8 | 3 | #759 | CLOSED | main | Refs #651, Closes #748 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/750-trust-boundary | 2026-11-15 | behind 3 / ahead 10 | 10 | #774 | CLOSED | main | Refs #750 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/752-host-helper-error | 2026-11-15 | behind 3 / ahead 6 | 6 | #778 | CLOSED | main | Refs #752 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/753-host-state-0700 | 2026-11-15 | behind 3 / ahead 3 | 3 | #775 | OPEN | main | Closes #753 | batch-3 | Open PR in planned Batch 3 |
| fast/754-pair-cleanup | 2026-11-15 | behind 2 / ahead 8 | 7 | #820 | CLOSED | main | Closes #754 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/760-cfg-gated-build-deps | 2026-11-15 | behind 3 / ahead 9 | 9 | #770 | CLOSED | main | Closes #760 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/764-setup-velnor-sha-test | 2026-11-15 | behind 3 / ahead 22 | 17 | #783 | OPEN | main | Closes #764 | batch-3 | Open PR in planned Batch 3 |
| fast/768-timings-tolerant | 2026-11-15 | behind 3 / ahead 6 | 6 | #780 | CLOSED | main | Closes #768 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/769-runner-samples | 2026-11-15 | behind 3 / ahead 9 | 7 | #781 | CLOSED | main | Closes #769 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/777-machete-policy | 2026-11-15 | behind 3 / ahead 5 | 5 | #791 | CLOSED | main | Closes #777 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/779-host-state-0700-cli | 2026-11-15 | behind 3 / ahead 7 | 6 | #784 | OPEN | main | Closes #779 | batch-3 | Open PR in planned Batch 3 |
| fast/782-stderr-forge | 2026-11-15 | behind 3 / ahead 13 | 10 | #789 | CLOSED | main | Closes #782 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/785-retire-started-unbound | 2026-11-15 | behind 3 / ahead 17 | 14 | #803 | OPEN | main | Closes #785 | batch-3 | Open PR in planned Batch 3 |
| fast/786-weaken-warnings | 2026-11-15 | behind 3 / ahead 24 | 20 | #788 | CLOSED | main | Closes #786 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/790-audit-parallel-tests | 2026-11-15 | behind 1 / ahead 4 | 2 | #793 | CLOSED | main | Refs #790 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/794-remove-test-temp-folders | 2026-11-15 | behind 3 / ahead 6 | 6 | #795 | CLOSED | main | Closes #794 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/796-release-metadata | 2026-11-15 | behind 3 / ahead 19 | 16 | #797 | CLOSED | main | Closes #796 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/799-wire-plan-outputs | 2026-11-15 | behind 3 / ahead 68 | 50 | #807 | CLOSED | main | Refs #799 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/800-long-helper-error-test | 2026-11-15 | behind 3 / ahead 5 | 5 | #805 | CLOSED | main | Closes #800 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/801-check-metadata | 2026-11-15 | behind 3 / ahead 52 | 40 | #812 | CLOSED | main | Closes #801 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/802-journal-show | 2026-11-15 | behind 4 / ahead 16 | 12 | #816 | CLOSED | main | Closes #802 | in-batch-1 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/806-fast-tests | 2026-11-15 | behind 3 / ahead 10 | 9 | #814 | CLOSED | main | Closes #806 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/808-plan-from-git | 2026-11-15 | behind 3 / ahead 49 | 37 | #813 | CLOSED | main | Closes #808 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/817-release-config-no-expressions | 2026-11-15 | behind 2 / ahead 78 | 55 | #819 | OPEN | main | Closes #817 | batch-3 | Open PR in planned Batch 3 |
| fast/822-observer-settling-tests | 2026-11-15 | behind 2 / ahead 11 | 8 | #826 | CLOSED | fast/754-pair-cleanup | Closes #822 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/827-main-runs-not-cancelled | 2026-11-15 | behind 3 / ahead 25 | 23 | #831 | CLOSED | main | Closes #827 | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| fast/batch-1 | 2026-11-16 | behind 0 / ahead 176 | 119 | #836 | OPEN | main | Refs #790 (via #793) | keep | Batch 1 integration branch, PR open (#836) |
| fast/batch-2 | 2026-11-16 | behind 0 / ahead 428 | 300 | #837 | OPEN | fast/batch-1 | see constituents | keep | Batch 2 integration branch, PR open (#837), base fast/batch-1 |
| fast/docs-agent-api-budget | 2026-11-15 | behind 3 / ahead 3 | 3 | #818 | CLOSED | main | none (docs) | in-batch-2 | Branch tip is an ancestor of the batch branch; PR closed with Landed-through comment |
| handoff/roadmap-2026-11-15 | 2026-11-15 | behind 8 / ahead 3 | 3 | - | - | - | - | keep-for-now | Orchestration doc branch: adds only HANDOFF.md (220 lines); no PR |
| wip/717-goldens | 2026-11-15 | behind 8 / ahead 5 | 5 | - | - | - | #717 | reject | Unverified goldens for #755 (WIP commit 3762185c01 says unverified); #755 (in batch 1) has its own approved head f945ff6ad1 |

## Summary

| Verdict | Count |
|---|---|
| in-batch-1 | 12 |
| in-batch-2 | 28 |
| batch-3 (open PR) | 8 (#821, #819, #815, #803, #787, #784, #783, #775) |
| keep (fast/batch-1, fast/batch-2) | 2 |
| keep-for-now (handoff/roadmap-2026-11-15) | 1 |
| reject (wip/717-goldens) | 1 |
| NO PR, human decision (fast/745-local-runner-pair) | 1 |
| already-in-main / superseded | 0 |
| MOVED after batching | 0 |

Total: 53 remote branches besides main.

## Rows needing a human decision

1. fast/745-local-runner-pair: real, unlanded work for #745 (4 own commits: `--runner pair` for velnor-local, retry pair start up to 3 times, sweep only pair objects, repository id on events). No PR exists. It is stacked on fast/744-local-host (#821); not in either batch nor in Batch 3. Decide: open a PR and add to Batch 3, or drop. Do NOT delete.
2. handoff/roadmap-2026-11-15: carries HANDOFF.md only; keep until the owner says the orchestration is done.
3. wip/717-goldens: proposed reject (unverified WIP goldens, #755 landed in Batch 1 with its own head). Confirm.
4. fast/ci-self-hosted: PR #835 is MERGED (2026-11-15T17:07:52Z) and the branch is no longer on the remote; nothing to delete.
5. #825 (fast/605-cache-families, in Batch 2): per #837's note 1, its newest review at the approved head is "changes needed (one test gap)"; the owner's approval is not linkable. Confirm before merging #837.

## Observations

- Every closed constituent branch is an ancestor of its batch branch (batch-1 for #836 PRs; batch-2, which contains batch-1, for #837 PRs). No branch moved after batching. After #836 and #837 merge to main the 40 in-batch branches are safe to delete.
- fast/batch-1 moved during triage (ahead-of-main went 175 to 176; tip is now 3680c7c60c). Someone is still pushing to it. The ancestor checks passed on the later read.
- All 8 open PRs of Batch 3 have a branch; all are ahead of main and based on main. The ahead counts for #819 (78 commits) and #815 (59) show they carry a lot of stacked history that overlaps the batches; expect heavy conflicts and rebase onto the batch result. The Batch 3 branches are not ancestors of batch-1/batch-2 (checked).
- PR closed state: every constituent is CLOSED with mergedAt null (as expected for "Landed through" closes); GitHub will not show them as merged.
- gh list returned exactly 600 PRs (numbers 837 down to 55), so every PR from the recent branches is covered.

## Orchestrator decisions (velnor-new-9a, 2026-11-16)
- `fast/745-local-runner-pair`: batch-3 as PR #842 (opened 2026-11-16, head 8719884855, under Opus review).
- #825 approval: its last delta review (2d49d5d) found one test gap in code owned by #798 (`.rs` filter in `metadata_file.rs`); that test landed on #798 at 22824c8625 (delta-approved, https://github.com/tailrocks/velnor-new/pull/798#issuecomment-6896525459), and #837 contains it. The batch-2 review of every added commit approved #837 at 979ddceb59. Counted as approved.
- `wip/717-goldens`: reject, delete now (unverified goldens; #755 landed with its own approved head in Batch 1).
- `handoff/roadmap-2026-11-15`: keep until the roadmap run ends; the session handoff doc lives there.
- Constituent branches of #836/#837: delete after each batch merges, with the source PR already closed and linked to the batch.

## Landed
- Batch 1 (#836) merged 2026-11-16 as 24620215ca37a0d8889f8ee5aac379fdadbc58b0. Its 12 source branches deleted (793 738 755 763 797 810 767 771 776 759 816 809); fast/batch-1 auto-deleted by GitHub.
- Deleted earlier as reject: wip/717-goldens.

## Batch 2 landed

- Merge commit: d73853673a08e229df7d9c60cdea370dcc1737f8 (PR #837, `fast/batch-2` into `main`, squash).
- Source PRs (28, all closed): #765, #766, #780, #781, #798, #804, #812, #813, #807, #824, #831, #823, #828, #825, #770, #773, #778, #805, #792, #795, #814, #818, #820, #826, #791, #774, #788, #789.
- Comments posted: 28 ("Landed in main via Batch 2 #837, merge commit d73853673a08e229df7d9c60cdea370dcc1737f8."). None skipped; none were already present.
- Head branches deleted on origin: 28, one per source PR (fast/712-sound-doc-steps, fast/705-gen-timings, fast/768-timings-tolerant, fast/769-runner-samples, fast/602-setup-prepare-restored-run, fast/713-full-toolchain-profile, fast/801-check-metadata, fast/808-plan-from-git, fast/799-wire-plan-outputs, fast/597-main-and-scheduled, fast/827-main-runs-not-cancelled, fast/653-dogfood-helper-source, fast/600-build-tests-once, fast/605-cache-families, fast/760-cfg-gated-build-deps, fast/736-socket-path-length, fast/752-host-helper-error, fast/800-long-helper-error-test, fast/740-doc-scope, fast/794-remove-test-temp-folders, fast/806-fast-tests, fast/docs-agent-api-budget, fast/754-pair-cleanup, fast/822-observer-settling-tests, fast/777-machete-policy, fast/750-trust-boundary, fast/786-weaken-warnings, fast/782-stderr-forge).
- `fast/batch-2`: already absent on origin before this step (404), so nothing deleted in step 4.
- Issues closed: none. Every issue that a source PR body says it closes was already closed: #705, #768, #713, #801, #808, #827, #760, #736, #800, #794, #806, #754, #822, #777, #786, #782, and #769 (named in the #837 body for #781 only).
- Refs-only issues left open (not closed by those PRs): #712, #602, #799, #597, #653, #600, #605, #752, #750.

## Batch 3 landed

- Merge commit: 0812a2c80e8ca7e0c730cda496705be0871e095c (PR #857, `fast/batch-3` into `main`, squash, merged 2026-11-16).
- Source PRs (9, all closed by `scratchpad/close-batched.sh 857`, each with 0 unresolved threads): #775, #784, #783, #803, #787, #821, #842, #815, #819.
- Comments posted: 9 ("Landed in main via Batch 3 #857, merge commit 0812a2c80e8ca7e0c730cda496705be0871e095c."). None skipped; none were already present.
- Head branches deleted on origin: 9 (fast/753-host-state-0700, fast/779-host-state-0700-cli, fast/764-setup-velnor-sha-test, fast/785-retire-started-unbound, fast/742-local-mode-pair, fast/744-local-host, fast/745-local-runner-pair, fast/728-release-build-publish, fast/817-release-config-no-expressions).
- `prep/batch-3`: deleted on origin. `fast/batch-3`: already absent on origin, nothing deleted there.
- Local: `fast/batch-3` deleted from the velnor-new checkout (was 0d8745db10). Checked-out branch unchanged (main).
- Issues closed: none. All issues named by the nine source PRs (#753, #779, #764, #785, #742, #744, #745, #728, #817) were already closed.
