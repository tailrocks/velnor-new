# Risk-triggered verification (RQ-9.8)

Coverage numbers are NOT behavioral evidence. When a change touches
high-risk logic, the reviewer MUST trigger the applicable technique below
before approval. No new infrastructure: manual runs with pinned tools only.

## Triggers and techniques

| Trigger (touched code) | Technique | How |
|---|---|---|
| Parsers: `contract/src/config/**`, `orchestrator/src/plan.rs`, `rust/src/metadata.rs`, `mise/src/lock.rs` | Mutation testing | `cargo mutants` (scope: `.cargo/mutants.toml`); surviving mutants MUST be killed or justified |
| Selection: `orchestrator/src/select.rs` | Mutation testing | Same run; selection mutants are release-blocking |
| Aggregation: `orchestrator/src/cover.rs`, `merge.rs` | Mutation testing | Same run |
| Parsers/planners/invariants, new grammar or ID rule | Property testing | `proptest` cases (see RQ-4.7 deviation: scheduled, not yet a dependency) |
| Untrusted input decoding (manifest bytes, plan JSON) | Fuzzing | `cargo-fuzz` target, short campaign; corpus kept with the crate |
| Unsafe or concurrency primitives (none in V1) | Miri / Loom | Required before ANY `unsafe` or threading lands; V1 forbids both |
| Public API change in `contract` | `cargo-semver-checks` | MUST pass before merge |

## Rules

- A trigger fires on TOUCH, not on suspicion: editing a listed file
  requires the technique, even for "trivial" edits.
- Surviving mutants, fuzz crashes, or semver violations block the change
  like a test failure.
- Before ANY of these tools runs in CI or on a schedule, its exact version
  MUST be pinned in the Mise-managed inventory (version-policy §2). Today
  all runs are manual; no CI wiring exists.
- Record the run (tool, version, scope, result) in the change description.
