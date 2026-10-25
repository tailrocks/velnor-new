# Velnor V1 Rust test policy

**Status:** Proposed specification, companion to the [Rust quality
contract](rust-quality-contract.md) §4. Landed implementation records live in
`docs/implemented/` and must change in the same implementation pull request.

The words **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are normative.

The preferred test layout keeps test implementations out of production files.
For `parser.rs`:

```rust
#[cfg(test)]
mod tests;
```

The implementation SHOULD be in `src/parser/tests.rs`; tests for `src/lib.rs`
SHOULD be in `src/tests.rs`. V1 does not enforce this layout with a custom Rust
source parser or test-placement linter. Alint cannot inspect Rust syntax, so it
MUST NOT be described as enforcing these rules. Strict test-layout enforcement
is deferred until a separate, approved mechanism is selected. Generated source
MUST still be explicitly classified with an owner and verification rule; it
MUST NOT become an unreviewed escape hatch.

Integration tests SHOULD use a small declared entry point, such as
`tests/integration.rs`, with related cases in `tests/cases/`. Do not create one
Cargo test binary per case file. Doctests MUST run in a separate gate because
Nextest does not execute them. Every Velnor product package MUST have at least
one registered unit or integration test once the owner-approved reset below is
complete; V1 has no permanent empty-suite exception. Outside that reset, the
generated Nextest task MUST pass `--no-tests fail`, so a selected feature
configuration that discovers zero tests fails with Nextest's documented
no-tests exit code.

## Owner-approved test reset

PR #391 records the owner's decision to clear the previous unit, integration,
and doctest suites so they can be rebuilt integration-first. During this reset
only, packages may have no tests. This exception does not change the V1 end
state, waive any requirement for new tests, or permit weakening tests to make
a check pass. Restore fail-on-empty package checks as replacement suites are
added; the reset ends when every Velnor product package has a registered unit
or integration test. Replacement coverage MUST be integration-first: start
with public API behavior, then add focused unit tests for distinct boundaries
or failure paths. New tests MUST meet the red-green, no-owned-mocks,
observable-behavior, hermeticity, and independent-review rules in `AGENTS.md`.

Tests MUST assert observable behavior, boundaries, and failure paths using
independent fixtures. They MUST NOT merely call the same helper used by the
implementation and compare its output with itself. Snapshot changes require
review. `proptest` SHOULD cover parsers, planners, and invariants.
