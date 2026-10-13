# Recorded deviations (SHOULD rows)

Per `docs/README.md`, a SHOULD recommendation may be changed only by
recording the reason and impact. Each entry names the row, the chosen
behavior, and why. MUST / MUST-NOT halves are unaffected and still bind.

## RQ-4.1 — test layout (SHOULD: `src/<unit>/tests.rs`)

Tests live in `crates/*/tests/impl_*.rs` behind a single `[[test]]` entry
point per crate, not in `src/**/tests.rs` modules.

- Reason: single-binary layout keeps production files free of test code and
  gives one deterministic test target per crate (`autotests = false`).
- Impact: none on enforcement — V1 MUST NOT enforce layout via a custom
  parser, and `.alint.yml` explicitly disclaims layout enforcement
  (MUST-NOT halves honored).
- Status: accepted deviation, no expiry.

## RQ-4.3 — integration entry naming (SHOULD: `tests/integration.rs` + `tests/cases/`)

Entry points are named `tests/velnor_<crate>.rs` with case files as
`tests/impl_*.rs` siblings, not `tests/integration.rs` + `tests/cases/`.

- Reason: per-crate entry names stay unique and greppable in a 7-crate
  workspace; `impl_*.rs` siblings avoid an extra directory level.
- Impact: none — exactly one test binary per crate; the MUST-NOT half (no
  binary per case) is satisfied via `autotests = false` + one `[[test]]`.
- Status: accepted deviation, no expiry.

## RQ-4.7 — `proptest` (SHOULD: cover parsers/planners/invariants)

No `proptest` dependency yet; parsers and planners are covered by
hand-written boundary fixtures instead.

- Reason: dependency set is still minimal; property tests are scheduled with
  the risk-triggered verification rollout
  ([verification-triggers.md](verification-triggers.md)).
- Impact: parsers/planners rely on example coverage until then.
- Status: temporary; revisit when the first risk trigger fires or at V1
  code-complete, whichever is first.

## RQ-2.12 — `mise.lock` absent (no deviation)

`mise.lock` does not exist at HEAD. This is the specified state: the file is
optional and repo-owned, and Velnor MUST NOT create or refresh it.
Recorded here only to close the audit row explicitly.

## VER-0.1 — version-policy file shape (residual gap, not a deviation)

`.velnor/version-policy.toml` now carries the §0 header (`schema`, `channel`,
`check_interval_hours`, `max_exception_days`) plus the catalog-mirror
sections. The fuller §2 registry/source-rule prose lives in the normative
spec, not duplicated into the file. No deviation is claimed: the file holds
every machine-checked field, and the mirror equality test still passes.
