# Velnor runner research: design feedback brief v1 (2026-11-19)

The panel is done. You were one of eight models that each answered the brief `~/.cache/ci-ambition/panel/brief-v1.md` independently, then critiqued three anonymised proposals. The manager has now drafted the **final design** and the **decision log** from all of that work. Before the design is finalized, each panel member reviews the draft independently.

The owner's direction for this round: **be more ambitious. Think bigger and think differently. Focus on the hardest problems, including ones that sound impossible.** The design must still keep the SAME confidence as today. Ambition never means cutting verification a change actually needs. But "it's hard", "it needs a rewrite in Rust" or "GitHub doesn't do it that way" are not reasons to give up on an idea. The owner accepts instability, extra work and protocol breakage in exchange for speed.

## Inputs (read them)

- The draft final design: `~/.cache/ci-ambition/panel/draft/velnor-runner-architecture.md`
- The draft decision log: `~/.cache/ci-ambition/panel/draft/decisions.md`
- The comparison of the eight proposals: `~/.cache/ci-ambition/panel/draft/comparison.md`
- The evidence pack: `~/.cache/ci-ambition/panel/evidence.md`. Erratum: the Rust toolchain is NOT baked into the runner image (velnor-runners `state/RESULT.md:115`).
- All eight proposals and reviews, now named: `~/.cache/ci-ambition/panel/draft/proposals/*.md` and `~/.cache/ci-ambition/panel/draft/reviews/*.md`. Your own proposal is among them.
- Code, read-only: `~/Projects/tailrocks/velnor-new`, `~/.cache/ci-ambition/refs/{velnor-legacy,actions-runner,arc}`. Verify the load-bearing claims you dispute.

## What to write

Write `design-feedback.md` in your scratch directory (the current working directory) with these `##` sections:

1. **Verdict.** In 5 lines or fewer: would this design reach the minimum push-to-merge time at the same confidence? Score it 1–10 on ambition and 1–10 on soundness, with one sentence each.
2. **Correctness and confidence holes.** Find what is wrong, unproven, or would silently lower confidence, slow things down, or break. Cite evidence (`path:line`, an evidence section, a run id). Give each a severity and a concrete fix.
3. **Where the design is too timid.** Find the places that tune a step instead of removing it, or settle for a safe target. For each: the bolder alternative, the mechanism, the expected numbers with their arithmetic, the risk, how confidence stays the same, and the cheapest experiment that proves or kills it within days.
4. **The hardest problems.** Pick the 3–5 hardest or seemingly impossible problems that stand between today and a PR verdict in about a minute with zero queue. Examples: breaking the 200–270 s single-test floor; zero cold start; reusing verdicts across the PR, the merge queue and main without trusting anything unproven; job placement GitHub doesn't let you control; CI starting before the push; flake-free required checks; the shared cluster's 92–95% CPU. Give your best concrete attack on each, not just a description.
5. **Decisions you would overturn.** List the entries in the decision log you disagree with, by id, with the evidence.
6. **Targets.** The numbers you would commit to, and the stretch numbers, per phase: PR verdict P50/P90, push-to-merge P50/P90, runner-minutes per merged PR, cache hit %.
7. **Anything missing.** Name what no one has considered yet.

Aim for 150–400 lines. Be specific, quantitative and blunt. Print the file as your final message too.

## Rules (binding; same as before)

- Read only, plus your scratch dir. No pushes, merges, comments, issues, or GitHub or cluster changes.
- Read-only kubectl verbs only.
- Never print secrets or kubeconfigs.
- Never run `rex`. Never touch `~/.local/share/mise`. Never use Grok. Kill processes by PID only.
