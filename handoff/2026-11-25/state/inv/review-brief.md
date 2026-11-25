# Independent review brief (tailrocks/velnor-new)
You did not write these PRs. For EACH PR listed in your prompt:
1. Read AGENTS.md, the PR body, linked issues/threads, and the full diff (`gh pr diff N`).
2. Verify: correctness of each claimed fix; the tests are behavioural and fail on main's code (run the new tests scoped against the base where cheap); no regressions; helper gates (0,1,116) where helper behaviour is unreleased; generated files consistent (`velnor-gen check`); no new comments; scope coherent.
3. Required green on the current head (`gh pr checks N`; ignore cancelled duplicate runs; the advisory `Cache` job may fail from the known runner-cache waste issue — not blocking). No unresolved threads.
4. Owner rule: never defer. If you find a problem, FIX IT YOURSELF in the PR (worktree; detached checkout; push to the PR branch without force; your own Co-Authored-By trailer; red/green test), then re-review your own fix only lightly — your verdict must say what you changed. Only if the fix is large (>150 lines) report it as "changes" with precise items.
5. Post `gh pr review N --comment --body "Independent review and verdict: ... Verdict: merge|changes"`.
Report ≤2 lines per PR: verdict, head, Required. Do not merge.
