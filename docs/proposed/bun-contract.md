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
only to callers that declare none. Node is not a catalog tool: `bun run`
provides a `node` alias for scripts, and a repository that needs a real Node
runs that check as a Mise check.

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
4. One `background: true` step per script (`bun run --cwd=<member dir>
   <script>`), joined by one `wait-all`. Display names are `Typecheck`,
   `Lint`, `Test`, `Build`; any other script is `Script <name>`.

Every Bun invocation runs under `mise exec` with the isolation environment and
`BUN_INSTALL_CACHE_DIR=$RUNNER_TEMP/velnor/bun-cache`.

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

## Playwright browsers (PW, not yet generated)

`[stacks.bun.members."<name>"] browsers = [...]` is accepted and validated, but
no browser provisioning step is generated yet; a member that needs one fails at
run time. Design decisions already taken:

- Never `playwright install --with-deps` and never repository-resolved code with
  `sudo`; system dependencies come from the hosted image.
- The Playwright version is the one `bun.lock` resolves for `@playwright/test`
  (`Member.playwright`); its `playwright-core` tarball integrity in the lock is
  the trust anchor for the browser revision table (`browsers.json`).
- Archives come from `cdn.playwright.dev` and are verified against SHA-256 values
  pinned in a reviewable Velnor lock (written by a CLI subcommand, like
  `deps.lock`); upstream publishes no checksums, so the first computation is
  trust-on-first-use, and every later run verifies it. The install lands in the
  `playwright-browsers` cache family (path
  `$RUNNER_TEMP/velnor/ms-playwright`, trusted writers only).

## Remaining work

- `bun-install` and `playwright-browsers` cache families (design 3.4): the Bun
  package cache is currently cold in every job.
- Playwright provisioning as above.
- A Bun member job that also consumes release assets or Maven bundles through the
  App-token block (documentation-portal, platform): the Bun job builder rejects
  App-token steps today.
- Members of an npm publication that depend on private npm scopes (the
  unprivileged build holds no registry credential).
- Live qualification against the private fixture repositories: the lifecycle phase
  against the real registry, the package-access grant and the `release` environment.

## Verification

- Contract: strict-table, grammar and bound tests (`config/bun_tests.rs`).
- Adapter: lockfile policy, JSONC, workspace resolution, `bunfig`, tool
  agreement, reads and claims (`velnor-actions-bun` tests).
- Renderer: job shape, permissions and role gates (`impl_renderer_bun.rs`).
- Orchestrator: generated jobs, determinism, project-policy and lockfile
  failures (`impl_bun.rs`) and plan dispositions (`impl_bun_select.rs`).
