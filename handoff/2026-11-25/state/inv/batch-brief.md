# Deferral batch brief (tailrocks/velnor-new)
Owner rules: never defer; address every finding in this PR; batch related fixes into one PR; commits end with your own session's Co-Authored-By trailer. Read ~/Projects/tailrocks/velnor-new/state/inv/pr-work-brief.md and AGENTS.md first.
Source list: ~/Projects/tailrocks/velnor-new/state/inv/deferral-batches.md — take ONLY the items in your named batch.
1. Branch from origin/main (worktree). For each item: open the source thread/issue, re-read current main code; if already handled on main, note evidence and skip; otherwise fix it with a test that is red before and green after (TDD where a cheap test exists).
2. Keep the PR coherent and ≤ ~400 changed lines; if the batch is larger, open a second PR for the remainder rather than dropping items.
3. Helper gates for unreleased helper behaviour: (0,1,116), no marker (scripts/check-helper-gates.sh). Generated files only via `velnor-gen generate` + `check`. No comments in code except existing style.
4. Scoped tests only (`cargo nextest run -p <crate> -E 'test(...)'`; machine is loaded), clippy -D warnings on touched crates.
5. Open PR with tailrocks-pr-create. Body: a checklist of every item with its source thread URL/issue ("Fixes #N" for issues) and how it was addressed. Mark ready (merge.ts ready, then --confirm), add ci:run once, poll Required every 3 min up to 40 min; fix failures. Address any new review threads in the PR.
6. Post a reply on each source thread on the merged PR: "Addressed in #<new PR>: <one line>".
7. Do not merge (an independent review follows). Report ≤4 lines: PR number(s), head, items fixed / already-on-main, Required.
