# Gradle stack contract

Status: partly implemented. Source design: the ChainArgos split stacks design
(§2, §2.6, §4.1). Decisions: [HD-4a and HD-5](../hard-decisions.md), adopted
per the split brief; owner may revisit. This page states what exists today and
what each remaining piece must satisfy; it does not claim the stack is
complete.

## Consumer shape (the ten ChainArgos Java repositories)

- Gradle root in `backend/` (`roots = ["backend"]`), Gradle wrapper pinned by
  `distributionUrl` and `distributionSha256Sum`, JDK declared in the
  repository `mise.toml` (`java = "oracle-graalvm-25.0.3"`).
- Cross-repository libraries are release-asset Maven bundles
  (`<id>-<version>-maven.zip`, a `repository/` tree, plus `.sha256`) pinned in
  `.velnor/deps.lock` as `[[maven_bundles]]`; see
  [private sources](private-sources-contract.md).
- jOOQ code generation and tests need a PostgreSQL service.

## Implemented: `velnor-actions-gradle`

A process-free adapter crate. It reads repository text and bytes and returns
typed findings, each with a stable code.

| Piece | Module | Findings |
|---|---|---|
| Version grammars (HD-4a) | `version` | `project_tool_version_not_exact` |
| Wrapper gates 1-3 | `wrapper`, `catalog` | `gradle_wrapper_properties_missing`, `gradle_distribution_url_untrusted`, `gradle_distribution_checksum_missing`, `gradle_distribution_checksum_mismatch`, `gradle_distribution_uncataloged`, `gradle_wrapper_jar_unverified` |
| Static settings model | `lex`, `settings` | `gradle_dynamic_settings:<reason>`, `gradle_settings_missing` |
| Project dependency edges | `edges` | none (`project(":x")` and `projects.fooBar` accessors) |
| Dependency integrity | `integrity` | `gradle_dependency_lock_missing:<project>`, `gradle_verification_metadata_missing`, `gradle_verification_disabled`, `gradle_verification_not_sha256:<artifact>`, `gradle_verification_trust_too_broad` |
| One-root analysis | `analysis` | all of the above, sorted |

Wrapper gate details:

1. `distributionUrl` must be exactly
   `https://services.gradle.org/distributions/gradle-<ver>-{bin,all}.zip`.
   Snapshots, release candidates, other hosts and query strings fail.
2. `distributionSha256Sum` must be present and equal the catalog digest for
   that version and flavour. The catalog (`catalog`) holds the official
   `services.gradle.org` digests and was read on 2026-10-20; rows are added by
   hand with their evidence date until `velnor-actions-freshness` maintains
   them. The digest is never taken from the repository under test.
3. The SHA-256 of `gradle/wrapper/gradle-wrapper.jar` must be an official
   wrapper-jar digest.

Dependency integrity is the `--locked` analogue: Gradle dependency locking
(`gradle.lockfile` in every project directory whose build script has a
`dependencies` block) and `gradle/verification-metadata.xml` with metadata
verification enabled, a SHA-256 for every artifact and no catch-all trust
entry. Jobs additionally pass `--dependency-verification strict`.

## Decisions

All three are adopted per the split brief; the owner may revisit.

**HD-4a, adapter-specific exact version grammars.** Hard decision 4's "exact
`X.Y.Z`" is widened per adapter, never loosened: `java` accepts
`<vendor>-<X.Y.Z>[+<build>[.<n>]*][.LTS]` (vendor one of `temurin`,
`oracle-graalvm`, `graalvm-community`, `corretto`, `zulu`); Gradle accepts
`N.N` or `N.N.N` from the wrapper URL. Majors, `lts`, `latest`, ranges,
templates and leading zeros still fail with `project_tool_version_not_exact`;
the `$GITHUB_ENV` guard for these keys is `[A-Za-z0-9.+-]`, single-line, never
echoed raw. Not to be reintroduced without owner approval: a vendor-less or
major-only JDK selector, or a wider character class for any other key.

**HD-5, one job per Gradle project or declared group.** A Gradle project (or
declared group) owns one job, `Gradle / <project>` (id `gradle-<slug>`); its
checks run as one `--continue` Gradle invocation, not as parallel steps.
Rationale: Gradle serializes concurrent builds in one root, so step concurrency
(decision 1) would only simulate parallelism; `--continue` keeps every
independent check running and required. Not to be reintroduced without owner
approval: per-phase Gradle jobs for one project, or concurrent Gradle writers
in one root.

**Release-asset Maven bundles (HD-7 follow-through).** Maven artifacts of
private libraries ship as `<id>-<version>-maven.zip` release assets with a
`.sha256`, pinned in `.velnor/deps.lock` as `[[maven_bundles]]`, because GitHub
Packages Maven cannot serve other private repositories with a scoped
credential.

## Not yet implemented

- `[stacks.gradle]` configuration, `Stack::Gradle`, generated per-project jobs
  (HD-5), the JDK resolver key (`VELNOR_TOOL_JAVA`) and the Gradle cache
  families.
- Service containers (`Job.services`, closed `postgres` catalog).
- `[[publish.maven]]` build, attest and publish jobs.
- In-job graph and task-input verification, affected selection for Gradle.
- Freshness maintenance of the Gradle checksum catalog.
