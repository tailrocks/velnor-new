# Velnor runner research: cross-critique brief v1 (2026-11-19)

You took part in an independent research panel. Every member answered the same brief, `~/.cache/ci-ambition/panel/brief-v1.md`, using the same evidence pack, `~/.cache/ci-ambition/panel/evidence.md`. Re-read the brief's goal, hypotheses and questions before you start.

Your task now is to critically review the THREE anonymised proposals listed in your kickoff. They are copies in your scratch directory under `review-input/`. You don't know who wrote them, and they may come from different models. Judge only what is written. Don't try to identify the authors. Read only the three files given; don't open other panel members' directories.

For EACH proposal, write:

1. **Weakest assumptions.** Name the 3–5 claims the design depends on most that are least supported. Check them against the evidence pack and the code: velnor-new, velnor-legacy, actions/runner and ARC (the paths are in the brief). For each, say what is wrong or unproven and what evidence would settle it.
2. **Factual errors.** List numbers or code facts that contradict the evidence pack or the code. Cite `path:line`, a run id, or an evidence section.
3. **Missed bottlenecks or risks.** Name the time sinks, failure modes or confidence losses the proposal ignores, especially anything that would make push-to-merge slower or confidence lower than claimed.
4. **Ideas worth taking.** Name the specific ideas the final design should adopt, and say why.
5. **Verdict.** Give one paragraph. Would this design reach minimum push-to-merge time at the same confidence? What is its single biggest flaw?

Then write a short **cross-cutting section**. Say where the three proposals agree, where they conflict, and which side the evidence supports. Say which hypotheses (H1–H5) you now judge differently after reading them, if any.

Rules (binding; same as the brief): read only, plus your scratch dir. No pushes, merges, comments, issues or GitHub/cluster changes. Use read-only kubectl verbs only. Never print secrets or kubeconfigs. Never run `rex`. Never touch `~/.local/share/mise`. Never use Grok. Kill processes by PID only.

Deliver: write `review.md` in your scratch directory, the current working directory: `##` headings "Proposal X" for each of the three, then "Cross-cutting". Print the review as your final message too. Aim for 150–350 lines. Be specific and blunt. Cite evidence. Praise only what you would actually adopt.
