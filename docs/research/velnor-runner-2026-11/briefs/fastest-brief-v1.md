You are an independent, critical reviewer. Read the final design of "Velnor", a native-Rust GitHub-Actions-compatible CI runner for tailrocks/velnor-new:
- ~/.cache/ci-ambition/pr-worktree/docs/proposed/velnor-runner-architecture.md
- the research folder ~/.cache/ci-ambition/pr-worktree/docs/research/velnor-runner-2026-11/ (decisions.md, delivery-plan.md, owner-decisions.md, evidence.md, experiments/, rust-only-rulings.md).

The question: **is this the fastest possible CI runner for this repository at the same confidence (zero false greens)?** The owner wants to call it "the fastest runner ever". Test that claim honestly.

Write ~/.cache/ci-ambition/panel/fastest-astra/fastest-review.md (120–250 lines) with:
1. ## Verdict (≤ 8 lines): is the claim justified, for this repository, against GitHub-hosted runners, ARC, and the best self-hosted alternatives you know (for example Buildkite-style agents, Bazel/RBE remote execution, namespace-style fast runners)? Name them, and say only what you can support.
2. ## Where the time goes, per stage: the design's numbers, which are measured and which estimated, and the true floor.
3. ## The strongest objections (5–10): each with its evidence, how likely it is, and the measurement that would prove or refute it.
4. ## Missed speed: concrete further gains at the same correctness, with numbers.
5. ## What to measure first to earn the "fastest" claim: a short list of benchmarks with pass criteria.

Rules: read-only except your scratch directory (~/.cache/ci-ambition/panel/fastest-astra). No GitHub writes. Never run rex. Never touch ~/.local/share/mise. Never print secrets.
