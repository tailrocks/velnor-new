# Velnor: what to borrow from other CI runners (brief v1, 2026-11-19)

Improve the design of Velnor (a native-Rust, GitHub-Actions-compatible CI runner for tailrocks/velnor-new) using what other well-known runners do. This is design work, not a benchmark.

Read:
- The landscape dossier: `~/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/runner-landscape.md` (public facts with citations).
- The final design: `~/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md`.
- The owner decisions: `.../velnor-runner-2026-11/owner-decisions.md`.
- The fastest-runner critiques and their comparison: `.../fastest/*.md` and `fastest-comparison.md`.
- Use your own public knowledge of these systems too; mark anything not in the dossier as "from my knowledge" and keep it factual.

Binding: owner decisions 1–24, including Rust only with two switchable executors, GitHub kept as the platform and its merge queue, correctness first with zero false greens, the label trigger flow, Kubernetes and Docker backends, configurable storage, node mirrors, migrate-and-delete, and nothing final. The hardware is fixed for now: 5 × EPYC 9454P (96 threads, 128 GB, NVMe/XFS) in two sites; buying more is allowed later.

Write `landscape.md` in your scratch directory (the current working directory), 150–350 lines:
1. `## Verdict` (≤ 8 lines): where Velnor's design is already ahead of each class of system, and where it is behind.
2. `## Ideas to borrow`: a table, then one paragraph each. Columns:
   - id (B01…);
   - technique;
   - who does it (system names);
   - what changes in Velnor (the design section);
   - expected gain, with arithmetic or a cited basis;
   - the correctness argument (why it can't cause a false green);
   - fit with owner decisions (fits / needs an owner question);
   - cost in agent-hours.
   Cover at least:
   - warm-pool and webhook provisioning;
   - forked-per-job volumes vs reflink snapshots;
   - sticky disks;
   - whole-sandbox snapshot restore;
   - cache proxying of stock actions (relevant to the GitHub Official executor);
   - co-located caches;
   - high-clock hardware for the serial floor;
   - REAPI-style action cache, CAS and FindMissingBlobs vs Velnor's ledger;
   - test splitting by timing;
   - flake quarantine;
   - test impact analysis and ML selection, only if correctness can be kept;
   - merge-queue batching, bisection and fast-forward;
   - optimistic merge with asynchronous tests, which likely conflicts with correctness-first: say so;
   - git mirror locking.
3. `## Ideas to reject`: what NOT to copy, and why (correctness, decisions, fit).
4. `## Where Velnor should lead`: what no listed system does that Velnor's design does, and how to make it stronger.

Rules: read-only except your scratch dir. No GitHub writes. Never run `rex`. Never touch `~/.local/share/mise`. Never print secrets.
