# Public cache producer receipt design

Status: selected architecture on 2026-10-03; integration and hosted qualification are incomplete. SEC19 remains open until consumers enforce this receipt before payload use.

## Authority boundary

Use dedicated immutable reusable producer workflows. Their OIDC-capable population contains only closed pure producer jobs. The authenticated signer workflow URI and source digest identify that population. Fulcio does not provide the originating `check_run_id`; ordinary multipurpose workflow certificates cannot prove which sibling signed a receipt.

The caller cannot select commands, action pins, helper bytes, predicates, subjects, payload paths, cache keys, save conditions or arbitrary environment values. It may select a generation-time closed recipe by exact compiled descriptor identity. The callee verifies that descriptor against the source-bound helper's compiled registry and catalog before processing inputs. Unknown descriptors or source identities fail closed.

Tool, native source and MBX writers require separate fixed recipes. A receipt role is a compiled callee output, never a caller string. Every signing-capable path in the qualified callee must obey the same restriction; unrelated OIDC jobs would reopen the forgery class.

All ordinary jobs receive server `cache-mode: read`. Pure producer jobs request literal `write` only when the existing protected default-branch push qualification succeeds. This limits token authority but does not prevent an attacker-controlled PR workflow from creating a same-key PR-scope shadow cache. The public receipt rejects that shadow's incorrect caller ref/source provenance.

## Signed statement

Reuse the existing source-bound operation/descriptor registry. Do not add a second shell-template or task execution engine. The producer computes a closed versioned predicate from its actual outputs:

| Field | Authority |
| --- | --- |
| Schema and producer role | Literal compiled recipe |
| Cache key and source identity | Admitted generation-time descriptor |
| Helper descriptor/body digest and tool/catalog identity | Verified source-bound helper and compiled catalog |
| Complete payload manifest digest | Producer's canonical traversal after successful pure production |
| Caller repository ID and source SHA | GitHub authenticated runtime context, checked against certificate/API identity by consumer |
| Qualified recipe/workflow policy digest | Immutable callee policy |
| Run ID and attempt | GitHub context, checked against certificate invocation and attempt API |

The canonical payload manifest covers every admitted path, entry type, mode, symlink target and file digest. Exclude credentials, state, links outside the admitted payload, unknown paths and unrelated metadata. Consumer recomputes the entire manifest before admitting executable state. Signing a self-reported digest marker inside an otherwise unchecked archive is insufficient.

The pinned `actions/cache` API does not expose the hidden transport archive digest. Do not label a manifest/content digest as an archive SHA-256. An actual archive subject is possible only when the qualified owner exposes the exact transported archive; avoid introducing a competing cache archive engine.

The callee creates its own predicate and subject files. Pinned `actions/attest` signs them and emits a public certificate/signature/transparency bundle. Only public evidence is transported with the cache. Raw OIDC JWTs, bearer tokens and private signing keys never enter a payload or report.

## Consumer admission

1. Restore into quarantine. Do not execute tools or materialize payload links before verification.
2. Use the exact qualified `gh attestation verify` binary and fixed trust configuration. Verify local bundle and subject bytes with exact signer workflow URI/digest, caller source ref/digest, repository identity, predicate type and hosted-runner restriction.
3. Read authority only from verified `verificationResult.signature.certificate` and signed `verificationResult.statement`. Unsigned bundle/API labels are not signing authority. The certificate JSON fields are flattened: `buildSignerURI`, `buildSignerDigest`, `sourceRepositoryURI`, `sourceRepositoryDigest`, `sourceRepositoryRef`, `sourceRepositoryIdentifier`, `buildConfigURI`, `buildConfigDigest`, `buildTrigger`, `runInvocationURI`, `runnerEnvironment`, `issuer`, and `subjectAlternativeName`; there is no `extensions` object. Require authenticated `buildTrigger == push`.
4. Validate the closed signed predicate, admitted key, descriptor/catalog identities and complete payload manifest. Reject unknown versions, extra fields and inconsistent entries.
5. Match the certificate's immutable signer policy and run invocation to fully paginated attempt-specific GitHub run/job metadata. Require a protected-default push source qualification and the exact recipe's successful producer job. Do not substitute the latest attempt or assume check-run IDs equal job IDs.
6. Admit the payload only after every check passes. Missing bundle, unavailable verification/API data, unknown issuer/source, wrong ref/role, corruption or expired evidence produces a reported cold fallback.

Consumer verification must bootstrap from an independently qualified helper/runtime. A cached tool cannot authenticate itself. Source-bound verifier distribution and exact pins remain part of the existing runtime qualification pipeline.

## Availability and qualification limits

The gh verifier defaults include both Public Good and GitHub private roots. If policy requires Public Good specifically, provide an independently qualified fixed trusted-root artifact; the OIDC issuer string alone does not identify the Sigstore instance. Caller-provided trust roots are forbidden.

Private/internal GitHub artifact attestations require the platform's supported Enterprise Cloud capability. Probe actual target support; unsupported repositories require a separately qualified public Sigstore signing route or remain cold. Do not silently claim private consumer cache qualification.

The raw GitHub OIDC alternative was rejected: an audience describes intended recipients but does not prove all configured relying parties reject the credential. OIDC expiration permits clock-skew leeway. Neither a custom audience nor waiting exactly until `exp` proves safe publication. Research-only verifier source and hashes are retained outside product code at `/tmp/velnor-cache-receipt-rejected-raw-jwt`.

Acceptance requires independent adversarial tests for PR-scope shadow caches, alternate signing sibling jobs, arbitrary caller recipe/predicate injection, wrong caller/source/ref/run attempt, failed producers, incomplete manifests, symlink escapes, missing metadata and offline verification failures. Current protection gaps remain visible; missing protection evidence does not grant write authority.

## Implemented source checkpoint

The orchestration draft emitter accepts an opaque recipe and private-field receipt source capability. Producer and consumer have separate frozen source closures: nine producer modules and nineteen consumer modules at the shared inventory checkpoint; the owned preparation adds one fixed module. Both launch absolute `/usr/bin/python3 -I -S`; repository modules, `PYTHONPATH` and startup hooks supply no verifier code. There is no generic cache OIDC role or shape-only signing authority. Draft YAML is publication input, not an approved workflow reference. Published activation and consumer admission remain closed until the publication owner supplies reviewed immutable callee/caller records, protected-origin evidence and an independently qualified trusted root.

The frozen transport layout preserves the original SDK root order and appends the receipt evidence root. Save and restore must use this identical full list because the cache service version includes path inputs. A sealed policy selects payload indices and the final evidence index; the evidence directory admits exactly bounded regular `manifest.json`, `predicate.json` and `bundle.sigstore.json` files. Evidence is excluded from the signed payload inventory, preventing circular subjects. The verifier obtains its bundle and manifest from this compiled mapping. It authenticates manifest bytes before minting a private witness. Numbered quarantine symlinks must exactly match the admitted forward transform of the authenticated original target; no inverse mapping or transport JSON grants target authority. Full canonical records and evidence are reinventoried after verification. A privately sealed quarantine grant binds the result to these bytes and requires a current inventory before materialization. Original signed target bytes are preserved. Redundant `predicate.json` must be canonical, duplicate free, finite, and equal to the authenticated statement predicate before witness minting. Root relocation requires separate functional tool qualification.

Inventory schema 2 signs admitted roots and descendants. Complete in-scope hardlink groups require observed path count equal to inode link count and identical mode/content; external aliases reject. Canonical identity omits alias topology, so an archive may flatten validated groups into independent regular copies. Fixed compiled root order maps numbered quarantine roots to their original logical paths; transport JSON is never authority. Non-admitted ancestor directories are structural prefixes rebuilt with mode 0700, with no signed metadata claim. Unknown indices, missing required roots, unsafe links, special files, depth above 128 components and paths above 4096 bytes fail cold. The producer and quarantine use the same logical inventory; schema 1 has no compatibility path.

The earlier Linux source suite passed 34 checks. The later local receipt/GH/API suite passed 37 checks with one Linux-only skip on macOS; the subsequent crypto-first witness checkpoint passed 56 checks with one platform skip. Independent isolated Python ran 40 targeted checks with no skips, plus 11 filesystem transform cases; those filesystem cases mock signature, API and metadata qualification. These are separate source checkpoints. The actual producer closure positive test fails on ordinary Linux filesystem metadata: the measured fresh regular file has `FS_NOCOW_FL` (0x00800000), which the shared reject-all metadata policy refuses. Five startup/public-bundle/cold tests pass. These results do not establish hosted producer or warm-cache qualification.

The draft source gate regenerates bootstrap, tool preparation, snapshots and reports through their sole owner factories, then compares complete records. Shared rooted descriptor traversal, stable file hashing and complete hardlink checks now live in the Mise inventory core; receipt adapters preserve receipt-specific errors and the authenticated target witness. Materialization uses the same descriptor reader with a bounded sink, builds an isolated tree, verifies it before selected root commits, and rolls back its newly installed roots on failure. Failed rollback raises a terminal exception outside the cold fallback family. Matching caller-supplied helper records, operation names and environment shapes is insufficient. Rust source preparation now replays its exact owner factory; Npm/Bun/Tofu native capsules require fresh whole-factory equality. Unsupported native operations reject closed. Rust startup uses fixed system PATH and absolute isolated Python. The fixed Public Good root is compiled only into the consumer closure and has an actual official-verifier compatibility proof. It does not qualify our pure callee or protected caller. Sealed policy publication remains absent, so consumer verification fails cold before any cached GH execution. Final integrated review and compile gates remain pending. macOS metadata and private Enterprise attestation support remain unqualified; neither receives a warm fallback.

## First native qualification boundary

The Foundation owner prepares an immutable first-step Node 24 action, with no preceding checkout, cache restore or repository execution. The fresh hosted runner and independently reviewed action source are the initial trust boundary. Its zero-argument private constructor retains the observed SDK closure and owned helper bytes in memory. Serialized observation files have `authority: false`; they cannot issue a profile.

The revised collector captures the declared Linux interpreter, standard library, extensions, loader, sysroot, TLS and codec scope in Node before its first Python probe. Probe-discovered roots must belong to that original captured scope. The previous probe-before-capture unit is withdrawn. The revised container proof observed Python 3.14.4 and 5,239 entries before the probe; prohibited startup-marker cases spawned no Python. This proves the tested source ordering, not a GitHub-hosted Foundation profile.

The fresh GH extension compiles that private record and the same owner's checker into a complete Python source capsule. Fixed absolute isolated Python receives only this bounded owned source on its private stdin, with replacement environment and cwd `/`. There is no caller record loader or request/acknowledgment protocol. Node checks continuity before and after the whole launch; Python checks before and after GH verification and API results. These checks do not detect transient mutations restored between observations: the first-step boundary must exclude hostile concurrent writers.

The proposed first GH probe uses the official public `cli/cli` bundle and fixed Public Good root. Success demonstrates native compatibility only. Actual immutable action/workflow hashes, authenticated hosted run/job metadata, complete new checker/source closure and independent evidence review are required before issuing any source-bound Foundation profile. No such hosted result or cache authority is established by the current draft.

The revised ARM Linux container probe verified that public bundle with sealed fresh GH. Its fixed environment disables GH telemetry: upstream telemetry otherwise creates a detached process group. The observed probe made no `setpgid`, `setsid` or network `connect` calls. This remains a container execution proof, separate from immutable publication and hosted qualification.

## Primary sources

- [Pinned attest action](https://github.com/actions/attest/blob/1e69f48acb82d1966a394da916b4c1698aa569d6/action.yml): subject/predicate inputs and bundle output.
- [Pinned verifier](https://github.com/cli/cli/blob/fc4b137cdef0a6bd28fd461b7cf9c84a5812a8cd/pkg/cmd/attestation/verify/verify.go): signer/source restrictions and trusted JSON fields.
- [Fulcio certificate extensions](https://github.com/sigstore/fulcio/blob/56b44b0d59ec5122492810c813b51861dedc73bd/pkg/certificate/extensions.go): signer/source/invocation identity; no check-run extension.
- [Fulcio GitHub claim mapping](https://github.com/sigstore/fulcio/blob/56b44b0d59ec5122492810c813b51861dedc73bd/config/identity/config.yaml): reusable signer identity and run-attempt invocation.
- [Pinned certificate JSON fields](https://github.com/sigstore/sigstore-go/blob/22d3691c7b8e0c5530fae3c05577690bfef5cd00/pkg/fulcio/certificate/summarize.go).
- [Scoped cache tokens](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#controlling-cache-access-with-cache-mode).
- [OIDC audience](https://docs.github.com/en/actions/reference/security/oidc#customizing-the-audience-value).
- [OIDC expiration](https://openid.net/specs/openid-connect-core-1_0.html#IDToken).
- [Pinned attestor dependency lock](https://github.com/actions/attest/blob/1e69f48acb82d1966a394da916b4c1698aa569d6/package-lock.json): the reviewed public v0.3 bundle uses a DER certificate, DSSE statement/signature and Rekor evidence, never a raw OIDC credential.
- [Linux filesystem flag definitions](https://github.com/torvalds/linux/blob/v6.18/include/uapi/linux/fs.h): distinguishes `FS_NOCOW_FL` from unsupported semantic inode flags.
