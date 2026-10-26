# File classification registry

Living registry for RQ-4.2 / RQ-5.2: every lockfile, fixture, vendored, and
generated file class in this repository has an explicit classification and
owner. Handwritten product code is the default class and needs no entry.

No entry here may be used to relabel handwritten code as generated, raise a
§5 limit, or add an arbitrary exclusion (RQ-5.3). Adding a row requires
review; the row must name a verification rule that actually runs.

## Classes

| Class | Paths | Owner | Verification |
|---|---|---|---|
| Cargo lockfile | `Cargo.lock` | repo maintainers (`CODEOWNERS`) | `cargo metadata --locked` in CI; `scripts/check-freshness.sh` declared-vs-locked probe (RQ-2.11) |
| Generator bootstrap lock | `.velnor/generator.lock` | repo maintainers | required-verdict generator validates the official release manifest, supported targets, and pinned Mise catalog |
| Version-policy mirror + inventory | `.velnor/version-policy.toml`, `.velnor/freshness-inventory.json` | repo maintainers | `repo_policy_mirror_matches_compiled_catalog`; `scripts/check-freshness.sh` |
| Alint baseline | none — no `.alint-baseline.json` at HEAD | repo maintainers | §5 limits are hard errors; no grandfathering ([update-procedure.md](update-procedure.md)) |
| Test fixtures (committed inputs) | `fixtures/**` | owning crate's tests | Excluded from Alint's walked index (`.alint.yml` `ignore:`); each fixture dir carries a `README.md` stating intent |
| Generated CI tree | `.github/workflows/ci.yml`, `.github/actionlint.yaml`, `.github/AGENTS.md`, `.github/CLAUDE.md` | generator (`velnor-actions` binary, generator version in header marker) | First-line version marker, no dates; regen must be byte-identical (RQ-1.9); validated by actionlint+zizmor before replace; inventory/condition edits are security-sensitive and review-gated (repo `CODEOWNERS` + branch protection — D7: the merge cannot distinguish regen from tampering, review is the trust root) |
| Generator preview output | `$RUNNER_TEMP/.../.github/**` (never committed) | ephemeral, CI job | Byte-compared against in-place render; outside repo root |
| Vendored code | none in V1 | — | No vendored sources exist. If added, each needs a row: upstream, pinned revision, sync procedure |
| Build artifacts | `target/`, `mutants.out/` (gitignored, never committed) | ephemeral | Not classified: never reviewed, never shipped |

## Notes

- Counts for §5 line limits are physical lines including comments and
  blanks; only the classes above are outside the handwritten budget.
- `mise.lock` is repo-owned and tracked after the authorized toolchain
  migration; Velnor only reads it and MUST NOT create or refresh it (RQ-2.12).
- `.velnor/generator.lock` is committed and records authenticated Velnor 0.1.55
  from source commit `693f3aae14406fb7851ac70f0aee647a869b5063`. This bootstrap
  identity does not establish hosted qualification of current source. The
  `.velnor/runner.lock` remains deferred V2; its row is pre-declared in
  [update-procedure.md](update-procedure.md).
