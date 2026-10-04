# Native cache audit

Status: native implementation and local executable evidence; integrated Cargo
gates, publisher authority, publication receipts and hosted qualification remain
pending. No cache transfer, queue, critical-path or hosted speed claim is made.

## Ownership and qualification

| Workload | Native owner and candidate payload | Current evidence and blocker |
|---|---|---|
| Bun 1.4.2 | Native package download store; expanded native payload under review | Actual full suite and fresh cold/warm/third sequence passed; poisoned warm cache reproduced. Safe native cache-clear repair costs downloads. Consumer enablement blocked on publisher and opaque metadata proof. |
| Node / npm | Official cacache `content-v2`; indexes, config, logs and temporary data excluded | Exact public identity/SRI and native consumption fixture passed; actual 1024-descriptor exec passed. Native candidate bound exceeds canonical transport admission; optional eligibility preflight, receipt and publisher qualification pending. |
| Gradle 9.5.1 | Native dependency artifacts and one exact qualified JavaCompile output key | Actual GRAAL_VM Community runtime and compiler-cache qualification active. Vendor, exact pin and source-qualified tool authority required. Kotlin DSL caches excluded: they contain private paths. |
| OpenTofu | V2 source-bound provider payload, native readonly checksum verification | Public native 1522-byte lock/source-environment/key fixture recorded; corrupted consumer restore cold fallback pending. |
| Docker | Pinned Buildx/BuildKit GHA v2 layer transport | Action/report and hosted qualification pending; RUN cache mounts have no persistence proof. |

Pure source producers alone export native payloads. Consumers restore and run
ordinary native installation/validation; they never save task-mutated native
stores. This removes repository scripts and same-UID background consumer writes
from the exporting process boundary. Source-producer roles and local native
proof do not establish server-enforced publisher authority.

## Source and security findings

1. Bun's native Git backend can retain bare repositories and origin URLs in its
   download store. Negative action globs do not exclude descendants once a
   literal ancestor enters the archive. A pinned-action executable reproduction
   archived a fake credential despite negative patterns. Expanded native Bun
   payload requires complete source, identity and opaque metadata qualification
   before consumer enablement; filename filtering is insufficient.
2. npm HTTP cache indexes may retain authorization headers named by response
   `Vary`. Payload ownership therefore excludes `_cacache/index-v5` entirely.
   Official cacache content is checked against exact SHA512 bytes; anonymous
   package/version metadata must also match the locked name, version, tarball
   and integrity. A registry URL alone cannot prove public readability because
   the public npm registry also hosts private scoped packages.
3. npm producer qualification requires all selected candidates to be public.
   Auth-required/private or unavailable/mismatched evidence denies publication;
   ordinary consumer `npm ci` remains the fallback. Warm verified content can
   avoid tarball download while metadata requalification remains an explicit
   cost. Metadata and tarball bytes are measured separately.
4. Immutable compiled source descriptors and helper step outputs carry source
   authority. A prior `PYTHONPATH`/`sitecustomize.py` fixture bypassed the real
   verifier and forged outputs. Fixed `/usr/bin/python3 -I`, minimal `env -i`,
   system PATH and cleared loader/startup selectors passed an independent
   hostile-environment recheck. Bounds cover descriptors, concurrent requests,
   time and bytes. Native candidate selection permits up to 4 MiB, but canonical
   helper admission rejects raw arguments above 524,288 bytes. Central optional
   source-record eligibility preflight remains pending.
5. Node children use owned per-root HOME/config/data/cache, `/dev/null` user
   config, empty owned global config and fixed npm registry. The closed `env -i`
   environment strips tokens, ambient npm selectors, proxies and `NODE_OPTIONS`.
   Source candidate collection rejects repository configuration overrides.
6. Gradle output caches contain arbitrary task outputs. Only the exact qualified
   JavaCompile key may be exported; broader native build-cache or Kotlin DSL
   archives are unqualified. Task type, compiler implementation/vendor/pin,
   source authority and declared inputs must match. Native build-cache keys do
   not by themselves prove safe/public task outputs. Private Java source and
   internal naming/version evidence belongs only in the private ledger.
7. OpenTofu direct-only installation and contained per-root public lock-source
   qualification are wired. The v2 namespace binds compiled source identity,
   not mutable post-task `hashFiles` evidence. Pure provider producers use native
   readonly lock checksum validation and copy selected regular files with
   normalized metadata; consumer provider stores are never exported. Missing,
   malformed, private, escaping or mismatched source evidence denies transport.
   Corrupted restored providers must still fall back to cold ordinary init;
   that consumer repair behavior remains pending.
8. Native checksum validation does not cover every archive field. An actual
   OpenTofu checksum fixture accepted private empty directories but rejected
   added regular files or renamed filenames. Export reconstructs only selected
   file-parent directories, rejects links and normalizes metadata. Native hashes
   alone cannot establish complete archive confidentiality.
9. A synthetic private xattr was invisible to the current payload observer,
   yet the pinned cache toolkit's BSD tar fallback exported it in PAX metadata.
   `COPYFILE_DISABLE=1` did not remove all xattrs. Private archive metadata must
   be rejected or normalized and the actual transport verified. A matching
   observer digest is insufficient proof of all exported bytes.
10. Pinned cache save action reserve/upload/finalize failures can still return
    action success. Supported lookup-only verification of the actual saved key
    is being implemented as a mandatory publication receipt. Action success,
    snapshot digest and an unsigned PR-shadow key do not authenticate a
    qualified publisher or prove that a cache was actually saved.
11. GitHub BuildKit layer exports do not persist RUN cache mounts. Host Cargo
    or MBX reuse cannot establish compiler reuse inside a container.

## Executed evidence and limits

The final local npm native fixture completed in 5.116 seconds using exact
official cacache identity and SRI checks. Cold, warm and third producer passes
made respectively 1, 0 and 0 tarball requests; corrupted content was repaired.
Private/auth-required selections denied the entire publication set, preserving
ordinary `npm ci` fallback. Real consumer npm execution used the produced native
content. This is local native behavior, not hosted transfer or wall-time proof.
An actual transport fixture with 1024 real tgz descriptors passed on Mac arm:
descriptor arguments were 275,837 bytes, base64 was 367,784 bytes across 45
chunks, and the full environment was 398,330 bytes against measured
`ARG_MAX=1,048,576`. A 4 MiB native candidate descriptor failed actual process
launch with `E2BIG` (`errno 7`), demonstrating the OS limit. That probe exceeds
the current canonical `HelperInvocation::validate` admission limit of 524,288
raw argument bytes; it does not show the current renderer accepts 4 MiB.
The native candidate bound overstates transport eligibility. Central optional
source-record preflight remains pending; the 1024-case pass does not establish
a passing maximum-bound suite.

Local npm security fixtures accepted exact anonymous public metadata/tarball
and rejected 401/403/404, redirects, missing or mismatched metadata, wrong bytes,
private origins and credential-bearing URLs. Requests carried no ambient
authorization, cookies or proxy credentials. Sanitizer fixtures covered private
additions and names, forged hashes, links and optional export failure; separate
fixtures exercised fixed arguments, child credentials and interpreter injection.

Actual Bun 1.4.2 full-suite and fresh cold/warm/third evidence passed. A poisoned
warm native store reproduced incorrect reuse. Native package-manager cache
clear provides safe repair, with download cost; publisher authenticity and opaque
metadata proof still block expanded payload consumer enablement.
The missing-lock fixture failed with `ConnectionRefused` during a proxy-blocked
manifest fetch, not native frozen-lock rejection. Bun 1.4.2 skips its frozen
guard on `NotFound`; the runtime flag alone can resolve an absent lock. The
orchestrator must require a committed, indexed `bun.lock` or `bun.lockb` for the
validation obligation. Unsupported binary-lock source caching stays omitted.
Source audit also found `bun pm cache rm` touches system-temp `bunx-*` entries.
The producer now binds `TMPDIR`, `TEMP` and `TMP` to its pristine owned
`native-tmp`. An actual product smoke completed locally in 907 ms, preserved an
external inherited `bunx-*` sentinel, cleared the stale native store and passed
a fresh proxy-blocked consumer. This timing is local only; emission remains
blocked and neither source nor hosted performance qualification follows.

Independent shared/tool report verification covered 720 cases, 91 controls and
18 actual Rust tests. Actual cache availability is reported separately from
failure and publisher authentication. Lookup insertion into producer roles
remains under integration by the role owner; report evidence does not establish
publication or authenticated publisher qualification.

OpenTofu's public native fixture carries an actual 1522-byte lock through source
environment and v2 key transport. Native checksum/source tests cover public
provider acceptance, altered bytes and host mismatch. Selected-file export
fixtures omit unselected siblings/empty directories and reject private filename,
content, symlink and hardlink additions. Tool-install stand-ins in some fixtures
do not prove actual tool transfers. Consumer corruption-to-cold fallback and
integrated transport gates remain pending.

Gradle 9.5.1 with the exact GRAAL_VM Community native runtime is under active
JavaCompile output qualification. Broader output and Kotlin DSL reuse remains
excluded. Source-bound private evidence and exact internal Java identifiers,
versions and paths remain in the private ledger.

Hosted cold, warm-second, warm-third and lock/tool invalidation remain
unqualified: actual save receipts, authenticated publisher, restore/upload bytes,
timings, provider concurrency and credential absence still need proof. Local
counts do not establish cache transfers, hosted queue time or critical-path
improvement. Workspace Cargo gates remain authoritative.

## Implementation evidence

- `workloads_cache.rs`: consumers restore only; pure producers own exports.
- `workloads_cache_npm_source_job.rs`, `workloads_cache_npm_proof.*` and
  `workloads_cache_npm_native_fixture.py`: exact native source proof and reuse.
- `workloads_env_node.rs`: owned configuration and closed native child env.
- `workloads_cache_bun_source_job.rs` and `workloads_cache_bun_producer.*`:
  Bun source producer and qualification boundary.
- `workloads_cache_gradle_producer*`, `workloads_cache_gradle_policy.init.gradle`:
  exact native compiler output qualification; private details excluded here.
- `tofu_producer_source.*`, `tofu_producer_job.rs` and
  `tofu_producer_transport_tests.rs`: immutable lock/source/key transport.
- [Cache version audit](ci-performance-cache-version.md) and
  [security review](ci-performance-security-review.md): archive metadata,
  publisher authority and publication receipt limitations.

## Sources

- [Bun native package manager source](https://github.com/oven-sh/bun/tree/bun-v1.4.2/src/install)
- [npm source](https://github.com/npm/cli/tree/v11.19.0/node_modules)
- [Private npm packages](https://docs.npmjs.com/creating-and-publishing-private-packages)
- [Gradle dependency artifact reuse](https://docs.gradle.org/9.5.1/userguide/dependency_caching.html#sec:cache-artifact-reuse)
- [Gradle complete task inputs](https://docs.gradle.org/9.5.1/userguide/build_cache.html#sec:task_output_caching_inputs)
- [OpenTofu native provider cache validation](https://opentofu.org/docs/cli/config/config-file/#provider-plugin-cache)
- [BuildKit layer/cache-mount boundaries](https://docs.docker.com/build/ci/github-actions/cache/)
