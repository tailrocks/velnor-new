You own one open PR in tailrocks/velnor-new and bring it to a mergeable state. You are in your own git worktree. Do NOT merge, close, or enqueue it.

Rules:
- Read AGENTS.md, docs/goals.md, the PR (`gh pr view N --comments`), its linked issue(s), and its unresolved review threads.
- Check out the PR head branch (`git fetch origin && git switch -c <head> origin/<head>` or `gh pr checkout N`). Bring main in with the tailrocks-repository-skills:tailrocks-pr-refresh skill (sync mode: merges the base, regenerates declared outputs, runs pre-push checks, pushes once). Known issue: if sync says "already contains the base" while `git rev-list --count HEAD..origin/main` > 0 (GitHub's baseRefOid for the PR is stale), run `git merge --signoff origin/main` yourself, regenerate declared outputs, run the pre-push checks, and push once. Never rebase; never force-push (the only allowed force-push is the skill's dco-fix mode).
- Every commit signed off (`git commit -s`). Regenerate generated files/goldens/samples/digests the repo requires; run the affected crates' tests (`cargo nextest run -p <crate>`) and the repo's lint checks before pushing.
- Make the fixes listed below with red/green evidence for any behaviour change (a failing test first, then the fix). Keep scope to what is listed; no extra refactors.
- Resolve review threads with the tailrocks-repository-skills:tailrocks-pr-threads skill: fix real bugs (red/green), reply with evidence otherwise. Never resolve a thread a human asked without answering it.
- Do NOT edit the PR title/body after your final push (it restarts CI and leaves a stale skipped run that GitHub treats as blocking). If the body needs changes, do them with tailrocks-pr-refresh metadata mode BEFORE your last push, or put notes in a PR comment.
- After the final push, add the `ci:run` label once (`gh pr edit N --add-label ci:run`) to get the full PR CI, then watch it (`gh pr checks N --watch`). If a required check fails due to the change, fix and push again (re-add ci:run). If it fails for an unrelated flake, record the run URL and cause in a PR comment.
- Finally run the tailrocks-repository-skills:tailrocks-pr-status skill and report its verdict.
- Clean up anything you created locally and report it.

Report (≤20 lines): head SHA, what you changed, red/green evidence, Required/DCO state, unresolved threads count, pr-status verdict, anything left.
- Write any PR body or comment file to a unique path you create with `mktemp` (never a fixed name in a shared scratch dir: other agents run in parallel and a shared name has already swapped PR bodies). Before your final push, check `gh pr view <PR> --json body` names the right issue.

## OWNER RULES (2026-11-25, override anything above)
- Never defer. Address every review comment/thread in the PR itself: fix it with a test, or answer it with evidence when the finding is wrong. No "follow-up issue" deferrals, no "out of scope".
- Batch: when several small fixes are related, do them in one PR rather than many.
- Commits end with your own session's Co-Authored-By trailer.
