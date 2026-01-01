# Owned tool publication execution

Status: source staging, typed candidate generation, and publisher checks implemented;
reviewed source refs and immutable source assets published; hosted binary qualification pending.
No upstream repository writes or qualified tool/generator binary promotion occurred. Source-only
receipts and refs are not qualified distribution records.

## Observed infrastructure

Authenticated GitHub inspection on 2026-10-03 establishes admin/push access to
`tailrocks/velnor-new`, Actions enabled, and no configured protected environments.
Workflow token defaults are write-capable and review approval is enabled; the
initial ruleset listing was empty. Main ruleset `24396608` and tag ruleset
`24397132` are now active, independently reconciled with canonical IaC state.
Each new workflow/job must explicitly narrow token permissions. None of these settings proves a protected publisher exists.
Live workflow inventory contains CI, Upstream Freshness, and the old `velnor.yml`
CI record. The organization repository census contains no owned Mise/MBX fork.
Parent authorization allows qualified tool assets under generator-repository
releases; creating an owned-source repository remains a separate implementation
decision. Existing `v0.1.0` initially reported `immutable=false`; its bytes must
not be replaced. A later latest-pointer metadata correction caused GitHub to
report `immutable=true` for that unchanged older release. That service state
does not qualify its build provenance or runtime.
The initial immutable-release API reported `enabled=false`. The reviewed setting
change now reports `enabled=true`, `enforced_by_owner=false`. New source releases
were immutable on publication; updating older release metadata also caused a
measured false-to-true immutability transition for `v0.1.0`. Exact receipts live in
`docs/reviews/ci-performance-protection-execution.json`. Existing main Required
checks remain failed; these settings do not qualify a generator runtime.

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

Historical action source staging succeeded at `06f353d41002af758d27490164f53c82e2165637`,
tree `810f992dc95bd1efe3db25e925d7b2b727c1bc27`. The exact committed executable
bundle SHA256 is
`a683a4fb2bc65b0c4447137c483cbe88dfe0ab80abb47ca864616fbee0d89489`.
This is local source staging, with no hosted run or artifact attestation.
That checkpoint is withdrawn: protected-save ordering was incorrect. Replacement
source `c3cbe8e56ccb4727624df45022357f49d2953075`, tree
`57a9336f26b9ce4a31f5f914c17594ecaa9c1248`, reports 108 owner tests and 52
independent security cases passed; its bundle SHA256 is
`b33856d88154e096439d1815a14118de72d0988d955ce213b2f530b6e32a8c22`.
Replacement source closure and trigger review passed. Its exact source commit
is published at `refs/heads/owned-source/mbx-action/c3cbe8e56ccb4727624df45022357f49d2953075`
in `tailrocks/velnor-new`; public commit/ref APIs confirm its tree. Do not publish
the historical checkpoint or treat local tests as hosted qualification.

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
  --source <PRIVATE_TMP> \
  --output <PRIVATE_TMP>
rtk proxy python3 scripts/stage-owned-tool-source.py mbx \
  --source <PRIVATE_TMP> \
  --output <PRIVATE_TMP>
rtk proxy python3 scripts/stage-owned-tool-source.py mbx-action \
  --source <PRIVATE_TMP> \
  --output <PRIVATE_TMP>
```

Current upstream bases:

| Source | Exact upstream base |
|---|---|
| Mise 2026.10.0 | `bc11f90c74eba23bf0d7350efb540e62fb7d9ffd` |
| MBX 1.21.1 | `a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313` |
| MBX action 1.6.0 | `1687e54eb349cadf61fa38b5813a77875489e8e6` |

Mise reviewed source is DCO signed-off commit
`dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96`, tree
`d5eefb0470da013d4f524555df763f16a0faf22d`. Local staging measured source archive
SHA256 `a2ed2eec09aecf7c964fbf408d6c50fae2ec62a8354f4afb9b5ddaa3812f5d92`
and base patch SHA256
`1faa8dd229d3f403695969fafb41cf5855e68d60fee04c456b851fb95a53637b`.
Its debug binary reports
`2026.10.0-owned-cargo-wrapper-DEBUG`; debug qualification cannot become release
qualification. Exact source-owner-approved release features are
`--release --locked --no-default-features --features native-tls,vfox/vendored-lua,owned-cargo-wrapper`;
the optimized banner must be `2026.10.0-owned-cargo-wrapper`.
MBX source owner is finishing relocation
corner-case fixes; its intermediate commit cannot identify final artifacts.
The source owner reported an initial native Mise qualification with 31 passed
cases and four failed positive routes (35 total). Reviewed replacement passed
57 local macOS ARM64 cases. Retain the failed evidence; neither local result
qualifies an optimized hosted release.

Both final source refs are reachable in the owned generator repository. Mise uses
`refs/heads/owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96`.
Independent full-tree event review and post-push API observations found no
workflow run triggered by either source-ref push. Their raw Git commit objects
contain DCO signoffs, no cryptographic signatures. Source release tags must point
to reviewed generator commit `c57c700459bbe1549fe7eedcb7d8689585c38986`,
never the foreign source trees with imported release triggers. Source-only
releases retain source archives, patches, receipts, and raw commit objects;
behavioral qualification and signed build provenance remain absent.
The first actual Mise source transaction retained draft release `402229309`
with all five measured assets. GitHub's published-release-by-tag endpoint returned
404 for that draft; initial promotion did not occur. Creation now captures the
release ID directly. Independent recovery review admitted only that witnessed
draft and its five existing asset IDs; no replacement upload or tag change
occurred. Both source releases are now published with `immutable=true`: Mise
`402229309` and action `402232689`. Actual URLs, asset IDs and service digests
live in `docs/reviews/ci-performance-owned-source-publication.json`. Independent
postpublication checks downloaded all ten assets and matched every reviewed
hash; no unexpected publication workflow ran. A later live check found the
source-only action release replaced GitHub's global latest pointer. A narrow
metadata correction now points latest discovery to `v0.1.0`. All thirteen
asset IDs, sizes and digests remain unchanged across the three releases.
GitHub additionally changed the older release's immutable flag from false to
true; no build provenance or runtime qualification follows from that flag.
Explicit string `make_latest=false` policy and regressions are implemented in
source-publisher revision `e824659`, independently reviewed with 15 focused
isolated tests. Preserve the original historical API observation; the backend
mechanism of the older release's state transition is unknown. Retain the
failed transaction evidence; never silently adopt a collision.

## Implemented generator infrastructure

`generate --owned-tool-candidates-only --output-dir <external-empty-directory>`
requires canonical generator identity and reviewed
`.velnor/owned-tool-sources.json`. No approval file or published URL is fabricated.
The Mise adapter admits exact source evidence; the renderer receives opaque
bindings. This pure preview route avoids the main installation planner's need
for the owned artifacts it must first build.

The emitted workflow supports default-branch dispatch or the closed
`owned-tool-candidates` infrastructure push category. Both bind the exact
repository, event/ref, and workflow source SHA. Current-run API admission now
runs before artifact download; qualification replays the retained repository,
run and workflow API bytes before candidate execution. Publisher evidence
integration remains pending. It builds all three native tool hosts with
explicit read-only permissions, verifies source tree/base patch/lock/licenses,
retains actual compiler output and MBX summaries, measures archive and binary
hashes, and uploads candidates once. Fresh native qualification jobs check out
policy independently and admit exact same-run artifact IDs and ZIP hashes before
executing those bytes. Candidate and failed qualification receipts upload once.
MBX qualification rejects unavailable suites.
There are no signer or publisher credentials in these jobs.

`scripts/publish-owned-tool-artifacts.py` independently checks all three host
receipts, fixed recipes, exact signed qualification predicates, downloaded
attestation bundles, immutable repository policy, fresh tag/release identities,
and re-downloaded asset bytes. It has no generated invocation until protected
signing and complete hosted evidence exist. Isolated helper tests prove admission
and rejection paths; they are not GitHub publication evidence.
Final signing binds the measured behavior ABI/report, candidate receipt, and
Actions artifact ID/ZIP digest. Full per-host qualified receipts remain durable
release evidence; a signed `passed=true` alone cannot establish that chain.
The standalone helper unit committed at `e2e6ff99a38de0e51c550eb9880d313c59990180`
passed 58/58 isolated tests independently. Its sole index correction removed one
trailing newline; AST equality and exact remaining file bytes were verified.
Committed patch passed strict apply and whitespace checks. Rust compilation and
real hosted qualification remain separate uncompleted gates.

## Build and promotion order

1. Source owners finish independent review, DCO sign off and commit each owned source.
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
Required next units are final MBX source commit/publication, compiled source
preview generation, hosted candidate runs, protected signing/publication
jobs, and typed owned Mise acquisition. Official MBX source-builder bootstrap
requires actual per-host archive and installed-byte qualification; a version-only
`mr-boxington@1.21.1` request does not complete that evidence. Upstream rows for
this bounded builder cannot satisfy the owned native transport runtime contract.
`catalog_qualification_records.rs` intentionally fails until all required owned
host artifacts are actually published and qualified. No catalog placeholder or
official-to-owned alias may bypass that failure.
