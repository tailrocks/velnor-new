# HANDOFF: tailrocks/velnor-new roadmap (from session velnor-new-c3, 2026-11-15)

Read this file first, then `docs/roadmap.md`, `docs/goals.md`, `AGENTS.md`, and the body of epic #574.

## 1. Where things stand

Refreshed 2026-11-15 at 10:00 UTC. Sources: one `gh pr list` (saved as scratchpad `open-prs.json`), the 2026-11-15 rows of `.audit/velnor-roadmap.tsv`, and the draft state. CI results, behind counts and unresolved threads were not re-read in this refresh (API budget). Check them before each merge.

**Merged today (2026-11-15):** #737 (PR 757), #716 (PR 758), #710 (PR 762).

**Merged in the previous session** (each issue closed, with its Done-when boxes ticked and linked to proof):
#594, #596, #595, #703, #694, #697, #719, #599, #632 (parts 3a, 3b, 3c and 4), #637 (two PRs), #640, #641, #639, #652, #582, #586,
#701 (goals.md rule 2: lockfile), #741 (goals.md: Velnor's two parts), #674 (decided; the work is #717), #707 (decided; the work is #742–#748), #720 and #685 (owner steps, done).

### 1.1 Open PRs (40)

"Approved at" is the SHA of the latest reviewer verdict, from the decision log. "Head" is the head at 10:00 UTC. When they differ, the gap is main merges unless the row says otherwise.

| PR | Issue | Approved at | Head (10:00) | Stacked on | Next action |
|---|---|---|---|---|---|
| 738 | #722 | 59521243e7 | 59521243e7 | main | Wait for Required and DCO, merge. |
| 755 | #717 | b47ac46678 (per draft) | 6129bede86 | main | Delta review of later commits. Lab proof (install, cache hit, wrong-SHA fails). Merge before 783 and 810. |
| 759 | #748 | bd56e450d5 (comment 6886517542) | ff4de53fe3 | main | Head is main merges only. Fresh CI, merge. Before 809. |
| 761 | #751 | f00a7108fe | 899201b209 | main | Main merge only. Fresh CI, merge. Before 816. Box 2 needs an owner Mac run (see #802). |
| 763 | #726 | 87bdb75890 | 87bdb75890 | main | Fresh CI, merge. Before 810 and 797. |
| 765 | #712 | 8b2c0248b9 | 02916d1258 | main | Main merges only. Fresh CI, resolve threads, merge. Before 798. |
| 766 | #705 | 95e5f2e194 | b155286aed | main | Main merges only. Fresh CI, merge. Before 780 and 781. |
| 767 | #718 | 03a1795a95 | 55b47de36f | main | Delta check on the head, then fresh CI and merge. Before 771. |
| 770 | #760 | e66cba4024 | e66cba4024 | main | Fresh CI, merge. Before 810 (not a stack). |
| 771 | #739 | 87c1dd7f21 | 87c1dd7f21 | main | Hold for 767. Then merge. |
| 772 | #723 | d2bc24cdad | d2bc24cdad | main | Fresh CI, merge. Before 810. |
| 773 | #736 | 2843fe1f0f | 2843fe1f0f | main | Fresh CI, merge. |
| 774 | #750 | 041394a519 | 041394a519 | main | Fresh CI, merge. Before 788 and 789. |
| 775 | #753 | cf2cf7cdf1 | cf2cf7cdf1 | main | Fresh CI, merge. Before 784. |
| 776 | #603 | c535eaa7e2 | c535eaa7e2 | main | Fresh CI, merge. Before 791. |
| 778 | #752 | b61a092b82 | b61a092b82 | main | Fresh CI, merge, then close #752 by hand (Refs). Before 805. |
| 780 | #768 | a68230825e | a68230825e | main (held by 766) | Hold for 766. Resolve 1 thread, merge. |
| 781 | #769 | 21016631a8 | 21016631a8 | main (held by 766) | Hold for 766. Fresh CI, merge. Close #769 by hand (no Closes line). |
| 783 | #764 | 23d6f09515 | 23d6f09515 | 755 | Hold for 755. Fresh CI, resolve threads, merge. |
| 784 | #779 | 366e602a7e | 366e602a7e | 775 | Still a draft. Hold for 775, mark ready, merge. |
| 787 | #742 | 8340e3caf8 | 8340e3caf8 | main | Fresh CI, merge. Local-mode release lock. |
| 788 | #786 | 69a8d8c31b | 69a8d8c31b | 774 | Hold for 774. Fresh CI, merge. |
| 789 | #782 | cebfbba1a9 | cebfbba1a9 | 774 | Hold for 774. Fresh CI, merge. |
| 791 | #777 | 485af91b53 | 485af91b53 | 776 | Hold for 776. Fresh CI, merge. |
| 792 | #740 | 9be683fd20 | 9be683fd20 | main | Fresh CI, merge. Keep #740 open (Refs). Owner choice A/B/C (see 1.3). |
| 793 | #790 | be840ea78c | be840ea78c | main | Fresh CI, merge. Keep #790 open for the three-runs box. Before 795. |
| 795 | #794 | 63a6489187 | 63a6489187 | 793 | Hold for 793. Fresh CI, merge. |
| 797 | #796 | 736b58a9bc | 736b58a9bc | 763 (held) | Hold for 763. Fresh CI, merge. |
| 798 | #602 | 1be83cc929 | 1be83cc929 | 765 | Hold for 765. Fresh CI, merge. Before 812 and 804. |
| 803 | #785 | none yet (review sent 09:02) | 3b12d3623e | 778 | Review, then CI. It was branched from an old #778 head: trial-merge current main and 778. |
| 804 | #713 | ff3cf75287 | ff3cf75287 | 798 (held) | Hold for 798. Fresh CI, merge. |
| 805 | #800 | 2719e66c68 | 2719e66c68 | 778 (held) | Hold for 778. Merge. |
| 807 | #799 | d2674fad1c | d2674fad1c | main (held by 813) | Hold for 813. Then 807 merges 813 and drops pull-requests handling. Conflicts in 3 files. |
| 809 | #743 | not approved | 1d57cccbaa | 759 | Changes requested at 09:50 (authority keys mode, exact-match positive case, current_exe at stop time). Haiku round 3 in progress. Then delta review. |
| 810 | #727 | 85149f5f01 | 85149f5f01 | main (held by 755, 763, 772) | Hold for 755, 763, 772. Fresh CI, merge. Before 815. |
| 812 | #801 | pre-round head (09:37) | 872f435661 | 798 | Round pushed at 8f80e70443 (09:47), not the head shown. Check the head, then delta review. |
| 813 | #808 | approved 09:46 (head not logged) | 8889f6d13e | main | Small round pending (require HEAD == GITHUB_SHA, rename-only test). Check it, then CI. Before 807. |
| 814 | #806 | none (changes requested 09:47) | c5b30a2c02 | main | Haiku round restores the stale-dir case. Then review. |
| 815 | #728 | none yet (Opus review sent 09:50) | 284a1bdf82 | 810 | Hold for 810. Review, then CI. |
| 816 | #802 | none yet | b97293cbde | 761 | Read-only `velnor-host journal show`. Review, then merge after 761. |

In progress with no PR: #754 (branch `fast/754-pair-cleanup`, local worktree exists).

**Not ready (waiting for a fix or a review):** 809, 814, 803, 815, 816, 812 (round check), 813 (round check).

### 1.2 Merge order

Merge one PR at a time. After each merge, run `git merge --signoff origin/main` on every other open branch. Never squash a child before its parent: the squash would swallow the parent.

Fixed constraints:
- #767 before #771.
- #813 before #807.
- #755, #763 and #772 before #810; #810 before #815.
- #755 before #783; #766 before #780 and #781; #774 before #788 and #789; #776 before #791; #775 before #784; #793 before #795; #765 before #798; #798 before #812 and #804; #761 before #816; #759 before #809; #778 before #803 and #805; #763 before #797.

Roots that can merge after a fresh CI run, in no fixed order: 738, 759, 761, 763, 765, 766, 770, 772, 773, 774, 775, 776, 778, 787, 792, 793, 813 (after its small round).

### 1.3 Owner decisions recorded today

- **#750: option A.** Project settings that make a check do less are followed, and each one is named on the output. Forged workflow commands are made impossible (#782). Decided 2026-11-14 (comment 6885629093). PR 774 is approved.
- **#740: waiting for the owner.** Box 2 ("no crate checked twice") is `needs-owner`. Choices:
  - A: accept the duplicate doc check for crates with dependents, as a documented exception to rule 4;
  - B: allow `RUSTC_WORKSPACE_WRAPPER=clippy-driver` on `cargo doc`;
  - C: leave the box open until cargo#8143 is fixed.
  PR 792 is approved, and #740 stays open (Refs). Hosted box 1 counts were proven on velnor-lab run 43696650241.
- **#705: the command name.** The timings command is `velnor-ci timings`. Recorded as a plan change on #705 (08:34).
- **#712: box 2 replaced.** The sound rule runs both doc steps on the lab by design. Recorded as a plan change (08:29).
- **#786: box 3 replaced.** Nextest overrides cannot filter tests. Recorded as a plan change (08:19).
- **#602: option B.** The plan reads changed files from git, with no token. Follow-up #808 (PR 813).
- **#713: prepare installs missing components and targets itself** (rustup component/target add, no shell). Decided 09:22, so no owner question.

### 1.4 Follow-up issues created today

All open. Most carry `in-progress`.

| Issue | Topic | Where it stands |
|---|---|---|
| #764 | setup-velnor refuses a wrong SHA-256 test | PR 783, held behind 755 |
| #768 | stale or malformed timings file | PR 780, held behind 766 |
| #769 | timings use only current-runner samples | PR 781, held behind 766. Close by hand after merge. |
| #777 | machete on dependency changes | PR 791, held behind 776 |
| #779 | every velnor-host state folder 0700 | PR 784, held behind 775 |
| #782 | velnor-ci output cannot forge a workflow command | PR 789, held behind 774 |
| #785 | retire a pair that started but never bound (Opus) | PR 803, under review. Also covers #761's capacity-release gap. |
| #786 | weakening warnings | PR 788, held behind 774 |
| #790 | velnor-ci audit tests in parallel | PR 793, approved |
| #794 | remove killed test temp folders | PR 795, held behind 793 |
| #796 | build metadata in release tags | PR 797, held behind 763 |
| #799 | wire every plan output into steps | PR 807, held behind 813 |
| #800 | long helper error is cut at the limit | PR 805, held behind 778 |
| #801 | metadata check and run hardening | PR 812, stacked on 798 |
| #802 | `velnor-host journal show` | PR 816, stacked on 761 |
| #806 | ci-helper tests under the 2 s slow-timeout | PR 814, changes requested |
| #808 | plan reads changed files from git | PR 813, approved |
| #811 | document the local test recipe (Haiku) | Opened 09:30. Its status was not checked in this refresh. |

Two issues are closed by hand after their PR merges: #769 (PR 781 has no Closes line) and #752 (PR 778 uses Refs).

### 1.5 API rate-limit lesson

- At 08:48 the GitHub core limit hit 5000/5000. Eleven single-PR watchers, four stale pollers and idle agents were stopped. Since then there is one combined merge watcher.
- At 09:38 the limit was exhausted again (reset 09:50 UTC). The likely cause was `gh run watch` at its default 3 s polling in the lab agents.
- Rule: `gh run watch <id> --interval 180` (300 s when GraphQL is low). Never `gh pr checks --watch` without `--interval 180`. Agents do not poll CI; the orchestrator watches and merges.
- Use git for everything git can answer (`git ls-remote`, `git merge-base`, branch heads). Keep `gh` calls to one `gh pr list` per refresh, and no loops.

### 1.6 Local test noise

- Plain `cargo nextest run --workspace` fails about 100 tests in velnor-actions-cli, velnor-actions-mise and velnor-archive-guard. The fixtures are environment-only. Main CI passes them.
- Run `bash scripts/verify-local.sh`, or the mise task `test-maintenance-helpers`. Run only the crates you touch otherwise.
- The recipe is being documented by #811.

## 2. Ready-issue query, as of the handover

```
#725 blocks:2 (in PR #756)   #603 blocks:1   #712 blocks:1   #717 (in PR #755)   #721 blocks:1   #723 blocks:1
#726 blocks:1   #742 blocks:1   #748 blocks:1
#705 #710 #716 #718 #736 #737 #739 #740 #750[needs-owner] #751 #752 #753 #754   (blocks:0)
```

Run the query from `docs/roadmap.md` for the current list.

**Chains of dependent work:**
- **Release, release-plz:** #721 → #722 → #723 → #724 → #725 → #726 → #727 → #728 → #729 → #731 → #733.
  - The design is at https://github.com/tailrocks/velnor-new/issues/680#issuecomment-6883047188.
  - #727 must make every job after `gate` require `needs.gate.outputs.ready == 'true'` (see the comment on #727).
  - Owner steps: #734 (bootstrap tag `rust-ci-v0.1.80`), #730 (seed tags) and #732 (switch the ruleset to App-only). The owner said this session may push these tags itself when their issue says it's time: the `gh` login is the owner's account.
- **Local mode:** #748 and #742 first, then #743 → #744 → #745 → #746, plus #747. #651 is their parent. The design is in #651's comments.
- **Runner bugs found by the ARM proof:** #751 (no recovery after a crash), #752, #753, #754, plus #736 and #737.
- **Generator follow-ups:** #740 (docs step: no double check, only affected crates), #712, #710, #705, #718, #716, #739, #603.

**Owner-only issues still open:**
- #662: one Qualification run on the self-hosted scale set; needs the production runner online.
- #680: closes when #733 (R11) lands.
- #682: the interim owner-only tag ruleset is live; closes when #733 lands.
- #734, #730, #732: the release tags and the ruleset switch.
- #750, #649, #648, #646, #623 and #604 carry `needs-owner`.

## 3. Decisions the owner made (don't re-ask)

- **Lockfile (#701):** a `Cargo.lock` change selects only the crates that use the changed library. `goals.md` is updated.
- **Rule 9 (#674):** it stays. `velnor-ci` is installed through `uses: tailrocks/velnor-new/actions/setup-velnor@<full sha>`. It may move to `tailrocks/setup-velnor` later.
- **Releases (#680):**
  - Velnor generates every workflow, with no hand-written ones.
  - release-plz is integrated; cargo-dist is not.
  - The Opus design is adopted (D1–D6). No workflow is triggered by a tag. The `velnor-release` App (ID 5762496) is the only identity that creates tags, and its key lives only in 1Password and in the `release` environment secrets.
- **Local mode (#707):**
  - The bouncer stays on and asks `velnor-local`. Only `local-mode` builds have it; release builds refuse that feature.
  - Local mode keeps no journal.
  - The approved-workflows check is off locally, with a warning on every run and an optional `--policy` flag. There is no prompt.
  - `--runner pair` is the default.
  - Velnor has two parts: the generator and the runner. `velnor-local` runs workflows through the runner.
- **#599 box 1:** reworded per configuration, along with D1 and the #578 box. The 2 doc re-checks are a known exception, fixed by #740. Rule 4 in `goals.md` is unchanged.
- **Quality versus speed:** never skip a verification to go faster. Optimise only useless work: checking the same thing twice, or checking something nothing changed.
- **Pull requests are collaborators-only** on every tailrocks repository. This was set through the API, and a TODO in github-terraform waits for provider PR integrations/terraform-provider-github#3479.
- **github-terraform:** in sync, and the owner has applied it (#57, #58, #59).
- **Cleanup rule** (in `~/.claude/CLAUDE.md`): every agent removes what it created, such as containers, volumes, networks, images, worktrees, branches and temporary folders, and reports what it removed.

## 4. How to run an issue (proven procedure)

1. **Claim it:** `gh issue edit N --add-label in-progress`, then move its project card to "In Progress". The ids are in `docs/roadmap.md`. If `gh` loses the `project` scope, the owner runs `gh auth refresh -h github.com -s project` in a separate terminal.
2. **The executor brief** must include:
   - the issue number;
   - "read AGENTS.md, docs/goals.md, and the #574 rules: 400-line file cap, I/O only in `io`, one test executable, no explanatory comments, a test plan before tests, red/green evidence";
   - a branch `fast/<n>-<slug>` from origin/main, signed-off commits, and main brought in only with `git merge --signoff origin/main`; never rebase or force-push;
   - a ready PR, not a draft (`gh pr create` often makes drafts, so run `gh pr ready`);
   - a body with `Closes #N`, a test plan, and every Done-when command with its output, ending with "🤖 Generated with [Claude Code](https://claude.com/claude-code)";
   - no edits to the PR body after a push, because that restarts CI; use PR comments instead;
   - answer every Codex thread: fix real bugs with red/green, or reply with evidence, then resolve the thread. After two rounds, fix only real correctness or security bugs;
   - wait until `Required`, `Required verdict` and DCO pass, with 0 unresolved threads (check through GraphQL) and the branch 0 behind main;
   - don't merge;
   - clean up, and report what was removed;
   - spend no money; velnor-lab (public) runs on GitHub-hosted runners for free.
3. **The reviewer** is a separate agent, briefed with the diff and the issue, never with the executor's conclusion. It must run mutations, check the test plan, post its review with `gh pr review N --comment`, and clean up its worktree. After a substantial push following its approval, send only the delta back to the same reviewer.
4. **Merge,** only when all of these hold:
   - `gh api repos/tailrocks/velnor-new/compare/main...<sha>` shows behind 0;
   - GraphQL shows 0 unresolved threads;
   - `Required`, `Required verdict` and DCO pass.

   Then run `gh pr merge N --squash --match-head-commit <sha>`. Never use `--admin`, and never force-push. velnor-lab requires only DCO.
5. **After the merge:** check the issue closed, remove `in-progress`, tick the Done-when boxes with proof links, print the ready query, and print one line for the issue: number, PR, merged, and the proof for each box.
6. **Merging one PR makes every other open PR "behind".** Merge one at a time, and have the executor run `git merge --signoff origin/main` right after each merge.

## 5. Model and parallelism rules (owner's instruction for the new session)

- **Models:** use only Haiku 5.5, Sonnet 5.5 and Opus 5.5. **Never use Fable.**
- **Who gets the work:** most work goes to Haiku 5.5 and Sonnet 5.5.
  - Haiku: small mechanical issues, checks, ticking boxes, reviews of small diffs.
  - Sonnet: most executors and reviewers.
  - Opus 5.5: only where an issue is security-critical or a deep design question. That means the release gate and publish, runner recovery, the bouncer and local authority, and trust-boundary research. Use Opus at high effort only when the issue says so.
- **Concurrency:** run up to about 8 issues at once, provided no two touch the same files. Group them by folder:
  - `generator/`
  - `ci-helper/`
  - `local/`
  - `crates/velnor-runner/`
  - `crates/velnor-actions-*`
  - `.github/`
  - velnor-lab

  Within one folder, run issues one after another if they touch the same module. Spawn independent agents in one message, in parallel.
- **Budget:** the account limit expires in about 20 hours. Work in parallel and keep agents busy. Don't poll; watchers notify you when checks finish.
