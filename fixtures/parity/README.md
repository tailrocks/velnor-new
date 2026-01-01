# parity corpus

Intent: refactor-parity goldens (plan text, plan JSON, matrix JSON,
task report, expected set, generated workflow, actionlint config).
Each case holds `input/` (repo fixture) plus `expected/` (goldens).
The comparator normalizes repo path, run key, and generator SHA.

Cases: minimal-cargo, multi-crate (sharded), ignored-stack,
malformed, malformed-ignored.
