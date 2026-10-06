# Owned Actionlint source and native build recipe

Status: source inputs measured, build recipe prepared. No native candidate was
built by this work; no owned distribution is admitted.

## Exact source inputs

| Input | Measured identity |
| --- | --- |
| Upstream repository | `https://github.com/rhysd/actionlint` |
| Upstream release commit | `914e7df21a07ef503a81201c76d2b11c789d3fca` |
| Upstream Git tree | `ecee8c9c752f012b2da57a5fc59863624539b43b` |
| Patch | `scripts/actionlint-cache-mode.patch` |
| Patch SHA-256 | `7c81196d799636344ea309336af5a11a4d67f2bd4ece3cee733765670dc00142` |
| Patched Git tree | `46225027f97bc1f98d914797f9625700527a4afa` |
| `go.mod` SHA-256 | `858b82eb613822def66e73aefca4c97189cfea3a25706854be03b6737c818818` |
| `go.sum` SHA-256 | `6da1057f64fd200c5f018bfee5bff88cbc792f2034c72413cf7e083dc0147bf1` |
| `LICENSE.txt` SHA-256 | `03a26b06d224380a02bf100e05fff3b2dfc71b14d4e2fa685ec9963a87563c22` |
| Recipe SHA-256 | `9cce10dc9a299b08384c0e307d7a4f1444d6e828dc38307c9dba2513a0207838` |

The upstream tree was returned by the official commit API, recorded by the
source qualification agent. The patched tree was independently reconstructed
from all 851 regular files of `<PRIVATE_TMP>`
using Git blob/tree hashing and executable modes; there were no symlinks.
No Git repository, object database, index, or native executable was created.
The temporary source has no `.git`; it is not an owned immutable source commit.
The upstream source archive bytes were not retained, so no archive digest is
claimed. Existing parser qualification is recorded in
`cache-producer-server-authorization.md`.

## Build and identity

`scripts/actionlint-owned-build-recipe.json` fixes source inputs, command argv,
environment, exact proposed Go compiler `go1.27.1`, and three native targets:
Linux AMD64, macOS ARM64, and macOS AMD64. This compiler version was observed
locally; its official asset authority remains unqualified.

Upstream exposes `version` and `installedFrom` through Go linker flags. The
recipe sets the actual executable's first banner line to
`1.7.12-velnor-cache-mode.1+patch.7c81196d7996`, its second line to
`velnor-owned-cache-mode`, and requires the compiler/platform third line.
This is a planned executable identity, not an observed owned release. The
upstream selection version remains `1.7.12`; owned reported identity is distinct.

`scripts/build_owned_actionlint.py` accepts `--source-root`, `--output-dir`, and
`--compiler-receipt`. It validates all source files, including untracked or
ignored files, then copies that exact tree into an isolated temporary directory.
It rejects source symlinks and nonregular entries. It downloads the fixed Go
module graph into a fresh module cache using the fixed Go proxy and checksum
database, verifies modules and unchanged source inputs, and builds offline with
CGO disabled, trimpath, no VCS metadata, and empty linker build ID. Native host
and compiler architecture must agree. Darwin hardware checks reject Rosetta.

The compiler receipt must exactly equal a closed `compiler.approved_assets`
record for that target. Its fields are `asset_url`, `archive_sha256`,
`compiler_binary_sha256`, `toolchain_tree_sha256`, and `go_version`. The full
extracted toolchain digest is SHA-256 over UTF-8 compact JSON of sorted
`[relative POSIX path, Git executable mode, file SHA-256]` tuples, excluding
directories. Symlinks and nonregular entries fail. Compiler bytes and the full
tree are checked before and after building. The map is currently empty: every
native build fails closed until primary compiler asset evidence is approved.
A supplied receipt alone cannot create compiler authority.

Successful execution creates only a new output directory containing `actionlint`
and `build-receipt.json`. The receipt binds the actual binary digest, source
inputs, recipe digest, compiler asset/digests, native host, and exact full banner.
It is a build candidate receipt, not a signed publication or qualification.

## Remaining publication gates

1. Approve exact official Go asset URLs, archive/executable/full-tree digests for
   all three native hosts; populate closed compiler authority.
2. Commit the owned source as an immutable owner-controlled commit and verify
   its tree equals the qualified patched tree.
3. Bind source archive, upstream base, patch, module lock evidence, license, and
   exact build recipe in a signed source receipt.
4. Execute each native build and real CLI behavioral qualification on its host.
5. Publish immutable archives and launch digests; admit those exact distributions
   through the closed runtime factory before workflow consumption.

Deterministic flags fix variable source paths, VCS data, and linker build IDs.
No repeated-build byte reproducibility result is claimed. No shared tool hooks,
tool pins, source indexes, or runtime distribution records changed here.
