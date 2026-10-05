# Update and exception procedure

Mechanical procedure for version-policy §1–§4, RQ-2.11, RQ-5.3, RQ-9.8,
VER-2.27, VER-3.2, VER-3.3, VER-3.4, VER-3.7, VER-4.4. No updater binary
exists: the mechanical maximum is `scripts/check-freshness.sh` (gates) +
Renovate (proposals) + this procedure (human steps). Nothing here invents
automation that is not wired.

## Roles

- Maintainer (see `CODEOWNERS`): reviews update sets, grants exceptions,
  merges pins after qualification.
- Renovate: proposes version changes only; never merges (see `renovate.json`).
- `scripts/check-freshness.sh`: fail-closed gate. Human `ok:` lines plus
  machine-readable `row: {...}` JSON lines (`check`/`subject`/`status`/
  `detail` with `status` one of `pass`/`fail`/`info`). Every fail row
  exits the run nonzero. Row-emitting checks: `inventory-shape`,
  `policy-header`, `policy-mirror`, `local-pin`, `lock-staleness`,
  `lock-mtime` (info only), `upstream-freshness` (tool/action rows plus
  the `runner` default-label row), `upstream-probe`
  (only with `--check-upstream`), `exception-expiry`,
  `standing-exception`, `advisories`.
- Usages: `scripts/check-freshness.sh` (offline gate),
  `scripts/check-freshness.sh --with-advisories` (plus the live
  `cargo deny` scan, 180 s bound),
  `scripts/check-freshness.sh --check-upstream` (plus the bounded
  read-only upstream probe; run by the generated weekly
  `.github/workflows/freshness.yml`, writes nothing),
  `scripts/check-freshness.sh --root DIR` (validate a fixture tree; the
  CLI policy suite uses this for every pass/fail case).

## The four separated checks

Local pin consistency, effective runtime identity, upstream freshness,
and security advisories are separate checks with separate evidence.
Local equality is never presented as an upstream freshness proof.

- `local-pin` / `policy-mirror`: compiled-in constants equal the
  reviewed inventory pins, and `.velnor/version-policy.toml` mirrors the
  same pins (tools, runner default + supported labels, all nine action
  records). The expected tool/action sets are asserted; a missing row
  and an unmapped extra row both fail.
- `lock-staleness`: effective identity. Every declared dependency in
  every form (string, table, workspace-inherited, renamed via `package`)
  and scope (`dependencies`, `dev-dependencies`, `build-dependencies`,
  `target.*`) resolves to a locked identity by name+version+source.
  Multiple locked versions of one name are retained, never overwritten
  in a map. The reverse direction holds too: every locked package must
  be reachable from a workspace member, and the local lock entries must
  equal the member set.
- `upstream-freshness` / `upstream-probe`: reviewed pins are backed by
  upstream evidence (source URL + check timestamp). See below.
- `advisories`: `deny.toml` policy plus the live scan. See below.

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

## Lock identity (RQ-2.11)

`Cargo.lock` MUST NOT remain stale merely because the build passes. The
script's `lock-staleness` probe gates this mechanically: every direct
external dependency MUST declare exact `=x.y.z` (VER-2.26) and MUST equal
the locked version, compared by name+version+source. A passing build with
a skewed lock still fails the gate. Git dependencies are forbidden; a
`git =` declaration fails the probe.

## Upstream evidence and the scheduled probe

No claim of "latest" is valid unless the inventory says what was checked,
where it was checked, and when. Each tool/action row carries `source`
(the upstream releases endpoint), `status`, and a `checked_at` timestamp
(per row, else the inventory top level). `status: current` requires
`qualified == pinned` AND evidence newer than `check_interval_hours`.
`status: held` requires a covering temporary hold. Any other status, any
stale evidence, and any row whose recorded `latest` differs from the pin
fails the gate. An operational lookup failure is a distinct failed check;
it MUST NOT be reported as current (VER-3.4 gate rule).

The scheduled producer is `.github/workflows/freshness.yml` (weekly
`cron: 0 6 * * 1` plus `workflow_dispatch`), generated from the
`ScheduleTrigger` contract
(`crates/velnor-actions-contract/src/workflow/`, landed via P05, built
by `crates/velnor-actions-workflow-renderer/src/freshness.rs` with
schedule wiring in
`crates/velnor-actions-orchestrator/src/freshness_emit.rs`). It runs
`scripts/check-freshness.sh --check-upstream` as a probe-only signal: it
writes nothing back to the repository, so evidence timestamps still
advance only through reviewed update sets, and the real-root
`upstream-freshness` rows stay honestly red once `checked_at` ages past
`check_interval_hours`.
`--check-upstream` is the bounded probe that the freshness job runs: one GET per
row (10 s timeout, 512 KiB cap, small fixed row count), parsing only
GitHub-releases tags, `crates.io` `max_version`, and the rust channel
manifest's `[pkg.rust]` version. It writes nothing. Every probe row
records its source URL and check timestamp; stale pins and lookup
failures fail as rows — signal for the next update set, not a build
gate. A `lookup_failed` row is fail-closed signal, never current:
re-run the probe to separate a transient fetch fault (a retry resolves
clean) from a persistent upstream change (repeated runs agree); both
fail, and only agreeing runs back an evidence refresh. The
GitHub-hosted runner family has no releases API: verify supported labels
against GitHub's hosted-runner reference, then record the actual operating
system and `Runner Image` image/version/release values from a successful
hosted workflow log. Keep a normal CI runtime observation separate from a
formal `Qualification` workflow result: a newer CI observation refreshes
image identity only and does not claim the qualification matrix ran again.
The [2026-10-04 evidence snapshot](freshness-evidence-2026-10-04.md)
records this distinction.

## Exceptions (≤14 days)

A temporary hold MUST name the exact held version, owner, blocking issue,
technical reason, `granted` and `expires` (YYYY-MM-DD), and is recorded in
`.velnor/freshness-inventory.json` under `temporary_holds`. Rules:

- `expires - granted` MUST be ≤ `max_exception_days` (14). The script fails
  longer spans, future `granted` dates, and inverted windows.
- The script fails once a hold expires. Renewal requires a NEW review and
  NEW evidence — never a date edit.
- Standing exceptions (`expires: null`) are allowed ONLY for the one
  permitted slot: `key = "asamarts/alint"` carrying non-empty `kind`,
  `expiry_policy`, `blessed_by`, and a `tag` that equals the reviewed
  `pinned_version` of the inventory's `asamarts/alint` action row. A
  pin move without a re-blessing fails, as does any other key without
  an expiry. No standing record is present at HEAD (inventory
  `exceptions: []`; the renderer pins Alint by full SHA and
  `.zizmor.yml` carries `ignore: []`) — the gate above constrains any
  future record, it does not grandfather a live one.
- Dated entries under `exceptions` carry the same full attribution as
  holds (`held_version`, `owner`, `issue`, `reason`, `granted`,
  `expires`) and the same ≤14-day, chronology, and known-subject rules.
  They are independent records: only `temporary_holds` covers a
  `status: held` row.
- Security fixes use the expedited path: same-day update set, minimal scope,
  qualification MAY run the affected-subset first but the full gate MUST
  still pass before merge (VER-1.7).

## Advisories

Every discovered Cargo workspace MUST have its own `deny.toml`, and each
file MUST keep `[advisories] ignore` empty: every ignored advisory fails the
gate and must become a policy exception instead. The live
`cargo deny check advisories` scan runs once per workspace in the CI Cargo
Deny job (or locally via `--with-advisories`), using that workspace's
manifest, lockfile, and `deny.toml`; only findings it reports against the
dependency graph count, each with its advisory id, package, and severity as
evidence. Never invent vulnerability claims from version numbers or from the
absence of a local audit tool. `cargo-deny` and `cargo-machete` remain
separate security and unused-dependency checks and do not prove freshness.

## Risk-triggered verification (RQ-9.8)

Untrusted parsers, identities, obligation selection, aggregation, and the
production wiring under review are covered by the mutation scope in
`.cargo/mutants.toml` (manual runs only, NOT wired into CI). The activated
tool is pinned first: `cargo-mutants = "27.1.0"` in
`.velnor/version-policy.toml` `[validation-tools]`, mirrored by the
`# pinned:` line in `.cargo/mutants.toml`, cross-checked by the gate.
Install exactly that release (`cargo install --locked cargo-mutants
--version 27.1.0`) and run `cargo mutants` from the repo root. Surviving
mutants MUST be killed or justified before the change lands; before any
scheduled/CI use, the tool must additionally be pinned in the Mise-managed
catalog.

Property testing is a runnable path today without new dependencies: the
`p12_property` CLI suites drive the public binary over seeded generated
repos and assert never-panic plus plan/generate determinism and exact
crate counts (`cargo nextest run --locked -p velnor-actions-cli -E
'test(p12_property)'`). Unit-level strategies over validators, task-ID
segments, and the final merge entrypoint migrate to `proptest` once the
requested pin lands: proptest `1.11.0` (exact `=1.11.0`, `rust-version`
1.85, inside MSRV 1.98; needs a manifest owner to add the dev-dependency
plus lockfile entry).

Fuzzing is infeasible in this wave, with evidence: a fuzz target needs a
new `fuzz/` crate manifest plus `cargo-fuzz` and `libfuzzer-sys` pins,
and version-policy §2 requires pinning `cargo-fuzz` (and any nightly
toolchain it needs) BEFORE enabling the check — none of those pins or
manifests exist, and manifest/version-policy edits belong to the parent,
not this change. The designated target when wiring lands is the
final-gate merge entrypoint over untrusted request JSON. Miri/Loom and
semver triggers live in `docs/implemented/verification-triggers.md`.

## Focused verification (policy-only changes)

A change touching only this procedure's owned files (the gate script,
`version-policy.toml`, the inventory, the `p12_*` CLI suites,
`mutants.toml`, this doc) qualifies with this focused path instead of
the full update-set list above:

1. `bash -n scripts/check-freshness.sh` plus `shellcheck` on the script.
2. `bash scripts/check-freshness.sh` (offline gate; exit 0 only with
   fresh evidence for every row) and
   `bash scripts/check-freshness.sh --check-upstream` as the bounded
   non-gating signal for the next update set.
3. `cargo nextest run --locked -p velnor-actions-cli -E 'test(p12_)'`
   (CLI policy suites; narrow to `test(p12_property)` for the seeded
   property path alone), plus `cargo fmt` and `cargo clippy` for any
   touched Rust files.

Formatting posture (X11): the CI gate is whole-tree (`cargo fmt --all
-- --check`), and rustfmt is deterministic, so contributors working
incrementally run `cargo fmt` scoped to the files they touched; there
is no partial-file or per-hunk formatting mode. A focused change that
leaves the rest of the tree untouched cannot introduce whole-tree fmt
drift, and the full gate re-verifies on every PR.

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
