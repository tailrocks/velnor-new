# Velnor runner research: final design review brief v1 (2026-11-19)

You are reviewing the FINAL design of "Velnor", a native-Rust, GitHub-Actions-compatible CI runner for tailrocks/velnor-new. The owner's goal is the shortest possible push-to-merge at the SAME confidence: correctness first, so fast but incorrect is wrong, and false greens have zero tolerance. Your review is posted on the research pull request and decides the last fixes before the owner approves the plan.

## Read
- The final design: `~/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md`.
- Its decision log: `~/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/decisions.md`.
- The owner's binding decisions:
  - 1–12: `~/.cache/ci-ambition/panel/rust-only-brief-v1.md`;
  - 13–24: `~/.cache/ci-ambition/manager/owner-decisions-late.md`.
- The rulings on contested facts: `~/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/rust-only-rulings.md`.
- The delivery plan: `.../delivery-plan.md` (same folder).
- The evidence and experiments: `.../evidence.md` and `.../experiments/*.md` (same folder).
- The proposals, reviews, feedback and Rust-only redesigns in the same folder, as needed.
- Code, read-only: `~/Projects/tailrocks/velnor-new`, `~/.cache/ci-ambition/refs/{actions-runner,arc,velnor-legacy,mr-boxington-cache,rustfs}`.

## Check, in this order
1. **Correctness holes.** Any path to a false green: gate bypasses, reuse keys that miss an input, stale caches or mtimes, the label flow, merge-queue settings, executor switching, storage tiers, mirrors. Severity: critical, high, medium or low.
2. **Contradictions with the owner's decisions 1–24,** or internal contradictions between sections, numbers or phases.
3. **Unsupported numbers.** Targets or estimates whose basis is missing or wrong against the evidence.
4. **Missed speed.** Where the design is still slower than it needs to be at the same correctness, and the concrete fix.
5. **Executability.** Could agents build this from the document and the delivery plan without guessing? Name the gaps.

## Write `final-review.md` in your scratch directory (the current working directory)
- `## Verdict`: ≤ 8 lines. Is it ready for the owner to approve? What must be fixed first?
- `## Findings`: a numbered list ranked by severity. Each finding has:
  - its severity;
  - the design section;
  - what is wrong;
  - the evidence: a `path:line`, a quote, a run or a measurement;
  - the exact fix to make in the text.
- `## Things done well`: ≤ 6 bullets. Only what you'd defend.

Aim for 80–250 lines. Be specific and blunt.

## Rules (binding)
- Read-only everywhere except your scratch directory.
- No GitHub writes: the manager posts your review.
- Read-only kubectl verbs only.
- Never print secrets. Never run `rex`. Never touch `~/.local/share/mise`. Kill processes by PID only.
