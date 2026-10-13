# Foundation Python qualification handoff

Source candidate only. `qualifiedFoundationPython()` takes no parameters and
currently returns `unqualified-foundation-image` on every host. Observation files,
caller digests, cached Mise/Python, PATH selection, and version strings cannot
enable it. This empty profile list is an outstanding qualification requirement.

The fresh hosted observer must run the collector and its inventory module as
source-owned embedded bytes through the GitHub server-selected Node 24 action
runtime at a reviewed immutable action SHA. Run before checkout, restoration,
caller helpers, downloaded tools, or other untrusted execution. An ambient
`node` invocation is only a developer reproduction command, not hosted authority.

`collectFoundationPython()` chooses a single absolute interpreter from the
source image mapping, resolves it for observation, then invokes exactly
`-I -S -B` with only `LANG=C`, `LC_ALL=C`, cwd `/`, no shell, and bounded output
and timeout. Missing interpreters fail; there is no fallback. Its schema-1 JSON
explicitly has `authority: false`.

Observation includes exact runner ImageOS/ImageVersion/platform/architecture,
interpreter and startup modules, complete stdlib/import trees including readable
bytecode, absent import paths, recursive native dependency files, loader
configuration/cache/preload, and ELF RPATH/RUNPATH search trees. Every inventoried
file receives SHA-256 and size. Parents, symlinks, directory children, mode,
uid/gid, read mutation, total files and bytes are checked. Caller-provided
observations are never passed to the runtime verifier.

Independent review found remaining blocking native-loader proof requirements:
built-in loader search directories, ld.so.conf include/glob targets, glibc
hwcaps selection, nonempty preload targets, and native dynamic loads must be
sealed or refused. Existing loader/config hashes and resolved `ldd` edges alone
do not establish that complete closure. The bridge must freeze the canonical
predicate and its bounded imported modules; generic arbitrary CPython extension
execution is not a qualified capability. No hosted profile may be added until
these findings are closed. File/dir mutation checks detect changes during reads;
the fresh execution boundary must also preclude concurrent untrusted mutation.

After independent review of actual fresh-host observations and immutable source
publication, add exact reviewed profiles directly to the verifier source. The
verifier re-inventories the full closure before returning an executable and
fixed isolated invocation/environment. Adapter code must treat all unavailable
results as cold/unsupported before using the canonical AppleDouble predicate.
Canonical `metadata_container.py` remains a separate source-bound asset.

Linux closure is the initial supported qualification target. macOS currently
fails qualification: local `/Applications` is root:admin 0775, and Xcode Python
depends on framework-relative load commands and libraries inside the OS dyld
shared cache. The collector refuses these unresolved dependencies; no macOS
closure or positive hosted route is claimed. A reviewed sealed fresh copy or
complete signed OS/shared-cache qualification must resolve this before support.

Local proof: Node syntax check passes; runtime returns cold with no profile;
seven isolated Linux filesystem probes cover stable inventory, changed bytes,
extra bytecode, missing-path insertion, symlink identity, mutable target ancestor
rejection, and cold admission. These do not qualify a hosted image or Python
execution. Independent review completed; the loader findings above remain open.
