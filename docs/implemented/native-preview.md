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
the full source SHA, and the full-history commit count. Its only public tag is
literal `preview`; stable uses a permanent version tag.
The canonical `velnor.package-release.v2` receipt validates the entire asset set,
source/workflow/run identity, product layouts, evidence declarations, and stable
versus preview identity rules. Parsing rejects duplicate keys, unknown fields,
noncanonical bytes, missing/extra assets, and inconsistent subject bindings.

The unprivileged producer entrypoint and CLI operation now bind the canonical
static plan to the channel event, exact repository/workflow/SHA context and
source Cargo inventory. Preview requires successful exact-source default-branch
push CI and enters through `workflow_run`; stable uses its manual coordinator.
Preview versioning additionally binds the full-history commit count. It executes only
the isolated declared task in one literal product/target job or complete-union
assembly job, verifies partial/full manifests and bytes, and stages the exact
raw upload inventory under runner temp. Native request and artifact contracts
are described in [native-preview-production.md](../proposed/native-preview-production.md). This is producer-side runtime code;
it does not establish that the generated workflow, signer, or publisher works.

Rolling publication requires a qualified existing mutable release with its fixed
positive ID. Its authenticated journal stages and verifies a complete signed
generation, replaces the authenticated old assets, verifies the canonical
inventory and tag, then writes the commit marker. Intermediate asset listings
are not an atomic publication guarantee.
Interrupted publication reuses the original signed bytes; stale candidates and
conflicting retries are rejected. Stable tags and published stable versions are
never replaced. Every mutation requires independently authenticated observations
and fresh eligibility. Structural receipt validation is not cryptographic
verification or publication authority.

Acceptance tests cover the four stable/preview enablement combinations, missing
source channels, qualified tool/platform and process limits, closed evidence and
credential boundaries, path and inventory collisions, deterministic identities,
strict receipts, and each partial asset-upload boundary. Jackin's inventory is
six archives and 29 release assets, of which 27 appear in the nonrecursive receipt.

## Remaining runtime and delivery gates

Preview workflow generation now separates exact-commit eligibility, unprivileged
production, signing, rolling publication, and original-evidence recovery. The
eligibility job checks out the exact workflow SHA with complete history before
running any Git-dependent gate. Privileged jobs acquire the official pinned
helper without checking out source. Recovery authenticates the original run and
signed bytes; it never rebuilds or re-signs them.

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
and unsafe executable modes. The Homebrew preparation and App writer boundaries
capture tap main, qualify its updater without write credentials, independently
validate literal formula changes, and reconcile an exact deterministic PR.
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
