# Update and exception procedure

Mechanical procedure for version-policy §3 / §4, RQ-2.11, RQ-5.3, VER-2.27,
VER-3.4, VER-3.7, VER-4.4. No updater binary exists: the mechanical maximum
is `scripts/check-freshness.sh` (gates) + Renovate (proposals) + this
procedure (human steps). Nothing here invents automation that is not wired.

## Roles

- Maintainer (see `CODEOWNERS`): reviews update sets, grants exceptions,
  merges pins after qualification.
- Renovate: proposes version changes only; never merges (see `renovate.json`).
- `scripts/check-freshness.sh`: fail-closed gate. Human `ok:` lines plus
  machine-readable `row: {...}` JSON lines (`check`/`subject`/`status`/
  `detail`). Checks: pin-match, `policy-header`, `exception-expiry`,
  `standing-exception`, `lock-staleness`, `lock-mtime` (info only).

## Update set (VER-3.2 / VER-3.3)

1. Renovate (or a release monitor) opens ONE change covering ALL available
   compatible updates as a coherent set. Incompatible updates are reported
   with a required migration — never silently omitted (VER-1.5).
2. The change records freshness timestamp + version delta, and refreshes
   ONLY Velnor-owned pins and locks: `Cargo.lock`,
   `.velnor/version-policy.toml`, `.velnor/generator.lock` (once seeded),
   `.velnor/runner.lock` (deferred V2). It MUST NOT edit `mise.toml`,
   `mise.lock`, or `rust-toolchain.toml` (VER-3.4, VER-4.4); those get
   maintainer recommendations only.
3. Run the full qualification: `cargo fmt --all -- --check`, policy checks,
   Clippy per selected package (`--locked`), tests, doctests, `cargo deny`,
   workflow validation, cache/selection fixtures, platform qualification.
4. Merge new pins only after qualification passes (VER-3.7). Normal builds
   use `--locked` and never resolve versions.
5. Any lock or platform-image change invalidates affected task-result and
   baseline identities (cache contract) — re-baseline deliberately.

## Lock staleness (RQ-2.11)

`Cargo.lock` MUST NOT remain stale merely because the build passes. The
script's `lock-staleness` probe gates this mechanically: every direct
external dependency MUST declare exact `=x.y.z` (VER-2.26) and MUST equal
the locked version. A passing build with a skewed lock still fails the gate.

## Exceptions (≤14 days)

A temporary hold MUST name the exact held version, owner, blocking issue,
technical reason, `granted` and `expires` (YYYY-MM-DD), and is recorded in
`.velnor/freshness-inventory.json` under `temporary_holds`. Rules:

- `expires - granted` MUST be ≤ `max_exception_days` (14). The script fails
  longer spans.
- The script fails once a hold expires. Renewal requires a NEW review and
  NEW evidence — never a date edit.
- Standing exceptions (`expires: null`) are allowed ONLY when spec-blessed
  with `kind` + `expiry_policy` (currently: the reviewed
  `asamarts/alint@v0.16.1` mutable tag). Anything else without an expiry
  fails the gate.
- Security fixes use the expedited path: same-day update set, minimal scope,
  qualification MAY run the affected-subset first but the full gate MUST
  still pass before merge (VER-1.7).

## Limits discipline (RQ-5.3)

No update or exception may raise a §5 limit, add an arbitrary exclusion,
relabel handwritten code as generated, or reduce test assertions to satisfy
a limit. There is no Alint baseline: §5 limits are hard errors, oversized
files MUST be split, and grandfathering new or existing debt is forbidden.

## Row pre-declaration (locks not yet created)

- `.velnor/generator.lock`: Velnor bootstrap identity; created at seed
  ([release-gates.md](release-gates.md)). Refresh rule: protected release
  job only, separate reviewed change.
- `.velnor/runner.lock`: deferred V2 self-hosted runner identity. Refresh
  rule: reviewed, binds image digest + runner version + arch + API versions.
