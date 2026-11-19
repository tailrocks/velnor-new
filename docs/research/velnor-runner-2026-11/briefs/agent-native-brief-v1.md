# Velnor: reimagine the runner for AI agents (brief v1, 2026-11-19)

The owner's prompt: "Imagine we need to reimagine runners because we have AI. How will runners look? My main point is that we run a lot of things locally. What if we run them through Velnor somehow? Then we can skip the same thing on the remote server, because we already verified it locally."

Context: almost all work on tailrocks/velnor-new is done by AI coding agents, many in parallel. They run `cargo build`, `cargo test` and clippy many times per task on local machines (Apple Silicon Macs) and in worktrees, and then CI runs it all again. Velnor is the native-Rust, GitHub-Actions-compatible runner designed in this research.

Read:
- the final design: `~/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md`;
- the user-experience draft: `~/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-user-experience.md`;
- the owner decisions: `~/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/owner-decisions.md`;
- `explainer.md` and `fastest-comparison.md` in the same folder;
- the code, read-only, as needed.

Binding:
- Owner decisions 1–24. Correctness first: zero false greens; a merge needs a green on the latest commit; GitHub stays the platform and its merge queue.
- Experimental ideas are welcome.
- An idea that conflicts with a decision must say so and become an owner question with the gain at stake. Don't drop it.

Write `agent-native.md` in your scratch directory (the current working directory), 150–350 lines:
1. `## The picture`: ≤ 15 lines: how CI and runners should look when agents do the work.
2. `## Ideas`: a table, then one paragraph each. Columns:
   - id (N01…);
   - idea;
   - mechanism in Velnor (components, commands, receipts, trust);
   - what it removes (duplicate runs, wait time), with numbers or arithmetic;
   - the correctness argument: why it can't cause a false green; the trust tier and the fallback;
   - fit with owner decisions (fits / owner question);
   - cost in agent-hours.
   Cover at least:
   - routing agents' local commands through Velnor, as a CLI, a cargo shim or a daemon, so they produce receipts;
   - running those commands on warm cluster nodes ("remote by default") against local sealed sandboxes;
   - trust tiers for receipts and how many trusted witnesses the gate needs;
   - the arm64 vs amd64 problem;
   - continuous verification of committed-but-unpushed work, which conflicts with decision 12's label flow: frame it as an owner question;
   - agents querying the ledger to avoid duplicate work between agents;
   - flake detection during development;
   - scheduler priority for interactive agent commands;
   - signing and attestation of local results;
   - how the PR verdict becomes mostly a lookup.
3. `## Risks`: how each idea could produce a false green, or waste, and how it is contained.
4. `## The first three steps`: concrete work items that deliver most of the value soonest.

Rules: read-only except your scratch dir. No GitHub writes. Never run `rex`. Never touch `~/.local/share/mise`. Never print secrets.
