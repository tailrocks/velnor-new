# velnor-new finalize: final report (2026-11-25)

Ledger: state/goal-ledger.tsv. Inventories: state/inv/branches-final.tsv, state/inv/issues-final.tsv.

## Merged: 94 PRs
2098 2325 2394 2397 2411 2464 2501 2506 2507 2508 2510 2559 2565 2579 2615 2616 2633 2637 2644 2650 2660 2683 2692 2694 2699 2716 2722 2723 2733 2734 2735 2736 2737 2738 2739 2740 2743 2747 2749 2750 2751 2755 2763 2764 2765 2767 2770 2772 2774 2776 2778 2780 2783 2786 2787 2788 2789 2790 2791 2792 2793 2794 2795 2796 2797 2798 2799 2802 2803 2805 2806 2807 2808 2809 2810 2823 2824 2826 2827 2829 2830 2831 2832 2834 2835 2837 2838 2841 2843 2845 2847 2850
Every merge had Required green, signed-off commits, no open threads, and an independent review. Security and concurrency changes got an opus/xhigh review.

## Closed PRs: 21 (with reasons on each PR)
- Replaced by a signed copy, because the Renovate commit had no DCO sign-off: 2746→2780, 2777→2784, 2842→2843, 2844→2845, 2758→2851.
- Closed as redundant or superseded: 2781 (redundant test), and 2260 2345 2454 2458 2477 2495 2505 2610 2642 2659 2710 2727 2728 2730 2731 2732.

## Issues
107 were closed with evidence. 127 remain open, and none is unclassified:
- 86 wait on an owner question (label needs-owner; the question is posted on the issue);
- 25 wait on a named blocker that ends in an owner decision (an open PR waiting on --accept-risk, the 0.1.116 release and repin, #1450, or #2448);
- 13 are epics or trackers (status comments list their children);
- 1 is the bot dashboard (#1233).
#1218 waits on an upstream turso release.

## Open PRs: 24, all waiting on a named owner decision
- **--accept-risk (workflow files; reviewed and green):** 2828 (repin to 0.1.115; 2833 is stacked on it and also needs ~1 runner-min/run acceptance), 2782, 2784, 2815, 2819, 2721, 2525.
- **Owner questions on the PR or its issue:** 2761 (+1.9 s PR audit), 2785 (rootless trim job), 2812 and 2779 (test-plan record or waive), 2693, 2577 (5-file re-merge deferred to the delete-scope answer), 2851 (hk 2.14.0, signed copy of 2758; reviewed, green; hk drops Intel macOS), 2183 (turso hold until about 12-04; the non-turso bumps are being split into a signed PR), 1986, 1751.
- **Follow the no-doctests decision on #2448/#2432:** 2448, 2657, 2604, 2445, 2379. 2379 is in a fix round (review found 5 scanner false negatives); 2657 has 2 confirmed scanner bugs that only matter if the Python scanner stays; 2445 may be superseded by 2379.
- **Owned by another session:** 2839 ([[image]] Variant A, session peer-session-A, merging with owner --accept-risk).

## Branches (64 remote)
- 27 belong to open PRs.
- **Keep:** g4-submodule-fixture (a live submodule fixture) and rescue/fast-1593 (an owner question is recorded on it).
- gh-readonly-queue/* are temporary merge-queue refs.
- **Safe to delete (merged, superseded, or rejected; reason in the ledger):**
  - Closed-PR branches: fast/2361-archive-exec-diagnostics, fast/2430-relative-build-dir-probe, fast/2438-tools-verify-followups, fast/2440-fu-from-source-reuse, fast/2470-audit-ctime-relist, fast/2471-purge-private, fast/2474-anchor-steps-ref, fast/2491-lock-review-followups, fast/2514-credential-report-order, fast/2529-binary-digest-docs, fast/2582-steps-invariant-gaps, fast/2584-mise-version-exact-set, fast/i2475-audit-fd-bound, fast/local-mode-lane-parallel, fast/repin-tools-0.1.110, fix/2528-mirror-inherited-off, fix/2682-tools-heal-removal-precondition, fix/2696-restore-only-tools-save, fix/refresh-rust-inputs-lock, issues-2587-2613-2620, speed/rh-slow-public-qualifier-split.
  - Rescue and salvage branches, rejected or carried elsewhere: rescue/docs-migrating-velnor-gen-20261120, rescue/gen-0107-20261120, rescue/mig-bn-gen-src-20261120, rescue/velnor-new-gen-20261120, rescue/fast-1555-cargo-guard-remerge-20261120, rescue/fix-local-strip-conformance-20261120 (superseded by #2043), salvage/2701-mbx-total-bytes (carried by #2740), salvage/2703-mbx-comments (carried by #2736), salvage/2702-mbx-report-tests-wip, fast/2580-steps-shapes, fx-2545, handoff/roadmap-2026-11-15.
  - The 4 merged or content-on-main branches listed in branches-final.tsv.

## example-org processor CI
The fix is in velnor-new: #2774 restores folder mtimes, and #2770 orders the generated steps (it replaces 384e86b040). Both are released in tools-v0.1.113+.
example-repo-100#98, example-repo-101#134 and example-repo-102#125 were regenerated with tools 0.1.115. All three are CLEAN, with Required and Cache green and "no waste".
Waiting on the owner's --accept-risk, because they change .github/workflows. After merge, check the first warm-cache run on main.

## Fixes landed this session for the processor repos
- #2847: the runner-image tar shim accepts setup-velnor's GNU long options (part of #2846; the scale-set image still needs a rebuild and rollout by an operator).
- #2850: the inputs audit runs before the prune and the saves in the lint-test shape (closes #2848). tron's scheduled audit stays red until it repins to 0.1.116; peer-session-C accepted that.

## Recommendations
- Renovate commits fail the DCO gate, so each one needs a signed replacement PR. Enable sign-off in Renovate's config, or exempt bot commits in the merge policy.
- After 2828 merges: release tools-v0.1.116, which holds many merged helper fixes, then repin. That unblocks #2535, #2440, #2709 and #2444's live checks.
- The local mise shim ~/.local/share/mise/shims/mise is a symlink to itself.

## Resume list (paused for credit limit, 2026-11-25)
Landers (bash, no credits) are attached and merge on green: 2448 2577 2693 2761 2815 2819 2855 2858 2859 2860 2863 2866 2869 (+2525 once threads clear).
Still to do after reset:
- 2525: 3 new Codex threads (scratch-repo external diff helpers, attribute bracket counting, rename-source pathspecs); then 2862 refresh on top (keep 2862's destination() parsing).
- 2864, 2865 (folds 2868): independent review, then land (gated 0.1.117).
- 2867: implement signer decision (ci.yml dedicated attestation job, no repo code; reword decision 8) + 4 Codex fixes (partial rerun attempt, artifact pagination, producer-run artifact id, bounded wait).
- 2871 (#2816 part 1): CI + security review; 2873 draft: full tests/goldens; part 3 (repo workflows/docs) after tools-v0.1.117.
- 2872 (Fixes #2870 image release-version): mark ready, CI, review.
- 2855: secret-scan fixture fix pushed; verify Required.
- Old open questions remain on 2183 (turso hold to ~12-04), 1986, 1751 drafts.
