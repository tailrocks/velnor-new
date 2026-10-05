# Current tool-version inventory delta

Captured 2026-10-05 18:08 UTC. This is a passive handoff; no source, index, ref, workflow, or release was changed and no tool binary was executed.

## Source snapshot

The policy baseline is immutable Velnor commit `e608f82039375a4c2a839d5229e1d2dd3035617b`, tree `f5c63ac98f7d79d68eb9448c6ff9600a872d0c2a`, captured under `/root/.cache/velnor-task-targets/20261005/pr12-e608-source-snapshot/`. Its `regular-files.sha256` manifest SHA-256 is `8686761c3d2a586370aa4f841b8b09ea40d0b93259e84e3aaf25fabd37cd0bab`. The exact policy and freshness inputs have SHA-256 `c5a32352a4c8d259bb2f7620e67ba5ccd9be71443bf26e13b8164a23e6f68247` and `28c03834b003aec319182814a86f886fb673e002d6ce215e4e6fb3acb5a00590`, respectively.

I did not use the dirty shared worktree as current truth. The steward’s live task clone has uncommitted overlays; rebind these findings to the next immutable source snapshot before applying or merging changes.

## Current pins and latest official releases

| Tool or action | e608 pin | Official latest observed | Delta and disposition |
|---|---:|---:|---|
| Mise CLI | 2026.9.18 | 2026.10.3 | Update is required. The existing freshness row observed 2026.10.3 at 12:26Z but still holds 2026.9.18. A separate owner packet reports the bounded Linux 4-test 10.3 regression passed; macOS and hosted qualification remain open. |
| Rust default | 1.98.1 | 1.99.0 | Set regular CI to 1.99.0 and retain a separately named 1.98.1 MSRV check; Cargo declares `rust-version = "1.98"`. The current e608 CI uses 1.98.1 throughout, so it has no distinct default/MSRV jobs. |
| `jdx/mise-action` | v5.0.0, SHA `9149ea85001c7435d5a66bb127d6a1b6227cb0a5` | v5.1.1, commit `2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca` | Update the pin and freshness record after review. This is a behavior change: v5.1.1 defaults `persist_github_token` to false, and `sha256` verifies the final Mise executable bytes. Current source supplies `sha256` and runs separate later Mise install steps, so token scope/persistence needs an explicit decision. |
| actionlint | 1.7.12 | 1.7.12 | The original package has no newer release. However, Mise 2026.10.3 now deprecates the `actionlint` registry entry and points to `jactionlint`; see the migration row below. |
| jactionlint | Not pinned | 1.8.2 | New maintained fork, not a numeric actionlint bump. The executable is renamed `jactionlint`; current CI invokes `actionlint`. Adoption requires a tool/policy rename, invocation update, and requalification. Existing `.github/actionlint.yaml` remains accepted; `.github/jactionlint.yaml` has precedence. |
| cargo-deny | CI/local tool command uses 0.20.2; absent from `[tools]` and the e608 freshness `tools` rows | 0.20.2 | Version is current. Add it to policy/freshness tracking if it remains a required check; this report does not change the check. |
| asamarts/alint action | `version-policy.toml` and CI pin v0.17.0 / SHA `d93c0283b19dd78afcd8a4b303f1556a7759ba81` | v0.17.0 | Current workflow/policy version is latest, but the e608 freshness action row is inconsistent: it records pinned v0.16.1 and status held. Reconcile the inventory before treating freshness as current. |
| cargo-nextest | 0.9.146 | 0.9.146 | Current. |
| actions/cache restore/save | v6.1.0 / SHA `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` | v6.1.0 | Current. |
| mr-boxington | 1.21.1 | 1.22.0, source commit `10474d43342ad65df3b02323dd8092d18ab38101` | Latest official binary provenance and UID 65534 `--version` smoke exist, but no cache lifecycle command was run; keep cache qualification pending. |
| jdx/mr-boxington-action | v1.6.0 / SHA `1687e54eb349cadf61fa38b5813a77875489e8e6` | v1.7.1 / commit `d0825fbaf3cc36ca2609aa38e71046265a1f1e37` | Existing hold is for exact released-source lifecycle/input/cache qualification. The separate #29 candidate SHA is not the v1.7.1 release. Do not credit provenance as runtime qualification. |
| gh, shellcheck, zizmor, OpenTofu, release-plz | 2.102.0, 0.11.0, 1.30.1, 1.13.1, 0.3.169 | Same values in e608 inventory | Existing freshness snapshot marked them current at 12:26Z; this pass did not independently re-query them. |

The e608 freshness snapshot records holds for Mise 9.18, Rust 1.98.1, Mise Action 5.0.0, Boxington 1.21.1 and its action 1.6.0. It also has an alint row pinned to 0.16.1 even though the committed action pin and CI use 0.17.0. Treat these as inventory state to reconcile with the current adoption task, not as new authorization to leave older pins in place.

## Official source proof for Mise Action 5.1.1

The official lightweight tag `v5.1.1` resolves to signed commit `2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca`, parent `3eb43e379b25fd45020fb54a44f614b26633539b`, tree `669c982e2765baa3f4974a32dced269c1eb8e789`. GitHub’s commit API reports `verified=true`, `reason=valid`. Exact official source bytes captured from that object are under `/root/.cache/velnor-task-targets/20261005/latest-tool-inventory-20261005/source/mise-action-v5.1.1/`:

| File | Git blob | SHA-256 |
|---|---|---|
| action.yml | `ca44ae318c8d9402934e2fd2879fbf58038f6b78` | `bf262f9440185cbf4516c48c4662e01ec54a821c51e447fbee394830ec7b721d` |
| src/index.ts | `2b740617ee46d76e23fc7dd029215b9c9c718fa3` | `a578f82ad1350017cb26e90dfb3abfe9d91ea7260f23820a07ec43d0604fcae3` |
| src/github-token.ts | `041983819ab04390603f3ad79c6ba7f72650decf` | `9a7304efb972deb1808e4b02ee53e3612eb2f62cafc18b9d1afc369a5cc0984b` |
| dist/index.js | `6aaf69649206f4ede494121a8d0579532833af76` | `ab0e0add61d1564c2cf5aca14b8253aec4e3d4c8256d19a0ee07fcc20c99e009` |

Official links: [Mise Action v5.1.1 release](https://github.com/jdx/mise-action/releases/tag/v5.1.1), [exact release commit](https://github.com/jdx/mise-action/commit/2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca), [Mise v2026.10.3 release](https://github.com/jdx/mise/releases/tag/v2026.10.3). The action’s `sha256` input is described as the binary digest and source hashes the installed Mise executable; when supplied, source skips its downloaded SHASUMS helper and compares the executable to the caller-supplied value. The changed token persistence default is the migration-sensitive behavior to test.

## Official source proof for jactionlint 1.8.2

Mise v2026.10.3 release notes and merged registry PR #13960 add `jactionlint` via `github:jdx/jactionlint` and deprecate the old registry name. The official [v1.8.2 release](https://github.com/jdx/jactionlint/releases/tag/v1.8.2) is published 2026-10-05T03:12:26Z at commit `e330e2a70eb69dedbffd2dee69161815509c3c2f`; GitHub reports a valid verified signature (key ID `B5690EEEBB952194`), parent `36f9ada9972c3f33c39b950dfcf83909e2bfdaa9`, tree `fa3a5be0ce5249b163ffbc2241486aa015950b29`. The release is maintenance-only; v1.8.0 made the rename and added new optional checks/workflow syntax.

The official latest-release JSON and GitHub’s asset `digest` metadata are recorded in `raw/jactionlint-release-latest.json`. Relevant Velnor platforms:

| Platform | Official asset | SHA-256 |
|---|---|---|
| Linux x86_64 | `jactionlint_1.8.2_linux_amd64.tar.gz` | `6533bd8047fc012a3f66238d84f65cacdac669da00b9f6cf042c1734c252bc93` |
| macOS ARM64 | `jactionlint_1.8.2_darwin_arm64.tar.gz` | `de9eb05af3cde409dc0b37fe2fd6a21b875a575374e7b26c02d45a7086645c28` |

I downloaded only the official checksum text (not the binaries); its SHA-256 matches the release API digest `24cef5185cbb97e84dc05048d379f9f74421ae85a18b22a98f5ecdb27128948e`, and its two platform lines match the release asset digests. The v1.8.0 release notes state that existing `.github/actionlint.yaml/.yml` files and ignore comments remain supported, while `.github/jactionlint.yaml/.yml` takes precedence. Current e608 source has `.github/actionlint.yaml` and invokes `actionlint`, so both the binary name and default config lookup need to be covered in migration tests. Official registry provenance: [Mise #13960](https://github.com/jdx/mise/pull/13960).

## Raw response and qualification evidence

Fresh raw official responses are saved in `/root/.cache/velnor-task-targets/20261005/latest-tool-inventory-20261005/raw/` and listed in `SHA256SUMS`. Relevant response SHA-256 values:

- Mise latest API: `9adbf8860c6cb466b70ddce68577e1b0eb8e7a1652b2b3efbdd9a6405e48386a` → v2026.10.3 (published 10:35:27Z).
- Rust latest API: `ad5514377a7ac4f8c65f63d9983da52dbc33e2de779869545b494fe6fbef07de` → 1.99.0 (published 2026-10-01 12:41:12Z).
- Mise Action latest API: `0995d158dec59aec2e0308434ff8943956b65730992212a6f4a68e70106e9df3` → v5.1.1.
- cargo-deny latest API: `19adfb8164482e12b5961ace07c22fa7f7b4d683d952f34bd1e04847860e6e39` → 0.20.2.
- actionlint latest API: `f649b78aa99fea6409f31267fa8acb409492127b4ebd3526975d0e3b9f35c7ec` → v1.7.12.
- jactionlint latest API: `09e5cc0ccd7b565fd4f6af6dd1a213c8fb6d80db1ddfdf6a4fbc3455e653817b` → v1.8.2.
- alint latest API: `f054e176755903c265dc2c0d04fb4cb6e95bf3e55745047df083b201523206be` → v0.17.0.
- actions/cache latest API: `fab1cd173714779bcd8cf67c96c3a29f04323b5a5389ad404afcd17abfb36e53` → v6.1.0.
- cargo-nextest crates.io API: `8e265c59f7a8bd8ea2e10b73777f1e44e31173129172d2cf495d2c37aa406a54` → max version 0.9.146.
- jactionlint tag ref, REST commit, Git commit and release-checksum-text SHA-256: `598829efef6c933a086ed55af4c83ea64f17e3dbf2425cd4d93dc4c0b6fe104c`, `968d94c682fb82f081b5fd5add785ef74798db872ac2e1e418ca245efeb26b78`, `b0fa7b8087add619c2daccd08b90603c257add6716199a20fa24b3c4e1a75df7`, and `24cef5185cbb97e84dc05048d379f9f74421ae85a18b22a98f5ecdb27128948e`.

The separate Mise 10.3 owner packet reports binary SHA-256 `8d48bc510b7d844fad0bc7156c0855c2a1e7230c51e498a7f6b63b021f8b57c5`, source/tag and four-test evidence at `/root/.cache/velnor-task-targets/20261005/no_python_policy-103-evidence`; its claim is limited to the focused Linux regression. The separate MBX 1.22 packet is `/opt/velnor-task-fixtures/20261005/mbx-1.22.0-official-provenance-20261005T1708Z`, with `artifact-files.sha256` SHA-256 `2c2c2f781d53fba7b6eabb3d6d00296d4540199445c1b666ef674cc9f9942852`, archive SHA-256 `eb3e8c74277dcc8314227ba2cd32ef00a9a362ec8a90cd06ef6580eb48a6b375`, and executable SHA-256 `124a1c7d856c2355e1a53b1e9bd04342d952eb6845b7edf7ac81d0eb1766f296`. Its only execution evidence is a non-root `--version` smoke; no cache command has been run.

No statement here claims full latest-tool qualification, macOS execution, hosted cache behavior, or any release/deletion readiness.
