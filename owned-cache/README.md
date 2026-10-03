# Velnor owned cache archive adapter v1

Owned action version: `6.1.0-velnor.1`. Upstream base: actions/cache
`55cc8345863c7cc4c66a329aec7e433d2d1c52a9`, tree
`dc231685132920fededfd7666b19601d06d8a998`. Upstream action and toolkit MIT
licenses remain present. `LICENSE.toolkit` covers the copied toolkit modules.

This source-owned adapter supports Linux GNU tar and macOS BSD tar. Windows
fails closed at module load. The runtime requires Node 24's built-in zstd
codec. The original restore/save paths, GitHub server
transport, authorization, ordered-path `getCacheVersion` algorithm, and cache
eligibility remain upstream. One shared adapter handles creation and extraction.
Its changed version salt `velnor-cache-archive-v1` prevents restoring old archives
whose metadata policy was different. Generator compatibility must bind this exact
source pin/version; use the same canonical ordered paths for restore and save.

GNU tar gets `--no-xattrs --no-acls`. BSD tar additionally gets `--no-fflags
--no-mac-metadata`. Creation uses a NUL-separated `--null --files-from` manifest
so filenames cannot become tar directives. The archive subprocess receives only
a fixed system PATH, C/LANG locale, and COPYFILE_DISABLE=1. TAR_OPTIONS/TAPE and
compressor options cannot leak in. Tar runs from an absolute qualified path and
receives raw argv. Active zstd creation pipes tar stdout through Node 24's
bounded source-owned codec; no external zstd executable or ambient compressor
selection is used. The pinned machine parser is `tar@7.5.22` with integrity
`sha512-MFO/QzvtAOmJbkhOaCTvbGcFN9L9b+JunIsDwaKljSOdcLMea3NJ1k9Usz/rjdfSXTq4dfzfeS7W4p4YOAAHeA==`.
The adapter retains upstream `-P`: canonical isolated roots are outside the
workspace. This archive is not an untrusted generic tar importer; producer trust,
exact restore identity, ownership manifest validation, and quiescent canonical
roots remain mandatory. The native producer must reject actual literal AppleDouble
containers before export: archive flags suppress generated metadata, not arbitrary
regular-file contents. Filename prefixes alone are not proof of that format. Metadata suppression prevents transport of xattrs,
resource forks, ACLs and file flags, including metadata whose bytes are not visible
in an ordinary file inventory. macOS itself may add com.apple.provenance to newly
extracted files; that attribute is absent from the archive and needs the separate
reviewed native observer policy.

Restore admission parses the compressed stream with the pinned `node-tar`
machine parser before writing. It rejects foreign members, duplicate paths,
special files, hardlinks, unsafe symlink closure, unsupported PAX metadata,
oversized components, and decompression or expanded-size limits. Admitted roots
map to numbered roots under a fresh runner-owned quarantine directory; the
manifest records ordered source roots, member kinds, modes, sizes, and content
hashes. Canonical roots are materialized only by the separately authenticated
receipt import.

Upstream GitHub workflow definitions were removed from this source-only owned
publication tree. No imported push/release/schedule automation can run from this
head. Publish the exact reviewed commit only through a dedicated owned-source ref;
release assets belong to a separately reviewed protected generator commit.

## Reproduce

1. `rtk npm ci --ignore-scripts`
2. `rtk npm audit` (locked graph must report zero advisories).
3. `rtk npm run build` (`apply.mjs` checks pristine/owned toolkit module digests).
4. `rtk npm test -- --runInBand`
5. `rtk node owned-cache/qualify.mjs`; `rtk node owned-cache/negative.mjs`.
6. `rtk docker build -f owned-cache/Dockerfile.proof -t velnor-cache-proof:local .`
7. `rtk docker run --rm -v "$PWD:/source" velnor-cache-proof:local node owned-cache/qualify.mjs`
8. Repeat `negative.mjs` in the same Linux container.

Qualification uses actual toolkit createTar/extractTar processes on a synthetic
secret xattr/resource fork and ACL; scans decompressed archive bytes; verifies
regular content, executable mode and symlink roundtrip across gzip,
zstd-without-long and zstd. Hostile TAR_OPTIONS/GZIP variables must not influence
the owned processes. The pristine upstream negative must transport canary bytes
(on GNU it explicitly inherits metadata-enabling TAR_OPTIONS). Local source
qualification does not prove hosted cold/warm/third-run receipts or performance.

A native archive library would replace the supported toolkit format, compression
selection, extraction semantics and archive-version contract. None is needed to
fix this boundary: the source-owned toolkit adapter retains those contracts while
making metadata controls reviewed policy. No workflow-side raw tar command exists.
