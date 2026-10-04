# Native APT delivery

V1 generates an APT consumer workflow and fixed native verification helpers. It
does not interpret a task graph, execute runner protocols, or install a Velnor
runtime. The generated family owns `.github/workflows/delivery-apt.yml`,
`.github/velnor/apt-delivery.jsonc`, and the helper paths listed in the renderer's
`APT_DELIVERY_TREE_PATHS` constant. The JSONC file consists of the exact generator
marker followed by a canonical JSON object. Its marker must match the helper.

## Configuration

`[delivery.apt]` is optional. When present it rejects unknown fields. Its string
fields are required except for the documented branch default:

| Field | Contract |
| --- | --- |
| `source_repository` | Exact GitHub owner/repository serving package releases |
| `consumer_repository` | Exact GitHub owner/repository owning this feed |
| `package` | Debian package name |
| `binary` | Packaged executable basename |
| `identity_directory` | Packaged identity basename under `usr/share` |
| `manifest_schema` | Exact preview package-manifest schema identifier |
| `keyring` | Safe repository-relative public keyring path |
| `signer_fingerprint` | Exact 40-character uppercase primary-key fingerprint |
| `origin`, `description` | Single-line APT Release text |
| `feed_url` | HTTPS origin without credentials, path, query, or fragment |
| `branch` | Exact consumer protected branch; defaults to `main` |
| `schedule` | Numeric five-field POSIX cron |
| `signer_workflow` | Exact upstream package signer workflow path |
| `oci_image_repository` | Qualified lowercase `ghcr.io` repository, without tag or digest |
| `oci_signer_workflow` | Exact upstream OCI provenance workflow path |

Tools are generation context, not executable config: qualified Mise binary SHA,
Python/Gh exact versions, Buildx action SHA/version, and immutable BuildKit image.
Mise uses owned homes and disables project configuration, environment hooks,
automatic installation, and caches. Platform GPG/dpkg tools come from the selected
versioned Ubuntu runner. The generator never accepts a script, action, template,
credential, or tool selector in this config.

## Admission and authority

The schedule and dispatch both admit the exact consumer source SHA using the
protected default-branch CI workflow, a successful `Required` job, and complete
substantive plan/final-report evidence. Dispatch defaults to `mode=validate`.
Only scheduled runs or explicit `mode=publish` dispatches on the bound consumer
branch may sign or deploy. Generation never dispatches a workflow.

Verification and admission use read authority. Signing receives only the fixed
APT private-key/passphrase secrets in `package-feed`. Deployment runs separately
in `github-pages` with Pages/OIDC authority and no signing secrets. Artifact names
and native metadata bind the workflow run and attempt. The final job requires
verification and admission; signing/deployment must either both succeed when
eligible or both be skipped.

Both artifact hops consume the immutable artifact ID and compressed-archive
SHA256 emitted by the producer upload step. A separate read-only transport step
checks GitHub metadata, exact repository/consumer source SHA/run/attempt and
successful producer job, compares the receipt with the REST digest, verifies the
complete ZIP bytes, and extracts only bounded regular files into a fixed absent
directory. No consumer relies on `download-artifact` checksum warnings or an
unsigned self-inventory. The same receipt covers staged indexes, webpages, keys,
configuration and every other staged file before Pages deployment. Its read
token is scoped to transport; the signing step receives no read token.

Fixed helpers run with Python isolated mode. The entrypoint loads only its named
generator-owned support modules through explicit regular source-file paths,
without adding a repository directory to `sys.path` or accepting cached Python
bytecode. Standard-library imports cannot resolve repository shadow modules.

## Source coherence

Stable resolves the tag to an exact commit; preview binds its rolling manifest to
the main source ref and version suffix. Both cover exactly amd64/arm64 packages.
Every package requires source repository/ref/SHA/workflow-bound GitHub provenance,
checksum agreement, Debian control identity, safe archive contents, matching
packaged build identity, and a matching 64-bit little-endian ELF machine.
Stable additionally binds packaged manifest and binary hashes to its release
record and checks architecture target triples.

Stable OCI agreement requires an explicitly qualified image repository and
GitHub provenance for the immutable index from the configured OCI workflow and
exact source ref/SHA. Only after provenance succeeds may the adapter inspect the
index/platform digests and image labels. An unsigned release record and matching
labels cannot establish this trust. A missing producer attestation fails closed;
historical absence does not authorize weaker verification.

## Signed feed and rollback

The adapter authenticates live publication records and APT indexes with the
pinned primary key, including signatures produced by its signing subkeys. Both
live package architectures must agree; unsigned channel metadata must match
build identities recovered from packages authenticated by signed index hashes.
Standard Packages/gzip/Release files are generated natively, without an unpinned
package-manager installation.

Stable requires a retained prior pair. Preview may bootstrap only when the suite
is wholly absent with explicit HTTP 404 responses. Network/server errors and a
partial suite fail closed. Each normal publication retains exactly the candidate
and one previous amd64/arm64 pair. The other authenticated suite is preserved.

The staged hidden proof binds every file, source identity, candidate pair, prior
live state, other suite, and workflow attempt. Deployment verifies these bytes
and signatures again. It refuses rollback, source collisions, changed candidate
bytes, vanished suites, or an intervening live publication that would invalidate
the retained rollback pair. An equal candidate may redeploy only with identical
source and package hashes; changed live state requires fresh staging.
