# Rules

Shared working agreement: keep this section byte-identical across projects. Project-specific rules live under `## This project` below.

- No legacy code. Finish every migration: remove old paths completely — no compatibility shims, aliases, or deprecation periods. Breaking changes are preferred.
- Research project: unsafe, breaking changes expected, never production-ready. Break things when needed; deliver fast.
- Verification gates: verify from the repo root before committing or merging. Run tests with cargo nextest, never cargo test.
- Tests are not forbidden. You must write tests wherever you add or change required behavior.
- Never write a test without a reviewed plan first: an independent subagent must confirm which required behavior it proves, which real positive and negative scenarios it covers, and why existing tests cannot cover them.
- Write tests only to prove required functionality works and keeps working. Forbid tests written for coverage numbers or KPIs.
- Test integration-first through public APIs, asserting resulting state, not interaction sequences. Test internals only for pure logic unreachable observably.
- Forbid mirror-logic assertions and unreviewed snapshots.
- Never mock owned code. Mocks and fakes are permitted only at unowned external I/O (network, clock, filesystem, OS services) behind interfaces you own.
- A red test must name the broken user-observable behavior. A red caused by a reshaped abstraction or a bad double is a bad test: fix the test. Analyze an expected behavior change before updating its tests.
- Prefer fewer high-signal tests over many low-signal tests. Forbid judging test quality by count or coverage %.
- When an issue escapes, analyze why current tests missed it first: spawn an architect subagent to rethink coverage conceptually, then write or refactor the test the analysis designs. Forbid appending a test without that analysis.
- Keep a test only if observed red on broken code and green on fixed code. Every kept test must pass independent subagent verification: fails on broken code, no mocks of owned code, hermetic and deterministic.
- Keep tests hermetic and deterministic: seeded randomness, fake clocks, no live network. Quarantine flakes immediately; never skip, weaken, edit, or delete a test to go green.
- Run narrow tests per change and the full relevant suite before done.
- No comments in code. Code is the source of truth; a comment creates ambiguity about what is right, code or comment. Keep a comment only when functionality requires it: SPDX license headers for REUSE compliance, clap `#[command]`/`#[arg]` help text and similar string literals consumed by tools, parser directives such as Dockerfile `# syntax=`, and machine-generated files that the repo's own generators verify byte-identical. Never keep explanatory, TODO, or SAFETY comments; when code seems to need explanation, rewrite the code instead.
- Judge correctness, consistency, and project fit. Never defer a known-wrong state for ROI, cost, effort, or edge cases.
- Stop only when the required change is proven impossible with the available tools or model. When uncertain, inspect, test, and measure first.
- Before fixing a bug, identify why the architecture permitted it and whether the same structure permits related bugs. Prefer fixes that remove the enabling condition; use a symptom-layer patch only when the root fix is proven infeasible or belongs in a separate change, and name the deferred root cause.
- Delegate first: use subagents for parallel research, implementation, review, and independent verification. Resolve ambiguity autonomously from evidence and project docs.
- Commit verified changes frequently; push regularly. Prefer one branch; add another only for safe isolation.
- Before every PR merge, read all reviews, comments, replies, and unresolved or outdated threads. Use independent subagents to verify findings against code, tests, docs, and recorded decisions.
- For accepted feedback: fix, verify, commit, push, and reply with the fixing commit URL before resolving. For rejected feedback: reply with evidence and rationale before resolving. Re-fetch feedback at the final head SHA. Merge only with zero unaddressed feedback and all required checks and approvals satisfied. Only explicit, PR-specific human authorization permits ignoring identified feedback.
- Keep instructions lean; explanations, plans, and progress belong in documentation, not here.

## This project (velnor-new)

- Hard decisions: read [`docs/hard-decisions.md`](docs/hard-decisions.md) first. Never silently change a hard decision; changing one requires the human owner's explicit approval.
- Sources of truth for GitHub Actions: [actions/runner](https://github.com/actions/runner), [actions/scaleset](https://github.com/actions/scaleset), [actions/runner-images](https://github.com/actions/runner-images). Check the original source for compatibility and record it in [`docs/proposed/github-actions-reference.md`](docs/proposed/github-actions-reference.md). Never guess protocol contracts or operational semantics.
- Requirements and adopted contracts: read `docs/reviews/pr-1.md`, `docs/reviews/pr-1-adoption.md`, and `docs/reviews/pr-1-disposition.md`. Specifications live in `docs/proposed/*` (OpenTofu: `docs/proposed/opentofu-contract.md`); procedures live in `docs/implemented/*`. Pins and exceptions: `.velnor/version-policy.toml` and `.velnor/freshness-inventory.json`.
- Cache: read `docs/proposed/cache-decisions.md` and `docs/proposed/cache-contract.md` before touching cache keys, families, saves, or pruning.
- Quality: enforced by CI and compiler gates; see `docs/proposed/rust-quality-contract.md`.
- Verification gates: exact commands, purposes, and usage: `docs/implemented/repo-verification.md`.
