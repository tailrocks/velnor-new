# Bun contract (hard decision 6)

Status: adopted per the split brief; the owner may revisit. Hard decision:
[decision 6](../hard-decisions.md). Origin: stack item B, PW and N of the
ChainArgos split design (section 3 and 4.2). Every choice marked
*(refinement)* departs from the design text and is recorded here.

Bun workspaces (the ChainArgos frontend, documentation portal, event catalog
and prototypes) get one generated job per workspace member. The adapter crate
is `velnor-actions-bun`; it reads text and launches nothing.

## Configuration

```toml
[stacks.bun]
roots = ["frontend"]                       # required, sorted, unique; "." is the repository root
timeout_minutes = 10                       # 1..=360; default workflow.crate_timeout_minutes
scripts = ["typecheck", "lint", "test", "build"]   # default; intersected with each member's scripts

[stacks.bun.members."@chainargos/ui"]
scripts = ["typecheck", "lint", "test"]    # replaces the list; every name must exist in package.json
browsers = ["chromium"]                    # sorted, unique; chromium | firefox | webkit
timeout_minutes = 30                       # 1..=360; this member's job only

[[stacks.bun.inputs]]
globs = ["frontend/packages/*/src/**"]
members = ["chainargos-docs"]
complete = true
```

The table is strict. There is no shell, argv, environment, YAML or `uses:`
field: scripts are `package.json` script names (`[a-zA-Z][a-zA-Z0-9:_-]*`),
members are package names (`@scope/name` over lowercase alphanumerics, `-`, `_`
and `.`), and nothing the shell or a `${{ }}` expression could evaluate is
admitted.

A member job's timeout resolves most specific first: the member's
`timeout_minutes`, then `[stacks.bun] timeout_minutes`, then
`workflow.crate_timeout_minutes`. A member value outside `1..=360` fails with
`bad_timeout:<n>:expected_1..=360`, never clamped (hard decision 2); see
[job timeout limits](../implemented/job-timeout-limits.md).

*(refinement)* `roots` is required rather than detected. The design detects
every `package.json` plus `bun.lock` pair from the file index; an explicit list
is the same decision stated where review sees it, and generation still checks
that each root holds `package.json` and `bun.lock`.

## Lockfile and tool policy

- The only accepted lockfile is the text `bun.lock`. `package-lock.json`,
  `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml` and the binary
  `bun.lockb` in a root fail with `unsupported_js_lockfile:<file>`; a missing
  `bun.lock` fails with `bun_lock_missing`. Repositories migrate to Bun in a
  standalone commit instead of getting a second package-manager adapter.
- Bun comes from the project `mise.toml` with an exact pin
  (`project_tool_version_not_exact` for `latest`). A root `packageManager`
  of `bun@<x.y.z>` must equal it (`conflicting_project_tool_versions:bun`);
  any other manager is `unsupported_package_manager`.

Bun is the catalog tool `PinnedTool::Bun` (Mise core `bun`). A project's
`[tools] bun` (or `core:bun`) is read at run time into `VELNOR_TOOL_BUN` like
every project-declared tool (hard decision 4); the catalog pin
(`BUN_VERSION`, freshness-gated against the `oven-sh/bun` release feed) applies
only to callers that declare none. Node is not a catalog tool. Project scripts
that start JavaScript tools use `bun --bun run` so Bun runs the script's
JavaScript executable.

## Workspace members

The members of a root are the entries of `bun.lock` `workspaces` other than
the root entry, checked against the `package.json` `workspaces` patterns in
both directions (`bun_lock_stale`: a lock member no pattern declares, or a
pattern that matches no lock member). A root without `workspaces` is one
single-package member. For each member:

| Fact | Source |
|---|---|
| name | member `package.json`; the lock must agree (`bun_member_name_mismatch`) |
| directory | root joined with the lock key |
| scripts | member `package.json` script names |
| dependencies | `workspace:` specifiers in the lock entry |
| Playwright | resolved `@playwright/test` (or `playwright`) version from the lock |

The job id is `bun-<slug>` (lowercase alphanumeric words joined by `-`) and the
display name `Bun / <name>`. Two members with one slug fail with
`bun_member_slug_collision`.

A member runs the stack `scripts` (default `typecheck`, `lint`, `test`,
`build`) it has, in that order. A member override must name scripts the member
has (`bun_script_missing`); a member with nothing to run fails with
`bun_member_no_scripts`, so a configuration that silently checks nothing cannot
be green.

## The Velnor-written `bunfig.toml`

`bun install` never reads the repository's registry settings. Velnor renders
`$RUNNER_TEMP/velnor/bunfig.toml` and passes it with `--config`:

- from the repository `bunfig.toml` it copies only `install.linker`
  (`isolated` or `hoisted`), `install.exact` and `install.saveTextLockfile`;
  every other key is dropped *(refinement: the design also copies test
  `preload` and `root`, which only matter to `bun test`, which uses the
  repository file as always)*;
- a repository `install.registry`, `install.scopes`, `install.cafile`,
  `install.ca` or `install.token` fails generation with
  `bunfig_registry_override`, so the repository cannot redirect the token;
- for each `[private_sources] npm_scopes` entry it adds
  `[install.scopes."@scope"]` with `url = "https://npm.pkg.github.com/"` and
  `token = "$VELNOR_NPM_READ_TOKEN"`. The lifecycle phase uses a second file
  without the scope section.

## Generated job

`bun-<slug>` runs on `ubuntu-26.04` with `contents: read`, needs the plan job
and joins `Required`; the plan marks it `not_affected` like any other job
(`!contains(needs.plan.outputs.not_affected_jobs, ',bun-<slug>,')`). The tools
cache prelude (Setup Mise, resolver, restore) is inserted around the member's
steps like every pinned-tool job. Steps:

1. `Prepare pinned tools`: `mise install bun@${VELNOR_TOOL_BUN}`.
2. `Verify Bun workspace`: `bun.lock` is a regular file, no foreign lockfile
   sits beside it, and `packageManager` (if present) equals the Mise-declared
   Bun. Generation checks the same agreement; the run-time check covers edits
   after generation.
3. `Install dependencies`: writes the Velnor bunfig under
   `$RUNNER_TEMP/velnor/bun/` and runs
   `bun install --frozen-lockfile --cwd=<root> --config=<file>` under
   `mise exec`. Bun parses `--config <path>` with a space as a package name, so
   both flags use the `=` spelling; `--config` replaces the `bunfig.toml` found in
   the working directory (measured with Bun 1.4.2), which is why a repository
   registry setting cannot redirect the install even before generation rejects it.
4. One `background: true` step per script (`bun --bun run --cwd=<member dir>
   <script>`), joined by one `wait-all`. Display names are `Typecheck`,
   `Lint`, `Test`, `Build`; any other script is `Script <name>`.

Every Bun invocation runs under `mise exec` with the isolation environment and
`BUN_INSTALL_CACHE_DIR=$RUNNER_TEMP/velnor/bun-cache`, the path of the
`bun-install` [cache family](cache-families.md) (one entry per root, keyed by
`bun.lock`): restored before the install, saved after it on a trusted push.
`node_modules` is never cached; every job rebuilds it from that cache.

### Registry phase (`bun_registry_install`)

When `[private_sources] npm_scopes` is non-empty, step 3 becomes two phases
and the job holds `packages: read`:

1. `Install dependencies (registry phase)` is the internal operation
   `bun-registry-install-v1` (step role `BunRegistryInstall`, after the staged
   helper under consumer policy). It is the only step that binds
   `${{ github.token }}`, and only as `VELNOR_NPM_READ_TOKEN`; the renderer
   rejects the binding anywhere else (`credential_binding_outside_private_fetch`).
   The operation reads the repository `bunfig.toml` at run time, fails with
   `bunfig_registry_override` on a registry, scope, token or certificate, writes
   `$RUNNER_TEMP/velnor/bun/registry.toml` (each scope mapped to
   `https://npm.pkg.github.com/` with `token = "$VELNOR_NPM_READ_TOKEN"`) and
   `lifecycle.toml` (the same without the scopes), then starts
   `mise exec bun@<pin> -- bun install --frozen-lockfile --ignore-scripts
   --cwd=<root> --config=<registry.toml>` under the private-fetch policy: a
   cleared environment, `PATH`/`HOME`/`TMPDIR`, the Mise isolation keys,
   `BUN_INSTALL_CACHE_DIR` and the one credential. `--ignore-scripts` means no
   repository code runs while the token exists.
2. `Install dependencies (lifecycle phase)` holds no credential. It removes every
   `node_modules` under the root and runs `bun install --frozen-lockfile
   --config=<lifecycle.toml>` from the cache the first phase filled, which runs
   the `trustedDependencies` lifecycle scripts. Measured with Bun 1.4.2: a
   plain `bun install` over an existing `node_modules` is a no-op and runs no
   scripts, while the same install after removing `node_modules` completes from the
   warm cache with the network unreachable and runs them. Whether Bun refetches
   scoped package metadata in this phase against the real registry is a
   qualification item; the fallback is for the registry operation to pre-fetch
   the scoped tarballs itself.

The org admin must grant "Manage Actions access" read on each npm package to the
consuming repository after its first publish; there is no API for it.

### Private release assets

A member job in a repository whose `.velnor/deps.lock` pins release assets
(documentation-portal, platform) starts with the HD-7 block: the staged helper,
then the mint, fetch and revoke steps for the `release-assets` repositories
(see the [private sources contract](private-sources-contract.md)). Everything
else follows the block, so the App token is dead before `bun install`, the
lifecycle scripts and every script step. A job with both release assets and
npm scopes holds both credentials in disjoint, ordered steps: the App token
(revoked) and then the `github.token` registry phase. Maven bundles are not Bun
inputs. The `velnor-deps` GitHub App exists (id 5394413, installed on the 17
producer repositories with `contents:read` and `metadata:read`); the
organization variable `VELNOR_DEPS_APP_ID` and secret
`VELNOR_DEPS_APP_PRIVATE_KEY` are set.

## Affected selection (hard decision 3)

A member is selected by a changed path in its directory (the deepest member
directory owns a nested path), by a file all members of the workspace share
(`package.json`, `bun.lock`, `bunfig.toml`, `tsconfig*.json` directly in the
root, `patches/**`), or by a declared input glob. Every member that depends on
a selected member through `workspace:` is selected with it.

Reads outside a member's directory are found statically in its `tsconfig*.json`
(`extends`, `references`, `paths`, `rootDirs`, `typeRoots`, `include`,
`exclude`, `files`) and in `*.config.{ts,mts,cts,js,mjs,cjs}` string literals
that name `..`. Such a member is always selected until a
`[[stacks.bun.inputs]]` rule that names it sets `complete = true`, the owner's
statement that its declared inputs list everything it reads outside its
directory.

## Playwright browsers (PW)

`[stacks.bun.members."<name>"] browsers = ["chromium", ...]` installs Playwright's
own browser build for the Playwright version `bun.lock` resolves, from a
hash-pinned lock. Decisions:

- Never `playwright install --with-deps` and never repository-resolved code
  (`node_modules`) with `sudo`. The `ubuntu-26.04` hosted image carries
  Chromium's system libraries but not WebKit's or Firefox's
  (tailrocks/velnor-new#352). A job whose browsers include `webkit` or
  `firefox` carries `VELNOR_PLAYWRIGHT_SYSTEM_DEPS=<sorted browsers>` on its
  install step; before the archives, the operation downloads the
  `playwright-core` tarball of the locked version from the npm registry,
  verifies it against the lock's `core_integrity` (itself checked equal to
  `bun.lock`'s), extracts it through the hardened archive reader and runs its
  own `install-deps <browsers>` under the Velnor-pinned Bun with
  `PLAYWRIGHT_HOST_PLATFORM_OVERRIDE=ubuntu26.04-x64`. The package list is
  Playwright's `nativeDeps` table for that platform inside the verified
  tarball; Playwright elevates through the hosted runner's passwordless `sudo`
  for `apt-get update` and `apt-get install -y --no-install-recommends` only.
  Package versions are the Ubuntu archive's, a host-provided system
  dependency ([version policy](version-policy.md) section 4). A browser not
  pinned by the step's archives fails
  (`playwright_system_deps_unpinned:<browser>`), a tampered tarball never runs
  (`playwright_core_integrity_mismatch`), and Chromium-only jobs are unchanged.
  Coordinator decision, 2026-10-23, under the owner's autonomy rule (the
  issue's option 1 through Playwright's own mechanism); owner may revisit.
- `velnor-actions playwright lock` (operator-run, like `deps lock`) writes
  `.velnor/playwright.lock` for the union of each member's `browsers` and the
  `browsers` of every suite it declares (a suites-only member is locked too). For each Playwright version it downloads the
  `playwright-core` tarball from the npm registry, verifies it against the
  integrity `bun.lock` records (`playwright_core_integrity_mismatch`), and runs
  its `install --dry-run <browser>` under the project Bun with
  `PLAYWRIGHT_HOST_PLATFORM_OVERRIDE=ubuntu26.04-x64` to read each archive's
  directory and URL. That keeps the URL table Playwright's own and runs only
  integrity-verified code. Every archive is downloaded and hashed (SHA-256,
  size); upstream publishes no checksums, so the first computation is
  trust-on-first-use of the download host and every later run verifies it.
  Entries record `core_integrity`, so a `bun.lock` bump without a re-lock fails.
  Chromium pulls `chromium`, `chromium_headless_shell` and `ffmpeg`.
- Generation requires, per member with browsers: a resolved Playwright
  (`bun_playwright_missing:<member>`), the lock's `playwright-core` to be that
  version (`bun_playwright_core_mismatch:<member>`), a lock file
  (`playwright_lock_missing:<member>`) with an archive per browser
  (`playwright_lock_missing:<version>:<browser>:<member>`) whose
  `core_integrity` equals `bun.lock`'s (`playwright_lock_drift:<version>:<member>`).
- The job step `Install Playwright browsers` is the internal operation
  `playwright-browsers-install-v1` (role `PlaywrightInstall`, after the staged
  helper). It carries the sorted `directory:sha256` pins literally, recomputes
  them from the checked-out lock and `bun.lock`, and refuses on any difference
  (`playwright_pin_drift`, `playwright_core_integrity_drift`). It downloads each
  archive with `curl` (https only, 1 GiB cap), verifies SHA-256 and size before
  extracting (`playwright_sha256_mismatch:<directory>` deletes the file and
  installs nothing), extracts through the hardened archive reader (a browser
  entry may be up to 1 GiB; Chrome is about 300 MB) into
  `$RUNNER_TEMP/velnor/ms-playwright/<directory>`, and writes
  `INSTALLATION_COMPLETE` and `velnor-sha256`. A directory that already carries
  the pinned digest (a restored cache) is reused; any other is replaced. URLs
  must be https on `cdn.playwright.dev` or
  `playwright.download.prss.microsoft.com`. It tries the locked URL first, then
  the other mirrors Playwright's registry uses for the same artifact
  (`PLAYWRIGHT_CDN_MIRRORS` in `packages/playwright-core/src/server/registry/index.ts`,
  v1.64.1: `cdn.playwright.dev/dbazure/download/playwright`,
  `playwright.download.prss.microsoft.com/dbazure/download/playwright`,
  `cdn.playwright.dev`, in that order; `builds/cft/` archives have no mirror
  upstream and get none). Each host keeps curl's own retries and backoff, the
  host that served the bytes is logged, and the lock's SHA-256 and size must
  match whichever host answered: a fallback changes only where the bytes come
  from. It holds no credential.
- Script steps of such a member get `PLAYWRIGHT_BROWSERS_PATH`,
  `PLAYWRIGHT_HOST_PLATFORM_OVERRIDE=ubuntu26.04-x64` and
  `PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1`, so Playwright cannot fetch or choose
  another build. Only `ubuntu-26.04` x86-64 is supported
  (`unsupported_platform` in the lock).
- The install lands in `$RUNNER_TEMP/velnor/ms-playwright`, the path of the
  `playwright-browsers` [cache family](cache-families.md) (trusted writers
  only). The member job restores it read-only before the install and saves it
  after the install on a trusted push, with size admission
  ([cache decision 8](cache-decisions.md)).

## Suites and Playwright sharding

Declared suites (separate jobs for groups of a member's scripts, optionally a
Playwright project and shard matrix) are specified in
[Bun browser suites](bun-browser-suites.md).

## Suites and Playwright sharding

Declared suites (separate jobs for groups of a member's scripts, optionally a
Playwright project and shard matrix) are specified in
[Bun browser suites](bun-browser-suites.md).

## Remaining work

- Members of an npm publication that depend on private npm scopes (the
  unprivileged build holds no registry credential).
- Live qualification against the private fixture repositories: the lifecycle phase
  against the real registry, the package-access grant and the `release` environment.

## Verification

- Contract: strict-table, grammar and bound tests, including the member
  timeout (`config/bun_tests.rs`).
- Adapter: lockfile policy, JSONC, workspace resolution, `bunfig`, tool
  agreement, reads and claims (`velnor-actions-bun` tests).
- Renderer: job shape, permissions and role gates for member and suite jobs
  (`impl_renderer_bun.rs`).
- Suites: see [Bun browser suites](bun-browser-suites.md#verification).
- Golden: `fixtures/bun-workspace` (two members, Chromium on one) must
  generate a byte-identical `ci.yml` (`impl_bun_golden.rs`); it proves that
  repositories which declare no suite are unchanged by later Bun features.
  `fixtures/bun-workspace-suites` (a sanitized platform configuration) is the
  opt-in golden. Regenerate deliberately with `VELNOR_UPDATE_GOLDENS=1`.
- Orchestrator: generated jobs, determinism, project-policy and lockfile
  failures (`impl_bun.rs`), the member timeout chain (`impl_bun_timeout.rs`)
  and plan dispositions (`impl_bun_select.rs`).
- Playwright: lock grammar and selection (`playwright_lock_tests.rs`), lock
  resolution and install fixtures with real zip and tar.gz archives
  (`playwright_lock_tests.rs`, `playwright_install_tests.rs` in the
  orchestrator), job shape and failures (`impl_bun.rs`, `impl_renderer_bun.rs`).
  A real-network run of `playwright lock` and the install against Playwright
  1.64.1 (Chromium, 3 archives) was checked by hand on macOS.
