Read-only verification of open GitHub issues in tailrocks/velnor-new against current main. Clone: ~/Projects/tailrocks/velnor-new (do NOT check out anything or modify the working tree; use `git fetch origin`, `git show origin/main:<path>`, `git grep <pat> origin/main`, `git log origin/main`). Do NOT comment, close, label or edit anything on GitHub.

For each issue number in your batch:
1. `gh issue view N -R tailrocks/velnor-new --comments` — read the body, Done-when / checklist, and comments (owner decisions, "blocked on" notes).
2. Search for delivery: `git log origin/main --oneline --grep '#N'`, `gh pr list -R tailrocks/velnor-new --state all --search 'N in:body' --json number,state,title,mergedAt`, and grep main for the code/test/doc the issue asks for.
3. Classify:
   - FIXED: every Done-when item is delivered on main. Cite the merged PR/commit for each item. Be strict; if any item is missing, it is not FIXED.
   - PARTIAL: list which items remain.
   - OBSOLETE: the code/feature it targets no longer exists or a later decision dropped it (cite).
   - DUPLICATE: another open/closed issue covers it (cite).
   - IN-PR: an open PR targets it (cite).
   - BLOCKED: waits on a named issue/PR/tag/owner step (name it; if the issue is labelled blocked but you can find no live blocker, say BLOCKER-GONE with evidence).
   - NEEDS-HUMAN: needs an owner decision; write the exact one-sentence question.
   - ACTIONABLE: concrete remaining work; give size S/M/L, area dir, tier (sonnet/medium small obvious; sonnet/high normal; opus/high or opus/xhigh for security/concurrency/architecture), and a 1-line scope of what remains.
4. Append a row to the output TSV (path given below), columns: number, class, evidence(≤200 chars, PR/commit refs), remaining_or_question(≤200 chars), size, area, tier.

Return only: counts per class and the list of numbers per class (≤25 lines).
