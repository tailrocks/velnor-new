# Restored Rust payload integrity

## Structural cause and repair authority

Pinned Mise `2026.10.0` checks Rustup component/target installation metadata.
That metadata can remain satisfied after an executable, standard library,
component file, or symlink target is damaged. Rustup does not expose an installed
tree checksum verifier. Compiler version probes alone cannot prove the tree.

Compiler version and host derive together from the catalog's closed compiler role.
The health constructor accepts no separate host, preventing a Mac host from
implicitly selecting the desktop compiler during root compiler release work.
Structural regression fixtures cover root Linux `1.98.1`, desktop Mac `1.99.0`,
and source-only root compiler release Mac `1.98.1`, without executing compilers.
Earlier desktop `1.97.1` measurements below are historical regression evidence;
they do not qualify the current `1.99.0` desktop authority.

`catalog_rust_health.rs` obtains the exact toolchain's root from the supported
`rustup toolchain list --verbose` interface. Rustup `1.29.1` prints this path
without invoking the compiler. `show --verbose` invokes `rustc` and is unsuitable
before payload verification. `RUSTUP_AUTO_INSTALL=0` prevents owner inspection
from implicitly installing an active toolchain.

The helper binds its inventory to the typed catalog version, supported host,
minimal profile, canonical required components and targets, exact Rustup manager
version/digest, `rustup-source-archive-tree-v2` policy, and the schema-2
`source_archive_inventory` payload. Each exact toolchain has its own marker
outside the selected toolchain subtree under the already owned Rustup home.
The registered eight-module source loader invokes the strict archive inventory
seam for that selected subtree only. It hashes canonical paths, entry kinds,
permission modes, regular-file bytes, hardlink groups, and confined symlink
targets; unsupported filesystem metadata and special entries fail verification.
Cache semantic observations use the same `{schema: 2, root, roots, purpose:
payload-v1}` context, but the public `source_archive_inventory(context, None)`
factory remains unqualified and fails before traversal. The original-filesystem
observer cannot create a cache grant. Variable fields use length prefixes.
Canonical roots must stay inside the owned home. External and dangling symlinks
fail verification. `/usr/bin/python3 -I -S` uses the hosted system interpreter
without repository imports, user sites, or Python environment hooks.

A missing or mismatched marker triggers the exact pinned manager's supported
`toolchain uninstall`; normal exact Mise installation then reacquires the
toolchain. Finalization checks the complete minimal profile, selected additional
components and targets, all seven required commands, and exact compiler version
before writing the new inventory. Restored executable bytes are not run before
an existing qualified inventory matches. Marker symlinks are removed lexically
by preparation; finalization refuses to follow a marker symlink.

`terminal_restore_script()` verifies the existing selected-tree digest and
byte-exact marker before component/target obligations and version probes. It
requires an existing integrity directory and never repairs, installs, downloads,
creates the directory, or rewrites the marker. Native bounded fixtures verify
unchanged marker bytes and modification time; payload damage, malformed markers,
extra trailing newlines, missing markers, and a missing integrity directory fail
before compiler execution. Both preparation and terminal verification compare
the expected newline-terminated marker bytes using `cmp`, avoiding shell command
substitution's removal of trailing newlines.

The current shared engine exposes raw metadata recording and strict rejection.
It does not yet expose a qualified archive projection. Therefore this terminal
API alone does not qualify authenticated warm receipt continuation across a
metadata-normalizing archive transport; the marker and full snapshot must use
the same approved selected-tree projection before that authority activates.

Cold authenticity comes from the qualified exact manager and Rustup's official
distribution checksums. The inventory detects accidental payload damage. It is
not authentication against an attacker able to replace both payload and inventory;
protected cache writer provenance remains necessary. Boundary corruption such as
an owned root becoming an external symlink fails closed.

## Local qualification, 2026-10-03

These are local Docker Linux and native macOS probes. They do not prove a fresh
GitHub runner's cache transport, server trust policy, or hosted performance.

The table below records the original byte/tree scanner. Shared inventory
consolidation adds complete filesystem metadata observation and bumps policy;
old markers fail verification and require exact owner reinstallation in generated
preparation. Historical timings do not measure the expanded shared scanner.

Linux Docker used the preexisting `rust:1.98.1-slim` image, whose manager was
Rustup `1.29.1` with SHA256
`dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71`.
The exact toolchain was uninstalled and reinstalled using `--profile minimal
--component clippy,rustfmt --no-self-update`; five components were downloaded.
Finalization checked Rust `1.98.1`, Cargo, rustdoc, Clippy and rustfmt. The complete
portable tree scan then preserved healthy state without downloading payloads.

| Probe | Local result |
| --- | --- |
| Final healthy Linux finalization, including command probes and tree digest | 3.733 seconds |
| Final healthy Linux preparation, complete tree digest | 1.753 seconds; zero downloads |
| Native macOS arm64 Rust `1.98.1` preparation with system Bash | 0.32 seconds; zero downloads |
| Exact desktop Rust `1.97.1` native macOS finalization | 1.13 seconds; seven commands and required host target verified |
| Exact desktop Rust `1.97.1` native macOS preparation | 0.35 seconds; zero downloads |
| Compiler replaced with an executable sentinel script | Exact owner uninstall; sentinel never executed; no download |
| Behavioral test: changed bytes, missing file/marker, extra file, malformed marker, escaped symlink, marker symlink | Owner fallback; external sentinel unchanged |

The Rust test runs the actual helper shell scripts against a bounded owner mock
and explicitly checks that healthy preparation never invokes `rustup run`.
It was compiled and executed directly with Rust `1.98.1` in Linux Docker and
native macOS; both passed, including external marker-symlink sentinel preservation.
no Cargo lane or dependency changes were required. The native macOS probe used
existing Rust `1.98.1` state to qualify portability and system Bash behavior.
The separate source-qualified desktop installation at
`/tmp/velnor-desktop-rust-final-pin-proof` then passed exact Rust `1.97.1`
finalization and unchanged preparation with the desktop component/target/MBX
selector bound into the marker. This verifies owned compiler state; it does not
verify a separate Mise MBX wrapper or hosted archive transport.

Linux runs were emulated x86_64 on an arm64 host; timings are individual samples,
not hosted latency or percentile evidence. Fresh restored-runner qualification,
hosted desktop role qualification, and full repository gates remain parent work.

## Shared inventory qualification

After consolidation, both Rust health fixtures passed on native macOS. Additional
coverage replaces a hardlink with identical independent bytes and adds an opaque
metadata canary: both changes require owner repair, and no canary value reaches
stdout or stderr. Exact desktop `1.97.1` finalization took 3.37 seconds; its unchanged
full metadata scan took 1.13 seconds without downloads. These remain local samples.

Linux Docker independently reproduced xattr changes and real POSIX ACLs exposed
through `system.posix_acl_access`. The shared scanner detected both. Its original
source SHA256 `6558d0128f12ed361d64b473b7abe32440f38efcdb4bed81c16242be802019ce`
also passed six standalone helper/inventory Rust tests and nonempty cold, warm,
third, and repaired snapshot probes. That source omitted Linux inode flags and
those successful export results are superseded.

The revised engine reads Linux `FS_IOC_GETFLAGS` without following links and checks
descriptor identity. Eight actual Docker probes passed: ordinary file flags were
`0x800000`; setting NODUMP produced `0x800040`, changed the opaque digest, and
prevented strict export. Removing NODUMP restored the initial flags. Symlink probes
did not follow their targets; mismatched regular-file metadata was rejected.
Engine SHA256:
`5f1c3df6ab4e03833ba025d3d2923b49e3bb104af41150558d88937fed9e7118`.
The revised engine also passed all six standalone Rust helper/inventory tests.
Canonical cold, warm, third, and repaired wrappers returned successful producer
outcomes while marking export unavailable and clearing useful-delta output.
Raw results: [current Linux observations](ci-performance-rust-tool-evidence/shared-inventory/linux-current.json),
[flag probes](ci-performance-rust-tool-evidence/shared-inventory/linux-flags.json),
and [standalone tests](ci-performance-rust-tool-evidence/shared-inventory/linux-current-tests.log).
The standalone Linux compiler was Rust `1.99.0`, not the catalog compiler;
these prove helper behavior, not a full pinned workspace build.

Strict rejection currently forbids every nonzero inode flag, including that
ordinary filesystem state. Consequently this Linux filesystem cannot export a
healthy payload under the current policy. This is a proven qualification limit,
not a successful Linux warmth result. Parent ownership must resolve or explicitly
qualify the supported filesystem policy before claiming Linux cache completion.

## Pinned sources

- [Mise Rust satisfaction checks](https://github.com/jdx/mise/blob/v2026.10.0/src/plugins/core/rust.rs)
- [Rustup owner listing](https://github.com/rust-lang/rustup/blob/1.29.1/src/cli/common.rs#L273-L335)
- [Rustup show commands and compiler invocation](https://github.com/rust-lang/rustup/blob/1.29.1/src/cli/rustup_mode.rs)
- [Rustup exact Linux manager checksum](https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-unknown-linux-gnu/rustup-init.sha256)
