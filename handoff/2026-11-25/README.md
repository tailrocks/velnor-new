# Goal handoff 2026-11-25

Sanitized copy of the coordinator's local working files, kept so nothing lives only on one machine. THIS BRANCH IS NEVER TO BE MERGED. Do not open a PR from it.

- `state/final-report.md`: final report of the finalize goal (also the body of the pinned handoff issue).
- `state/goal-ledger.tsv`: decision ledger. `state/triage.md`, `state/codex-jobs.tsv`: triage notes and job log.
- `state/inv/`: inventories and briefs: branches, issues, PR sweeps and reviews, deferred threads, owner questions, verification batches, salvage list (if present).
- `.audit/`: audit trail, including `velnor-roadmap.tsv` and the `cxrv*` review notes.
- `HANDOFF.md`, `m8.tsv`, `tj.tsv`, `0`: top-level handoff notes and small logs.
- `tools/land.sh`, `tools/land-risk.sh`: the lander scripts used to merge PRs.

Sanitization: private organisation, repository and peer-session names are replaced with aliases (`example-org`, `example-repo-N`, `peer-session-A/B/C`); the home path prefix is replaced with `~`; the secret scan found no credentials. `scripts/private-names-guard.py` passes on this tree.
