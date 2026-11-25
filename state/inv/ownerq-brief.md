# Owner-question re-verification brief (tailrocks/velnor-new)
For each issue in your list (read-only on code; `git -C /Users/donbeave/Projects/tailrocks/velnor-new fetch -q` then inspect origin/main):
1. Read the issue and its comments; find the latest owner question posted (often an "Audit against main" or "Question for the owner" comment).
2. Check whether today's merges (PRs #2770–#2847 merged 2026-11-24/25) already resolved what the question is about, or made the question moot (code removed/replaced, item implemented). Use `git log --oneline origin/main --since=2026-11-24 -- <relevant paths>` and grep.
3. Decide per issue:
   - still-open: the question is still valid -> do nothing (no comment).
   - moot-close: everything is done or obsolete -> close with a comment citing commits/PRs as evidence.
   - question-update: the question still stands but its premise changed -> post one short comment updating it (what changed, the exact question now).
   - actionable: the owner question was answered in the thread by the owner (a comment that explicitly decides it) -> report it; do NOT implement.
4. Ensure `needs-owner` label on still-open/question-update issues.
Output TSV (path given in your prompt): N, decision, evidence/comment URL. Report counts per decision and list actionable ones (≤6 lines). Never print secrets. Max ~150 gh calls.
