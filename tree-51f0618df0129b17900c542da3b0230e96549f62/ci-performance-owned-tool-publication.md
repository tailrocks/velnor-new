# Owned tool publication execution

Status: source staging implemented; publication and hosted qualification pending.
No upstream repository writes or artifact promotion occurred. Local source-only
receipts are not qualified distribution records.

## Observed infrastructure

Authenticated GitHub inspection on 2026-10-03 establishes admin/push access to
`tailrocks/velnor-new`, Actions enabled, and no configured protected environments.
Workflow token defaults are write-capable and review approval is enabled; the
ruleset listing is empty. Each new workflow/job must explicitly narrow token
permissions. None of these settings proves a protected publisher exists.
Live workflow inventory contains CI, Upstream Freshness, and the old `velnor.yml`
CI record. The organization repository census contains no owned Mise/MBX fork.
Parent authorization allows qualified tool assets under generator-repository
releases; creating an owned-source repository remains a separate implementation
decision. Existing `v0.1.0` reports `immutable=false` and must not be replaced.

The current candidate publisher is unsuitable: it uploads one generator target,
uses `--clobber`, and does not attach signed build provenance. Independent threat
review confirmed these gaps against `candidate.rs`. Reuse the typed release
admission and noncanceling publication lock, not that upload implementation.

## Source staging

`scripts/stage-owned-tool-source.py` requires a clean committed repository root
and the exact current upstream base as an ancestor. It emits a source tar, binary
full-tree base patch, and receipt containing measured source commit/tree,
archive/patch/lock/license hashes. Action receipts additionally hash the committed
executable bundle. Raw Git blobs build the archive: Git export attributes cannot
omit locks/licenses or substitute source text. Gitlinks and escaping symlinks
fail. Existing stage directories fail rather than replace files.

Action source staging succeeded at `06f353d41002af758d27490164f53c82e2165637`,
tree `810f992dc95bd1efe3db25e925d7b2b727c1bc27`. The exact committed executable
bundle SHA256 is
`a683a4fb2bc65b0c4447137c483cbe88dfe0ab80abb47ca864616fbee0d89489`.
This is local source staging, with no hosted run or artifact attestation.

Independent `publication_threat_review` executed isolated Git fixtures on the
final script. Normal staging, bounded links, exact raw blobs under
`export-ignore`/`export-subst`, and patch reconstruction of the recorded tree
passed. Duplicate destinations, subdirectory sources, direct/absolute/chained
symlink escapes, and direct/indirect symlink cycles were rejected. Initial review
found export-attribute, subdirectory, and chained-link proof gaps; each received
a structural fix and an independent rerun. No remaining finding in tested
source-staging scope; no shared Cargo build was run.

```sh
rtk proxy python3 scripts/stage-owned-tool-source.py mise \
  --source /tmp/velnor-mise-owned-sourcefix-2026.10.0 \
  --output /tmp/velnor-publication-mise-source-stage
rtk proxy python3 scripts/stage-owned-tool-source.py mbx \
  --source /tmp/velnor-mbx-source-1.21.0 \
  --output /tmp/velnor-publication-mbx-source-stage
rtk proxy python3 scripts/stage-owned-tool-source.py mbx-action \
  --source /tmp/velnor-mbx-action-work \
  --output /tmp/velnor-publication-action-source-stage
```

Current upstream bases:

| Source | Exact upstream base |
|---|---|
| Mise 2026.10.0 | `bc11f90c74eba23bf0d7350efb540e62fb7d9ffd` |
| MBX 1.21.1 | `a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313` |
| MBX action 1.6.0 | `1687e54eb349cadf61fa38b5813a77875489e8e6` |

Mise still requires its reviewed source commit. Its debug binary reports
`2026.10.0-owned-cargo-wrapper-DEBUG`; debug qualification cannot become release
qualification. Release builds must enable `owned-cargo-wrapper`, omit
`self_update`, retain required TLS/Lua features, and prove the actual optimized
banner. The source owner must approve and exercise the exact feature argument
vector before freezing its recipe. MBX source owner is finishing relocation
corner-case fixes; its intermediate commit cannot identify final artifacts.
The source owner reported an initial native Mise qualification with 31 passed
cases and four failed positive routes (35 total). A revised Toolset wrapper-provisioning fix is in
progress; retain the failed evidence and do not publish the initial artifact.

## Build and promotion order

1. Source owners finish independent review, sign and commit each owned source.
   Stage those exact revisions. Publish source identities in an owned repository
   or source release with its full-tree source evidence; never claim a local SHA
   exists in an official upstream repository.
2. Generator infrastructure produces a bounded source-only qualification
   workflow. Build native Linux AMD64, Linux ARM64, and macOS ARM64 tool artifacts
   from the same exact staged source, through verified pinned bootstrap tooling.
   Build jobs receive no release/OIDC credentials. Preserve effective compiler,
   linker, image, exact arguments, source/lock, run/attempt/workflow, and artifact
   IDs. Create each candidate once and retain both archive and executable SHA256.
3. Independent jobs consume those exact artifacts without rebuilding. Prove
   Mise NoConfig excludes every `.miserc` discovery phase; wrapper dispatch
   admits only the canonical absolute SHA-verified MBX and invokes native Cargo.
   Prove MBX native transport/domain/useful-state and relocation behavior through
   fresh hosted runners. Qualified action bundle consumes the measured executable
   and rejects altered paths/digests before execution.
4. Protected publisher verifies complete same-source receipts and signed
   source/workflow-bound attestations. Serialize by release identity; reject
   existing tags/releases/assets. Upload without replacement, download and hash
   every uploaded asset, then verify provenance before publishing. Never hand
   build a consumer workflow to obtain this evidence.
5. Descriptor owner records only actual qualified published URLs, reported owned
   versions, archive/executable hashes, source commit/tree, and behavior ABI.
   Typed owned Mise acquisition must land first: pinned Mise action hardcodes
   upstream URLs and has no custom artifact input. Passing an owned banner to it
   cannot establish a cold bootstrap path.
6. Freeze qualified generator source after tool records land. Build all generator
   targets: Linux AMD64, macOS ARM64, macOS AMD64. Assemble its manifest from
   measured final bytes after building, qualify exact artifacts, publish through
   protected release flow, then adopt byte-identical manifest and generator lock
   in separate reviewed changes. This ordering avoids the binary digest cycle.

## Remaining execution gates

The staged sources do not constitute hosted evidence or signed provenance.
Required next units are final source commits, generator-only typed qualification
workflow, protected immutable publisher, and typed owned Mise acquisition.
`catalog_qualification_records.rs` intentionally fails until all required owned
host artifacts are actually published and qualified. No catalog placeholder or
official-to-owned alias may bypass that failure.
