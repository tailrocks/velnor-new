# Velnor CI performance package

Review date: 4 October 2026.

## Start here

Read **analysis.md** for the diagnosis and measured comparisons. Read **specification.md** for the testable requirements. Use **goal.md** to start implementation. Keep all files together.

The goal assigns **GPT-6-Luna max** to implementation roles and **GPT-6.1-Sol medium** to all review roles. It requires aggressive delegation, small frequent commits, autonomous decisions, and fast verified releases.

## Contents

| File | Purpose |
| --- | --- |
| analysis.md | Findings, timings, causal limits, and release order |
| specification.md | Twelve requirement groups, tests, cache/seed contracts, and performance targets |
| goal.md | Copy-ready autonomous implementation goal |
| implementation-checklist.md | 91 evidence-backed completion items |
| implementation-checklist.csv | Editable completion ledger; all items start OPEN |
| branch-inventory.csv | All 20 reviewed branch refs and proposed dispositions |
| job-timings.csv / job-timings.json | 43 observed job records with normalized timestamps and source URLs |
| paired-job-timings.csv | Matched hosted/local job times; nonterminal/cancelled ratios remain empty |
| phase-timings.csv | Selected archive and heavy-job phase timings; nested phases are identified |
| evidence-index.md | Exact source revisions, primary evidence, and known limits |
| SHA256SUMS | Integrity hashes for the files in this package |

## Important limits

This package records research and CI performance evidence. It does not prove that source changes were merged, products were released, or a host was updated. Check the evidence index for later run refreshes.

The first capture was **37178675286, attempt 1**. Its job response was nonterminal. E18 records the later completed **attempt 2**. Keep the two attempts separate. Every duration is tied to a job or log. The CSV files do not claim a controlled hardware benchmark or an achieved speedup.

Do not add nested compilation/test phases to their containing Nextest step. Do not treat a missing measurement as zero. Do not treat source-only fixes or an open PR's test claims as deployment proof.

The performance budgets in the specification are proposed acceptance targets. The goal stays open when a mandatory target or test has no passing evidence.
