# Native preview implementation status

The normative contract is [native-preview-contract.md](../proposed/native-preview-contract.md).

## Implemented contract layer

The Rust binary-release configuration has independently enabled preview and
stable policies sharing one product inventory. Homebrew distribution declares
closed channel destinations; promotion requires its selected channel to be
enabled. Both release channels and tap promotion default to disabled. Producer
inputs, qualified Mise tools, optional native macOS Swift/Xcode exact pins, runner
platform, resource budgets, product inventories, companion paths, evidence
policy, and environment boundaries are validated. Native tool pin declarations
reuse the named-check `CheckSystemTool` contract, require exact version/build
identity, and are valid only on macOS producer runners. They describe existing
host tools; they do not install or download an SDK. The producer reuses the
existing named-check preparation and system-tool verification boundary.

The pure preview identity derives the version from a numeric Cargo base version,
source SHA A, and the full-history commit count. The immutable public tag is
`preview-<full-A-sha>`; stable uses a permanent version tag.
The canonical `velnor.package-release.v2` receipt validates the entire asset set,
source/workflow/run identity separately (A for the source, B for the workflow
authority), product layouts, evidence declarations, and stable versus preview
identity rules. Preview requires authenticated `A <= B <= H`; stable retains
its existing identity rules. Parsing rejects duplicate keys, unknown fields,
noncanonical bytes, missing/extra assets, and inconsistent subject bindings.

The native selector authenticates exact-source CI A and current workflow
authority B before checkout. A then enters a credential-free, complete-history
checkout; canonical A configuration, package inventory (via bounded pure
manifest projection), declared task and qualified-tool declarations are
admitted before source task execution. Native task discovery uses
`discover_preview_producer`. B supplies workflow custody and trusted bootstrap
tools. Preview source execution uses the sealed A projection directly, without
general workspace discovery or Cargo metadata bootstrap from B. A's declared
qualified task acquires its own tools in an isolated home; source checkout also
uses A's `checkout_lfs` setting. Stable keeps its existing preparation path. Preview versioning binds A and its complete-history count. A bounded
source matrix builds the complete declared product/target inventory, and
assembly verifies the complete GitHub API artifact union by immutable IDs and
bytes. Native request and artifact contracts are described in
[native-preview-production.md](../proposed/native-preview-production.md).
This status describes source implementation and tests; it does not establish
that the generated workflow, signer, or publisher works in hosted execution.

Preview publication creates one immutable prerelease for A's full-SHA tag and
never replaces, deletes, recreates or republishes it. Interrupted publication
resumes only with the original retained signed bytes and authenticated receipt;
conflicting retries fail closed. Stable tags and published stable versions are
never replaced. Every mutation requires independently authenticated observations
and fresh eligibility. Structural receipt validation is not cryptographic
verification or publication authority. A later protected tip does not revoke
an ancestor A release when `A <= B <= H` remains valid, but Homebrew promotion
proceeds only while A is the current protected `main` tip; otherwise it returns
a typed clean skip.

Acceptance tests cover the four stable/preview enablement combinations, missing
source channels, qualified tool/platform and process limits, closed evidence and
credential boundaries, path and inventory collisions, deterministic identities,
strict receipts, and each partial asset-upload boundary. Jackin's inventory is
six archives and 29 release assets, of which 27 appear in the nonrecursive receipt.

## Remaining runtime and delivery gates

Preview workflow generation separates source selection, credential-free A
checkout/admission, unprivileged production, signing, immutable publication,
and original-evidence recovery. Source selection authenticates A and B before
checkout. Eligibility then checks out A with complete history before running
source-derived gates; B remains the workflow and bootstrap custody authority.
Privileged jobs acquire the official pinned helper without checking out source.
Recovery authenticates the original run and signed bytes and rechecks exact-A CI
plus `A <= B <= H`; it never rebuilds or re-signs them.

Current execution authority also requires the unique generated role's active
job in the exact run attempt. Aggregate `queued` is permitted only with this
job-level proof, because GitHub can report it while some producer jobs execute.
Only immutable job locators are cached; later guards refresh the individual job.
Original signed-run evidence does not confer authority on a recovery or tap
caller. See the [official compatibility sources](../proposed/github-actions-reference.md#native-release-job-authority-checked-2026-10-21).

Runtime implementations independently validate archive contents, canonical
receipts, Cosign signatures, GitHub attestations, interrupted tag/draft state,
and remote asset bytes. Archive admission rejects malformed gzip trailers,
concatenated streams, trailing tar data, extended headers, unexpected members,
and unsafe executable modes. Homebrew preparation captures tap main; the native
route derives the selected formula from its typed specification, while the
source-qualified custom route runs its updater without write credentials.
Both routes independently validate literal formula changes before the App
writer boundaries reconcile an exact deterministic PR.
These implementations include generated Homebrew preparation/writer/merge jobs
and exact-head feedback/check gates. They still require full integration, actual
review-completion credentials, and executable-workflow verification.
Stable uses the shared product inventory, canonical receipt and channel-aware
publisher, with permanent tags and immutable releases. No release or unattended
tap merge is authorized merely by decoding configuration.

The shared channel feature shipped in Velnor 0.1.30. Jackin
run `39495895585` exposed the aggregate-status admission failure before a native
release was published; the active-job correction still requires its official
release and hosted consumer qualification. Jackin and tap integration, repository
settings and credentials, exact-head review-completion integration, actual stable
publication, and two real consecutive Homebrew upgrades remain completion gates.
Local acceptance and security tests do not establish live pipeline success.
