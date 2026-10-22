# Velnor V2 Mise Tools Cache Contract

This document defines the generated workflow's V2 archive for pinned Mise tools, Rustup components, and
Cargo-installed binaries. It complements the broader [cache and report contract](cache-contract.md); Cargo
registry sources, MBX objects, target directories, and task results retain their separate owners.

## Archive ownership

The tools archive contains exactly these paths:

```text
~/.local/share/mise
${{ runner.temp }}/velnor/rustup
${{ runner.temp }}/velnor/cargo/.crates.toml
${{ runner.temp }}/velnor/cargo/.crates2.json
${{ runner.temp }}/velnor/cargo/bin
```

`CARGO_HOME` and `MISE_CARGO_HOME` are `${{ runner.temp }}/velnor/cargo`; `RUSTUP_HOME` and
`MISE_RUSTUP_HOME` are `${{ runner.temp }}/velnor/rustup`. The archive does not include Cargo registry or Git
sources, credentials, Cargo targets, MBX objects, or task artifacts. The source layer owns only Cargo's
`registry/index`, `registry/cache`, and `git/db` paths.

Restored Rust is never trusted as restored. In every job that installs Rust, `Verify restored Rust
toolchain` runs right after the restore (before `Prepare pinned tools`, which already runs `rustup` and
`rustc` on a hit): it deletes the restored `cargo/bin` (rustup and its proxies; Mise reinstalls a fresh
rustup) and verifies the restored toolchain file by file against the component tarballs of its pinned
channel manifest, failing the job on any difference. See
[verified Rust installs](../implemented/rust-channel-manifest.md).

## Re-verification after restore

The restore composite's last step, `Re-verify restored Mise installs`
(`cache_steps_tools_installs_verify.sh`), runs after the admission step on
every hit or seed:

- `plugins/` and `shims/` are deleted (code no pin covers; Mise rebuilds them);
- for each `installs/<tool>/<version>` except Rust, it looks up the archive URL
  and SHA-256 for the runner platform in the workspace `mise.lock`, or else in
  the Velnor catalog pins the generator embeds (`VELNOR_TOOL_PINS`: jactionlint,
  OpenTofu), downloads the archive over HTTPS, checks its SHA-256, extracts it,
  and requires every file of the install to be byte-identical to a file of the
  archive. Mise's own `.mise-*` bookkeeping (the packslip statement, shell
  completions) is data and may hold no executable file; every symlink must stay
  inside the install;
- an install that fails any of this, or has no pin, is deleted with a
  `velnor: discarding restored <tool>@<version>: <reason>` line, so
  `Prepare pinned tools` reinstalls it (checked against `mise.lock` again);
- every entry is visited, dot-named ones included (`find -mindepth 1
  -maxdepth 1`, never a glob, which skips them): a dot-named tool or version
  directory is verified like any other, so without a pin it is discarded;
- a version alias (`1.26 -> ./1.26.0`) is kept only when its target is a bare
  sibling name that is a real version directory verified in the same pass; a
  path, a chain of aliases, a dangling link or a link to a discarded directory
  is removed. Mise's per-tool `.mise.backend.toml` stays only as a plain,
  non-executable file; any other file is removed.

### The checks run hermetically

Setup Mise runs before the restore and adds `~/.local/share/mise/bin` and
`~/.local/share/mise/shims` to `PATH` with `core.addPath`; both persist to
every later step, and the cache restores both directories. A cached
`bin/sha256sum`, `curl` or `tar` would therefore run inside the very check
meant to gate it, and a cached `bin/bash` would even be the step's shell,
because the runner resolves a bare `shell: bash` through that `PATH`
(`WhichUtil.Which`, `addPath` entries first) but runs a fully qualified
shell path as given. So:

- the admission and re-verification steps use the shell
  `/bin/bash --noprofile --norc -e -o pipefail {0}` (`VERIFY_SHELL`);
- each of them, and the Rust toolchain check
  ([Rust channel manifest](../implemented/rust-channel-manifest.md)), assigns
  `PATH=/usr/bin:/bin:/usr/sbin:/sbin` (`VERIFY_PATH`, root-owned system
  directories a cache restore cannot write) before running anything;
- the admission's `verify_mise` refuses a `bin/` holding anything besides the
  binary (checked against its pinned SHA-256) and Setup Mise's plain,
  non-executable `mise-version` record. A planted `bin/<helper>` fails the job.

After the composite, `shims/` is gone and `bin/` holds only the verified
binary, so later steps that resolve bare names through `PATH` see no restored
code.

The generator derives the static digest from exact Mise action and binary pins, selected tool versions, Rust
toolchain and components, target, runner lane, and the owned paths. A tool the project's Mise config declares
([hard decision 5](../hard-decisions.md)) enters the static digest only as its `${VELNOR_TOOL_<NAME>}`
reference, so the identity step's `d` input appends, for exactly the project-declared tools the job installs
(sorted), the resolved version and that tool's lock digest: `<static>-${{ env.VELNOR_TOOL_<NAME> }}-${{
env.VELNOR_TOOL_<NAME>_LOCK }}`. The resolver step exports both. The lock digest is the SHA-256 of the tool's
`mise.lock`/`.mise.lock` `checksum` and `url` entries for the runner platform at the resolved version, read
by a fixed `awk` parser (the lock is never loaded or executed), or `unlocked` when the lock has no such entry,
in which case the version alone binds the tool. Tools the project does not declare keep their Velnor pin
inside the static digest. No `hashFiles` enters the tools identity: an edited Mise task, a comment, or
another tool's pin or lock entry leaves every job's tools key unchanged, while bumping an installed declared
tool's version or its lock checksum changes it. The workflow hashes `d` together
with the qualified hosted image and validated absolute cache roots. The canonical key is
`mise-tools-v2-${{steps.v2.outputs.identity}}`; the output already binds both static and runtime dimensions,
so the static digest is not repeated in the key. A missing, unsupported, or mismatched runtime identity
disables seed import, workflow restore, and save; pinned tool setup continues cold.

## Workflow lifecycle

Supported hosted jobs with a source checkout run, in order: Setup Mise, the generated **Resolve project tool
versions** step (when the job reads a project-declared version; it needs only the Setup Mise binary and reads
stored values without loading the project config), one fixed renderer-owned prelude composite, the read-only
restore, then pinned tool preparation. The prelude qualifies the hosted image and absolute cache roots,
imports the matching exact-key host seed only when identity is enabled, and forwards the identity's `enabled`
and `identity` outputs to the workflow step. The workflow restore call uses a separate fixed generated
composite that binds the pinned `actions/cache/restore` action to the exact paths above; its caller supplies
only the canonical runtime key and the job's pinned Setup Mise binary SHA-256. Because Setup Mise installs its
binary under `~/.local/share/mise/bin` before the restore, a miss sweeps every other entry of the Mise root,
and both a hit and a miss re-verify that binary against the pinned SHA-256 before any later step runs it.
The V2 prelude requires the workflow checkout, so checkout-less jobs such as the
`required` report fan-in retain pinned Setup Mise and tool installation but take the cold path without a V2
restore or save. Mise's built-in cache is disabled so only the V2 archive owns these paths. Save uses the same
key and path set and runs only after success on a protected default-branch push when runtime identity passed.

Paired hosted and Scale Set jobs preserve checkout, cache, and shared-work ordering. Only the hosted lane gets a
V2 prelude; Scale Set identity remains unqualified, so it emits no V2 restore/save and takes the cold path.
Both lanes render Setup Mise before their lane-specific steps, outside the shared composite, which resolves
project tool versions itself as its first step.
Elected hosted saves remain on their winning job after the shared action.

No V1 tools-cache key, built-in Mise cache path, or legacy Cargo-home alias is emitted by the renderer.
