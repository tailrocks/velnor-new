# Keep-ready brief: owner-blocked PRs (tailrocks/velnor-new)
These PRs wait on an owner decision. Goal: when the owner answers, they can merge immediately. Do NOT change the PR's substance or answer the owner's question yourself.
1. Read /Users/donbeave/Projects/tailrocks/velnor-new/state/inv/pr-work-brief.md.
2. If it conflicts with its base: `git merge --signoff origin/<base>`; resolve trivially; if non-trivial, stop and report the files. Run `velnor-gen generate` + `check` (never hand-edit workflows).
3. If Required fails on the latest real run: read the log (`gh run view --log-failed`), fix only if the cause is the PR's own code or a stale merge; if it's an unrelated flake, rerun the failed job once.
4. Unresolved threads: list with tailrocks-pr-threads. Bot findings you can verify → fix or answer with evidence and resolve. Threads that ARE the owner question → leave open.
5. If GitHub says BLOCKED with all gates passing (github_disagrees): push one empty signed-off commit.
6. Push (no force), re-add ci:run, wait Required (poll ≤3 min), pr-status. Respect the machine load: rely on CI, don't run the full local test suite.
Report ≤4 lines: head, verdict, what's left (should be only the owner decision). Do not merge.

## OWNER RULES (2026-11-25, override anything above)
- Never defer. Address every review comment/thread in the PR itself: fix it with a test, or answer it with evidence when the finding is wrong. No "follow-up issue" deferrals, no "out of scope".
- Batch: when several small fixes are related, do them in one PR rather than many.
- Commits end with your own session's Co-Authored-By trailer.
