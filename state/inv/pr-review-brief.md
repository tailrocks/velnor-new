First-pass review of a cluster of related open PRs in github.com/tailrocks/velnor-new. Read-only on GitHub: do NOT post reviews/comments, push, merge, close, or edit PRs. Read AGENTS.md and docs/goals.md first (400-line file cap, I/O only in `io`, one test executable, no explanatory comments, test plan before tests, red/green evidence).

For each PR in your cluster:
1. `gh pr view N -R tailrocks/velnor-new --json title,body,headRefName,headRefOid,baseRefName,isDraft,commits,files,statusCheckRollup,reviews` and `gh pr view N --comments`; read the linked issue(s) and whether they are still open.
2. Use the tailrocks-repository-skills:tailrocks-review-pr skill on the PR (do not post). Judge: correct, tested (red/green), a real improvement vs churn/speculative abstraction/noise; whether it is still needed on current main (has main already delivered it another way? `git fetch origin; git log origin/main --grep`); whether it overlaps/duplicates another PR in the cluster (and which one should carry the work).
3. List what stands between it and merge: conflicts (`git merge-tree --write-tree origin/main origin/<head>`), failing required checks (name check + 1-line cause from `gh run view --log-failed` if cheap), DCO, unresolved review threads (count + whether they are real bugs), draft status, unchecked Done-when items.
4. Recommendation: KEEP (and the exact work list: refresh, fix X, answer threads Y, un-draft), REWORK (substantial changes, describe), SUPERSEDED-BY #M, REJECT (reason), or HUMAN (exact owner question).

Append one row per PR to the TSV path given, columns: pr, recommendation, merge_blockers, work_list(≤250 chars), reason(≤250 chars), overlaps.
Return a compact table (one line per PR) plus a suggested landing order for the cluster. ≤40 lines. Clean up anything you create.
