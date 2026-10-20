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
`BUN_INSTALL_CACHE_DIR=$RUNNER_TEMP/velnor/bun-cache`. No step holds a credential.
A `[private_sources] npm_scopes` entry fails generation with
`bun_registry_phase_unavailable` until the registry phase lands.

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

## Verification

- Contract: strict-table, grammar and bound tests (`config/bun_tests.rs`).
- Adapter: lockfile policy, JSONC, workspace resolution, `bunfig`, tool
  agreement, reads and claims (`velnor-actions-bun` tests).
- Renderer: job shape, permissions and role gates (`impl_renderer_bun.rs`).
- Orchestrator: generated jobs, determinism, project-policy and lockfile
  failures (`impl_bun.rs`) and plan dispositions (`impl_bun_select.rs`).
