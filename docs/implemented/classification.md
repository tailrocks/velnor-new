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
| Version-policy mirror + inventory | `.velnor/version-policy.toml`, `.velnor/freshness-inventory.json` | repo maintainers | `repo_policy_mirror_matches_compiled_catalog`; `scripts/check-freshness.sh` |
| Alint baseline | none — no `.alint-baseline.json` at HEAD | repo maintainers | §5 limits are hard errors; no grandfathering ([update-procedure.md](update-procedure.md)) |
| Test fixtures (committed inputs) | `fixtures/**` | owning crate's tests | Excluded from Alint's walked index (`.alint.yml` `ignore:`); each fixture dir carries a `README.md` stating intent |
| Generated CI tree | `.github/workflows/ci.yml`, `.github/actionlint.yaml`, `.github/AGENTS.md` | generator (`velnor-actions` binary, generator version in header marker) | First-line version marker, no dates; regen must be byte-identical (RQ-1.9); validated by actionlint+zizmor before replace; inventory/condition edits are security-sensitive and review-gated (repo `CODEOWNERS` + branch protection — D7: the merge cannot distinguish regen from tampering, review is the trust root) |
| Retired generator-owned paths | `.github/CLAUDE.md` | generator (retired, never re-emitted) | No longer emitted; the preserve-copy skips retired paths so the whole-tree swap deletes stale copies on regen |
| Generator preview output | `$RUNNER_TEMP/.../.github/**` (never committed) | ephemeral, CI job | Byte-compared against in-place render; outside repo root |
| Vendored code | none in V1 | — | No vendored sources exist. If added, each needs a row: upstream, pinned revision, sync procedure |
| Build artifacts | `target/`, `mutants.out/` (gitignored, never committed) | ephemeral | Not classified: never reviewed, never shipped |

## Notes

- Counts for §5 line limits are physical lines including comments and
  blanks; only the classes above are outside the handwritten budget.
- `mise.lock` is intentionally absent: it is repo-owned and Velnor MUST NOT
  create or refresh it (RQ-2.12). If maintainers adopt it, add a row here.
- `.velnor/generator.lock` / `.velnor/runner.lock` do not exist yet (pre-seed
  / deferred V2). Their rows are pre-declared in
  [update-procedure.md](update-procedure.md) and move here on creation.
