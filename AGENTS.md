# Rules

- Official GitHub Actions sources of truth:
  - [actions/runner](https://github.com/actions/runner)
  - [actions/scaleset](https://github.com/actions/scaleset)
  - [actions/runner-images](https://github.com/actions/runner-images)
  Always check the original source code for compatibility and make it clear in reference doc files (`docs/proposed/github-actions-reference.md`). Never guess protocol contracts or operational semantics.
- Previous Velnor reference: [tailrocks/velnor](https://github.com/tailrocks/velnor) contains earlier Velnor experiments. Consult its code, designs, tests, and results when they can help the current task. Reuse or adapt suitable code after independent checks against current requirements and contracts. Use that repository as a reference, not an authority. Reject approaches that fail current requirements or have less useful results. Do not copy code or keep old behavior only because that repository contains it.
- No legacy code. Finish every migration: remove old paths completely—no compatibility shims, aliases, or deprecation periods. Breaking changes are preferred.
- Research project: unsafe, breaking changes expected, never production-ready. Break things when needed; deliver fast.
- Requirements and adopted contracts: read `docs/reviews/pr-1.md`, `docs/reviews/pr-1-adoption.md`, and `docs/reviews/pr-1-disposition.md`. Specifications live in `docs/proposed/*` (OpenTofu: `docs/proposed/opentofu-contract.md`), and procedures live in `docs/implemented/*`. Pins and exceptions: `.velnor/version-policy.toml` and `.velnor/freshness-inventory.json`.
- Quality and proof invariants: enforced by CI and compiler gates; read `docs/proposed/rust-quality-contract.md` for quality contracts.
- Work principles:
  - Judge by correctness, consistency, and goal fit. Never defer a known-wrong state for ROI, cost, effort, or edge-case claims.
  - Stop only when a change is proven impossible with available tools or model. When uncertain, inspect, test, and measure first.
  - Before fixing a bug, identify why the architecture permitted it and whether relatives hide nearby. Prefer structural fixes removing enabling conditions; symptom patches must name deferred root causes.
  - Delegate first: use subagents for parallel research, implementation, review, and verification. Resolve ambiguity autonomously from evidence and docs.
- Commits and review integrity:
  - Commit meaningful, verified changes frequently with DCO signoff (`git commit -s`); push regularly. Prefer one working branch.
  - Before every PR merge, read all reviews, comments, replies, and unresolved or outdated threads. Verify findings with independent subagents against code, tests, docs, and recorded decisions.
  - Accepted feedback: fix, verify, commit, push, and reply on GitHub with fixing commit URL before resolving. Rejected feedback: reply with evidence and rationale before resolving. Re-fetch feedback at final head SHA; merge only with zero unaddressed feedback and all required checks passing.
  - Keep agent instructions lean. Put explanations, plans, and progress in documentation, not here.
- Run relevant checks before each commit.
- Run all required checks before release, activation, or merge.
- Use Cargo Nextest for unit and integration tests.
- Use the pinned MBX `test --doc` command for doctests only.
- Read `docs/implemented/repo-verification.md` for the required commands.
