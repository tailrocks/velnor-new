# Freshness update evidence — 2026-10-17

This reviewed update set advances the four unheld pins whose official stable
release endpoints moved: Mise `2026.10.8` → `2026.10.13`, zizmor `1.30.1` →
`1.31.1`, `asamarts/alint` `v0.18.1` → `v0.18.4`, and `jdx/mise-action`
`v5.1.2` → `v5.2.0`. It keeps the existing five temporary holds unchanged.
The latest observations for held Rust, Mr. Boxington, artifact actions, and
Mr. Boxington action were refreshed without changing their held versions,
qualified versions, reasons, owners, or expiry dates.

The bounded unauthenticated upstream probe is retained at
[`probe-unmodified-pins.log`](evidence/freshness-2026-10-17/probe-unmodified-pins.log).
It reported genuine stale pins and HTTP 403 lookup failures for several GitHub
endpoints after exhausting the unauthenticated API allowance; these failures
were not converted into success. Authenticated GitHub REST captures for the
release endpoints and immutable tag refs are stored alongside it. The probe's
successful rows confirmed the unchanged Mise/zizmor candidates and its other
non-held releases. The authenticated captures confirm the remaining release
observations and exact action commits.

Mise `v2026.10.13` was checked against the official release artifact digest
metadata and downloaded bytes. Archive hashes and extracted executable hashes
match the official values. The Linux x64, macOS ARM64, and macOS x64 raw
binaries are `94c45523c7b6760b7da704f16051171ca32e8447a849a02266a439a4455e9a12`,
`92fd6898dc213808d1c81f0bead9668dbea036e5946c94c0e82554603205a3aa`, and
`4e4a996f4747eb59d55497fdd4b0d60d3add9a06c0dc5d65a70402015ee445d0`,
respectively. The official source tag peels to commit
`580a418ed8ce379669a1a98e9aa405aa6a860996` and tree
`bc9af7a05b0e29cab8d2f3ecba106db8aba65e05`. The isolated macOS ARM64 binary
reports `2026.10.13`.

The zizmor `v1.31.1` ARM64 macOS asset hash matches GitHub release metadata;
the executable reports `zizmor 1.31.1`, and strict workflow analysis passed.
The Alint `v0.18.4` ARM64 macOS asset hash matches its official release
metadata; its executable reports `alint 0.18.4`, and both `validate-config`
and `check --fail-on-warning` passed with only the repository's three existing
informational notices. The immutable action refs are `jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7`
and `asamarts/alint@18e1a66db7444fed817c5d8c75292c7709fff32e`. The
mise-action v5.2.0 input changes were reviewed against the exact values Velnor
uses: its cache is disabled, no Mise monorepo config roots are set, and its
explicit version and SHA inputs remain present.

The runner observation is backed by the successful CI `Plan` job at the
reviewed main commit, recorded in
[`main-ci-plan.log`](evidence/freshness-2026-10-17/main-ci-plan.log), which
reports `ubuntu-26.04`, image `20261012.163.1`, and release
`ubuntu26/20261012.163`. The current GitHub-hosted runner reference continues
to list `ubuntu-26.04`, `ubuntu-24.04`, and `ubuntu-22.04` for public
repositories.

This evidence records qualification inputs and observations. The full Velnor
update qualification remains the repository's normal required validation path;
the reviewed inventory is marked current only after those checks pass.
