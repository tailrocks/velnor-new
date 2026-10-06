# C06 source availability and confidentiality

Pinned Cargo inspected: `cargo 1.98.1 (797e8a9bc 2026-08-05)`.
`cargo fetch --help` supports `--target`, `--manifest-path`, `--locked`,
`--offline`; it has no package or feature selection flags.

## Reproduced defect

A hermetic local registry contains normal, optional and Windows-only packages
locked at 1.0.0. Removing the optional package archive gives:

- `cargo metadata --locked --offline --format-version 1`: exit 0.
- `cargo fetch --locked --offline`: exit 101, missing `optional-dep`.

Default-feature metadata therefore cannot certify complete locked source
availability. Platform-filtered metadata likewise cannot certify dependencies
for another selected platform. Behavioral fixtures retain both cases.

## Implementation

Preparation and validation discover Cargo configuration from the checkout cwd.
Previously preparation used a clean cwd and ignored source replacement used by
validation. Preparation now consumes the same configuration after the standard
credential-unset shell prelude. Mise keeps `--no-config --no-env --no-hooks`.
Renderer source-fetch names require the scrub overlay, including nested roots.

Both offline availability and online fill use Cargo fetch. Selected locked
workspace roots follow the lane's exact Cargo package member identities;
unknown or ambiguous ownership broadens to all discovered locked roots.
Package/feature-level narrowing is unavailable through pinned Cargo fetch.
The isolated producer descriptor reports conservative scope
`complete_locked_workspace`; it includes optional and target dependencies.
This scope may fetch more than the selected package obligations need.

Plan and validation jobs have read-only cache authority. A separate public
source producer captures admitted manifests and locks, creates inert target
placeholders outside checkout, and invokes native Cargo fetch. It never builds
or runs repository code. Existing checkout Cargo configuration disables this
optional producer. Missing executable publication qualification fails generation;
runtime verifies the pinned Mise manager, discards restored execution state,
and performs exact qualified Rust preparation before executing Cargo or Rustc.
Snapshot keys and digests are bookkeeping, never executable authority.
Source restore and save share
one canonical Source3 descriptor and the exclusive public producer namespace.

Source3 key derivation projects out only the runtime compiler-version field;
schema, target, roots, captured manifests and locks, archive checksums, mode and
selected package/features remain bound, together with the acquisition-program
digest. The opaque typed projection describes compatibility after full owner
and current compiler-pin validation; it grants no execution or reuse authority.
Replacing full-descriptor JSON with this projection changes the derived cohort
identity within the existing v4 namespace. Exact-cohort restore prefixes cannot
reach historical full-JSON identities without a cryptographic collision; there
is no alias or migration fallback. Hosted warm-cache and compiler-change reuse
remain unproven.

## Supported narrower native API

Pinned native `cargo tree -p … --features … --target …` uses selected member
resolution and downloads accessible sources. It can prepare a smaller containing
closure than fetch. This disproves a general claim that stable Cargo cannot
narrow source preparation. The [pinned fetch implementation](https://github.com/rust-lang/cargo/blob/797e8a9bc/src/cargo/ops/cargo_fetch.rs)
and [pinned tree implementation](https://github.com/rust-lang/cargo/blob/797e8a9bc/src/cargo/ops/tree/mod.rs)
use different selection paths. Tree remains a dependency view, rather than an
exact compiler unit graph.

Hermetic source probe: default Linux selection acquired six packages, alpha
feature seven, while complete fetch acquired ten. Logical selected archive
bytes were 2244 versus 3683 for complete fetch (four extra packages, 1439 bytes).
These are fixture archive sizes, not measured network transfer. Selecting a
Windows target retained required host dependencies and acquired its target
dependency; no Windows compiler execution is claimed.

Actual offline compiler proof used fresh private Cargo homes and targets:
native tree followed by `cargo check/test --locked --offline` succeeded for
default and alpha selections. Normal, build, host-transitive, proc-macro,
dev and optional sources were compiled as applicable. No check/test source
hash changed and locks remained unchanged. Raw command/result files are
mode 0600 under the mode 0700 evidence directory
`/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/velnor-c06-offline-build-proof-xk7qqz1z`;
its evidence manifest SHA256 is
`46142aba7270ecdd1ca5b41e9c2c017928a08470c87fa4c2d0ea011252068318`.

The generator now binds native tree preparation to unique authoritative Cargo
package IDs, admitted target triples and exact feature sets. Both the isolated
producer and ordinary reader use the same native tree selection. Unknown
mapping or flags retain the complete-workspace fallback. This integration awaits
Rust gates, independent review and actual hosted-runner qualification; local
fixture success does not close C06 performance gates. Workspace fallback proves
containing source availability, rather than exact selected source scope.

An authenticated public producer receipt is required before useful opaque
registry index state can be retained. The current owner has no receipt opt-in:
it discards restored index and extracted sources, forces anonymous native Cargo
refill, and records `source_untrusted_refill_elapsed_ms` separately. Archive
checksums and sparse path layout are independently checked. This conservative
path deliberately makes no unchanged-warm zero-download claim.

Readable source transport qualifies only public crates.io and local package
lock sources. Custom registries, source replacement, registry credential
configuration, Git dependencies and unknown inputs disable source transport;
tasks still run. Identity hashing alone cannot certify private-source access.
Cargo-home credentials/configuration are excluded from the archive payload.

## Evidence limits

Local fixtures prove Cargo behavior and generated-script configuration/known
runner token handling. They are not hosted fresh-runner cache benchmarks.
Private registry authentication/reader isolation remains unqualified. The
existing scrub list removes known runner credentials; it is not a sandbox for
arbitrary ambient secret variable names or malicious trusted producers.
