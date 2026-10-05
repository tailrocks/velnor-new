# Owned tool publication: historical evidence

Status: candidate staging, generation, qualification, and publisher utilities
were retired on 2026-10-05. Reviewed source refs and immutable source assets
remain published; hosted binary qualification and qualified tool/generator
binary promotion never occurred. Source-only receipts and refs are not
qualified distribution records.

## Observed infrastructure

The infrastructure and policy details in this section are historical
observations from the recorded reviews, not current release instructions.

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

The then-current candidate publisher was unsuitable: it uploaded one generator
target, used `--clobber`, and did not attach signed build provenance. Independent
threat review confirmed these gaps against `candidate.rs`. The historical review
recommended the typed release admission and noncanceling publication lock; the
owned candidate pipeline was retired and this recommendation is not an active
publisher procedure.

## Historical source staging evidence

The retired staging utility required a clean committed repository root and the
exact upstream base as an ancestor. It emitted source archives, base patches,
and receipts with source/tree and archive/patch/lock/license hashes. The hashes,
source refs, and test outcomes below document completed source-only work; no
staging command remains supported. Raw Git blobs, export attributes, gitlinks,
symlink checks, and destination protections were tested for that utility only.

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

Upstream bases at staging time:

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
At that time, the MBX source owner reported relocation corner-case fixes in
progress; its intermediate commit could not identify final artifacts. The source
owner reported an initial native Mise qualification with 31 passed
cases and four failed positive routes (35 total). Reviewed replacement passed
57 local macOS ARM64 cases. Retain the failed evidence; neither local result
qualifies an optimized hosted release.

At the recorded API check, both final source refs were reachable in the owned
generator repository. Mise used
`refs/heads/owned-source/mise/dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96`.
Independent full-tree event review and post-push API observations found no
workflow run triggered by either source-ref push. Their raw Git commit objects
contain DCO signoffs, no cryptographic signatures. The publication rule for
those source-only releases required their tags to point to reviewed generator
commit `c57c700459bbe1549fe7eedcb7d8689585c38986`, never the foreign source trees
with imported release triggers. The releases retain source archives, patches,
receipts, and raw commit objects;
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

## Retired candidate-generation design

The PR implemented a candidate-only generator route, event and artifact
admission, build receipts, and publisher checks. The route was not activated:
the default-branch workflow and candidate branch were not found, no candidate
binary received hosted qualification, and MBX native qualification was
unavailable. No signer or publisher credentials were used. The generator route,
owned build/qualification tools, and their dedicated tests were retired as one
feature closure.

An isolated helper revision passed 58/58 tests, and the source-publisher
revision passed 15 focused isolated tests. Those results are historical code
evidence only; they do not establish a hosted run, qualified binary, or signed
build provenance. The published source-only releases and their measured asset
hashes above remain passive evidence.

## Qualification status at retirement

The source-only stages did not complete native candidate builds, hosted
qualification, protected signing, or typed owned-Mise acquisition. No build,
promotion, or consumer rollout procedure remains in force. The existing source
releases do not qualify binaries, and this cleanup does not change their refs,
tags, receipts, or assets.
