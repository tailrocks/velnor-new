# Homebrew release qualification

The historical Homebrew qualifier separates authenticated release verification
from the hosted macOS install. GitHub CLI custody uses Linux facilities, while
Homebrew requires the hosted ARM macOS runner. The previous qualifier combined
both operations in one process and supplied a GitHub token to that process.

`admit-homebrew-qualification-v1` runs on Linux. It calls the existing full
historical verifier for A and D, including protected-main lineage, immutable
release and asset identities, signed receipts, cosign, SLSA evidence, producer
runs, and A's recovery admission. It writes `homebrew-admission/admission.zip`
with the canonical admission, both raw receipts, and the archives the ARM Mac
installation needs. A separate `homebrew-admission/admission.json` provides the
canonical admission digest for the workflow output.

The canonical admission binds the repository, source commit, workflow commit
and ref, run ID and attempt, candidate manifest digest, and protected-main tip.
It also binds the exact plan, formula specification, A and D records, and A's
recovery admission. Every exported file has a size and SHA-256. Each pinned
receipt contains the complete historical release asset inventory and hashes.

`qualify-homebrew-v1` runs on a fresh hosted ARM Mac. The workflow downloads the
single bundle by its exact artifact ID. Upload uses `archive: false`, and
download uses `skip-decompress: true`. The generated dependencies supply the
raw bundle's SHA-256 and size together with the canonical admission digest.
The process checks its own source-bound candidate binary, the raw bytes,
the admission context, the exact ZIP entry inventory, both pinned receipts,
and the selected archives. It creates a private `AdmittedLocalRelease`
only after these checks. This type cannot be deserialized or substituted for
`VerifiedPublicPackageRelease`.

The raw bundle cannot exceed 2 GiB. The process checks the ZIP directory count
and size before constructing the ZIP parser. The bundle permits at most 131
regular, stored entries, derived from `2 * (64 + 1)` handoff files plus
`admission.json`. It rejects duplicate paths, traversal paths, symlinks,
compressed entries, ZIP64, and comments. Each JSON file cannot exceed 1 MiB,
and each payload archive cannot exceed 256 MiB. ZIP contents remain in memory.
No downloaded artifact is extracted to the filesystem.

Payload archive parsing and expected-member checks precede the Homebrew profile
replacement. The existing real A install, D upgrade, preserved-keg checks, and
disabled stable formula check remain required. The process writes
`homebrew-result/result.json` only after all these operations succeed.

The Mac process starts through `env -i` with explicit noncredential context.
The Rust entry point also rejects every environment variable outside its
allowlist. GitHub tokens, OIDC request credentials, and Actions runtime
credentials cannot enter the candidate environment. The Mac platform encoding
variable must contain the expected numeric fields for the current user.
The workflow's artifact
actions retain their own platform credentials outside that process.

`finalize-homebrew-qualification-v1` runs on a separate Linux runner after the
Mac job succeeds. The result is one canonical JSON file, uploaded without a
transport archive. The finalizer downloads the exact admission and result
artifacts and checks their raw sizes and digests, canonical digests, context,
and success fields. It then repeats
the full historical verifier and requires the new admission to equal the
original admission. A changed protected-main tip, receipt, payload, catalog,
or run context fails finalization. Publication depends on this finalizer.

The trust model requires a fresh GitHub-hosted runner and independently checked,
reviewed source-bound candidates. Admission and result artifacts are trusted
through the generated job dependencies, exact artifact IDs, and content
digests. These checks do not isolate an adversarial root process or authenticate
an arbitrary candidate that can replace the trusted job's outputs.

The external download action writes raw bytes before the owned size check.
The workflow does not claim to bound network transfer or temporary disk use.
The owned reader checks the resulting file before ZIP parsing or privileged
operations. A refusal leaves raw input in the fresh hosted runner's temporary
workspace, which GitHub discards with that runner.

The local boundary suite runs with:

```sh
cargo nextest run --locked --offline -p velnor-actions-orchestrator --lib \
	-E 'test(homebrew::qualification::handoff)' --no-tests fail
```

The suite checks deterministic admission, file, archive, environment, result,
and drift refusals. It also invokes the public Mac entry point in a subprocess
to reject unadmitted bytes and credentials before Homebrew activation. Local
tests do not establish a successful hosted A-to-D run. That execution remains
a required release-workflow check.
