# Native channel acceptance and failure matrix

This is observable acceptance for immutable per-source previews and manual
stable releases. It adds no proof boolean, provider waiver, or
self-authentication field to public configuration. The parent [channel
contract](native-preview-contract.md) owns semantics; [stable
coordination](binary-release-contract.md) and [Homebrew merge
admission](native-homebrew-merge-contract.md) retain their separate
authorities.

## Configuration and generated workflow acceptance

| Case | Required observation |
| --- | --- |
| Both channels omitted or disabled | No binary channel workflow or channel promotion is emitted. |
| Preview only, stable only, both enabled | One shared product/target/bin/companion inventory; only enabled channel roles and destinations are emitted. |
| Old root enabled/package/bin, preview-owned producer/products, singleton Homebrew source/formula | Strict unknown-field rejection; no aliases or old workflow fallback. |
| Missing/ambiguous version package; package outside declared products; unequal version inventory | Generation rejects rather than choosing the first product. |
| Preview version or source SHA is malformed | Generation rejects; runtime binds the exact full lowercase source SHA and canonical `preview-<sha>` tag. |
| Stable without matching enabled git-tag release-plz, one package, manifest, environment or v-version tag | Generation fails closed. |
| Release registry omitted | No Cargo-registry publication, credential or OIDC registry role; explicit crates-io mode retains its separate contract. |
| Version-only release PR | Static workflow inventory/ownership bytes remain identical; exact-source data resolution changes the resolved plan. |
| Stable manual prepare | Only pinned release-plz release-pr can run; no tag, build, release or promotion. |
| Stable manual release | Source-free native exact-SHA tag creation completes after retained intent and before any binary build. |
| Preview merge to main | Latest-main CI and trust gates pass before the preview job publishes the merged source SHA as its own immutable prerelease. |
| Push, tag, workflow_run or schedule presented to stable binary role | Stable role rejects; incoming image/package event families are not removed or widened. |
| Plz GitHub-release enablement, hooks, extra packages or alternate repository config | Effective config rejects/omits them; no empty immutable release precedes assets. |
| Producer channel/version/environment override | Typed request authority wins; arbitrary input cannot choose channel, source or version. |
| Source-free sign/publish/recovery job | No checkout, project resolver, Cargo build or repository updater/task; only qualified tools and admitted bytes. |

## Immutable preview publication and recovery

| Injected state or interruption | Required result |
| --- | --- |
| New eligible main source SHA | Derive the unchanged `P-preview.N+sha7` version and the unique `preview-<full-lowercase-sha>` tag; create a prerelease with exact source and closed asset inventory. |
| The preview tag is absent | Create that exact tag from the authenticated source SHA, reobserve it, then create the draft release. Do not combine tag and draft mutations. |
| Tag or release already exists for the same source SHA | Reconcile only if exact tag target, draft/prerelease state, source, receipt and complete asset bytes match; otherwise fail closed without mutation. |
| Different source SHA or version attempts to reuse an existing tag/release | Reject. Never move, overwrite, delete, or republish the existing preview. |
| Tag target changes, release becomes immutable, draft, or non-prerelease unexpectedly | Reject before further publication or tap promotion. |
| Failure before or during each upload | Retain the authenticated original candidate and observed asset identities; resume only exact missing work. No incomplete release or tap promotion is accepted. |
| Timeout after upload succeeded but response was lost | Reobserve exact name/ID/state/size/digest; never blindly duplicate or delete another asset. |
| Missing, extra, changed or duplicate committed asset | Closed inventory and actual bytes reject; matching version alone is insufficient. |
| Tag/receipt/inventory changes during download | Before/after generation reread rejects all acquired bytes for this attempt. |
| An older source SHA is retried after main advances | It cannot become current publication authority or replace the newer source preview. Its existing immutable publication remains untouched. |
| Current main or latest exact-source CI changes before commit | Unfinished older preview stops; retained source/evidence are not fresh publish authority. |
| Same original authenticated signed candidate retried | Missing exact work may resume; original signer/run/receipt bytes remain unchanged. |
| Unknown asset/origin or altered recovery envelope | No arbitrary adoption, cleanup, or reconstruction from untrusted release hints. |
| Source reader absent | Preview publication fails closed before writes. Stable's immutability reader remains mandatory. |

Each preview is a distinct prerelease keyed by its complete source SHA. Global
immutable releases remain enabled. A failed or superseded preview is never
rewritten to represent another commit; the next eligible main merge receives a
new tag and release identity. Recovery may finish only the original authenticated
source and signed bytes.

## Stable permanence and origin

| Case | Required result |
| --- | --- |
| Manual stable source lacks completed exact full CI or human approval | No permanent tag or publication. |
| Native version data, selected package, tag or source disagree | Reject origin and build no assets. |
| Tag exists before intent admission, or no original retained intent exists | Conflict, not an adopted release. |
| Main advances after authorized stable tag creation | Continue only the exact permanent tagged source; never switch to new main. |
| Signed receipt is not yet available at tag creation | Tag origin binds the actual manual run, source CI and tag; it does not invent a future receipt digest. |
| Tag creation succeeds before final origin upload | Rerun all jobs of the original same-source workflow; authenticate original intent and exact tag/annotation, record later completion attempt before building. |
| Matching draft interruption after signing | Reauthenticate original intent/origin and retained signed bytes; finish only exact missing work without rebuilding. |
| Existing immutable stable release matches completely | Continue distribution without mutation; different bytes or source reject. |
| Preview token or mutable release is supplied | It cannot satisfy stable origin, approval or immutable publication admission. |
| Stable tag or published asset would be moved/deleted/overwritten | Reject; preview behavior does not change this rule. |

## Both-channel Homebrew admission

| Case | Required result |
| --- | --- |
| Signed preview or stable receipt | Only the corresponding closed destination and deterministic channel branch may change. |
| Same formula path for both channels; channel absent/disabled | Generation rejects. |
| Patch changes executable Ruby, path, mode, alias or another file | Independent metadata-only validator rejects before writer credentials. |
| Preview generation changes before PR creation or final merge | Do not merge stale candidate; recapture latest verified generation and canonical base. |
| Generation changes concurrently with cross-repository merge | Reobserve and reconcile supersession; do not claim GitHub supplied an atomic source-generation precondition. |
| Required check fails, App identity is wrong, C/R incomplete, unresolved or undisposed feedback | Keep PR unmerged; empty comments, elapsed time and quota failure are not completion. |
| Same candidate already promoted | Authenticate historical exact PR/base/head/author/feedback/check/merge proof; equality alone grants no new write. |
| Stable permanent version is newer than candidate | No downgrade; preview count ordering is not stable SemVer authority. |
| Preview source differs from the formula's signed receipt or binary digest | Checksum/source admission fails closed; each immutable source URL remains tied to its exact bytes. |
| Stable formula needs new executable install behavior or a Cask | Ordinary independently reviewed source migration first; no metadata patch expansion. |

## Delivery evidence

Required local gates include meaningful old-condition RED controls, type/config
boundaries, real composed generation, all caller/fixture migrations, fixed-source
CLI output parity, pinned workflow lint/security, strict quality and full
repository checks. Version preparation and a genuinely released,
API/crypto/runtime-verified Velnor generator precede consumer regeneration. Never
hand-edit managed workflows.

Then require actual exact-head PR gates and complete final feedback, exact merged
full main CI, six successful cold payloads, assembly and signatures,
channel-correct publication, and real Homebrew prebuilt installation. Exercise
recovery of an interrupted signed preview without changing its source/tag, and
publish previews from two successive eligible main merges, verifying distinct
immutable tags and successful Homebrew installation from binaries. Separately
exercise a manually approved stable tag/release and its native stable formula
promotion. No mocked API, build timing hypothesis, or manually edited formula
counts as those live acceptances.
