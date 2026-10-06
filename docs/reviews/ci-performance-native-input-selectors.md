# Native input selector design evidence

Status: design only. No executable qualification or restored workflow claimed.

## Source predicates

The audited Ruby/Shellcheck predicates below are shallow shell globs or explicit
paths. No evidenced recursive Ruby/Shellcheck glob justifies a recursive option.

| Repository | Historical predicate | Preserve |
| --- | --- | --- |
| jackin-project/homebrew-tap | `for f in Formula/*.rb; do ruby -c "$f"; done` | Every nonhidden immediate child ending `.rb`; required nonempty match |
| jackin-project/homebrew-tap | `if [ -d Casks ]; then for f in Casks/*.rb; do [ -e "$f" ] || continue; ruby -c "$f"; done; fi` | Optional directory, shallow nonhidden `.rb`, skip nonexistent target, allow zero matches |
| jackin-project/homebrew-tap | `shellcheck scripts/*.sh` | Every nonhidden immediate child ending `.sh`; required nonempty match |
| tailrocks/holla-apt | `shellcheck scripts/*.sh` | Same predicate; newly added scripts must enter validation |
| tailrocks/velnor-apt | Four literal Shellcheck arguments | Exact four files; adding another `.sh` never expands this obligation |
| tailrocks/homebrew-velnor | `ruby -c Formula/velnorctl.rb` | Exact literal file; no inferred formula wildcard |

Source witnesses:

1. Homebrew tap: [mise.toml at 569523b](https://github.com/jackin-project/homebrew-tap/blob/569523b635959442839f636a2241ac6f7adf8ca7/mise.toml), lines 26, 36, 48. Local bytes:
   `/tmp/velnor-wave-a-audit/homebrew-tap/closure-base-source/jackin-project-homebrew-tap-569523b/mise.toml`.
   SHA256 `67d78c8b8cfe0c46c4d68dda445f4da33648e2e857b77a0d7f9b8b069b298f5d`.
   `/tmp/velnor-wave-a-registry-map/homebrew-tap.json` preserves source/log links,
   watch inputs, ambient Ruby qualification gap and future-added-file warning.
2. Holla APT: [mise.toml at 85c7a69](https://github.com/tailrocks/holla-apt/blob/85c7a69c10d0f9f1a53ebb7e851d6b8baa088318/mise.toml), line 16.
   Local `/tmp/velnor-ci-performance-wave-b/dist/raw/local/holla-apt/decoded_mise.toml`.
3. Velnor APT: [mise.toml at d7b6a0d](https://github.com/tailrocks/velnor-apt/blob/d7b6a0d98362ace3daf0fb4fff0c9294cbb4e820/mise.toml), line 18.
   Exact argument order: `scripts/verify-release.sh`, `scripts/test-verify-release.sh`,
   `scripts/release-discovery.sh`, `scripts/test-release-discovery.sh`.
   Local `/tmp/velnor-ci-performance-wave-b/dist/raw/local/velnor-apt/decoded_mise.toml`.
4. Homebrew Velnor: historical mapping cites [ci.yml at 6c23880](https://github.com/tailrocks/homebrew-velnor/blob/6c23880af22d03b5a17db016ee465d064c1ebf59/.github/workflows/ci.yml).
   Independently inspected cached PR2 patch supplies identical literal syntax
   command at [PR2 head 0ebb6c7](https://github.com/tailrocks/homebrew-velnor/blob/0ebb6c71f1245a3a839beee6ff66f6875634b20b/.github/workflows/ci.yml).
   Local `/tmp/velnor-ci-performance-wave-b/w0-closure/dist/raw/homebrew-velnor/pr_2_files.json`.
   Source archive verifier and Homebrew audit remain separate obligations.

## Structural cause

`WorkloadConfig.paths` admits only sorted explicit file inventories. Validation
requires every entry to remain indexed. `workloads_operations::relative_files`
then passes this inventory to the fixed Ruby/Shellcheck leaf. A historical glob
flattened into this field loses its predicate: additions are omitted, deletions
fail against a stale inventory even when the historical glob still has matches.
`workloads.rs` also copies that inventory into identity inputs.

`FileIndex` cannot recover the lost predicate. Its filesystem index applies
discovery exclusions, keeps regular files, resolves confined links, rejects
dangling links and walks directory symlinks using canonical target paths.
Shell globs instead match directory entries, including matching directories and
symlinks, and exclude dot-prefixed basenames. The Casks `[ -e ]` filter is an
additional predicate. Filtering `FileIndex.files()` by suffix is insufficient.

## Minimal closed contract

Replace source-selection data with a tagged typed selection: `ExplicitPaths`
or ordered `ImmediateChildren` groups. Keep operation kind separate. No command,
shell text, arbitrary glob, task reference or execution graph enters this type.

Each immediate-child group needs only:

1. Validated repository-relative directory and a closed suffix (`Ruby`, `Shell`).
2. Directory policy: `Required` or `IfDirectory`.
3. Entry policy: `AllMatchingEntries` or `ExistingTargetOnly`.
4. Empty policy: `Fail` or `Allow`.

Hidden basenames are always excluded; traversal is always one level; there are
no consumer exclusion patterns. These fixed rules match all evidenced globs.
Formula uses Required/AllMatchingEntries/Fail. Casks uses
IfDirectory/ExistingTargetOnly/Allow. Shell globs use
Required/AllMatchingEntries/Fail. Explicit paths retain their exact predicate.

Keep groups independent: an existing Casks file cannot mask empty Formula.
Include the selector itself in canonical profile identity and required-registry
digest. Resolved paths are evidence, never replacement authority. Discovery
exclusions cannot narrow an explicit required selector.

The generated fixed native leaf must resolve its selector against the checked
source checkout when validation runs. Merely resolving once while generating
and serializing an argv inventory repeats the added-file bug for future commits.
Resolution and tool invocation stay inside the adapter-owned validation
primitive; V1 adds no generic runner or second task graph.

Invoke selected paths as separate arguments. A matching directory must reach
the tool and fail, rather than disappear. A dangling Formula/Shellcheck link
must fail; a dangling Casks link must be skipped. Internal link aliases must keep
their selected lexical names. External/looping links, non-UTF8 names, unusual
filenames and directory-root symlinks require explicit execution qualification;
if the compiled envelope cannot preserve their semantics, reject qualification
and report that boundary. Never claim successful parity after dropping entries.
Ruby `compile_file` versus historical `ruby -c` remains a separate leaf-semantics
qualification issue; equivalent selection alone does not qualify execution.

## Support and negative fixtures

Audit `mapped_supported` means candidate API fit, not runtime approval. Current
explicit API supports literal Ruby/Shellcheck file sets; glob rows are missing
predicate preservation. Homebrew tap additionally lacks historical pinned Ruby.
Homebrew Velnor's historical platform/archive/audit wrappers remain unqualified.
Installed Shellcheck, arbitrary `.sh` files, unused human Mise tasks and a
repository-policy script are never independent standalone lint obligations.
Wave C Brown's pinned `prek`/gitleaks hook stays unsupported; suffix selectors
cannot describe its hook-selected all-files scan.

Planned independent second-repository fixture: Holla-style `scripts/*.sh` under
an unrelated repository/component name. Add a new immediate `.sh`: it is checked;
remove one of multiple scripts: remaining scripts still checked; remove all:
failure. Add nested `.sh`, hidden `.sh`, unrelated `.rb`: none selected. Add a
matching directory: tool failure. Add dangling link: failure. Repeat with
Formula required and optional Casks; dangling Casks skips, empty Casks succeeds,
empty Formula still fails. Repeat Velnor APT literal four with a fifth `.sh`:
fifth stays outside this historical obligation. Excluding `scripts/**` from
discovery must not remove the selector's work. Rename repository/component:
identical selection behavior. Alter group policy or suffix: profile digest
changes and old required-registry digest no longer matches.

Maintain `native_execution = Unknown(native_tool_undeclared_reads)`,
`undeclared_reads = true` and disabled task/compilation reuse. Selection evidence
does not establish full tool/environment/read closure or qualify root gates.
