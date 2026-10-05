# Owned archive preflight

Python tools that still inspect gzip or tar inputs require the native archive
guard from this exact checkout. The guard reads the immutable archive snapshot
on standard input, applies byte, expansion, member, and metadata limits, and
must accept those same bytes before `tarfile` opens them. The Python bridge has
no PATH lookup, environment override, download fallback, or build behavior.

On a clean checkout, provision Rust 1.98.1 through the repository's pinned
Mise tool and build the guard with the lockfile:

```sh
bash scripts/build-owned-archive-guard.sh
```

For a consumer invocation, use the wrapper so provisioning completes before
the command starts:

```sh
bash scripts/with-owned-archive-guard.sh -- python3 path/to/consumer.py ...
```

The builder requires Mise to match `.mise-version` and
`.velnor/version-policy.toml`, pins and installs Rust 1.98.1, changes to the
checkout, rejects Cargo or Rust toolchain configuration inherited from
outside the checkout, rejects configuration files inside its isolated Cargo
home, and isolates Cargo's home and target directory under the user cache
at `$HOME/.cache/velnor/archive-guard/`. The checkout-owned binary is written
to `.velnor/archive-guard/bin/velnor-archive-guard`, separate from Cargo's
hard-linked build outputs. The wrapper then
replaces itself with the requested command. Direct consumer invocations remain
fail-closed: the bridge checks the checkout and every fingerprint-input parent
for symlink or ownership changes, requires an owned non-writable guard in that
checkout-local binary, and compares the binary's embedded fingerprint to the
current source closure. Cargo, the CLI tests, and the Python bridge consume the
same `scripts/archive-guard-inputs.txt` manifest. Its Rust resolver, every file
under the native crate's `src/` and `bin/` trees, and the complete local
`velnor-actions-contract` path dependency are included automatically, so adding
or rewiring a module changes the source fingerprint. The closure is
bounded to 512 discovered tree entries, 512 files and directories, 16 MiB per
file, and 32 MiB total. Before building, the shell builder asks locked Cargo
for the full transitive normal/build dependency graph and admits only local
package paths inside those validated trees; unknown package identities and
new local paths outside the closure fail before compilation.
Optional checkout Cargo config and Rust toolchain files are included by that
manifest. A missing or stale binary requires rerunning the shell provisioning
command; Python never builds or downloads executable code. Before invoking
Cargo, the builder validates the checkout ancestry and each manifest source's
parents, ownership, mode, hard-link count, file size, and streamed tree bounds.
The builder and its source-check helper are themselves in the shared manifest.
