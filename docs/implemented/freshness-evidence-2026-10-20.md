# Freshness qualification — 2026-10-20

The native upstream probe failed with global timeouts after 253.418 seconds.
Its [unchanged transcript](evidence/freshness-2026-10-20/native-upstream-timeout.log)
is retained. Authenticated official GitHub REST captures and official Rust and
crates.io responses provide the fallback observations. Two captures agree for
all 23 provider rows and all 12 action tag commits; the
[agreement record](evidence/freshness-2026-10-20/agreement.json) and
[capture inventory](evidence/freshness-2026-10-20/capture-inventory.json)
bind the raw bytes. Initial crates.io requests returned 403; requests carrying
an identifying User-Agent succeeded. Those failures remain evidence.
The Rust manifest capture is a bounded 128 KiB prefix, not a full-manifest hash.

Four stable releases moved: Mise 2026.10.15, Mr. Boxington 1.25.0,
release-plz 0.3.173, and mise-action 5.2.1. No holds, exceptions, or observation
interval changes were introduced. Historical qualification remains historical.

Mise 2026.10.15's official source is 597c1897dfed28d1c0dea9f98fba581059e42cd3,
tree 4786f2724d63cb59e06226bde8d3d58c09cb80dd. Five downloaded subjects
passed exact-identity Cosign and GitHub SLSA verification; signed checksums and
archive members agreed. The raw Linux AMD64, macOS Intel and macOS ARM64 hashes
are e3b186d4c70627f27037be615becaf4acfb30e41fb6a1819486717dc94e4e2a9,
ea45cc6e787d412deb8e069eec57875c71863e33a36acf69dd59791c27c9c073 and
826c4833fa7437a63f6027b86bc10c8b61b1163dc793c03ecde92f68ad167890.
Their exact byte limits are 168019312, 164421392 and 135143184.
Private MacARM execution passed 37 version, configuration-isolation and selector
checks using the actual HOME and private tool homes. This does not claim Linux
or Intel runtime execution. Cargo-dependent source checks remain release gates.

Mr. Boxington 1.25.0's product source is
2ade2bc6b661f1b40a55cfa7083fa9a4efc9348d,
tree 54170d2d1f8fa99bbd7f1bbce328cd69562e6305. The three supported archives
passed API digest/member measurements, exact-identity Cosign verification and
GitHub provenance verification. The signed Packslip binds the product source;
SLSA binds workflow source 9c70fabd7db412e497e5ddbb61606456cee21890 and
run 39257355707/1. The workflow resolves the release tag, builds that exact
resolved commit, and passes it to Packslip. An initial check that incorrectly
required the product commit as SLSA workflow source failed and was retained.
These distinct identities were independently reviewed before execution.
Private MacARM checks passed exact version/help, isolated empty cache checks,
locked offline dependency-free compilation and execution, and a fresh second
target restored one cached compilation with no network transfer. Linux and
hosted cache roundtrip execution are not claimed. The new upstream
MBX_REMOTE_TRUST_WRITES option remains absent/default false; no cache or
scheduler settings were changed to adopt the release.

The official bootstrap source tuples and Python source admission bases advance
together for Mise and MBX. Both authorities reject a descriptor based on a
different upstream commit. Synthetic owned-version examples are not relabeled
as newly qualified distributions.

mise-action 5.2.1 is commit fc42bcc7ddbf51124c30f95a877a0c5168a3bfde.
Its source and packaged JavaScript were reviewed: releases from 2026.9.3 onward
must provide a signed Packslip instead of falling back after a missing bundle.
Velnor's action inputs, token persistence, environment and disabled-cache
boundaries remain unchanged.

release-plz 0.3.173's crate is 127298 bytes with SHA256
581521a1d9c8a490cbbd184764774f61a27e5ca0a98d12f764ebeafdb938627d;
its embedded VCS source is 907af6d031bbb69d392354803c04cd75bb6e6fa0.
The official MacARM prebuilt executable is 42903792 bytes with SHA256
5e1e7b9e622426fd3785873a56982ff4342acee52090355fdfe64fcae6498a87.
Its archive agreed with the authenticated GitHub API digest. No separate
binary signature is claimed. Exact version and three help commands passed,
then an actual private Mise 2026.10.15 shorthand installation selected the aqua
prebuilt backend and produced the same bytes before execution. No Cargo
fallback, publishing or release-PR mutation occurred. Registry delegation and
OIDC semantics were reviewed; the implicit Cargo registry remains implicit.

The [runner observation](evidence/freshness-2026-10-20/runner-observation.json)
combines a fresh official hosted-runner reference with actual successful main
CI 39350286471/1 at d30b50c493ba589ce15dfec55928fbb37cd4f2a2. Its Plan job
reports ubuntu-26.04, image 20261012.163.1 and the matching image release.
The supported labels remain ubuntu-26.04, ubuntu-24.04 and ubuntu-22.04.

The [qualification receipts](evidence/freshness-2026-10-20/qualification/inventory.json)
bind the measured subjects, cryptographic command results, local runtime
observations and reviewed upstream source diffs. They contain no executable
assets or credential values. Full private transcripts remain retained.
