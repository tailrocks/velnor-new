# Rules

## V1 generator boundaries

- V1 is a workflow generator: `plan`/`generate` turn repository evidence into CI workflows. It is not a runner, interpreter, or second task graph.
- Do not implement runner behavior in V1 generator crates. The active runner spec is `docs/proposed/macos-scaleset-runner.md` (nested `crates/velnor-runner`). Conflicting deferred clauses are superseded. `actions/runner` and `actions/scaleset` are the protocol sources of truth: match them, never guess.
- No legacy code. Finish every migration: remove old paths completely—no compatibility shims, aliases, or deprecation periods. Breaking changes are preferred.
- Research project: unsafe, breaking changes expected, never production-ready. Break things when needed; deliver fast.

## Source of truth

- Requirements: `docs/reviews/pr-1.md`, adopted in `docs/reviews/pr-1-adoption.md`, tracked in `docs/reviews/pr-1-disposition.md`.
- Contracts: `docs/proposed/*`; implemented gates and procedures: `docs/implemented/*`.
- OpenTofu: `docs/proposed/opentofu-contract.md`, tracked in `docs/reviews/opentofu-evidence.md`.
- Pins and exceptions: `.velnor/version-policy.toml`, `.velnor/freshness-inventory.json`.

## Verify from the repo root

- `cargo fmt --all -- --check`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo nextest run --locked --workspace` (fallback: `cargo test --locked --workspace`)
- `alint validate-config && alint check --fail-on-warning`
- `cargo deny check`
- `bash scripts/check-freshness.sh`

## Proof invariants

- Effective edition 2024 in every crate (`edition.workspace = true`; root sets `edition = "2024"`); MSRV 1.98 resolves everywhere.
- `unsafe` forbidden; `Result` never ignored; no `unwrap`/`panic!`/`todo!` in product code; lints inherited purely (`[lints] workspace = true`).
- Locked resolution: `--locked` everywhere, exact `=x.y.z` pins, registry-only sources; `Cargo.lock` committed.
- Size gates: 400 lines per file, 150 for `lib.rs`/`main.rs`, 80 per function. No baseline or ratchet: split, never grandfather.
- This file stays under 100 lines / 16 KiB (alint-enforced); details live in `docs/`.

## Work principles

- Judge by correctness, consistency, and goal fit. Never defer a known-wrong state for ROI, cost, effort, or edge-case claims.
- Stop only at a proven tool/model/project limit. When uncertain, inspect, test, and measure first.
- Before fixing a bug, find why the architecture permitted it and whether relatives hide nearby. Prefer structural fixes that remove the enabling condition; a symptom patch must name the deferred root cause.
- Delegate first: use subagents for parallel research, implementation, review, and verification. Resolve ambiguity autonomously from evidence and docs.
- Velnor execution, research, implementation, and testing agents use `gpt-6-luna` with `max`; independent reviews use `gpt-6.1-sol` with `medium`. Never use `gpt-6-astra`.
- Every spawn sets `model` and `reasoning_effort`, and passes `fork_turns: "none"`. Verify the actual latest `turn_context` bound to the exact spawn/handle; never resume actors launched under another model.

## Commits and review integrity

- Commit meaningful, verified changes frequently with DCO signoff (`git commit -s`); push regularly. Prefer one working branch; merge small PRs promptly after all gates pass.
- Before every PR merge, read all reviews, comments, replies, and unresolved or outdated threads. Verify findings with independent subagents against code, tests, docs, and recorded decisions.
- Accepted feedback: fix, verify, commit, push, and reply on GitHub with the fixing commit URL before resolving. Rejected feedback: reply with evidence and rationale before resolving. Address general comments in linked PR replies. Never delete feedback or resolve it without a justified disposition.
- Re-fetch feedback at the final head SHA. Merge only with no unaddressed feedback or unresolved threads and all required checks and approvals satisfied. Only explicit, PR-specific human authorization waives identified feedback.
- Keep agent instructions lean. Put explanations, plans, and progress in documentation, not here.

## Subagent model policy

- Implementation, execution, and implementation-oriented research use exactly `gpt-6-luna` with `max` reasoning effort.
- Review, verification, and independent assessment use exactly `gpt-6.1-sol` with `medium` reasoning effort.
- Do not use fallback, automatic routing, or model substitution. If the exact model and effort cannot be guaranteed, stop and report.
- Keep implementation and review separate; substantive work requires independent review. Follow the full role, delegation, and compliance rules in [the mandatory subagent model policy](docs/implemented/subagent-model-policy.md).
