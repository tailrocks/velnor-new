# dbt contract (dbt-fusion stack)

Status: implemented except for the items under "Not yet implemented". Source design: the ChainArgos split stacks design
(section 6.1). Decision: the `dbt-fusion` version grammar of HD-4a, adopted per
the split brief; owner may revisit. This page states what exists today and
what each remaining piece must satisfy; it does not claim the stack is
complete.

## Consumer shape (`ChainArgos/chainargos-dbt`)

- Project directory `chainargos-dbt/` (holds `dbt_project.yml`, `profiles.yml`,
  `packages.yml`, `package-lock.yml`, `models/`, `macros/`), repository-root
  `mise.toml` declaring `dbt-fusion = "2.0.0-preview.219"` (aqua backend) and a
  `mise.lock` with per-platform URLs.
- `profiles.yml` is environment driven (`DBT_HOST`, `DBT_USER`,
  `DBT_PASSWORD`, ...); `dbt parse` needs no warehouse connection, so
  placeholder values suffice.
- The documented parse path is the pinned Fusion Docker image (README "Build
  and parse"): `dbt deps && dbt parse`. The proposed generated job will use the
  Mise-installed binary at the same exact version.
- `lightdash-content/` is a separate concern (a Bun-stack tool member).

## Configuration

```toml
[stacks.dbt]
projects = ["chainargos-dbt"]        # required, sorted, unique; "." is the repository root
services = ["postgres"]              # closed catalog; needed by compile
env = { DBT_HOST = "127.0.0.1", DBT_PORT = "40000", DBT_USER = "chainargos", DBT_PASSWORD = "chainargos", DBT_DBNAME = "chainargos", DBT_PROFILES_DIR = "." }
checks = ["parse", "compile"]        # closed; default ["parse"]; "compile" requires a service
timeout_minutes = 10                 # 1..=360; default workflow.crate_timeout_minutes
```

The table is strict. There is no shell, argv, YAML or `uses:` field. `dbt deps`
always runs first and is not configurable. `env` keys are `DBT_[A-Z0-9_]+` and
values are inert literals over `[A-Za-z0-9._:/-]` (at most 128 bytes, 32
entries): no expression, quote, `$` or whitespace, so a value can never carry
syntax. `DBT_PASSWORD` is a fixed non-secret placeholder for a service container
and is never a credential. With `services = ["postgres"]`, `DBT_PORT`,
`DBT_USER`, `DBT_PASSWORD` and `DBT_DBNAME`, when present, must equal what the
catalog service publishes (host port `40000`, user, password and database
`chainargos`); a different value fails generation with
`dbt_env_service_mismatch:<key>`.

## Implemented: `velnor-actions-dbt`

A process-free adapter crate. It reads repository text and returns typed
findings, each with a stable code.

| Piece | Module | Findings |
|---|---|---|
| Version grammar (HD-4a) | `version` | `project_tool_version_not_exact` |
| Project model | `project` | `dbt_project_name_invalid`, `dbt_project_profile_invalid`, `dbt_path_escapes_project:<key>` |
| Package lock policy | `packages` | `dbt_package_lock_missing`, `dbt_package_not_locked:<id>`, `dbt_package_lock_unpinned:<id>`, `dbt_local_package_escapes_project:<dir>`, `dbt_package_source_unsupported:<kind>`, `dbt_packages_unparsed:<line>` |
| Profile hygiene | `profiles` | `dbt_profile_literal_secret:<key>` |
| Artifact admission | `tools` | `dbt_fusion_lock_missing`, `dbt_fusion_lock_platform_missing:<p>`, `dbt_fusion_url_untrusted:<p>`, `dbt_fusion_checksum_missing:<p>` |
| Affected selection | `select` | none |

Rules:

1. **Version.** `X.Y.Z-preview.N` with canonical decimal components. A bare
   `X.Y.Z`, `latest`, a range, a template, a leading zero or any character
   outside `[A-Za-z0-9.-]` fails with `project_tool_version_not_exact`.
2. **Packages.** `dbt deps` downloads project packages. Every hub or Git package
   in `packages.yml` needs an entry in `package-lock.yml` pinned by an exact hub
   version or a 40-hex Git revision. `local:` packages need no lock entry but
   must stay inside the project directory. Tarball packages are refused. The
   lock's `sha1_hash` is not recomputed (dbt's hashing is not specified for
   Fusion); entries are matched by package identity and pin.
3. **Paths.** Every `*-paths`, `target-path`, `log-path` and
   `packages-install-path` stays inside the project directory, so the project
   directory is the complete input set of its job.
4. **Profiles.** A credential key (`password`, `token`, `private_key`, ...) must
   read `env_var(...)`; a literal fails.
5. **Artifact.** Only
   `https://public.cdn.getdbt.com/fs/cli/fs-v<version>-<triple>.tar.gz` is
   admitted, for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and
   `aarch64-apple-darwin`. The checksum must come from the project's
   `mise.lock`; a row without one is a finding, never a skipped check.
6. **Selection.** A change under a project directory selects its job; a
   repository-root project is selected by any change.

## Decisions

**HD-4a, `dbt-fusion` grammar.** Adopted per the split brief; owner may revisit.
The adapter-specific grammar `X.Y.Z-preview.N` widens hard decision 4's exact
`X.Y.Z` for the `dbt-fusion` key only; the `$GITHUB_ENV` guard for that key is
`[A-Za-z0-9.-]`, single-line, never echoed raw. Not to be reintroduced without
owner approval: a prerelease range, `latest` or a bare `X.Y.Z` for `dbt-fusion`.

## Consumer notes

- `mise lock` records the `dbt-fusion` URLs but no SHA-256: the aqua entry has no
  checksum source, and an install on one platform adds only a BLAKE3 for that
  platform. The consumer therefore commits `sha256:` rows for all three
  platforms, copied from the `.sha256` files `public.cdn.getdbt.com` publishes
  next to each archive (`mise install` honours and verifies them).
- The root `mise.toml` is not inside the dbt project directory, so a tool-pin
  change is handled by the declared-version comparison of decision 3, not by the
  project-directory claim.

## Implemented: the `dbt / <project>` job

`WorkflowTaskPolicy::Stack` (renderer `stack_jobs`, shared with Ansible) carries
one job per configured project; the orchestrator resolves it in `dbt_inventory`
(analysis, fail closed), `dbt_tasks` and `dbt_steps`, and plans it in
`select_dbt`. The job id is `dbt-<slug>` and the display name `dbt / <dir>`.

- **Shape.** Checkout without credentials, `Setup Mise`, `Resolve project tool
  versions` (which defines `VELNOR_TOOL_DBT_FUSION` only because the job
  references it), `Install dbt-fusion`, `dbt deps`, then one background step per
  check joined by `wait-all`. The job holds `contents: read` only and refuses any
  `uses:` step or token role. It runs on the hosted `ubuntu-26.04` runner.
  No tool is installed through Mise, so the tools-cache v2 steps are not
  emitted.
- **Tool provisioning.** `Install dbt-fusion` rebuilds the admitted
  `public.cdn.getdbt.com` URL from the project-declared version and requires it
  to equal the `linux-x64` row of the repository `mise.lock`; the row's SHA-256 is
  the only trust anchor, checked with `sha256sum -c` before extraction.
  Generation also fails closed (`dbt_fusion_checksum_missing:<platform>` and the
  other lock findings) when any of the three platform rows lacks a checksum.
- **Service.** `services = ["postgres"]` adds the digest-pinned catalog Postgres
  service ([service catalog](workflow-contract.md)) to the job; `compile` runs
  `dbt compile --static-analysis off` against it.
- **Plan.** Every project has a `run` or `not_affected` disposition. A change
  under the project directory selects it; an unknown change set, a global-config
  path (including the Mise config and lock) or a full-run event selects every
  project. The `dbt-` prefix owns no obligations.
- **Caches.** The `dbt-packages` family is specified
  ([cache families](cache-families.md)); its restore and save steps are not
  generated yet, so `dbt deps` runs against the network on every run.

## Not yet implemented

- The `dbt-packages` restore and save steps.
- `dbt build --empty` (phase 2, needs the seeded warehouse schema bundle).
- The `docker-dbt-fusion` image under `[[images]]` and the Lightdash Bun member.
