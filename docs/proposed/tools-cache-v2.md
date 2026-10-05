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

The generator derives the static key digest from exact Mise action and binary pins, selected tool versions,
Rust toolchain and components, target, runner lane, and the owned paths. The workflow computes a runtime
identity from the qualified hosted image and validated absolute cache roots. A missing, unsupported, or
mismatched runtime identity disables both restore and save; pinned tool setup continues cold.

## Workflow lifecycle

Supported hosted jobs with a source checkout run the renderer-owned identity step and read-only restore before
Setup Mise. The composite identity action requires the workflow checkout, so checkout-less jobs such as the
`required` report fan-in retain pinned Setup Mise and tool installation but take the cold path without a V2
restore or save. Mise's built-in cache is disabled so only the V2 archive owns these paths. Save uses the same
key and path set and runs only after success on a protected default-branch push when runtime identity passed.

Paired hosted and Scale Set jobs preserve checkout, cache, and shared-work ordering. Only the hosted lane gets a
V2 prelude; Scale Set identity remains unqualified, so it emits no V2 restore/save and takes the cold path.
Elected hosted saves remain on their winning job after the shared action.

No V1 tools-cache key, built-in Mise cache path, or legacy Cargo-home alias is emitted by the renderer.
