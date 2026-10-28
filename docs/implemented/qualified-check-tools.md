# Qualified check tools

Status: source integration contract. This document does not establish full
gate completion, live qualification of an example, or publication of a new
release. It defines the explicit tool boundary for named Mise checks.

## Offline qualification authoring

`velnor-actions qualify-tools --request /ABS/request.json --work-dir
/ABS/new-stage --output /ABS/new-candidate.json` prepares candidate declarations
from archives already downloaded by the operator. The CLI does not fetch
archives, resolve dependencies, compile source, or change Velnor configuration.
It validates the request, verifies every supplied archive against its declared
SHA-256, extracts and normalizes only the declared payload, measures the
installed tree and executable bytes, and runs only the fixed declared native
probes. Those upstream executables are untrusted code and run with the command's
host privileges; the cleared, owned environment is not an OS sandbox.

The closed request has `schema_version = 1`, one supported `platform`, bounded
`timeout_seconds`, and a dependency-first `tools` list (1 through 32). Each
entry contains a hash-free `recipe` and an ordered `archives` list of absolute
local `path`, exact recipe `url`, and independently measured lowercase
`sha256`. Archive URLs and order must exactly match the recipe's primary then
dependency archives. Core tools remain explicitly limited to Rust, Zig, and
Python. Prebuilt Aqua recipes are admitted when the shared package,
release-source, platform, layout, probe, and dependency validators accept
them. Source builds and dependency artifacts are rejected. Unknown fields,
symlinks, special files, ambiguous payload roots, malformed archive metadata,
and tool output that does not match the declared probes fail closed.
The Cargo backend is not admitted by this authoring request.

The repository's `maintenance-helpers` check explicitly selects the measured
Linux x64 `jactionlint-linux-x64` and `shellcheck-linux-x64` declarations,
alongside `cargo-nextest` and Rust. Adding those configuration inputs does not
claim full CI gate qualification. The generic Aqua authoring path also covers
`jdx/mr-boxington` 1.27.0 as an authoring example, not a measured pin.

The output is a versioned authoring candidate containing the request digest,
dependency execution order, measured complete `qualified_tools`, observed
probe records, and archive byte/digest observations. It is an input for human
review and later configuration generation, not a CI receipt, signature, trust
record, or authorization. Runtime still validates the complete registry and
all mandatory measurements independently. In particular, hash-free
`QualifiedToolRecipe` admission shares metadata, provenance, path, layout,
probe, platform, and DAG validation with runtime types without inventing
placeholder hashes or an authoring-only relaxed validator.

## Selection and declaration

Schema 1 accepts a root `qualified_tools` array. A named check's `tools` array
contains only IDs from that registry. Missing IDs fail validation; matching a
built-in catalog name does not grant authority. Pure native checks may select
`tools = []`. Native macOS `system_tools` pins are a separate observation
contract, documented in [Named Mise checks](named-mise-checks.md).

Each `QualifiedTool` contains `id`, `backend`, exact `version`, typed `options`,
sorted `depends_on` IDs, and sorted `platforms`. The complete registry is
sorted uniquely by ID and forms an acyclic dependency graph. Selection resolves
the prerequisite closure for the check's explicit platform. Conflicting
backend versions, duplicate executable names, unknown dependencies, and
missing platform declarations fail closed. Rust/OpenTofu adapter catalog
selection remains separate from this registry.

Compiler prerequisites follow the shared `QualifiedTool::requires_compiler`
rule across every backend. A non-Rust declaration requires exactly one direct
`depends_on` entry whose backend is `core`/`rust` when it uses Cargo source
installation, exposes an executable named `cargo` or `rustc`, or uses a
`cargo_nextest_version` or `rustc_verbose` probe on any platform. This also
applies to Aqua and Cargo Prebuilt declarations; an indirect Rust dependency
does not satisfy admission. Every direct prerequisite must have qualification
for every platform declared by its dependent tool, regardless of backend.

This TOML example shows the actual field layout. Its digest placeholders must
be replaced with measured nonzero lowercase SHA-256 values; it is a template,
not a runnable or already qualified declaration. The version and URL illustrate
a supported source shape and are not a freshness recommendation.

```toml
schema = 1

[[qualified_tools]]
id = "bun-ci"
version = "1.3.14"
depends_on = []
backend = { kind = "core", tool = "bun" }
options = { kind = "default" }

[[qualified_tools.platforms]]
platform = "linux_x64"
dependency_artifacts = []
install_tree_sha256 = "<measured installed-tree SHA-256>"
artifacts = [
  { url = "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun-linux-x64.zip", sha256 = "<measured archive SHA-256>" },
]

[[qualified_tools.platforms.executables]]
name = "bun"
path = "bin/bun"
sha256 = "<measured executable SHA-256>"
probe = { kind = "version", expected = "1.3.14" }

[[checks]]
id = "javascript-tests"
task = "test:javascript"
inputs = ["package.json", "scripts/test-javascript.sh"]
tools = ["bun-ci"]
[checks.runner]
label = "ubuntu-26.04"
platform = "linux_x64"
executor = "hosted"
```

`QualifiedToolBackend` supports `core` with `tool = "rust" | "node" | "bun" |
"zig" | "python"`,
`aqua` with explicit `package = "owner/project[/tool]"`, and `cargo` with an
explicit `crate_name`. These identities choose closed source admission and
layout recipes; they do not invoke ambient backend plugins or registry
installation logic. Versions are exact numeric `major.minor.patch` values.
Arbitrary install flags, environment, settings, plugin URLs, and backends are
not schema fields.

`QualifiedToolOptions` supports:

- `kind = "default"` for Node, Bun, Zig, Python, or Aqua.
- `kind = "rust"`, with sorted `components` and `targets`, for Rust.
- `kind = "cargo"`, with `default_features`, sorted `features`, and a tagged
  `installation`, for a Cargo package.

Cargo installation is explicitly `mode = "source"`, with
`source_lock_sha256`, or `mode = "prebuilt"`, with an explicit release
`repository`. Source mode requires exactly one direct qualified Rust
prerequisite. Prebuilt
mode uses default features without custom features and directly normalizes
only its declared, verified release payload; it does not invoke an installer
or require a synthetic installer prerequisite. Missing binary qualification
cannot fall back to a source build. `cargo-binstall` is an ordinary explicit
qualified tool only when the native task itself needs that command.

Core Zig qualification accepts only the exact official
`https://ziglang.org/download/{version}/zig-{platform}-{version}.tar.xz`
asset for Linux x64, macOS ARM64, or macOS x64. The declared executable is the
regular file `zig` at the payload root and uses the fixed `zig version` probe.
Core Python qualification accepts only the exact
`astral-sh/python-build-standalone` dated release asset
`cpython-{version}%2B{date}-{platform-triple}-install_only_stripped.tar.gz`.
The release date is a valid calendar date repeated in the filename. The
platform triples are `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, and
`x86_64-apple-darwin`; the exposed command name is `python`, backed by the
regular file `bin/python3.{minor}`. Its fixed `--version` output binds the
exact version.
Only the canonical uppercase `%2B` separator is accepted; URL decoding,
mirrors, arbitrary percent escapes, and backend installer fallback are not
allowed. Both sources still require measured archive, full installed-tree,
and executable SHA-256 values before runtime use. The hash-free
`QualifiedToolRecipe` type shares this same metadata, URL, layout, DAG, and
probe validation with complete runtime declarations; it omits measurements
rather than introducing placeholders or a relaxed authoring mode.

## Frozen acquisition and ownership

The orchestrator owns downloads, archive extraction, layout materialization,
tool homes, receipts, and cleanup. Mise remains the read-only inspection and
subprocess adapter: it constructs typed commands over the explicit selected
declarations and owner-verified paths. Acquisition cannot expand the selected
dependency closure, infer a backend, consult ambient settings, or choose a
replacement source. Local Mise links register already verified prefixes;
backend installation is not the acquisition authority.

For each selected platform, `artifacts` contains the complete primary archives;
`dependency_artifacts` contains the exhaustive dependency source archives for
Cargo source builds. Each archive has an admitted exact HTTPS source URL and
SHA-256. URL admission binds the supported upstream host/repository/version;
floating tags, credentials, query strings, fragments, and escaped paths fail.
Artifact lists are sorted uniquely by URL. The helper downloads into an owned
SHA-addressed location, verifies the bounded bytes, and extracts only verified
archives. Archive paths, links, size bounds, and installation layout are checked
before any acquired executable runs.

Cargo source mode checks the unpacked `Cargo.lock` against
`source_lock_sha256`, verifies the exhaustive archive/lock dependency closure,
and prepares an owned directory source. Compilation uses the qualified Rust
prerequisite with `cargo install --locked --offline --no-track --path`, an owned
source replacement configuration, explicit feature options, and owned output
paths. Network resolution, registry credentials, ambient Cargo config, and
implicit source fallback cannot fill gaps in qualification.

Rust installation retains the complete qualified prefix, including support
libraries and selected components/targets. It does not project only compiler
executables. The entire prefix is byte-bound, and the check's
`RUSTUP_TOOLCHAIN` points to that owned prefix with isolated Cargo/Rustup homes;
repository toolchain files cannot select another installation.
The task PATH exposes only the executable names declared and successfully
probed for that platform. Retaining a component in the prefix does not expose
its commands implicitly.

Before any Mise invocation, the helper locates candidate bytes, checks the
platform-specific pinned SHA-256, and exclusively copies those same bytes into
the fixed owned `bin/mise` path with read-only executable permissions. It then
uses that copy for version probing, local links, and task invocation. A program
that merely prints the pinned version cannot pass the checksum boundary.

Acquisition and task execution clear ambient credentials and use owned HOME,
XDG, Mise, Cargo, and Rustup state under the existing proxy passthrough contract.
Typed host-container capability endpoints/context metadata and OrbStack's
explicit runtime-directory connection are task-only and excluded from
acquisition commands. A host container profile is an independent installed
CLI/signed SDK/daemon identity contract; it does not authorize a qualified-tool
backend, app installation, daemon start, or ambient credential inheritance.
Repository task config contributes only the approved
static task projection; its tools, plugins, settings, environment, hooks, and
install behavior cannot authorize acquisition.

## Installed identity and final proof

Each platform declares `install_tree_sha256` and a nonempty, uniquely sorted
`executables` list. A tree digest hashes canonical JSON entries sorted by
relative POSIX path: directories carry `path`/`kind`; files also carry
`sha256`/`executable`; symlinks carry `target` and must resolve inside the
prefix. The root entry, ownership, and timestamps are excluded. Thus support
libraries, scripts, executable modes, and internal links participate in the
installed identity.

Each executable declares its exposed `name`, normalized prefix-relative `path`,
SHA-256, and a closed `probe`. Supported probes are `version` (`--version`),
`version_subcommand` (`version`), `rustc_verbose` (`rustc -vV`),
`cargo_nextest_version` (`cargo-nextest --version`), and `shellcheck_version`
(`shellcheck --version`). Rust's formatter component
also has the closed `rustfmt_version` probe with fixed `--version` arguments.
It admits only Core Rust with the `rustfmt` component selected and the canonical
`rustfmt`/`bin/rustfmt` or `cargo-fmt`/`bin/cargo-fmt` name/path pair. Its truthful
`rustfmt <component-version>-stable (<compiler-prefix> <compiler-date>)` banner
uses the formatter's own version, not the parent Rust release. The ten-character
lowercase compiler prefix and valid calendar date must match the same platform's
canonical `rustc`/`bin/rustc` declaration and its validated `rustc_verbose` full
compiler hash/date. This relationship is checked at shared recipe admission and
again through complete runtime declaration and receipt validation.

The dedicated `shellcheck_version` probe is limited to Aqua package
`koalaman/shellcheck` and executable name `shellcheck`; it always runs
`shellcheck --version`. Its expected value is the complete version-bound banner
with no final newline:

```toml
probe = { kind = "shellcheck_version", expected = "ShellCheck - shell script analysis tool\nversion: 0.11.0\nlicense: GNU General Public License, version 3\nwebsite: https://www.shellcheck.net" }
```

Runtime accepts exactly that full stdout, with at most one optional final LF.
It rejects a missing or changed line, CR, extra output, or another trailing
blank line.

Expected output is exact: the first stdout line for ordinary probes, or
complete stdout without its trailing newline for `rustc_verbose` and
`rustfmt_version`. Extra formatter output lines fail closed. The new ShellCheck
family does not change those existing output rules. Probe families are
validated against their declared tool identity; arbitrary probe argv is
unsupported.

The helper verifies the full installed tree, each executable's bytes, and its
typed probe before exposing the tool to the task. Its execution receipt binds
the selected declaration, platform, archive/dependency identities, installed
tree digest, and observed executable/probe proofs. The final Required fold
revalidates those receipts against the plan's qualification fingerprint and
exact declarations. Missing, foreign, duplicate, or mismatched tool proof
fails the check independently of the task's exit status.
When a check declares a host-container profile, final aggregation also requires
its capability receipt and compares observed CLI, SDK, runtime connection, and
daemon identity against the typed profile. The receipt also binds the exact
declared socket owner: required Docker `socket_uid`
(including zero for root) or OrbStack SDK's positive `runtime_uid`. Neither
HOME ownership nor an optional default can supply this authority. Daemon identity is
execution-scoped: a nonempty initial ID must remain stable through pre-task and
post-task observations; it is not a predeclared fleet-wide daemon ID. The signed
SDK's source/owned tree recipe additionally binds full permission mode, unlike
the qualified-tool tree's executable flag. Archive tool qualification cannot
substitute for that host-runtime proof.

Qualified acquisition proves tool identity. It does not make opaque repository
tasks reusable: compilation reuse, task-result cache reuse, native Mise task
caching, reusable task artifacts, and trusted-baseline coverage remain denied.
