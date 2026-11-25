# Re-queue brief: PR ejected after #2828 (helper repin to 0.1.115) merged
1. Wait until the PR leaves the merge queue: poll every 3 min (up to 60 min)
   gh api graphql -f query='{repository(owner:"tailrocks",name:"velnor-new"){pullRequest(number:N){mergeQueueEntry{state}}}}' --jq .data.repository.pullRequest.mergeQueueEntry
   until it prints null. Pushes to a queued branch are refused (GH006), so do not push before that.
2. Detached checkout of the PR head; `git merge --signoff origin/main`. Generated workflows/goldens: never hand-resolve — take either side, then run `velnor-gen generate` (build with `cargo build -p velnor-gen` and use target/debug/velnor-gen if the mise shim is broken) and `velnor-gen check`. If a non-generated conflict is non-trivial, stop and report.
3. Commit message ends with the Co-Authored-By line from your own session attribution reminder (the model you run as). Push without force to the PR branch; remove and re-add label ci:run.
4. Poll `gh pr checks N -R tailrocks/velnor-new` every 3 min (up to 40 min) until Required finishes. If red, read the failed log and fix if caused by the merge (stale golden/pin); flake → rerun once.
5. Do not enqueue or merge. Report ≤2 lines: new head and Required result.

## OWNER RULES (2026-11-25, override anything above)
- Never defer. Address every review comment/thread in the PR itself: fix it with a test, or answer it with evidence when the finding is wrong. No "follow-up issue" deferrals, no "out of scope".
- Batch: when several small fixes are related, do them in one PR rather than many.
- Commits end with your own session's Co-Authored-By trailer.
