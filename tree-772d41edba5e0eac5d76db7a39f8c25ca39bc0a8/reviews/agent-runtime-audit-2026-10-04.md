# Agent runtime audit — 2026-10-04

## Required runtime policy

The updated task instruction sets implementation, research, and test work to
`gpt-6-luna` at `max`. Independent review may use `gpt-6.1-sol` at `medium`.
`gpt-5.6` and `gpt-6-astra` do not satisfy this task policy.

## Incident

Some nested tasks were spawned with full-history forks after the policy changed.
They inherited `gpt-5.6-luna/max`; the prompt labels did not change their runtime.
The affected work was read-only review or consumer research:

- lineage security review;
- full Git-ref grammar audit;
- generated-output preservation architecture and final reviews;
- OpenTofu candidate-diff and caller reviews;
- three early consumer-audit cohorts.

Their reports remain useful investigation leads. They do not count as independent
acceptance evidence. No product-source changes came from these nested tasks.

## Remediation and status

- Active noncompliant review tasks were stopped. Their notes and process evidence
  were preserved; no worktree or user data was discarded.
- Direct implementation lanes were checked against their own latest
  `turn_context`; the active lanes use `gpt-6-luna/max`.
- Consumer cohorts A, B, and C were reissued as `gpt-6-luna/max`; their fresh
  reports, plus the remaining deep-cohort reports, own the current scope audit.
- Required source reviews are being rerun by independent
  `gpt-6.1-sol/medium` reviewers after the exact source commit is frozen.
- The separately completed Jackin deep review used the permitted
  `gpt-6.1-sol/medium` review runtime and remains valid review evidence.

## Verification rule

For every accepted delegated result, bind the exact spawned task to its
`session_meta` agent path and parent thread, then verify the latest `turn_context`
model and effort. A prompt label, inherited parent metadata, or an unrelated
repository JSONL is not runtime proof. Spawn descendants with `fork_turns: none`
and an explicit model and effort.

This is a status record for the 2026-10-04 task. Recheck remaining reviews and
consumer cohorts before final acceptance.
