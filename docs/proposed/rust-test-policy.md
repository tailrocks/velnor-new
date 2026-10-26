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
Nextest does not execute them.

## Required behavior and test evidence

Tests exist to prove required behavior, not to satisfy a package-count or
coverage quota. Before adding tests for a behavior change, a reviewed plan MUST
identify the observable behavior and real positive and
negative scenarios, record which existing tests already prove them, and explain
why each proposed test is needed. Existing tests satisfy an obligation when
they provide that evidence. Tests SHOULD exercise public APIs and resulting
state first; tests for internal pure logic are appropriate only when the
behavior cannot be reached observably. A failure that exposes incorrect
observable behavior is valid evidence; a failure caused by a malformed
harness, environment, or bad test double calls for diagnosing that cause,
not weakening a valid assertion. Kept tests MUST demonstrate failure for the
broken behavior and success when it is fixed. Kept tests MUST remain
deterministic and hermetic, and MUST NOT mock owned code, as required by
`AGENTS.md`.

Every declared required suite MUST fail when its complete test selection
discovers zero tests. An intentionally filtered or hash-partitioned subrun MAY
allow an empty selection, but the generated task plan MUST select the complete
declared suite, and execution evidence MUST confirm the selected tasks ran.
This rule is scoped to declared suites; it does not impose a test count for
every package.

A test source file alone does not establish a registered suite. When a package
sets `autotests = false` or marks a target `test = false`, the intended Cargo
test target MUST be enabled or explicitly registered, and the generated task
plan MUST select it, with execution evidence confirming that it ran. V1 has
no permanent empty-suite exception except the explicit per-repo
`[stacks.rust] allow_empty_test_suite = true` opt-out defined in the
task-execution contract: an opted-in repository's unsharded `nextest run`
steps emit `--no-tests warn` instead of `--no-tests fail`, and every other
repository keeps the fail-on-empty default.

## Owner-approved test reset

PR #391 records the owner's decision to clear the previous unit, integration,
and doctest suites so they can be rebuilt integration-first. During this
transition, empty test targets remain disabled and generated CI may omit their
test steps. This exception does not change the V1 behavior-based end state,
waive a required behavior, or permit weakening a valid test to make a check
pass. The reset ends when the reviewed required-behavior plan is satisfied by
registered test targets selected by the generated plan, with execution
evidence and independent review of the behavior plan and its red/green
evidence. Replacement coverage MUST be
integration-first: start with public API behavior. Test internal pure logic
only when its behavior cannot be reached observably, as described above. New
tests MUST meet the red-green, no-owned-mocks, observable-behavior,
hermeticity, and reviewed-plan rules in `AGENTS.md`.

Tests MUST assert observable behavior, boundaries, and failure paths using
independent fixtures. They MUST NOT merely call the same helper used by the
implementation and compare its output with itself. Snapshot changes require
review. `proptest` SHOULD cover parsers, planners, and invariants.
