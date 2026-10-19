# Velnor version freshness and locking policy

**Status:** This document defines the repository version policy. Repository
freshness checking and policy-mirror validation are implemented in
`velnor-actions-freshness` and `scripts/check-freshness.sh`. Implementation
coverage and verification evidence are recorded in `docs/implemented/`.

This policy defines what “latest versions” means for Velnor and how strict
freshness coexists with reproducible builds. Exact locks make a run repeatable;
the freshness gate prevents those locks from becoming stale.

`.velnor/version-policy.toml` MUST start with:

```toml
schema = 1
channel = "stable"
check_interval_hours = 24
max_exception_days = 14
# Optional: default_cooldown_days = 3  # integer in 1..=90 (defaults to 3 when omitted)
```

The checker MUST reject unknown keys and any value that weakens this policy.
When `default_cooldown_days` is present, it MUST be an integer in `1..=90`
(defaulting to `3` days when omitted, matching GitHub Dependabot's 3-day default
package release cooldown contract). Security advisories and `cargo-deny` checks
remain immediate and are never delayed by `default_cooldown_days`.

## 1. Normative rule

At each reviewed update, every versioned input MUST use the newest stable,
non-yanked release that supports the selected platform and required feature
set. Prereleases, nightly builds, moving Git refs, wildcards, and floating
`latest` selectors are forbidden in normal builds. Resolve the newest allowed
release, then record its exact version, immutable revision, or digest in the
generator's compiled-in release catalog and `Cargo.lock` for crates. Velnor
dogfood validates an optional `.velnor/generator.lock`; when absent, it builds
and verifies a same-source pre-seed helper. The Velnor-only
`.velnor/version-policy.toml` mirrors the compiled catalog for freshness and
release checks; consumer generation MUST NOT require it. Local development,
CI, releases, and agents MUST consume the same committed pins; no job may
independently resolve versions.

“Latest” means latest upstream stable release at the update's recorded check
time. An incompatible newest release is a migration requirement, not a reason
to silently keep an older version. A temporary exception MUST name the exact
held version, owner, blocking issue, technical reason, and expiry no more than
14 calendar days later. The update workflow MUST fail once that exception
expires. Renewing it requires a new review and evidence. Security fixes MUST be
handled on an expedited update path.

The default language channel is stable. V1 MUST NOT use nightly Rust. If a
later check requires nightly, its exact dated toolchain, purpose, owner, and
weekly update qualification MUST be recorded; moving `nightly` is forbidden.
The declared `rust-version` MUST track the selected stable toolchain's
major/minor version unless a separately approved consumer compatibility
requirement exists.

## 2. Authoritative version inventory

The compiled-in catalog MUST identify each tool called by generated workflows
with its exact stable version, canonical upstream source, supported platforms,
and artifact digest when available. It includes Rust, MBX, Nextest, `gh`,
cargo-deny, cargo-machete, actionlint, zizmor, and every other generated-task
tool. Generated workflows embed these exact values and pass them as explicit
Mise tool arguments; they never read a consumer version-policy file or resolve
`latest` independently. Velnor's `.velnor/version-policy.toml` MUST mirror
every catalog entry and is checked for equality during dogfood CI and release
qualification. Mise bootstrap itself is pinned by `.mise-version` and the
compiled-in Mise release record in generated workflow output.

| Versioned input | Authority | Freshness requirement |
|---|---|---|
| Rust compiler and components | Compiled-in generator catalog; embedded in generated workflows and run through Mise | Latest stable Rust and matching current components |
| Mise bootstrap | `.mise-version` and compiled-in Mise release record | Latest qualified stable Mise release; exact artifact digest |
| MBX, Nextest, `gh`, and policy/CI tools | Compiled-in generator catalog; embedded in generated workflows and run through Mise | Latest stable releases; includes cargo-deny, cargo-machete, actionlint, ShellCheck, and zizmor |
| Direct and transitive Rust crates | Workspace manifests, `Cargo.lock` | Newest stable graph; major updates included; all changed versions pass the full locked gate |
| Velnor workflow bootstrap | Consumer output embeds the exact generating release version, immutable target asset URL, and binary SHA-256; Velnor dogfood validates an optional `.velnor/generator.lock`; when absent, it builds and verifies a same-source pre-seed helper | Latest qualified stable Velnor release for each supported target |
| GitHub Actions | Compiled-in Velnor action registry; optional exact overrides in `.velnor/config.toml` | Default record is the latest reviewed stable release; workflow uses immutable full commit SHA |
| Alint GitHub Action | Compiled-in Velnor action registry; emitted as a full-SHA pin like every other action | `v0.19.0` (`f39a25a7e061bb8738a87372561d8b9a6a29ca87`) is the current reviewed default as of 2026-10-19. No tag exception exists; changing the pin requires a reviewed Velnor version-policy update |
| GitHub-hosted OS image | Generated `runs-on` label and recorded runner metadata | Latest stable supported Ubuntu image family after host qualification; exact versioned label is the pin. GitHub may update its image contents in place, so record `ImageOS` and `ImageVersion` as runtime evidence and cache identity, not as immutable pins |
| Runner runtime inputs | `.velnor/runner.lock` | Latest qualified Docker Desktop/Engine, official runner, base-image digest, GitHub API version, protocol revision, and runner dependencies including Turso |

Consumer workflows MUST be self-contained: all tool pins, action pins, runner
labels, and the bootstrap binary identity are embedded in generated output.
They MUST run after `velnor-actions init` and `generate` without requiring
`.velnor/version-policy.toml` or `.velnor/generator.lock`. The generator MUST
fail generation when its executable lacks verified release provenance needed
to embed a real immutable bootstrap asset and digest. Velnor's own workflow
instead validates its optional protected lock or builds and verifies a
same-source pre-seed helper when that lock is absent.
The pre-seed check binds helper bytes to the checked-out source and this run's
artifact transport; it does not review that source or establish immutable
official-release provenance.

`rust-toolchain.toml`, `mise.toml`, and `mise.lock` are optional,
repository-owned inspection inputs. They do not define Velnor's generated
workflow tool versions. Velnor checks for their existence, extracts useful
values when parseable, and compares them with this latest-version policy. It
reports concrete manual recommendations for missing or stale values and never
creates, edits, or refreshes those files. Repository maintainers apply and
review any recommended updates themselves. See the
[tooling input contract](tooling-input-contract.md).

### GitHub Action defaults

Velnor ships a compiled-in action registry so projects need no action-pin file
to generate workflows. Each ordinary record contains the action repository,
latest stable release, full commit SHA, required runner version, and verified
input/output metadata. Generation emits the exact SHA and a matching version
comment; it never queries a floating `latest` ref. No tag exception
exists: `asamarts/alint` pins a full SHA like every other action.

Verified current defaults on 2026-10-19:

```yaml
uses: jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7 # v5.2.0
uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
uses: jdx/mr-boxington-action@47ce49b32025d93a7a2f4570815afaa2b7092240 # v1.7.2
uses: actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
uses: actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
uses: asamarts/alint@f39a25a7e061bb8738a87372561d8b9a6a29ca87 # v0.19.0
```

Release records: [mise-action v5.2.0](https://github.com/jdx/mise-action/releases/tag/v5.2.0),
[checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1),
[download-artifact v8.0.2](https://github.com/actions/download-artifact/releases/tag/v8.0.2),
[mr-boxington-action v1.7.2](https://github.com/jdx/mr-boxington-action/releases/tag/v1.7.2),
[cache v6.1.0](https://github.com/actions/cache/releases/tag/v6.1.0),
[upload-artifact v7.0.2](https://github.com/actions/upload-artifact/releases/tag/v7.0.2),
[Alint v0.19.0](https://github.com/asamarts/alint/releases/tag/v0.19.0).

Checkout, Mise setup, cache restore/save, and artifact transfer are emitted
where required by the workflow graph. The Mr. Boxington action is emitted only
when Rust scanning detects project use of MBX. Cargo sources use the shared
exact-path `actions/cache` layer in every Rust mode. All references come from the registry. The full-SHA
`asamarts/alint` pin is emitted only by the separate Velnor
repository-policy job.
`taiki-e/install-action`, `actions/setup-*`, branch refs, unapproved tag refs,
and floating action refs are forbidden. `actionlint` v1.7.12 and
ShellCheck v0.11.0 are the verified stable tool defaults on 2026-09-28. Both
are installed through Mise. `taiki-e/install-action` and other tool-installer
actions are forbidden. V1 emits no Python run steps, so it does not install
Pyflakes.

The only per-project pin override is `[actions.overrides]` in
`.velnor/config.toml`. Each ordinary value contains an exact 40-hex commit SHA
and its matching stable version. Keys must be exact allowlisted action
repositories, and the pair must appear in that action's bundled approved-pin
catalog (latest release plus maintained compatibility pins). The Alint action
is not a per-project override: Velnor emits exactly
`asamarts/alint@f39a25a7e061bb8738a87372561d8b9a6a29ca87` (`# v0.19.0`).
Changing that pin is a Velnor version-policy change,
not a consumer configuration change. Velnor validates every ordinary pair and
the action's input/output schema before replacing generated output. Unknown
actions, tags, mismatched SHA/version pairs, and invalid inputs fail
generation. Omitted overrides always use compiled-in defaults. Velnor's
scheduled freshness check compares bundled defaults with upstream latest stable
releases. The Alint pin is checked as a reviewed version record; it is not
silently rewritten during generation. A stale default blocks Velnor release
unless an approved, expiring exception records the hold.

The file `.velnor/version-policy.toml` MUST declare the policy schema, stable
channel, registry/source rules, freshness cadence, and maximum exception age.
Its runner inventory MUST include the latest pinned default and an explicit
list of older labels that GitHub still supports:

```toml
[github_runner_images.linux_x64]
default = "ubuntu-26.04"
supported = ["ubuntu-26.04", "ubuntu-24.04", "ubuntu-22.04"]
```

The `default` MUST equal the latest stable supported image family. The
`supported` array MUST contain only exact versioned labels; it MUST NOT contain
`ubuntu-latest`, `*-latest`, or unversioned aliases. The generator MUST validate the default and every listed override against its supported-label set.
The generator MUST reject all runner labels absent from this inventory. A
consumer's optional `workflow.runner_label` is the only exception to the
latest-runner default: it may select an older listed label as an explicit
compatibility override, recorded as `config_override`. No config means the
generator emits `default` and records `latest_default`. Unsupported or unversioned runner labels MUST block generation.

Alint runs as a separate job only in Velnor's `velnor-repository-v1` profile,
using this exact generated step (current pin as of 2026-10-19):

```yaml
- uses: asamarts/alint@f39a25a7e061bb8738a87372561d8b9a6a29ca87 # v0.19.0
```

It reads `.alint.yml` and enforces only the rules supported and explicitly
configured there, such as file/path placement, required files, structured
configuration checks, and file line limits. Ordinary `consumer-v1` workflows
do not require Alint configuration. Velnor does not add another repository
structure linter or claim that Alint covers Rust dependency architecture,
module graphs, or test-layout semantics.

All required or activated optional tools MUST have exact versions in the
Mise-managed inventory. Tools used only by scheduled deeper checks MUST still
be pinned before that check is enabled. The version policy MUST reject unlisted
installers, `cargo install`, ad hoc `rustup component add`, and tool versions
hidden in scripts or container files. This includes activated tools such as
`cargo-hack`, `cargo-mutants`, `cargo-fuzz`, `cargo-llvm-cov`,
`cargo-semver-checks`, and any nightly toolchain used by Miri. No validation
task may silently use a preinstalled Rust or Rust-related binary from the
GitHub image.

For Cargo dependencies, the freshness checker compares both the declared
requirements and every package resolved in `Cargo.lock` to the newest stable
registry release. Every direct external dependency MUST use an exact
`=x.y.z` requirement in the workspace dependency declaration; `Cargo.lock`
pins its complete transitive resolution. Renovate MUST propose major-version
and minor/patch changes; the maintainer updates source and exact requirement as
needed. `cargo-deny` and `cargo-machete` remain separate security and
unused-dependency checks and do not prove freshness.

Git dependencies are disallowed by default. An approved Git dependency MUST
use a full commit revision and have an equivalent stable registry release
checked first. A Git dependency is an expiring exception, never a permanent
substitute for a published release.

## 3. Update cadence and gates

1. Under Velnor's repository profile, Alint runs on every pull request, merge
   group, and protected default-branch update as a separate required job. V2
   adds the runner-lock inventory.
2. Renovate or the corresponding upstream release monitor opens reviewed
   update changes. It MUST cover all available compatible updates in one
   coherent update set; incompatible updates are reported with a required
   migration, not silently omitted.
3. The update change records freshness timestamp and version delta. It refreshes
   only Velnor-owned pins and locks (`Cargo.lock`,
   `.velnor/version-policy.toml`, `.velnor/generator.lock`, and later
   `.velnor/runner.lock`), then runs formatting, policy, Clippy, tests,
   doctests, dependency checks, workflow validation, cache/selection fixtures,
   and platform qualification. It never edits `mise.toml`, `mise.lock`, or
   `rust-toolchain.toml`.
4. The branch-protection freshness check fails if any pin is stale, missing,
   mismatched, unreviewed, or held past expiry. Operational lookup failure is
   a distinct failed check; it MUST NOT be reported as current.
5. New exact Velnor-owned pins and `Cargo.lock` are merged only after
   qualification. Normal builds use them with `--locked`; they never contact
   registries to select versions.

The scheduled job MUST produce a machine-readable inventory containing
component, current pin, latest stable release, source URL, check timestamp,
status, and exception metadata when present. Any update that changes a lock or
selected platform image invalidates affected task-result and baseline
identities under the cache contract.

## 4. Reproducibility boundaries

Full-SHA GitHub Action pins, exact crate/tool versions, immutable generator
artifacts, and image digests are reproducible identities. GitHub-hosted VM
images are not immutable: Velnor pins an explicit supported image family,
records the concrete `ImageOS`/`ImageVersion` in task evidence, and qualifies
image-family changes. It MUST NOT claim that `ubuntu-26.04` fixes every image
package version. GitHub controls packages inside that hosted image; Velnor
MUST install version-sensitive validation tools through Mise and MUST record
the image metadata for host-provided system dependencies.

Refresh only Velnor-owned locks and pins: `Cargo.lock`,
`.velnor/version-policy.toml`, `.velnor/generator.lock`, and
`.velnor/runner.lock`. Do not edit or refresh repository-owned
`mise.toml`, `mise.lock`, or `rust-toolchain.toml`; report recommendations for
those files for a maintainer to apply.

For the Scale Set runner (`crates/velnor-runner`), `.velnor/runner.lock` MUST be reviewed and
must bind image repository and digest, base distribution, official runner
version and archive SHA-256, architecture, Docker Desktop/Engine versions, API
version, GitHub REST API version, and native protocol source revision. Runner
auto-update behavior is part of the qualified identity and refresh process.
The exact `turso` crate version is declared through workspace dependencies and
locked in `Cargo.lock`; each version update reruns journal migration,
transaction, locking, durability, and crash-recovery qualification. Turso
Cloud sync and remote access remain disabled for the local journal.

No claim of “latest” is valid unless the inventory says what was checked, where
it was checked, and when. A green build using stale pins is a freshness failure
even when every test passes.
