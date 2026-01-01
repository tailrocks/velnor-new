# Implementation checklist

All items start OPEN. This package reports research, not completed implementation.

Use these states: OPEN, IN_PROGRESS, PASS, FAIL, BLOCKED_EXTERNAL, NOT_APPLICABLE. A not-applicable item requires source evidence and an independent Sol review. A missing permission, failed target, or unrun test is not not-applicable.

For each item record: owner; requested and resolved model; source commit; review agent; command/test; run and attempt; artifact digest; measured result; evidence path. Tick an item only after its acceptance evidence exists.

P0 items ship in small release waves. P1 and P2 items remain mandatory for the full goal. See specification.md for acceptance details.

## S01 — Measurement (P0)

- [ ] **S01-01** — Refresh the consumer head, tested merge, generator source, and current run attempt.
- [ ] **S01-02** — Capture every instantiated job and each relevant workflow with complete pagination.
- [ ] **S01-03** — Normalize queued start placeholders, missing values, and cancelled observations correctly.
- [ ] **S01-04** — Capture the actual daemon, runner, image, provider, guest architecture, and resource identities.
- [ ] **S01-05** — Split queue, preparation, setup, checkout, tools, sources, compile, tests, reports, and post time.
- [ ] **S01-06** — Record cache key/version/scope, origin bytes, peak physical disk, memory, and CPU pressure.
- [ ] **S01-07** — Add collector regression tests for partial responses, reruns, nested phases, and missing data.

## S02 — Branch integration (P0)

- [ ] **S02-01** — Review #29 on its refreshed head and select one MBX lifecycle carrier.
- [ ] **S02-02** — Compare and integrate unique useful codex/mbx-lifecycle-fix changes without duplicate ownership.
- [ ] **S02-03** — Verify #26 and #27 are already present before carrying forward their fixes.
- [ ] **S02-04** — Extract or merge independent worker-progress fixes from #25.
- [ ] **S02-05** — Adapt the unique capacity-pressure change to current runner code.
- [ ] **S02-06** — Extract correct test preparation and other coherent fixes from the performance branch.
- [ ] **S02-07** — Resolve all remaining related #28/#20 changes and record all branch dispositions.
- [ ] **S02-08** — Preserve foreign owned-source and fixture refs unless a separate verified disposition applies.

## S03 — Generated work and task selection (P0/P1)

- [ ] **S03-01** — Generate one outer checkout followed by local shared actions, with no equivalent nested checkout.
- [ ] **S03-02** — Prove zero consumer-repository action archive downloads and one consumer checkout.
- [ ] **S03-03** — Verify complete generated-tree drift, repeatable output, size, and protected job identities.
- [ ] **S03-04** — Reduce Plan to the proven minimal tool and metadata closure.
- [ ] **S03-05** — Select work before allocating unnecessary compilation workers.
- [ ] **S03-06** — Test base/head graphs, deleted/renamed files, build scripts, features, and shared settings.
- [ ] **S03-07** — Add strict skipped-obligation receipts and rejection of unexplained missing work.
- [ ] **S03-08** — Force independent required work on both lanes in full paired qualification.

## S04 — Tools and sources (P0)

- [ ] **S04-01** — Use one typed owner and path list for each restore/save layer.
- [ ] **S04-02** — Include required Mise data, Rustup data, and executable links in the tool payload.
- [ ] **S04-03** — Use minimal tool/component sets and remove duplicate or unrelated installation.
- [ ] **S04-04** — Exclude credentials and separate tool executables from immutable Cargo sources.
- [ ] **S04-05** — Verify real GitHub cache versions and resolve hosted/local path portability or namespaces.
- [ ] **S04-06** — Fetch only the selected complete manifest/target closure and prove offline locked builds.
- [ ] **S04-07** — Run an authorized producer plus two new-worker readers with useful payload verification.
- [ ] **S04-08** — Test tool, lockfile, target, layout, and component invalidation plus partial cold recovery.

## S05 — MBX and Rust builds (P0)

- [ ] **S05-01** — Remove obsolete manual bundle lifecycle after native cache parity is demonstrated.
- [ ] **S05-02** — Pass the actual Rust toolchain and verify one exact MBX executable for main and post actions.
- [ ] **S05-03** — Prove missing PATH tools cannot select an unpinned latest binary.
- [ ] **S05-04** — Validate restore prefixes, trust scope, platform, ABI, profile, flags, and feature identities.
- [ ] **S05-05** — Prepare test binaries with the verified Nextest/Cargo route, not an unrelated dev build.
- [ ] **S05-06** — Preserve real product-build obligations, doctests, features, and expected test counts.
- [ ] **S05-07** — Benchmark compatible compile groups or test archives without cross-lane proof substitution.
- [ ] **S05-08** — Bound physical object/target/export/archive footprint and preserve active object leases.
- [ ] **S05-09** — Prove useful remote reuse, one-file invalidation, low-disk recovery, and safe export.

## S06 — Worker progress (P0)

- [ ] **S06-01** — Verify exited-worker cleanup, journal recovery, occupied slots, and fresh admission.
- [ ] **S06-02** — Separate listener/admission progress from slow host and network effects.
- [ ] **S06-03** — Use bounded probes, downloads, deadlines, retries, and cancellation.
- [ ] **S06-04** — Measure effective Docker guest CPU, memory, and actual image/cache filesystems.
- [ ] **S06-05** — Define visible missing-telemetry behavior, hysteresis, and zero-admission drain mode.
- [ ] **S06-06** — Tune one/two/four-worker throughput with compilers, tests, DinD, and other repositories included.
- [ ] **S06-07** — Prove the healthy free-slot admission response budget and multi-wave queue progress.
- [ ] **S06-08** — Prove cancellation, restart, redelivery, session expiry, network loss, and Docker restart cleanup.

## S07 — Action archives (P1)

- [ ] **S07-01** — Implement the pinned official runner action archive cache contract without a runner fork.
- [ ] **S07-02** — Resolve authorization and exact action SHAs before private cache access.
- [ ] **S07-03** — Validate digest and archive structure; publish atomically and coalesce concurrent fills.
- [ ] **S07-04** — Mount only the job-authorized read-only cache projection before runner startup.
- [ ] **S07-05** — Keep missing-entry download fallback; quarantine corrupt entries and reject unauthorized access.
- [ ] **S07-06** — Measure archive copy/extraction and qualify optional unpacked or private copy-on-write trees.
- [ ] **S07-07** — Prove warm zero-origin content and JavaScript/Docker/composite/pre/post compatibility.

## S08 — Git seeds (P1)

- [ ] **S08-01** — Create host-owned repository and authorization-scoped object sources without executing repo code.
- [ ] **S08-02** — Create a new private job checkout and preserve the exact synthetic PR merge and needed history.
- [ ] **S08-03** — Verify the pinned official checkout origin, cleanup, shallow-fetch, and credential behavior.
- [ ] **S08-04** — Dissociate shared references or hold immutable leases until worker cleanup.
- [ ] **S08-05** — Test changed PR refs, cold missing commits, submodules, LFS, modes, and cancellation.
- [ ] **S08-06** — Prove no cross-repository object or credential access and no mutable shared git metadata.
- [ ] **S08-07** — Prove warm zero-origin Git object bytes and content parity with a cold official checkout.

## S09 — Tool/source/image seeds (P1)

- [ ] **S09-01** — Create exact target-bound, checksum-verified manifests for tool and source seed closures.
- [ ] **S09-02** — Project seeds read-only while keeping worker modifications private.
- [ ] **S09-03** — Compare Linux-managed storage with Mac-shared storage and select from measured evidence.
- [ ] **S09-04** — Seed each inner DinD daemon before services need its images.
- [ ] **S09-05** — Prove outer-daemon image presence is not mistakenly treated as inner-daemon readiness.
- [ ] **S09-06** — Bound fill concurrency, storage quota, reserve disk, eviction, and active leases.
- [ ] **S09-07** — Prove concurrent port isolation and complete job-resource cleanup with seeds retained.

## S10 — Host diagnosis and heavy tests (P1)

- [ ] **S10-01** — Establish native/emulated execution and actual provider resource allocation.
- [ ] **S10-02** — Measure network transfer, archive extraction, compression, Git file creation, and compile throughput.
- [ ] **S10-03** — Attribute CPU, filesystem, network, and scheduler costs without guessing their causes.
- [ ] **S10-04** — Trace image, startup, migration, fixture, test body, and teardown time for heavy tests.
- [ ] **S10-05** — Improve fixture scope/readiness only with unchanged assertions and isolated concurrent data.
- [ ] **S10-06** — Bound test parallelism against total host resources and verify failure/cancellation teardown.
- [ ] **S10-07** — Qualify native-amd64 deployment only when justified; never relabel arm64 as amd64 parity.

## S11 — Results and rollout (P0/P1)

- [ ] **S11-01** — Bind artifacts to lane, task, job, run, attempt, tested commit, tools, and input closure.
- [ ] **S11-02** — Reject ambiguous, duplicate, missing, stale, cancelled, mismatched, and failed-post evidence.
- [ ] **S11-03** — Publish only source-bound trusted products after exact-commit required checks.
- [ ] **S11-04** — Regenerate and commit the full consumer tree and manifest with the published generator.
- [ ] **S11-05** — Publish and deploy daemon/images as separate products where only those products change.
- [ ] **S11-06** — Verify the running process and newly created worker image digests; test drain and rollback.
- [ ] **S11-07** — Verify current protected main and required checks after integration.

## S12 — Performance confirmation and maintenance (P1/P2)

- [ ] **S12-01** — Run cold functional, authorized writer, and two independent warm reader cases per relevant release.
- [ ] **S12-02** — Collect at least twenty matched observations per workload class and lane for final statistics.
- [ ] **S12-03** — Meet setup, origin-byte, queue-progress, hosted, and Scale Set improvement targets.
- [ ] **S12-04** — Report all 120-second budget violations without removing work or hiding queue/post time.
- [ ] **S12-05** — Complete archive tuning, cache-byte efficiency, eviction, bounded diagnostics, and upgrade tests.
- [ ] **S12-06** — Verify version freshness and CI/CD workflows outside the measured Rust qualification run.
- [ ] **S12-07** — Close every identified material finding with code/tests or a proven not-applicable disposition.
- [ ] **S12-08** — Obtain GPT-6.1-Sol medium final acceptance with exact product and measurement evidence.
