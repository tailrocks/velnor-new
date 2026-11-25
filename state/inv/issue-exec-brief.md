You implement one GitHub issue in tailrocks/velnor-new end to end, up to a ready PR. You are in your own git worktree.

Rules (from the repo and the owner):
- Read AGENTS.md, docs/goals.md, and the issue with all comments (`gh issue view N --comments`). Follow every rule there (file-size cap, I/O only in `io`, one test executable, no explanatory comments, test plan before tests, red/green evidence).
- Claim: `gh issue edit N --add-label in-progress`.
- Branch `<type>/<N>-<slug>` from origin/main, where <type> is one of feat|fix|docs|chore|refactor|test|ci|build|perf (repo policy `branches.name_pattern`; `fast/` is rejected) (`git fetch origin && git switch -c <type>/<N>-<slug> origin/main`). Every commit signed off (`git commit -s`). Bring in main only with `git merge --signoff origin/main`; never rebase or force-push.
- TDD where a cheap test exists: write the failing test first, run it and capture the red output, then fix and capture green. Run the affected crate's tests with `cargo nextest run -p <crate>` (and the repo's lint/format checks named in AGENTS.md) before pushing.
- Scope: deliver only the issue's remaining Done-when items (listed below). No refactors, no speculative abstractions. Smallest change that solves it.
- Open the PR with the tailrocks-repository-skills:tailrocks-pr-create skill: ready (not draft — run `gh pr ready` if needed), body from .github/PULL_REQUEST_TEMPLATE.md, containing `Fixes #N`, the test plan, the red/green output, and each Done-when command with its output, ending with "🤖 Generated with [Claude Code](https://claude.com/claude-code)".
- Then add the `ci:run` label once to trigger the full PR CI (`gh pr edit <PR> --add-label ci:run`). Do not edit the PR body after pushing (it restarts CI); use PR comments instead.
- Watch CI (`gh pr checks <PR> --watch` or poll with gh run view). If a check fails because of your change, fix it with red/green and push (re-add `ci:run` if the label was consumed). If it fails for an unrelated/flaky reason, record the run URL and cause in a PR comment and say so in your report.
- Answer any bot review threads (Codex etc.) with the tailrocks-repository-skills:tailrocks-pr-threads skill: fix real bugs, reply with evidence otherwise.
- Do NOT merge. Do NOT touch files outside your issue's scope. Never commit state/, .audit/, HANDOFF.md, m8.tsv, tj.tsv or 0.
- If the issue turns out to be already done on main, or needs an owner decision, stop, do not open a PR, and report that with evidence (and the exact question).
- Clean up what you created locally (temp dirs, containers) and report it.

Report (≤20 lines): PR URL and head SHA, red/green evidence summary, CI status (Required check result), threads state, anything left.
- Write any PR body or comment file to a unique path you create with `mktemp` (never a fixed name in a shared scratch dir: other agents run in parallel and a shared name has already swapped PR bodies). Before your final push, check `gh pr view <PR> --json body` names the right issue.

## OWNER RULES (2026-11-25, override anything above)
- Never defer. Address every review comment/thread in the PR itself: fix it with a test, or answer it with evidence when the finding is wrong. No "follow-up issue" deferrals, no "out of scope".
- Batch: when several small fixes are related, do them in one PR rather than many.
- Commits end with your own session's Co-Authored-By trailer.
