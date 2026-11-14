# HANDOFF: tailrocks/velnor-new roadmap (from session velnor-new-c3, 2026-11-15)

Read this file first, then `docs/roadmap.md`, `docs/goals.md`, `AGENTS.md`, and the body of epic #574.

## 1. Where things stand

**Merged in the previous session** (each issue closed, with its Done-when boxes ticked and linked to proof):
#594, #596, #595, #703, #694, #697, #719, #599, #632 (parts 3a, 3b, 3c and 4), #637 (two PRs), #640, #641, #639, #652, #582, #586,
#701 (goals.md rule 2: lockfile), #741 (goals.md: Velnor's two parts), #674 (decided; the work is #717), #707 (decided; the work is #742–#748), #720 and #685 (owner steps, done).

**Open PRs. Finish these first.** Each one is green or nearly green and has a reviewer verdict.

| PR | Issue | State when handed over | What is left |
|---|---|---|---|
| #738 | #722 the engine leaves velnor-gen workflows | Reviewer approved. Codex rounds are capped. | Wait for `Required` and DCO, check it is not behind main, merge. |
| #749 | #724 release-gate reads the required checks from the rules | Reviewer approved. The executor was adding two tests: a `diverged` compare status must exit 1, and a case that needs the `integration_id` -1 "any source" value. | Check those tests are pushed, red/green. Then merge. |
| #756 | #725 release record and publish | Reviewer re-approved at `b6fa367` (all 6 mutations go red). | Wait for `Required`, then merge. |
| #755 | #717 the setup-velnor action | The executor was still doing the velnor-lab proof runs. No review yet. | Check the lab proof (install, cache hit, SHA mismatch fails). Run a reviewer (Opus 5.5, low). Merge the lab PR it opened, then this one. |

These four issues carry the `in-progress` label. If you restart an issue with a new executor, give it the existing branch; don't open a second PR.

**A research task that was running.** #750 (trust boundary for project config that weakens checks) needs an owner decision. An Opus agent was writing a recommendation. Re-run that research (Opus 5.5, high): what Velnor should follow, warn about or reject, with industry sources. Then bring the owner one rule, a table of cases, and a pick. Write it in plain, short sentences: the owner asked for ADHD-friendly explanations, one question at a time, with real examples.

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
