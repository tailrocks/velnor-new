# Operator toolchain guard

## Problem and scope

Normal generation, planning, and Homebrew source-policy preparation discover Rust
workspaces through Cargo metadata. A caller that must enforce a local toolchain
guard cannot currently delegate these internal commands. Guarding a separate
metadata probe does not guard the command that Velnor subsequently executes.

This proposal adds an invocation-only operator option. It does not change the
default CI execution policy, repository configuration, generated workflows,
metadata payloads, or the authority required for package publication.

## Public contract

The global CLI option is `--toolchain-guard ABSOLUTE_EXECUTABLE`. It applies to
normal `generate`, `plan`, and `homebrew source-policy` export/check operations.
It is not a repository configuration field or a declared environment variable.
Unsupported command combinations, including owned-tool-only previews, fail
before writing artifacts instead of silently ignoring the option.

The executable must be an absolute, canonical, executable regular file. Invalid
operator configuration is a fatal execution-policy error. Existing public
library entry points retain their default behavior; explicit typed preparation
options carry the operator policy through the supported paths.

Guarded Rust metadata execution uses catalog-selected exact Rust and Mr.
Boxington versions:

```text
GUARD -- mise --no-config --no-env --no-hooks exec rust@EXACT mr-boxington@EXACT -- mbx metadata EXACT_METADATA_ARGUMENTS
```

The discovery payload remains `--format-version 1 --no-deps --manifest-path
MANIFEST`. Qualification retains `--format-version 1 --locked --offline
--all-features --manifest-path MANIFEST`. Arguments remain separate OS strings;
no shell, PATH shim, hook override, or unguarded retry is introduced. The explicit
MBX provider avoids dependence on an ambient Cargo shim. Guarded execution
removes inherited `MBX_CARGO_SHIM_MODE` and `MBX_CARGO_SHIM_PATH`.

The execution boundary retains its working directory, credential isolation,
installation restrictions, process-group cleanup, output cap, and deadline.
Guard failure is not an ordinary incomplete inventory. A rejected command,
unavailable guard, or invalid guard path aborts preparation before generation or
policy export. Check operations preserve existing policy bytes on failure.

User-facing diagnostics distinguish the following conditions:

| Condition | Diagnostic phrase |
| --- | --- |
| Relative path | `operator toolchain guard requires an absolute path` |
| Missing, non-regular, or non-executable file | `operator toolchain guard requires an executable regular file` |
| Non-canonical path | `operator toolchain guard requires a canonical path` |
| Unsupported operation | `operator toolchain guard is unsupported for this command` |
| Metadata execution denied | `operator toolchain guard rejected metadata` |

## Local guard compatibility

The golden collector also accepts an explicit `--toolchain-guard` option with
the same executable-path requirements. Guarded collection requires an explicit
positional CLI binary and delegates every Plan/Generate operation through that
binary's global option. Binary version and archive identity checks retain their
existing admission path. Ambient environment variables do not select a guard.

Release dogfood collection delegates both locked Cargo fetches through
`GUARD -- mise exec -- cargo fetch --locked`. Before fetching, it verifies that
the staged root `mise.toml` is a regular, non-symlink file whose bytes match the
indexed candidate tree. Each guarded fetch receives invocation-only Mise trust
for that exact file through `MISE_TRUSTED_CONFIG_PATHS`. This replaces inherited
trust paths for the fetch, changes no persistent trust state, and permits the
guard to evaluate the actual staged project environment. Collection without an
explicit guard retains the existing hosted behavior.

An external guard must explicitly admit the isolated pinned MBX metadata form.
Removing isolation flags or tool pins to fit an older guard is not compatible.
Its environment preflight must use the same Mise isolation flags and exact tool
specifications as execution, with installation disabled.
Both exact tool pins must already be installed and healthy. The resolved MBX
executable must belong to the selected managed Mr. Boxington installation;
an ambient executable or a successful environment probe is insufficient.
Existing workspace, manifest, alias, configuration, output-root, and disk checks
remain required.
An admitted invocation does not claim operating-system-wide enforcement.

## Verification plan

Two public integration journeys cover normal preview/planning and source-policy
export/check. They verify resulting artifacts or plan output under valid
delegation, fatal denial with fresh absent outputs, preservation of existing
policy bytes, and diagnostic-specific invalid configuration. Doubles are limited
to external OS tools; no owned execution or discovery implementation is mocked.

The external Mise fixture accepts both the existing and guarded metadata
transports and returns the same complete graph. After a successful run, a
temporary delegation-bypass mutation must fail because a denying guard no longer
prevents a valid artifact or successful check. Restore the exact source and
repeat the successful run. An unknown-option failure alone is insufficient proof
of fail-closed delegation.

The external host guard receives a separate functional replay: exact discovery
and locked/offline qualification succeed; altered payloads, additional tools,
configuration overrides, escaping manifests, selection drift, and insufficient
free space fail. Its prior normal-command and security-database protections must
also continue to pass before activation.
