# Owned cache archive adapter qualification

Status: isolated source candidate; no remote source publication or generator pin
is qualified. Hosted cold/warm/third-run and full authenticated receipt integration
remain required. Candidate checkout: `<PRIVATE_TMP>`.

## Source choice and boundary

The exact upstream action base is
`actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9`, tree
`dc231685132920fededfd7666b19601d06d8a998`. Owned action version is
`6.1.0-velnor.1`; archive schema salt is `velnor-cache-archive-v1`.
The MIT action license and copied toolkit MIT license remain in source.
The action/toolkit npm graph was exact-pinned and updated to zero reported npm
advisories. A final clean source checkpoint, lock/license manifest and bundle
digests are pending the restore admission extension and independent review.

A source-owned toolkit tar adapter preserves the existing cache server protocol,
fixed Node 24 zstd codec and ordered-path getCacheVersion algorithm. The native
archive-library alternative would replace PAX, absolute/external-root behavior,
symlinks/modes and streaming zstd, requiring a separate compatibility proof.
The source-owned action retains one canonical ordered restore/save path sequence;
its new salt isolates the metadata-free archive format from upstream archives.
No ad-hoc workflow-side raw tar invocation is introduced.

Creation uses GNU `--no-xattrs --no-acls`, with BSD additionally
`--no-fflags --no-mac-metadata`. Creation uses a NUL manifest. Archive subprocess
policy removes TAR_OPTIONS/TAPE and compressor option variables; COPYFILE_DISABLE
is always 1. All imported upstream workflow definitions are removed from the
candidate publication head so unrestricted push/release/schedule workflows cannot
execute on the foreign source ref.

## Historical baseline local evidence

1. Actual toolkit createTar/extractTar on macOS BSD tar: gzip,
   zstd-without-long and zstd all passed. Decompressed bytes contain no synthetic
   secret xattr/resource-fork canary, SCHILY/LIBARCHIVE xattrs, ACLs or AppleDouble
   metadata. Regular content, executable mode 0755 and a symlink roundtrip match.
2. The same three formats passed on native Linux arm64 GNU tar under Node 24.
   Private xattr/ACL fixtures are absent from decompressed bytes and restored
   metadata. Hostile TAR_OPTIONS/GZIP settings do not affect the owned action.
3. Pristine upstream toolkit negative controls actually transport canary metadata
   on macOS; GNU/Linux does so with inherited metadata-enabling TAR_OPTIONS.
4. Actual owned restore/save bundled getCacheVersion bodies are byte identical to
   each other and the original algorithm. Canonical ordered payload versions
   match; reversing order changes the version. All four bundled entrypoints
   contain the new metadata/manifest/salt policy.
5. Upstream tests plus supported v2 save-publication receipt cases: 87 passed.
   `cache-saved-key` and `cache-id` outputs require a nonnegative safe integer
   returned by the pinned v2 toolkit after successful service finalization.
   No-op/conflict/failure/invalid ID produce no outputs. Legacy v1/GHES IDs are
   reservations, so they do not produce publication receipts.

Private raw files: `<PRIVATE_TMP>`,
`<PRIVATE_TMP>`,
`<PRIVATE_TMP>`,
`<PRIVATE_TMP>`,
`<PRIVATE_TMP>`, and
`<PRIVATE_TMP>`. Reproduction programs are in the
candidate `owned-cache/` directory.

The amd64 GNU tar probe on this host's emulation failed on filesystem extraction
with `Function not implemented`; native arm64 Linux succeeded. That failed
attempt is recorded in `<PRIVATE_TMP>` and does not establish
an amd64 action qualification.

## Findings that prevent publication qualification

Independent review demonstrated upstream `-P` extraction can write arbitrary
absolute and `../` members outside the workspace. Expected isolated canonical
roots can themselves be outside the workspace, so dropping `-P` alone cannot
preserve the contract. The parent authorized bounded machine parsing of the
compressed tar, exact member/root and symlink/hardlink closure admission before
any writes, followed by owned quarantine extraction and authenticated complete
payload-manifest verification before canonical materialization or execution.
Cache keys and server scopes do not authenticate archive authorship. This
extension is implemented and has bounded independent source probes; immutable
publication and hosted runtime qualification remain separate gates.

Independent review also found upstream command-string flattening breaks workspace
and archive paths containing spaces. Creation now uses raw argument subprocess calls; space/newline roundtrips pass.

macOS may add incidental `com.apple.provenance` when new files are extracted;
that attribute is absent from the metadata-free archive. Native payload observers
need an exact source-policy-bound incidental-metadata rule rather than perpetual
all-attribute rejection. Literal valid AppleDouble containers are ordinary file
contents and survive tar suppression flags. The common native producer must refuse
actual metadata containers before export; a `._` filename prefix is not format
proof and must not discard ordinary source work. Both native policy changes are
owned separately and remain publication gates.


## Defensive integration checkpoint (2026-10-03)

Active transport is now fixed Node 24 in-process `zstd-without-long` on both
supported platforms. Ambient zstd discovery cannot change save/restore identity.
The authoritative parser dependency is exact registry `tar@7.5.22`; its current
locked graph passes `npm audit` with zero findings. Prior gzip/long-mode results
above are historical backend evidence, not current transport qualification.

Actual restore requires a new owned `RUNNER_TEMP/velnor/cache-staging/<digest>`
quarantine. The action validates this input before service lookup. Typed archive
admission rejection emits `cache-restore-error` and `cache-hit=false`, without
quarantine outputs; invalid configuration fails the action. Successful quarantine
outputs describe untrusted staging only. Canonical roots remain untouched until
the separate receipt verifier authenticates the complete logical inventory.

A clean dependency install, guarded source patch application, TypeScript check,
and all eight source API test suites passed under native Node 24.20.0: 91 tests.
Tests include private temporary directory guards and fixed rejection outputs.
That test run preceded the frozen rebuild checkpoint below.

Independent bounded archive probes confirmed component, member-count, expansion,
and quarantine input limits, with no canonical workspace writes. Existing filesystem alias regressions now reject before writes. Literal metadata
container checks fail closed until hosted interpreter qualification is available. The exact shared Python metadata predicate requires a fresh
qualified interpreter bridge; no separate JavaScript predicate is authorized.
No source commit, publication, hosted service qualification, or warmed-cache
qualification is claimed by this checkpoint.


The fixed active codec also passed three local source-owned create/decode/quarantine
roundtrips on macOS Node 24.20.0 and native arm64 Linux Node 24.21.0. They preserve
content, executable mode, symlink target, and space/newline filenames while leaving
original payload roots untouched. Proof program: candidate
`owned-cache/codec-qualification.mjs`; private receipts:
`<PRIVATE_TMP>` and
`<PRIVATE_TMP>`. These local rounds do not establish
hosted cache service publication, receipt authorship, or cold/warm qualification.


Active metadata probes now include nonzero physical flags: macOS `hidden` and
Linux `nodump`. Both source fixtures retain their payload entries while raw
`SCHILY.fflags`/`LIBARCHIVE.fflags` fields remain absent. Current active receipts:
`<PRIVATE_TMP>` and
`<PRIVATE_TMP>`. This proves the tested
serialization boundary, not immutable source publication or metadata authority.

Independent Node 24 alias regressions reject all four existing casefold/NFC
fixtures across gzip/zstd before materialization. Absolute original source paths
are converted only in the creation manifest to workspace-relative transport
names; original ordered SDK inputs remain unchanged for cache-version hashing.
Exact typed SDK restore errors now propagate to the action classifier. Final
shared-predicate integration and source/bundle review still block publication.


## Audit repair and transport semantics

The clean final dependency audit exposed GHSA-vfj7-8cjw-p6xm through the legacy
TypeScript ESLint development graph. Exact upgrades to TypeScript ESLint 8.71.0
and eslint-plugin-jest 29.16.6 removed that dependency branch; the new audit has
zero findings. TypeScript, 91 tests, and targeted ESLint pass after this change.
Exact prior/current lock comparison confirms all 57 production package
path/version/resolved/integrity tuples remain identical. Private proof:
`<PRIVATE_TMP>`. Earlier bundle
hashes are superseded and must not be published.

Root approved complete in-scope hardlink groups as independent portable regular
files. The native shared engine proves original group completeness; the adapter
must validate direct regular targets and equal modes before writes, then flatten
only into private quarantine. This integration now passes bounded source proofs. Numbered-root
symlink rewriting requires an authenticated signed-manifest raw-target witness;
transport diagnostic JSON cannot supply that witness or recover lexical targets.

No immutable cache-action commit, published pin, private projection capability,
or qualified hosted runtime profile exists. Shared metadata policy remains strict.


## Frozen build checkpoint

All twelve emitted assets match across two clean installations and builds in
different directories; the complete 147-file source postimage remained unchanged.
Raw command streams and exact source/asset hashes are retained in
`<PRIVATE_TMP>`. Candidate identity and remaining
authority fields: `<PRIVATE_TMP>`. These diagnostic
hashes do not mint a source qualification capability.

Final Node 24 checks pass: TypeScript, eight suites/91 tests, targeted ESLint,
zero audit findings, identical cache-version functions in all four bundles, and
eleven malformed service-finalization response rejections. Hardlink groups flatten
into independent regular byte copies. Actual installed Rust/Node/Python bytes
pass synthetic group roundtrips on macOS; native arm64 Linux Node bytes also pass.
Original host inode-group completeness remains the native producer gate.

An independent parent review found pathname copying could follow replaced names
and clone metadata. The structural fix holds no-follow/nonblocking descriptors,
checks stable identity and content, writes through an exclusive output descriptor,
and applies mode through that descriptor. Source/destination symlink, FIFO, and
actual source-inode replacement negatives pass on macOS and arm64 Linux. Node
lacks openat: fresh quiescent owned job ancestors remain an explicit scope
requirement; arbitrary concurrent mutation by the same user is not qualified.

No cache-action source commit or public pin exists yet. Hosted Foundation profiles
remain empty, projection capability remains unavailable, and native metadata policy
stays strict. No cold/warm cache service qualification is claimed.


The active fixed zstd codec also roundtrips a complete private two-path hardlink
group through the actual owned SDK creator and quarantine restore on both hosts.
It preserves bytes/0755 and produces independent nlink-one files; the original
group stays unchanged. Receipts: `<PRIVATE_TMP>` and
`<PRIVATE_TMP>`. The proof script lives outside the
frozen action source and makes no hosted qualification claim.


## Raw symlink traversal correction

The 147-file checkpoint above is superseded. Independent shared-core review
reproduced `a/link -> dlink/../file` with `a/dlink -> ../b`: normalization
selected `a/file` while the OS opened unadmitted `file` outside those roots.
Archive admission had the same enabling condition.

The adapter now checks every original target component against the complete
member map before collapsing parent steps. Initial prefixes and intermediate
components must be directories; only exact compiled root structural ancestors
may be synthetic. Symlink, file, missing-directory, and trailing-slash-on-file
traversals reject before writes. Direct cross-root links stay supported.

Actual OS-open proofs pass 22 cases each on macOS and arm64 Linux across gzip
and the active zstd codec. Positive cases compare staged bytes/type with the
original symlink open. Receipts: `<PRIVATE_TMP>` and
`<PRIVATE_TMP>`. Node high-level realpath normalization
is not semantic evidence; fixtures record actual stat/read results. A fresh
148-file source rebuild is required; no old checkpoint may grant qualification.

## Complete payload creator correction

The 148-file raw-link checkpoint rebuilt successfully and remains historical.
The new 150-file checkpoint supersedes it: inherited `--exclude cache.tzst`
silently omitted legitimate payload files. Creator filename exclusions are now
removed. Before any manifest write or tar execution, the creator validates
exact roots through the existing root authority and compares held filesystem
identities against actual output-directory ancestors. Nested output, including
a symlink alias to that directory, rejects. Existing control-file symlink or
hardlink aliases and selected-file collisions reject as well.

This boundary requires fresh quiescent job ancestors; Node lacks openat and this
does not claim safety against arbitrary concurrent same-UID ancestor changes.
Actual macOS Node 24.20 and arm64 Linux Node 24.21 proofs preserve legitimate
`cache.tzst`, `cache.tgz`, newline filename bytes and 0755 modes through both
gzip and the fixed active zstd codec. Nested and aliased output rejects before
control writes. Active metadata/ACL/nonzero flags suppression passes again on
both hosts. No filename-based payload pruning remains.

Checkpoint: `<PRIVATE_TMP>`; full source modes,
hashes, raw build streams and 12 reproducible assets:
`<PRIVATE_TMP>`. Source diagnostic SHA:
`46097ad2044500021467f456753f07c6147238210b143685cdefff3013eef3dd`.
Two clean-directory builds, 91 tests, zero audit findings, hidden-version parity
and service-ID checks pass. Independent parent source review cleared the bounded
quiescent namespace scope. Optional missing roots remain omitted upstream;
both save APIs reject an entirely empty match before calling the creator. No action
commit, published pin, hosted profile or projection capability exists.

Only the canonical `metadata_container.py` blob is copied into this adapter;
the seven-file Python inventory engine is not duplicated. Future external
qualification must bind current Archive template
`10acfb73193a7fa81b325c8ad11aafd07d6d76c86466380c41a14b6f7b3fc88e`.
