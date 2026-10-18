# Native preview implementation status

The normative contract is [native-preview-contract.md](../proposed/native-preview-contract.md).

## Implemented contract layer

The Rust binary-release configuration has an independently enabled preview policy.
The top-level distribution configuration has a closed Homebrew tap destination.
Both default to disabled. Preview-only operation is subject to consumer policy;
a destination cannot reference a missing or disabled preview channel. Producer
inputs, qualified Mise tools, optional native macOS Swift/Xcode exact pins, runner
platform, resource budgets, product inventories, companion paths, evidence
policy, and environment boundaries are validated. Native tool pin declarations
reuse the named-check `CheckSystemTool` contract, require exact version/build
identity, and are valid only on macOS producer runners. They describe existing
host tools; they do not install or download an SDK. The producer reuses the
existing named-check preparation and system-tool verification boundary.

The pure preview identity derives the version and immutable tag from a numeric
Cargo base version, the full source SHA, and the full-history commit count.
The canonical `velnor.package-release.v2` receipt validates the entire asset set,
source/workflow/run identity, product layouts, evidence declarations, and stable
versus preview identity rules. Parsing rejects duplicate keys, unknown fields,
noncanonical bytes, missing/extra assets, and inconsistent subject bindings.

The unprivileged producer entrypoint and CLI operation now bind the canonical
static plan to a push on the default branch, exact repository/workflow/SHA
context, source Cargo inventory, and full-history commit count. It executes only
the isolated declared task, verifies its output manifest and bytes, and stages
the raw upload inventory under runner temp. This is producer-side runtime code;
it does not establish that the generated workflow, signer, or publisher works.

The retry planner has explicit non-overwriting transitions for absent identities,
tag-only interruption, authenticated partial drafts, and complete immutable
releases. Its input contract requires independently authenticated observations,
original evidence/bytes, and fresh eligibility before every mutation. Structural
receipt validation is not cryptographic verification or publication authority.

Acceptance tests cover the four stable/preview enablement combinations, missing
source channels, qualified tool/platform and process limits, closed evidence and
credential boundaries, path and inventory collisions, deterministic identities,
strict receipts, and each partial asset-upload boundary. Jackin's inventory is
six archives and 29 release assets, of which 27 appear in the nonrecursive receipt.

## Remaining runtime and delivery gates

Preview workflow generation now separates exact-commit eligibility, unprivileged
production, signing, immutable publication, and original-evidence recovery. The
eligibility job checks out the exact workflow SHA with complete history before
running any Git-dependent gate. Privileged jobs acquire the official pinned
helper without checking out source. Recovery authenticates the original run and
signed bytes; it never rebuilds or re-signs them.

Runtime implementations independently validate archive contents, canonical
receipts, Cosign signatures, GitHub attestations, interrupted tag/draft state,
and remote asset bytes. Archive admission rejects malformed gzip trailers,
concatenated streams, trailing tar data, extended headers, unexpected members,
and unsafe executable modes. The Homebrew preparation and App writer boundaries
capture tap main, qualify its updater without write credentials, independently
validate literal formula changes, and reconcile an exact deterministic PR.
These implementations still require full integration and executable-workflow
verification. Homebrew job generation and the merge runtime remain unfinished.
Stable publication still uses its existing runtime receipt until the common
publisher migration is implemented. No preview release or unattended tap merge
is authorized merely by decoding configuration.

The feature must still pass generated and executable workflow, security-boundary,
retry, and regression tests; receive independent review; and ship in an official
Velnor release. Jackin and tap integration, repository settings and credentials,
exact-head review-completion integration, and two real consecutive Homebrew
upgrades remain required completion gates. No live pipeline success is claimed.
