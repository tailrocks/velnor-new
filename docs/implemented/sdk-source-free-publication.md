# Source-free SDK attestation and publication

The SDK release graph applies hard decision 8 to its own privileged jobs, as
well as to consumer publication. The three target attesters, manifest attester,
and protected publisher contain fixed rendered steps. They do not check out
source, invoke local repository actions or scripts, compile source, read project
configuration, or execute the candidate binary.

## One candidate artifact per target

Each credential-free build uploads exactly three raw files through the pinned
artifact action: the target binary, its exact SHA-256 sidecar, and its
source-bound provenance record. Qualification, attestation, candidate-manifest
construction, and publication download that same immutable build artifact ID.
There is no second transport whose bytes could differ from the qualified input.

The pinned download action handles its own transport format. Before use, fixed
validation requires the exact three immediate entries, regular nonlinked files,
one hard link per file, nonempty content, nonexecutable modes, and no extra or
nested entries. Checked reads enforce the existing 268435456-byte aggregate
content limit. Sidecars must contain the exact single checksum row, and the
binary digest must agree with both sidecar and provenance. Provenance binds the
source commit, target, Rust version, and MBX version.

Only an unprivileged qualifier makes its verified binary executable, with mode
0500, immediately before running it. Privileged jobs keep candidate files as
data. The native archive guard remains responsible for the other archive
protocols; the SDK transport no longer bootstraps that guard in a privileged job.

## Project versions without privileged source readers

The source-side candidate-manifest job resolves project Rust and MBX declarations
and validates the three build records. GitHub CLI uses the qualified catalog
version; production project-tool discovery excludes GitHub CLI. The job exports
these exact version values. Privileged jobs consume them through dedicated
authority environment variables and validate exact, non-leading-zero semantic
versions before using them.

Each shell validation has an explicit failure exit. This preserves rejection on
macOS Bash 3.2, whose `errexit` handling does not stop a standalone `[[ ... ]]`
when more commands follow. Negative controls prove malformed authority versions
cannot reach tool installation or the provenance reader.

GitHub CLI installation and execution use fixed Mise commands with configuration,
environment files, and hooks disabled. The version is a quoted, validated data
argument. Rust and MBX retain hard decision 4's project-declared identity, and
GitHub CLI retains its qualified authority pin. Privileged jobs need no
repository configuration reader.

## Publication authority

The publisher retains source/workflow identity checks, current protected-main
identity, the latest successful exact-source CI attempt and Required check, and
repeated eligibility checks before irreversible operations. It does not require
a local Git checkout to establish those API-authenticated identities. Canonical
manifest bytes, signer bindings, complete immutable asset inventory, protected
environment approval, and acceptance receipts remain required.

Consumer adoption additionally requires the entire product producer run to
complete successfully. An image or host release from a failed or cancelled
coordinator does not authenticate an SDK adoption.

## Qualification evidence

Run 40176575960 proved the corrected resource-probe image build, smoke,
attestation, and publication, but did not publish the SDK. Intel qualification
exceeded its five-minute whole-job limit, and historical Homebrew qualification
reported an unattributed Brew-command failure. The SDK platform qualification
budget is now separately bounded at ten minutes; signing and publication retain
five minutes, and historical Homebrew retains thirty minutes.

Homebrew install errors now report only a closed operation label, failure phase,
and optional numeric exit code. They do not disclose command paths, environment
values, or captured output. The real failing Brew operation and successful SDK
publication still require a fresh protected hosted run after this change.
