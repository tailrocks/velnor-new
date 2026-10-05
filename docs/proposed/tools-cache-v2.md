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

The generator derives the static digest from exact Mise action and binary pins, selected tool versions, Rust
toolchain and components, target, runner lane, and the owned paths. The workflow hashes that digest together
with the qualified hosted image and validated absolute cache roots. The canonical key is
`mise-tools-v2-${{steps.v2.outputs.identity}}`; the output already binds both static and runtime dimensions,
so the static digest is not repeated in the key. A missing, unsupported, or mismatched runtime identity
disables seed import, workflow restore, and save; pinned tool setup continues cold.

## Workflow lifecycle

Supported hosted jobs with a source checkout run one fixed renderer-owned prelude composite before the
read-only restore and Setup Mise. That composite qualifies the hosted image and absolute cache roots, imports
the matching exact-key host seed only when identity is enabled, and forwards the identity's `enabled` and
`identity` outputs to the workflow step. The workflow restore call uses a separate fixed generated composite that binds the pinned
`actions/cache/restore` action to the exact paths above; its caller supplies only the canonical runtime key.
The V2 prelude requires the workflow checkout, so checkout-less jobs such as the
`required` report fan-in retain pinned Setup Mise and tool installation but take the cold path without a V2
restore or save. Mise's built-in cache is disabled so only the V2 archive owns these paths. Save uses the same
key and path set and runs only after success on a protected default-branch push when runtime identity passed.

Paired hosted and Scale Set jobs preserve checkout, cache, and shared-work ordering. Only the hosted lane gets a
V2 prelude; Scale Set identity remains unqualified, so it emits no V2 restore/save and takes the cold path.
Elected hosted saves remain on their winning job after the shared action.

No V1 tools-cache key, built-in Mise cache path, or legacy Cargo-home alias is emitted by the renderer.
