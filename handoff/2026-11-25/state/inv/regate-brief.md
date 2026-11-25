# Re-gate brief: tools-v0.1.116 was tagged (on 5e3c38d6ec, owner choice). Behaviour in your PR gated at (0,1,116) did NOT ship in 0.1.116, so move it to (0,1,117).
1. Detached checkout of the PR head; `git merge --signoff origin/main` (resolve; generated files only via `velnor-gen generate`).
2. Change every helper gate/version for this PR's unreleased behaviour from 0.1.116 → 0.1.117: Rust constants `(0, 1, 116)`, test expectations, docs/CONFIG text, PR body. Do NOT change references to genuinely released 0.1.116 behaviour from main (check `git show origin/main` for what already says 0.1.116 — leave those).
3. `scripts/check-helper-gates.sh origin/main` must pass (no marker for unreleased gates), `velnor-gen generate` + `check`, scoped tests of touched crates, clippy -D warnings.
4. Commit (sign-off, your own Co-Authored-By trailer), push without force (detached → push to refs/heads/<branch>), toggle ci:run ONCE (remove, sleep 30, add), poll Required every 3 min ≤40 min; fix failures caused by the change. Address any review thread in the PR (never defer).
5. Do not merge. Report ≤2 lines per PR: head, Required.
