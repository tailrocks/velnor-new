[Skip to content](https://github.com/jdx/mise/releases#start-of-content)

You signed in with another tab or window. [Reload](https://github.com/jdx/mise/releases) to refresh your session.You signed out in another tab or window. [Reload](https://github.com/jdx/mise/releases) to refresh your session.You switched accounts on another tab or window. [Reload](https://github.com/jdx/mise/releases) to refresh your session.Dismiss alert

{{ message }}

[jdx](https://github.com/jdx)/ **[mise](https://github.com/jdx/mise)** Public

- Sponsor







# Sponsor jdx/mise























##### GitHub Sponsors

[Learn more about Sponsors](https://github.com/sponsors)







[![@jdx](https://avatars.githubusercontent.com/u/216188?s=80&v=4)](https://github.com/jdx)



[jdx](https://github.com/jdx)



[jdx](https://github.com/jdx)



[Sponsor](https://github.com/sponsors/jdx)









##### External links









[https://jdx.dev](https://jdx.dev/)









[Learn more about funding links in repositories](https://docs.github.com/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/displaying-a-sponsor-button-in-your-repository).




[Report abuse](https://github.com/contact/report-abuse?report=jdx%2Fmise+%28Repository+Funding+Links%29)

- [Notifications](https://github.com/login?return_to=%2Fjdx%2Fmise) You must be signed in to change notification settings
- [Fork\\
1.5k](https://github.com/login?return_to=%2Fjdx%2Fmise)
- [Star\\
34.6k](https://github.com/login?return_to=%2Fjdx%2Fmise)


# Releases: jdx/mise

Releases · jdx/mise

## Release list

Previous [Next](https://github.com/jdx/mise/releases?page=2)

Jump to release

- [v2026.10.1: Task timeout and Ctrl-C fixes, Windows daemon and shim fixes](https://github.com/jdx/mise/releases#release-v2026.10.1)
- [v2026.10.0: Stricter signer checks for cosign and GitHub attestations, trust for inline options in .tool-versions](https://github.com/jdx/mise/releases#release-v2026.10.0)
- [v2026.9.18: Remote config includes, OCI task catalogs, and a trust fix for inline tool options](https://github.com/jdx/mise/releases#release-v2026.9.18)
- [v2026.9.17: Self-update waits 24 hours for new releases and verifies signed packslips](https://github.com/jdx/mise/releases#release-v2026.9.17)
- [v2026.9.16: Per-tool libc for aqua tools, monorepo task path aliases, and packslip pins that survive repo renames](https://github.com/jdx/mise/releases#release-v2026.9.16)
- [v2026.9.15: vfox tools in OCI images, faster shell prompts, and safer dotfiles pattern matching](https://github.com/jdx/mise/releases#release-v2026.9.15)
- [v2026.9.14: conf.d folder fragments, Stow-style dotfiles options, and mise-versions for any public GitHub repo](https://github.com/jdx/mise/releases#release-v2026.9.14)
- [v2026.9.13: OpenTelemetry for tasks, shared daemon providers, \`mise backends switch\`, and declarative dotfile removal](https://github.com/jdx/mise/releases#release-v2026.9.13)
- [v2026.9.12: Tasks that require daemons, worktree-aware ports and URLs, Scoop and zypper packages, and official Docker images](https://github.com/jdx/mise/releases#release-v2026.9.12)
- [v2026.9.11: macos-app bootstrap packages, task template inheritance for flags and file tasks, and Swift on Linux fixes](https://github.com/jdx/mise/releases#release-v2026.9.11)

Previous [Next](https://github.com/jdx/mise/releases?page=2)

## v2026.10.1: Task timeout and Ctrl-C fixes, Windows daemon and shim fixes

[v2026.10.1: Task timeout and Ctrl-C fixes, Windows daemon and shim fixes](https://github.com/jdx/mise/releases/tag/v2026.10.1)[Latest](https://github.com/jdx/mise/releases/latest)

[Latest](https://github.com/jdx/mise/releases/latest)

Compare

# Choose a tag to compare

## Sorry, something went wrong.

Filter

Loading

## Sorry, something went wrong.

### Uh oh!

There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).

## No results found

[View all tags](https://github.com/jdx/mise/tags)

![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)

released this

9 hours ago
03 Oct 14:12


Immutable
release. Only release title and notes can be modified.

[v2026.10.1](https://github.com/jdx/mise/tree/v2026.10.1)

This tag was signed with the committer’s **verified signature**.


[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)

GPG key ID: 8B81C9D17413A06D

Verified
on Oct 3, 2026, 04:34 AM

[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).


[`050ce5a`](https://github.com/jdx/mise/commit/050ce5a20287a0aafd872b1191699a5fdafff5ac)

This commit was created on GitHub.com and signed with GitHub’s **verified signature**.


GPG key ID: B5690EEEBB952194

Verified
on Oct 3, 2026, 04:29 AM

[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).


This release is mostly bug fixes. `mise run` now stops tasks properly when `--timeout` expires or you press Ctrl-C. Task daemons and native shims work better on Windows. `core:rust` now follows `mise.lock` and picks up new stable and beta toolchains. Lockfile, GitHub asset selection, Homebrew cask and plugin update problems are also fixed.

## Fixed

### Tasks

- **`mise run --timeout` now stops the tasks it was running.** Before, mise printed the timeout error and exited, but the task processes could keep running in the background. Now the whole-run timeout (`--timeout` or the `task.timeout` setting) stops tasks the same way a per-task `timeout` does. On Unix, mise sends SIGTERM and then SIGKILL after 5 seconds. On Windows, it runs `taskkill /F /T`. Tasks with `raw = true` are not stopped by the whole-run timeout. [#13876](https://github.com/jdx/mise/pull/13876) by [@Marukome0743](https://github.com/Marukome0743)
- **A single Ctrl-C lets tasks shut down cleanly.** Before, one Ctrl-C could make mise exit right away while tasks were still cleaning up. This happened in three cases: a task that runs `mise run` itself got SIGINT twice; a tool like `docker compose up` treated the duplicate SIGINT as a force-quit; and a task that exits non-zero on SIGINT caused mise to send SIGTERM to its sibling tasks. Now mise waits for tasks to finish and then exits with status 130. A second Ctrl-C still force-quits. [#13904](https://github.com/jdx/mise/pull/13904)
- **Tab completion for the `:task` shorthand.** In a monorepo, `mise run :<TAB>` now suggests tasks from the current config root, and task flags complete after the shorthand. [#13882](https://github.com/jdx/mise/pull/13882) by [@pikeas](https://github.com/pikeas)

### Daemons

- **Task daemons start on Windows.** A daemon declared with `task =` failed under `cmd /C` with `'exec' is not recognized`. mise now registers it as an argv command that pitchfork starts without a shell, so `args` reach the task exactly as written on every platform. **This needs pitchfork 2.28.0 or later.** With an older pitchfork, mise shows an error that tells you to upgrade, for example with `mise use pitchfork@latest`. Task daemons that use `init` still run through a shell, so they still don't work under `cmd /C`. [#13714](https://github.com/jdx/mise/pull/13714) by [@JamBalaya56562](https://github.com/JamBalaya56562)

- **Daemon `run` commands can use `[env]` and `[vars]`.** Before, a template such as `{{ vars.test_var }}` failed with `Variable 'vars' is not defined`. `mise x` now renders the command when the daemon starts, using the project's `[env]`, `[vars]` and mise template filters. Pitchfork's own variables, such as `{{ name }}`, still work. This applies only to `run` and requires a pitchfork release newer than 2.29.0. [#13894](https://github.com/jdx/mise/pull/13894)



```
[vars]
greeting = "it's"

[daemons.hello]
run = "exec echo {{ vars.greeting | quote }} from {{ name }}"
```


### Windows shims

- **No more endless process chains from duplicate shim copies.** If two copies of `mise-shim.exe` were on PATH (for example, one from winget's `Links` directory), they could keep calling each other through `mise x`. Running `mise-shim` by its own name now exits with an error. If `mise x` resolves a tool to another shim copy, mise stops after one step and names the PATH directory to remove. [#13681](https://github.com/jdx/mise/pull/13681) by [@JamBalaya56562](https://github.com/JamBalaya56562)
- **Node IPC works through the `node.exe` shim.** A Node parent that spawned the shim with an `'ipc'` stdio entry used to wait forever. JSON IPC messages and disconnects now pass through the shim. Passing socket or server handles over the channel is still not supported. [#13903](https://github.com/jdx/mise/pull/13903)

### Rust

- **`mise upgrade rust` updates `stable` and `beta`.** mise didn't recognize rustup 1.29's new `update available:` text. Even when it detected an update, the upgrade skipped the toolchain as already installed. mise now reads both spellings, counts only updates for the toolchain it manages, and updates the toolchain in place. If the update fails, the old toolchain stays usable. [#13898](https://github.com/jdx/mise/pull/13898)
- **`core:rust` follows `mise.lock`.** mise mistook rustup's symlinks for `mise link`ed versions, so it ignored the lockfile and installed the newest version even with `locked = true`. [#13915](https://github.com/jdx/mise/pull/13915)

### Backends, lockfiles and bootstrap

- **GitHub auto-detection no longer installs metadata files.** SBOMs, signatures, checksums and other sidecar files with platform names, such as `*.tar.gz.sbom.json`, could be chosen as the tool and saved to `mise.lock`. Automatic selection now skips them. Explicit `url` and `asset_pattern` options are unchanged. [#13908](https://github.com/jdx/mise/pull/13908)
- **`mise lock` removes outdated duplicate entries.** After you changed a tool option, for example by adding `uvx = false` to a `pipx:` tool, `mise lock --upgrade` could leave the old unbound entry next to the new bound one. Unfiltered `mise lock` runs now remove the old entry, unless it has platform data (checksum or URL) that the new entry doesn't have. [#13909](https://github.com/jdx/mise/pull/13909)
- **Aqua registry cache errors after upgrading.** Compiled registry caches from earlier versions could load but then fail when a package was resolved. mise now ignores those caches and rebuilds them. [#13884](https://github.com/jdx/mise/pull/13884)
- **Packslip installs retry missing skills.** If the binary installed but a declared skill couldn't be fetched, mise still marked the install as complete. Now the install fails with an error. The next `mise install` fetches only the missing skills and doesn't reinstall the tool. [#13885](https://github.com/jdx/mise/pull/13885)
- **Pkg-based `brew-cask` packages are no longer reinstalled on every run.** Casks such as `google-drive` list package IDs for several architectures, and mise expected every one of them to be installed. Receipts now store only the patterns that match on your machine. Casks recorded by earlier versions are reinstalled once to write a corrected receipt. [#13893](https://github.com/jdx/mise/pull/13893)
- **`mise plugins update` works when the remote isn't named `origin`.** This happens, for example, when git's `clone.defaultRemoteName` is set to something else. mise uses `origin` if it exists and otherwise uses the first remote. [#13914](https://github.com/jdx/mise/pull/13914)

## Changed

- `mise skills sync`, the table output of `mise skills ls`, and other human-facing messages now show paths under your home directory with `~`. This includes output from `mise deps install`, task source lines, `mise completion --install` and daemon messages. `--json` output and script-oriented commands still print full paths. [#13910](https://github.com/jdx/mise/pull/13910)

**Full Changelog**: [`vfox-v2026.10.0...v2026.10.1`](https://github.com/jdx/mise/compare/vfox-v2026.10.0...v2026.10.1)

## 💚 Sponsor mise

mise is built and maintained by [@jdx](https://github.com/jdx), an open source developer at [**entire.io**](https://entire.io/), the title sponsor of his open source work.

If mise saves you or your team time, please consider becoming an [individual or company sponsor](https://jdx.dev/sponsors.html). Your support funds ongoing development and helps keep mise fast, free, and independent.

### Contributors

- [![@pikeas](https://avatars.githubusercontent.com/u/686573?s=64&v=4)](https://github.com/pikeas)
- [![@JamBalaya56562](https://avatars.githubusercontent.com/u/88115388?s=64&v=4)](https://github.com/JamBalaya56562)
- [![@Marukome0743](https://avatars.githubusercontent.com/u/146040408?s=64&v=4)](https://github.com/Marukome0743)

pikeas, JamBalaya56562, and Marukome0743


Assets55

- [install.sh](https://github.com/jdx/mise/releases/download/v2026.10.1/install.sh)



sha256:6fc71f919aac1152e8d7bc94dc57ea3d0db7c29babbb7a84821f12b0fe72e9ed



14.8 KB9 hours ago2026-10-03T14:07:41Z

- [install.sh.minisig](https://github.com/jdx/mise/releases/download/v2026.10.1/install.sh.minisig)



sha256:3a4ce8c4f437092e40fce4a019bab777181b770af43cb19a7b886db8264ab85c



305 Bytes9 hours ago2026-10-03T14:07:41Z

- [install.sh.sig](https://github.com/jdx/mise/releases/download/v2026.10.1/install.sh.sig)



sha256:0caaf66cf122ebb3a7db9efd055ea5f1b76e025b0080006f96d982df864a61cb



5.59 KB9 hours ago2026-10-03T14:07:41Z

- [mise-v2026.10.1-linux-arm64](https://github.com/jdx/mise/releases/download/v2026.10.1/mise-v2026.10.1-linux-arm64)



sha256:d4785456a86d1c836f09ccfe00aa6b044b18d29d79aae46a1820c96024cf9c38



128 MB9 hours ago2026-10-03T14:07:41Z

- [mise-v2026.10.1-linux-arm64-musl](https://github.com/jdx/mise/releases/download/v2026.10.1/mise-v2026.10.1-linux-arm64-musl)



sha256:5a82c0d197311a15ce20cf6a7ef7d02146551d3445f5434c5fd3c40e88d3e3a9



128 MB9 hours ago2026-10-03T14:07:41Z

- [mise-v2026.10.1-linux-arm64-musl.tar.gz](https://github.com/jdx/mise/releases/download/v2026.10.1/mise-v2026.10.1-linux-arm64-musl.tar.gz)



sha256:1a68e659d939e67710546a09cf6d683b19d814ac261e5318a9ce91ab461bf06e



49.9 MB9 hours ago2026-10-03T14:07:41Z

- [mise-v2026.10.1-linux-arm64-musl.tar.xz](https://github.com/jdx/mise/releases/download/v2026.10.1/mise-v2026.10.1-linux-arm64-musl.tar.xz)



sha256:d213d3e0b8f767e40c5f2a362636aac4314dea8c587c7b4037b5ce0443e55ff0



28.5 MB9 hours ago2026-10-03T14:07:42Z

- [mise-v2026.10.1-linux-arm64-musl.tar.zst](https://github.com/jdx/mise/releases/download/v2026.10.1/mise-v2026.10.1-linux-arm64-musl.tar.zst)



sha256:42c46155501893f3a387a93c02a5a9b30f2732a7a5d4b86c92e2d65e3cae1329



35 MB9 hours ago2026-10-03T14:07:42Z

- [mise-v2026.10.1-linux-arm64.tar.gz](https://github.com/jdx/mise/releases/download/v2026.10.1/mise-v2026.10.1-linux-arm64.tar.gz)



sha256:15b7e978812d1657e615f42f366c4101f9d8733b96a85b12779a9f3f3e2d8596



49.7 MB9 hours ago2026-10-03T14:07:43Z

- [mise-v2026.10.1-linux-arm64.tar.xz](https://github.com/jdx/mise/releases/download/v2026.10.1/mise-v2026.10.1-linux-arm64.tar.xz)



sha256:d2f911f365abb3cff62f7d4f2f44f97934bb3899b8014ba451da7956009fc4a1



28.4 MB9 hours ago2026-10-03T14:07:43Z

- [Source code (zip)](https://github.com/jdx/mise/archive/refs/tags/v2026.10.1.zip)

15 hours ago2026-10-03T08:34:38Z

- [Source code (tar.gz)](https://github.com/jdx/mise/archive/refs/tags/v2026.10.1.tar.gz)

15 hours ago2026-10-03T08:34:38Z

- [Release attestation (json)](https://github.com/jdx/mise/attestations/52424026/download)

15 hours ago2026-10-03T08:34:38Z

- Show all 55 assets
Loading


### Uh oh!

There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).

👍2petr-korobeinikov and aleksa-radojicic reacted with thumbs up emoji❤️1kitswas reacted with heart emoji

All reactions

- 👍2 reactions
- ❤️1 reaction

3 people reacted

## v2026.10.0: Stricter signer checks for cosign and GitHub attestations, trust for inline options in .tool-versions

[v2026.10.0: Stricter signer checks for cosign and GitHub attestations, trust for inline options in .tool-versions](https://github.com/jdx/mise/releases/tag/v2026.10.0)

Compare

# Choose a tag to compare

## Sorry, something went wrong.

Filter

Loading

## Sorry, something went wrong.

### Uh oh!

There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).

## No results found

[View all tags](https://github.com/jdx/mise/tags)

![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)

released this

2 days ago
02 Oct 01:42


Immutable
release. Only release title and notes can be modified.

[v2026.10.0](https://github.com/jdx/mise/tree/v2026.10.0)

This tag was signed with the committer’s **verified signature**.


[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)

GPG key ID: 8B81C9D17413A06D

Verified
on Oct 1, 2026, 07:03 PM

[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).


[`bc11f90`](https://github.com/jdx/mise/commit/bc11f90c74eba23bf0d7350efb540e62fb7d9ffd)

This commit was created on GitHub.com and signed with GitHub’s **verified signature**.


GPG key ID: B5690EEEBB952194

Verified
on Oct 1, 2026, 06:56 PM

[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).


This release tightens supply-chain verification. Keyless cosign bundles must now match a pinned signer identity, and GitHub attestation workflow checks no longer accept partial matches. It also closes a `.tool-versions` trust gap that could leak `GITHUB_TOKEN`, adds a per-cask `appdir` option for Homebrew casks, and fixes problems with locked SLSA installs, musl hosts and `mise backends switch`.

## Security

- **Inline tool options in `.tool-versions` now require trust.** This is the `.tool-versions` version of the `mise.toml` fix in 2026.9.18. An untrusted project could ship a `github:` entry with inline options, such as `[api_url=...]`, pointing at another host. Commands such as `mise ls`, `env`, `current`, `outdated` and `latest` would then send your `GITHUB_TOKEN` to that host without asking for trust. Any entry whose tool name contains `[` now requires trust, the same as Tera templates. Plain lines like `node 20.0.0` still load without trust, and a `[` inside a comment is ignored. Run `mise trust` for projects you rely on. `MISE_SAFE=1` still skips trust checks. ( [GHSA-wcqh-j26q-g44x](https://github.com/jdx/mise/security/advisories/GHSA-wcqh-j26q-g44x "GHSA-wcqh-j26q-g44x")) [#13869](https://github.com/jdx/mise/pull/13869)\
- **Keyless cosign verification now checks who signed.** Before, the aqua backend only checked that a bundle chained to Sigstore's Fulcio CA. Any GitHub Actions workflow in any repository can get such a certificate, so a bundle signed by the wrong workflow would still pass. mise now applies the registry's `--certificate-identity[-regexp]`, `--certificate-oidc-issuer[-regexp]` and `--certificate-github-workflow-{repository,ref,name,trigger,sha}` options to the signing certificate. It does this for both current and legacy bundles. Keyless verification now requires a pinned identity, and an unknown or empty `--certificate-*` option is an error. Key-based verification (`--key`) is unchanged. Registry patterns that use RE2 `\Q…\E` quoted literals, such as the one for `vfox`, are supported. [#13875](https://github.com/jdx/mise/pull/13875), [#13879](https://github.com/jdx/mise/pull/13879)\
- **GitHub attestation signer workflow matching is anchored.** The expected `signer_workflow` must now match the end of the certificate's workflow path as whole path segments. Before, it was a substring match against the whole identity, so a longer workflow file name such as `release.yml.evil.yml`, or a ref that contained the expected path, would pass. An empty `signer_workflow` now fails verification. Both the bare form (`.github/workflows/release.yml`) and the repository-qualified form (`owner/repo/.github/workflows/release.yml`) still work. [#13877](https://github.com/jdx/mise/pull/13877)\
\
## Added\
\
- **Per-cask app directories.** A `brew-cask:` bootstrap package can set its own `appdir`. This overrides the global `MISE_BREW_CASK_OPT_APPDIR` setting, expands `~/`, and also applies to the cask's dependencies. [#13865](https://github.com/jdx/mise/pull/13865)\
\
\
\
```\
[bootstrap.packages]\
"brew-cask:1password" = { appdir = "/Applications" }\
```\
\
\
\
\
\
\
\
The setting only applies to installs and upgrades: apps that are already installed are not moved. A first install into a new `appdir` won't replace an existing app it doesn't own unless you set `adopt = true`. Other package managers ignore `appdir` and print a warning.\
\
- **`slsa_signer_identity` and `slsa_signer_issuer` options for aqua tools.** These work the same as in the github backend. They let `mise lock` verify and record SLSA provenance for packages whose registry entry has no signer, such as `aqua:google/osv-scanner`. You must set both options to non-empty strings, and together they override any signer in the registry, including version overrides. [#13856](https://github.com/jdx/mise/pull/13856)\
\
- **Registry:**`cloudflare-cf`, Cloudflare's `cf` CLI, which is in beta and installs from `npm:cf`. It provides the `cf` and `cloudflare` binaries. Pin a beta version for now, such as `mise use cloudflare-cf@1.0.0-beta.10`. An unpinned install currently resolves to an unrelated old `0.x` release. [#13871](https://github.com/jdx/mise/pull/13871)\
\
- **Docs:** a new Releases page (under About in the docs) shows a timeline of release sizes, the issues each release resolved, and expandable release notes. [#13855](https://github.com/jdx/mise/pull/13855)\
\
\
## Fixed\
\
- **Locked SLSA installs work again without a registry signer.** Since 2026.9.17, a lockfile that recorded a checksum and SLSA provenance failed with "Aqua registry metadata has no signer\_identity and signer\_issuer" for tools such as `aqua:google/osv-scanner` and `aqua:fluxcd/flux2`. A lock entry with a checksum and recorded provenance is now trusted for SLSA too: the install only checks the artifact digest, as it already did for other provenance types. `locked_verify_provenance` or paranoid mode still re-verify and still require a signer. [#13856](https://github.com/jdx/mise/pull/13856)\
- **aqua on musl hosts.** On Alpine and other musl hosts, an aqua tool whose registry entry only names a glibc build (such as `zizmor`) failed with "no asset found: ...-unknown-linux-musl...". mise now installs the asset the registry names. The binary still needs glibc or `gcompat` to run. `mise lock` for `linux-x64-musl` records the same asset. [#13857](https://github.com/jdx/mise/pull/13857)\
- **`mise backends switch` handles stale lock entries.** Sometimes `mise install` warned that a tool was locked to a replaced backend, for example `asdf:clojure` instead of `vfox:jdx/vfox-clojure`, but `mise backends switch` then reported there was nothing to switch. This happened when the config's version no longer matched the lock entry. The command now switches those entries too. Entries at a version the config no longer resolves to are relocked at the config's version and reported as `replacing stale <tool>@<version>`. [#13859](https://github.com/jdx/mise/pull/13859)\
- **Ctrl-C exits with status 130.** Interrupting `mise install`, `upgrade`, `exec` and similar commands used to exit with 1, the same as an ordinary failure. They now exit with 130 (128 + SIGINT), matching `mise run`, so shells and scripts can tell when a user interrupted. A repeated Ctrl-C during `mise run` also exits with 130. This applies on Unix and Windows. [#13862](https://github.com/jdx/mise/pull/13862)\
- **Declining a trust prompt skips the config for that run.** Before, declining still failed the current command with "not trusted". [#13868](https://github.com/jdx/mise/pull/13868)\
- The one-time startup migration for stale `latest` runtime directories has been removed. It was due to expire in this release and would have blocked normal installs. `mise install` still repairs a stale `latest` directory, but passive commands such as `mise ls` no longer touch it. [#13868](https://github.com/jdx/mise/pull/13868)\
- **Stale dotfile history watchers are diagnosed.** A history watcher started on an older mise can fail every capture with an unknown-field error for newer settings such as `exclude`. `mise doctor` and `mise dot status` now report that the watcher is outdated and tell you to run `mise bootstrap services apply`, which restarts it. [#13864](https://github.com/jdx/mise/pull/13864)\
- **Java:** the missing-metadata error now names the target platform, for example `no metadata found for version zulu-8 on windows-arm64`. [#13873](https://github.com/jdx/mise/pull/13873) ( [@jsiu93](https://github.com/jsiu93))\
\
## Breaking Changes\
\
- **`--from-git` removed from `mise bootstrap`.**`mise bootstrap --from-git` and `mise bootstrap remote --from-git` now fail with an unexpected-argument error. Use `--adopt`, which has been the documented flag since 2026.9.3: [#13872](https://github.com/jdx/mise/pull/13872)\
\
\
\
```\
mise bootstrap --adopt git@github.com:me/dotfiles.git\
```\
\
- **vfox tool plugins that use keyless cosign must pin an identity.** If a `PreInstall` attestation sets `cosign_sig_or_bundle_path` without `cosign_public_key_path`, it must also set `cosign_certificate_identity` or `cosign_certificate_identity_regexp`. You can also set `cosign_certificate_oidc_issuer`. Without an identity, the attestation is rejected. Registry entries that already work with the aqua CLI are not affected. [#13875](https://github.com/jdx/mise/pull/13875)\
\
- **Alpine/musl:** if a registry entry names a gnu asset but the release also ships a musl build, mise now installs the gnu build. Before, it switched to musl. Set `libc = "musl"` on that tool to keep the musl build. [#13857](https://github.com/jdx/mise/pull/13857)\
\
- **Exit code on Ctrl-C:** scripts that checked for exit status 1 after an interrupt should now check for 130. [#13862](https://github.com/jdx/mise/pull/13862)\
\
\
## New Contributors\
\
- [@jsiu93](https://github.com/jsiu93) made their first contribution in [#13873](https://github.com/jdx/mise/pull/13873)\
\
**Full Changelog**: [`vfox-v2026.9.20...v2026.10.0`](https://github.com/jdx/mise/compare/vfox-v2026.9.20...v2026.10.0)\
\
## 💚 Sponsor mise\
\
mise is built and maintained by [@jdx](https://github.com/jdx), an open source developer at [**entire.io**](https://entire.io/), the title sponsor of his open source work.\
\
If mise saves you or your team time, please consider becoming an [individual or company sponsor](https://jdx.dev/sponsors.html). Your support funds ongoing development and helps keep mise fast, free, and independent.\
\
### Contributors\
\
- [![@jsiu93](https://avatars.githubusercontent.com/u/12907730?s=64&v=4)](https://github.com/jsiu93)\
\
jsiu93\
\
\
Assets55\
\
Loading\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
👍7JamBalaya56562, iliqiliev, german-quantica, petr-korobeinikov, msmafra, aleksa-radojicic, and kitswas reacted with thumbs up emoji🎉1methbkts reacted with hooray emoji\
\
All reactions\
\
- 👍7 reactions\
- 🎉1 reaction\
\
8 people reacted\
\
## v2026.9.18: Remote config includes, OCI task catalogs, and a trust fix for inline tool options\
\
[v2026.9.18: Remote config includes, OCI task catalogs, and a trust fix for inline tool options](https://github.com/jdx/mise/releases/tag/v2026.9.18)\
\
Compare\
\
# Choose a tag to compare\
\
## Sorry, something went wrong.\
\
Filter\
\
Loading\
\
## Sorry, something went wrong.\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
## No results found\
\
[View all tags](https://github.com/jdx/mise/tags)\
\
![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)\
\
released this\
\
3 days ago\
30 Sep 12:51\
\
\
Immutable\
release. Only release title and notes can be modified.\
\
[v2026.9.18](https://github.com/jdx/mise/tree/v2026.9.18)\
\
This tag was signed with the committer’s **verified signature**.\
\
\
[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)\
\
GPG key ID: 8B81C9D17413A06D\
\
Verified\
on Sep 30, 2026, 07:19 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
[`5a63dc5`](https://github.com/jdx/mise/commit/5a63dc5e8536b70c2d6661a8fe9d099aeb742ab7)\
\
This commit was created on GitHub.com and signed with GitHub’s **verified signature**.\
\
\
GPG key ID: B5690EEEBB952194\
\
Verified\
on Sep 30, 2026, 07:14 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
`mise.toml` can now `include` a shared config file from a git repository or OCI registry, and `task_config.includes` accepts OCI artifacts. The release also closes a trust bypass that could send `GITHUB_TOKEN` to an attacker-controlled host, adds gem registry sources, and fixes several daemon and dotfiles problems.\
\
## Security\
\
- **Inline tool options now require trust.** Before this change, a `mise.toml` in an untrusted directory could hide options in a tool key, for example a `github:` tool key with `[api_url=...]` pointing at another host. mise loaded the file without trust because the value was a plain version string. Commands such as `mise ls`, `env`, `current`, `outdated`, `upgrade --dry-run` and `latest` then sent `GITHUB_TOKEN` to that `api_url`. Now any tool key that contains `[` requires trust, the same as `{ ... }` option tables already did. Plain keys like `node` or `"cargo:eza"` still load without trust. If you use inline options in a project you haven't trusted yet, run `mise trust`. `MISE_SAFE=1` still skips trust checks entirely. [#13849](https://github.com/jdx/mise/pull/13849)\
\
## Added\
\
- **Include shared config from git or OCI.** Organizations can keep tool versions, env and hooks in one place and pull them into every repo: [#13843](https://github.com/jdx/mise/pull/13843)\
\
\
\
```\
include = [\
    "git::<repo-url>//mise.toml?ref=main",\
    "oci::ghcr.io/myorg/platform-config@sha256:0f1e2d3c...",\
]\
\
[tools]\
node = "22"   # the file's own entries override the included ones\
```\
\
\
\
\
\
\
\
A `git::` include points at a `.toml` file in a repository. An `oci::` include points at an artifact with a `mise.toml` at its root. The included file is merged beneath the file that includes it, and a later include overrides an earlier one. It uses that file's trust, config root and lockfile. A fragment may contain `[tools]`, `[tool_alias]`, `[env]`, `[vars]`, `[hooks]`, `[alias]`, `[shell_alias]`, `[plugins]`, `[wrappers]` and `min_version`. Anything else is an error, including nested `include`, `[settings]`, tasks, `[dotfiles]` and `[daemons]`.\
  - An untrusted config never fetches, and safe mode never fetches for project config.\
  - Paranoid mode requires a full commit sha or an OCI digest.\
  - Fragments are cached in `MISE_CACHE_DIR/config-includes`. Pinned refs are never fetched again. Branches and tags are refreshed after `fetch_remote_versions_cache` expires, and only by commands that check remote versions, such as `install`, `up` and `use`. If a refresh fails, the cached copy is used with a warning.\
- **OCI task catalogs.**`task_config.includes` accepts `oci::` references, in addition to `git::`. The artifact is pulled, verified against its digests, cached in `MISE_CACHE_DIR/remote-oci-tasks-cache`, and loaded like a local task directory. Credentials come from `docker login`/`podman login`. Artifacts with symlinks or special files are rejected. Signatures are not verified, so pin `@sha256:` if you need the contents to stay the same. [#13820](https://github.com/jdx/mise/pull/13820)\
\
\
\
```\
[task_config]\
includes = ["oci::ghcr.io/myorg/shared-tasks:1.0.0"]\
```\
\
\
\
\
\
\
\
\
\
```\
oras push ghcr.io/myorg/shared-tasks:1.0.0 build.toml scripts/deploy\
```\
\
- **`mise bootstrap --from` accepts `?ref=`** to select a branch, tag or commit, for example `mise bootstrap --from 'git::<repo-url>?ref=v1'`. The `git::` prefix is optional. With `--update`, mise resolves the ref on origin again and fast-forwards branches. A ref that was deleted upstream is an error. [#13822](https://github.com/jdx/mise/pull/13822)\
\
- **Install a gem from a specific registry.** The new `source` option sends version lookup and install for one gem to that registry, and leaves the machine's `gem sources` unchanged. Credentials in the URL are redacted from logs and install metadata. [#13391](https://github.com/jdx/mise/pull/13391) ( [@waynehoover](https://github.com/waynehoover))\
\
\
\
```\
[tools]\
"gem:internal-tool" = { version = "latest", source = "<registry-url>" }\
```\
\
\
\
\
\
\
\
A GitHub Packages source (the `rubygems.pkg.github.com` host) without credentials now uses the GitHub token mise already resolves. The token needs `read:packages`. GitHub Packages has no versions API, so you must pin an exact version there. [#13832](https://github.com/jdx/mise/pull/13832) ( [@waynehoover](https://github.com/waynehoover))\
\
- **`mise lock --sidecars`** lists the native dependency sidecar directories (aube for npm, uv for Python) that must be committed along with `mise.lock`. It doesn't resolve, install or write anything. It marks missing sidecars, follows symlinked lockfiles, and supports `--json` for tools such as Renovate. [#13819](https://github.com/jdx/mise/pull/13819)\
\
- **Daemon presets export their named ports** as environment variables, for example `CRDB_HTTP_PORT` for a `cockroachdb` daemon named `crdb`, or `AUTHZ_HTTP_PORT` and `AUTHZ_METRICS_PORT` for a `spicedb` daemon named `authz`. The values include worktree offsets from `port = "auto"` and any `ports.*` overrides, so you can use them in `[env]` without working out the port yourself. [#13835](https://github.com/jdx/mise/pull/13835)\
\
- **`proxy_idle_timeout` for daemons** is now documented, typed in the JSON schema and validated. Set a duration such as `"30m"` to stop a proxy-started daemon, and then its dependencies, after that long without traffic. Set it to `false` to opt out. mise rejects `true`, bare numbers and values that don't look like durations, and names the daemon in the error. This requires pitchfork 2.27.0. [#13830](https://github.com/jdx/mise/pull/13830), [#13836](https://github.com/jdx/mise/pull/13836)\
\
- **Registry:** added `lstk`, the CLI that replaces LocalStack's old one. Installing `localstack` now warns that it is deprecated and suggests `mise use lstk`. Registry entries can now set a `deprecated` message. [#13817](https://github.com/jdx/mise/pull/13817)\
\
\
## Fixed\
\
- **`mise generate install-script`** no longer panics with or without `--version`. The generated wrapper now passes its pinned version to the installer. Before, a wrapper named for one release could install and keep running an older one. Without `--version`, the pin is the release `mise self-update` would pick. [#13816](https://github.com/jdx/mise/pull/13816)\
- `mise oci build`, `push` and `run` no longer fail on a `required` env var when the project's `[oci.env]` gives it a value. You can now use a placeholder for a secret that only exists at runtime. Other commands still require the variable. [#13821](https://github.com/jdx/mise/pull/13821)\
- `mise lock` now prints a warning with the cause when it skips a tool, for example a GitHub rate limit, instead of only counting it as skipped. You get one warning per tool. [#13831](https://github.com/jdx/mise/pull/13831)\
- **Daemons:**\
  - `mise daemons start|stop|restart --all` now works and applies to every daemon in the current project. Before, pitchfork rejected the command. `--all` can't be combined with daemon names or `--group`. [#13827](https://github.com/jdx/mise/pull/13827)\
  - The first `mise daemons start` or `restart` now installs the preset's tool. Before, it failed with a false "requires ... but \[tools\] selects ..." error. Only the tools of the requested daemons and their dependencies are installed. [#13837](https://github.com/jdx/mise/pull/13837)\
  - URLs printed for projects without an explicit `[daemons_settings] namespace` now route through pitchfork's proxy instead of returning 404. This needs a pitchfork that supports `config add --label`. mise checks for the flag itself and picks it up when pitchfork is upgraded. [#13833](https://github.com/jdx/mise/pull/13833)\
  - When an automatically allocated daemon port is already taken, mise explains how to pin a different port in `mise.local.toml`. mise no longer adds its own error lines after pitchfork's message, and it exits with pitchfork's status. [#13839](https://github.com/jdx/mise/pull/13839)\
- **Dotfiles:**\
  - `mise dot` and other commands that use mise's internal Git calls work again with Git for Windows 2.56. [#13812](https://github.com/jdx/mise/pull/13812) ( [@genskyff](https://github.com/genskyff))\
  - After an upgrade, the history watcher now notices that the mise binary was replaced, saves pending edits and exits so the service manager restarts it on the new version. A watcher started by hand with `mise dot watch` has to be started again. `mise dot status` now says when captures are failing. [#13845](https://github.com/jdx/mise/pull/13845)\
\
**Full Changelog**: [`vfox-v2026.9.19...v2026.9.18`](https://github.com/jdx/mise/compare/vfox-v2026.9.19...v2026.9.18)\
\
## 💚 Sponsor mise\
\
mise is built and maintained by [@jdx](https://github.com/jdx), an open source developer at [**entire.io**](https://entire.io/), the title sponsor of his open source work.\
\
If mise saves you or your team time, please consider becoming an [individual or company sponsor](https://jdx.dev/sponsors.html). Your support funds ongoing development and helps keep mise fast, free, and independent.\
\
### Contributors\
\
- [![@waynehoover](https://avatars.githubusercontent.com/u/115143?s=64&v=4)](https://github.com/waynehoover)\
- [![@genskyff](https://avatars.githubusercontent.com/u/43875771?s=64&v=4)](https://github.com/genskyff)\
\
waynehoover and genskyff\
\
\
Assets55\
\
Loading\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
👍4german-quantica, iliqiliev, petr-korobeinikov, and little-sparkleeesss reacted with thumbs up emoji🎉1methbkts reacted with hooray emoji❤️4kitswas, iliqiliev, fapont, and twelvelabs reacted with heart emoji\
\
All reactions\
\
- 👍4 reactions\
- 🎉1 reaction\
- ❤️4 reactions\
\
8 people reacted\
\
## v2026.9.17: Self-update waits 24 hours for new releases and verifies signed packslips\
\
[v2026.9.17: Self-update waits 24 hours for new releases and verifies signed packslips](https://github.com/jdx/mise/releases/tag/v2026.9.17)\
\
Compare\
\
# Choose a tag to compare\
\
## Sorry, something went wrong.\
\
Filter\
\
Loading\
\
## Sorry, something went wrong.\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
## No results found\
\
[View all tags](https://github.com/jdx/mise/tags)\
\
![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)\
\
released this\
\
5 days ago\
29 Sep 10:06\
\
\
Immutable\
release. Only release title and notes can be modified.\
\
[v2026.9.17](https://github.com/jdx/mise/tree/v2026.9.17)\
\
This tag was signed with the committer’s **verified signature**.\
\
\
[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)\
\
GPG key ID: 8B81C9D17413A06D\
\
Verified\
on Sep 29, 2026, 04:40 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
[`5a5b928`](https://github.com/jdx/mise/commit/5a5b9286b8d41f7a16d2eff5a0edb15fa5f4418c)\
\
This commit was created on GitHub.com and signed with GitHub’s **verified signature**.\
\
\
GPG key ID: B5690EEEBB952194\
\
Verified\
on Sep 29, 2026, 04:33 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
`mise self-update` and the mise.run installer now pick the newest stable release that is at least 24 hours old. Updates also check the release's signed packslip before replacing the binary. This release also adds a machine-local global `miserc`, an opt-in way for command-not-found to install registry tools, and a `postinstall` mode that runs on every install. It fixes several Homebrew formula builds and closes a trust gap in paranoid mode.\
\
## Changed\
\
- **Self-update and installs wait for a minimum release age.** When no version is pinned, `mise self-update`, automatic updates, update notifications, and the mise.run installer now choose the newest stable release published at least 24 hours ago. Explicit versions skip the delay. An unpinned update never downgrades a newer installation, even with `--force`. The age is taken from, in order: `--minimum-release-age`, then `self_update.minimum_release_age`, then the global `minimum_release_age` setting, then `24h`. Use `0s` to get releases right away. [#13782](https://github.com/jdx/mise/pull/13782)\
\
\
\
```\
[settings]\
self_update.minimum_release_age = "7d"\
```\
\
\
\
\
\
\
\
\
\
```\
mise self-update --minimum-release-age 0s\
curl -fsSL https://mise.run | MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE=7d sh\
```\
\
\
\
\
\
\
\
The installer reads environment variables only (`MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE`, `MISE_MINIMUM_RELEASE_AGE`), and it accepts integer `s`/`m`/`h`/`d`/`w` durations. A saved copy of the installer no longer pins a default version, so set `MISE_VERSION` if you need reproducible installs.\
\
- **Self-update verifies signed packslips.** For releases v2026.9.3 and later, `mise self-update` now requires a valid signed packslip, on top of the embedded archive signature it already checked. mise checks the archive digest and size, the version, the release workflow, and the transparency-log timestamp. Trust is pinned to mise's GitHub repository ID (`586920414`), so a rename or move to another organization still works, but a different repository that takes over the name is rejected. If the manifest is missing or invalid, mise stops and leaves the current binary in place. Releases 2026.9.2 and older still update with signature-only checks. Custom mirrors must serve the original signed manifests and archives. [#13785](https://github.com/jdx/mise/pull/13785)\
\
- `mise self-update` now downloads with mise's own HTTP client and progress display, and extracts only the expected executable from the verified archive. Plugin-update failures during self-update now show as warnings and no longer fail the command. [#13783](https://github.com/jdx/mise/pull/13783)\
\
- **Registry:**`timoni` (0.35.0+) and `worktrunk` (0.80.0+) now install from signed packslips, which include completions and skills. Older versions still install through their existing backends, and you can list them with `mise ls-remote aqua:stefanprodan/timoni` or `mise ls-remote aqua:max-sixty/worktrunk`. [#13780](https://github.com/jdx/mise/pull/13780)\
\
\
## Added\
\
- **Machine-local global miserc.**`~/.config/mise/miserc.local.toml` applies from any directory and overrides fields in the shared global `miserc.toml`. You can use it to pick an environment on one machine without editing shared files. Project miserc files, `MISE_ENV`, and `-E` still take precedence over it. [#13778](https://github.com/jdx/mise/pull/13778)\
\
\
\
```\
# ~/.config/mise/miserc.local.toml\
env = ["work"]\
```\
\
- **Command-not-found can install tools you haven't configured (opt-in).** With `not_found_auto_install_registry = true`, running an unknown command installs the matching registry tool at `latest` and adds it to your global config. This only happens when exactly one registry tool provides that command. mise skips commands with several providers, and it skips disabled tools and tools that don't support your OS. The default is `false`. [#13781](https://github.com/jdx/mise/pull/13781)\
\
\
\
```\
[settings]\
not_found_auto_install_registry = true\
```\
\
- **`postinstall` that runs on every install.** With `when = "always"`, a tool's `postinstall` command runs on every `mise install` that selects the tool, even when that version is already installed. Dry runs skip it. The plain string form and tables without `when` still run only on a fresh install or repair. [#13789](https://github.com/jdx/mise/pull/13789)\
\
\
\
```\
[tools]\
node = { version = "26", postinstall = { run = "npm install -g corepack", when = "always" } }\
```\
\
- **Warnings for outdated lockfile formats.** If a lockfile format was replaced more than six months ago, mise warns once per file during commands like `mise install`, `mise exec`, and task runs. The warning shows the command to fix it: `mise lock --upgrade`, or `mise lock --global --upgrade` for a global config. [#13779](https://github.com/jdx/mise/pull/13779)\
\
- **Per-machine email for dotfiles history commits.** The new `[history].git_email` setting sets the commit email, and `{hostname}` is filled in when each commit is made, so you can tell which machine saved a checkpoint. Without the setting, commits still use `mise@localhost`. [#13791](https://github.com/jdx/mise/pull/13791)\
\
\
\
```\
[history]\
git_email = "mise@{hostname}"\
```\
\
\
## Fixed\
\
- **Paranoid mode:**`--yes`, `MISE_YES=1`, and CI auto-confirmation no longer approve trust for new or edited config files. Unattended runs now fail until you approve the file with `mise trust` or at an interactive prompt. [#13796](https://github.com/jdx/mise/pull/13796)\
- **npm with pnpm 12:** mise now passes `minimum_release_age` to pnpm as `--config.minimum-release-age`. pnpm 12 silently ignored the camelCase spelling, so the cutoff wasn't applied to transitive dependencies. The new spelling also works on pnpm 10.16+ and 11. [#13764](https://github.com/jdx/mise/pull/13764) ( [@Nagato-Yuzuru](https://github.com/Nagato-Yuzuru))\
- `mise upgrade --bump` now updates an exact-release request to the latest release with the same prefix, for example `29.1` to `29.1.1`. Before, it kept the old version. [#13759](https://github.com/jdx/mise/pull/13759) ( [@ryoikarashi](https://github.com/ryoikarashi))\
- `go:` installs that resolve `latest` to a version no longer retry without the `v` prefix after a failure. That extra retry used to hide Go's original error. Explicit unprefixed versions still get the retry, and if both attempts fail, the error now shows both failures. [#13794](https://github.com/jdx/mise/pull/13794)\
- **Homebrew formula builds:**\
  - Formulas that write files with `Pathname#write` no longer fail after the build with `super: no superclass method 'write'`. This affected generated completions (such as starship) and `inreplace`. [#13760](https://github.com/jdx/mise/pull/13760) ( [@jacobbednarz](https://github.com/jacobbednarz))\
  - Formulas that include Homebrew's `Language::*` mixins (such as `qmk`) no longer fail with a `NameError` while mise reads them. Install-time helpers that mise doesn't support now produce a clear error message. [#13328](https://github.com/jdx/mise/pull/13328) ( [@waynehoover](https://github.com/waynehoover))\
  - Source archives whose URL has no file extension, such as GitHub codeload tarballs, are now detected by their contents and unpacked. Before, they were copied into the build directory unextracted. This also applies to casks. [#13750](https://github.com/jdx/mise/pull/13750) ( [@jacobbednarz](https://github.com/jacobbednarz))\
\
## Documentation\
\
- The landing page now has a seven-minute showreel of mise, and the mise run music video replaces the theme song. [#13797](https://github.com/jdx/mise/pull/13797), [#13799](https://github.com/jdx/mise/pull/13799)\
\
## New Contributors\
\
- [@ryoikarashi](https://github.com/ryoikarashi) made their first contribution in [#13759](https://github.com/jdx/mise/pull/13759)\
\
**Full Changelog**: [`vfox-v2026.9.18...v2026.9.17`](https://github.com/jdx/mise/compare/vfox-v2026.9.18...v2026.9.17)\
\
## 💚 Sponsor mise\
\
mise is built and maintained by [@jdx](https://github.com/jdx), an open source developer at [**entire.io**](https://entire.io/), the title sponsor of his open source work.\
\
If mise saves you or your team time, please consider becoming an [individual or company sponsor](https://jdx.dev/sponsors.html). Your support funds ongoing development and helps keep mise fast, free, and independent.\
\
### Contributors\
\
- [![@waynehoover](https://avatars.githubusercontent.com/u/115143?s=64&v=4)](https://github.com/waynehoover)\
- [![@jacobbednarz](https://avatars.githubusercontent.com/u/283234?s=64&v=4)](https://github.com/jacobbednarz)\
- [![@ryoikarashi](https://avatars.githubusercontent.com/u/5750408?s=64&v=4)](https://github.com/ryoikarashi)\
- [![@Nagato-Yuzuru](https://avatars.githubusercontent.com/u/169710796?s=64&v=4)](https://github.com/Nagato-Yuzuru)\
\
waynehoover, jacobbednarz, and 2 other contributors\
\
\
Assets55\
\
Loading\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
👍5petr-korobeinikov, JamBalaya56562, iliqiliev, mkvlrn, and little-sparkleeesss reacted with thumbs up emoji😄1iliqiliev reacted with laugh emoji🎉2NonlinearFruit and methbkts reacted with hooray emoji\
\
All reactions\
\
- 👍5 reactions\
- 😄1 reaction\
- 🎉2 reactions\
\
7 people reacted\
\
## v2026.9.16: Per-tool libc for aqua tools, monorepo task path aliases, and packslip pins that survive repo renames\
\
[v2026.9.16: Per-tool libc for aqua tools, monorepo task path aliases, and packslip pins that survive repo renames](https://github.com/jdx/mise/releases/tag/v2026.9.16)\
\
Compare\
\
# Choose a tag to compare\
\
## Sorry, something went wrong.\
\
Filter\
\
Loading\
\
## Sorry, something went wrong.\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
## No results found\
\
[View all tags](https://github.com/jdx/mise/tags)\
\
![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)\
\
released this\
\
last week\
28 Sep 10:27\
\
\
Immutable\
release. Only release title and notes can be modified.\
\
[v2026.9.16](https://github.com/jdx/mise/tree/v2026.9.16)\
\
This tag was signed with the committer’s **verified signature**.\
\
\
[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)\
\
GPG key ID: 8B81C9D17413A06D\
\
Verified\
on Sep 28, 2026, 04:46 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
[`2184db8`](https://github.com/jdx/mise/commit/2184db810a7e2c29475abdba53e5af910e96859a)\
\
This commit was created on GitHub.com and signed with GitHub’s **verified signature**.\
\
\
GPG key ID: B5690EEEBB952194\
\
Verified\
on Sep 28, 2026, 04:40 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
Aqua tools can now choose glibc or musl builds one tool at a time, and monorepo roots can get short task path aliases. Packslip tools keep installing after their GitHub or GitLab repository is renamed, because mise now pins them by repository ID, recorded in a new lockfile revision 3. SLSA provenance checks now require the expected signer identity. This release also fixes regressions in `mise run --no-timings`, `cargo +nightly` and the `outdated`/`upgrade` version comparison, and speeds up shims and config loading.\
\
## Added\
\
- **Per-tool `libc` for aqua tools.** On glibc Linux, mise prefers a release's gnu build even when the aqua registry names the musl one. That breaks tools whose musl build is the fully static one, such as `aqua:domcyrus/rustnet`. You can now pick the build for a single tool instead of changing the global `libc` setting. [#13701](https://github.com/jdx/mise/pull/13701)\
\
\
\
```\
[tools]\
"aqua:domcyrus/rustnet" = { version = "latest", libc = "musl" }\
```\
\
\
\
\
\
\
\
The option accepts `glibc` (or `gnu`) and `musl`. mise never falls back to the other libc for that tool. The option applies to install, `mise lock`, and checksum, signature and provenance lookups, and it is recorded in the lockfile's tool options. A platform that already names a libc (a musl host or a `linux-*-musl` lockfile platform) still wins. A version that is already installed keeps its build until you run `mise install --force`. `mise ls-remote` still uses the host libc. If a registry template uses a variable named `libc`, set it as `vars.libc`.\
\
- **Path aliases for monorepo tasks.** Deeply nested config roots can now have a short name. [#13756](https://github.com/jdx/mise/pull/13756)\
\
\
\
```\
monorepo_root = true\
\
[monorepo]\
config_roots = ["foo/bar/baz/abc/123"]\
\
[monorepo.path_aliases]\
"123" = "foo/bar/baz/abc/123"\
```\
\
\
\
\
\
\
\
`mise run //123:build` runs `//foo/bar/baz/abc/123:build`. Aliases also work in task dependencies, in patterns like `//123:*`, and in child paths like `//123/sub:build`. Each alias must be a single path segment, must point at a configured root, and can't overlap an existing root path. A task's full path is still its canonical name.\
\
- **Packslip tools keep installing after a repository rename.** mise now pins GitHub and GitLab packslip projects by the repository ID recorded in the signing certificate, not only by name. If `old/tool` is renamed to `new/tool` under the same owner, `packslip:github.com/old/tool` keeps installing and prints a warning once, asking you to update the config. You don't need `mise packslip forget`. mise refuses a transfer to another owner. It also refuses a different repository that takes over a pinned name, which is how a deleted and re-created name looks. To accept either one, run `mise packslip forget` for the old name, and for a re-created repository also remove the tool's `mise.lock` entries. [#13702](https://github.com/jdx/mise/pull/13702), [#13738](https://github.com/jdx/mise/pull/13738)\
\
In lockfile revision 3, the IDs are stored as:\
\
\
\
```\
[tools.hk."platforms.linux-x64"]\
repository_ids = { repository = "922514152", owner = "216188" }\
```\
\
- **`mise dot track --allow-plaintext`.** Directly tracking a file with a credential-like name (for example `~/commit-mossy-token.md`) used to report success while every history save quietly left the file out. `mise dot track` now asks whether to save the file in plaintext, and the default answer is No. In non-interactive use, pass `--allow-plaintext`. `--yes` does not approve plaintext. The choice is saved as `allow_plaintext = true` on the `[dotfiles]` entry. For real credentials, use `--encrypt`. [#13749](https://github.com/jdx/mise/pull/13749)\
\
- **Registry:**`mise use mbx` now resolves to `mr-boxington`. [#13752](https://github.com/jdx/mise/pull/13752)\
\
\
## Fixed\
\
- `mise outdated` and upgrade warnings no longer offer an older release as an update when the installed version has a `v` or `V` prefix. For example, `v2.1.280 → 2.1.278` was shown as an update. Versions that differ only in build metadata (for example `1.36.4+k3s1` and `1.36.4+k3s2`) are now treated as equal. [#13690](https://github.com/jdx/mise/pull/13690) ( [@himkt](https://github.com/himkt))\
- `mise run --no-cache` and `mise tasks run --no-cache` now clone remote `git::` task includes again, and fetch remote tasks that run as dependencies again. Before, both kept using the cached copy. [#13697](https://github.com/jdx/mise/pull/13697) ( [@irisTa56](https://github.com/irisTa56))\
- `mise run --no-timings` hides each task's "Finished in …" line again, not only the run total. It also overrides `MISE_TASK_TIMINGS=1`. This had regressed in v2025.11.2. [#13718](https://github.com/jdx/mise/pull/13718)\
- `cargo +nightly` works again with `rust = "nightly"`. Since 2026.8.6 mise installs a dated nightly, so rustup had no toolchain named `nightly`. Depending on rustup's auto-install setting, `cargo +nightly` then either failed or downloaded a second, unpinned nightly. mise now also sets up rustup's `nightly-<host>` toolchain from the pinned nightly, using reflinks or hardlinks. It leaves alone a rustup nightly that is newer or has extra components or targets. Explicitly dated requests such as `nightly-2026-08-13` don't touch it. Existing installs pick this up on their next nightly install, or right away with `mise install -f rust`. [#13707](https://github.com/jdx/mise/pull/13707)\
- Running `mise dot track` again on a path that is already tracked now reports "already tracked". It no longer prompts, rewrites the config, or records an empty checkpoint. Changed file contents and flags that change the declaration (such as `--no-autosave`) are still saved. [#13648](https://github.com/jdx/mise/pull/13648)\
- Blob-pack downloads from the remote cache now retry transient stream errors, the same way single blob downloads do. [#13715](https://github.com/jdx/mise/pull/13715)\
\
## Security\
\
- **SLSA provenance must come from the expected signer.** Before, any valid Sigstore signature, even from an unrelated workflow, passed SLSA verification. mise now checks the certificate's URI identity and OIDC issuer against the values configured for the tool:\
\
\
  - aqua registry entries: `signer_identity` and `signer_issuer` under `slsa_provenance`\
  - `github:` tools: the `slsa_signer_identity` and `slsa_signer_issuer` tool options (the identity supports `{{version}}` templating)\
  - vfox plugins: `slsa_signer_identity` and `slsa_signer_issuer` returned from `PreInstall`\
\
If a tool doesn't configure both values, mise skips the SLSA check and uses any other verification available. For now this applies to the bundled aqua packages that have SLSA metadata but no signer fields. SLSA lock entries are checked again on every install, even when a checksum is present. [#13725](https://github.com/jdx/mise/pull/13725)\
\
- Public-key DSSE bundles used by aqua and vfox verification must now have a SHA-256 subject digest that matches the downloaded artifact. Before, a valid bundle could be reused to verify a different download. [#13721](https://github.com/jdx/mise/pull/13721)\
\
\
## Performance\
\
- Shims no longer run `rustup` checks when `rust` is configured alongside other tools. The same goes for `mise exec` with auto-install disabled. One report measured the `go` shim at about 31 ms with `rust` in the config, compared with 12 ms without it. `mise install`, and `mise exec` with auto-install on, still detect and repair missing rustup components. [#13705](https://github.com/jdx/mise/pull/13705)\
- Config loading and fuzzy version resolution (for example `node = "24"`) do less work: plugin shorthands are built without checking every registry tool's backends, global-config checks stop resolving symlinks for every tool, and fuzzy matching no longer compiles regexes. [#13694](https://github.com/jdx/mise/pull/13694), [#13695](https://github.com/jdx/mise/pull/13695), [#13696](https://github.com/jdx/mise/pull/13696)\
\
## Documentation\
\
- The task docs now give the correct default job count (8). They also describe the default output mode correctly: `prefix` when tasks run in parallel and `interleave` when they run in sequence. [#13716](https://github.com/jdx/mise/pull/13716)\
\
## Breaking Changes\
\
- **Lockfile revision 3.** New and empty `mise.lock` files are written as `lockfile_version = 3`, and older mise versions reject them. Existing lockfiles keep their revision when mise writes to them. When a revision 2 lockfile gets packslip repository IDs, mise warns and leaves them out. To store them, run `mise lock --upgrade` once everyone who shares the lockfile is on this release.\
- **SLSA checks for locked tools.** A lockfile entry that requires SLSA now fails with an explanation if the tool has no expected signer configured. To fix it, configure the signer or refresh the entry with `mise lock`.\
- **Dotfiles history shared across machines.** Older mise versions can't read enrollment metadata that includes `allow_plaintext`. Upgrade every machine that shares the history before you use `--allow-plaintext`.\
\
## New Contributors\
\
- [@irisTa56](https://github.com/irisTa56) made their first contribution in [#13697](https://github.com/jdx/mise/pull/13697)\
\
**Full Changelog**: [`vfox-v2026.9.17...v2026.9.16`](https://github.com/jdx/mise/compare/vfox-v2026.9.17...v2026.9.16)\
\
## 💚 Sponsor mise\
\
mise is built and maintained by [@jdx](https://github.com/jdx), an open source developer at [**entire.io**](https://entire.io/), the title sponsor of his open source work.\
\
If mise saves you or your team time, please consider becoming an [individual or company sponsor](https://jdx.dev/sponsors.html). Your support funds ongoing development and helps keep mise fast, free, and independent.\
\
### Contributors\
\
- [![@himkt](https://avatars.githubusercontent.com/u/5164000?s=64&v=4)](https://github.com/himkt)\
- [![@irisTa56](https://avatars.githubusercontent.com/u/27466252?s=64&v=4)](https://github.com/irisTa56)\
\
himkt and irisTa56\
\
\
Assets55\
\
Loading\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
👍3Marukome0743, petr-korobeinikov, and iliqiliev reacted with thumbs up emoji🎉3methbkts, youssefadly237, and iliqiliev reacted with hooray emoji\
\
All reactions\
\
- 👍3 reactions\
- 🎉3 reactions\
\
5 people reacted\
\
## v2026.9.15: vfox tools in OCI images, faster shell prompts, and safer dotfiles pattern matching\
\
[v2026.9.15: vfox tools in OCI images, faster shell prompts, and safer dotfiles pattern matching](https://github.com/jdx/mise/releases/tag/v2026.9.15)\
\
Compare\
\
# Choose a tag to compare\
\
## Sorry, something went wrong.\
\
Filter\
\
Loading\
\
## Sorry, something went wrong.\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
## No results found\
\
[View all tags](https://github.com/jdx/mise/tags)\
\
![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)\
\
released this\
\
last week\
27 Sep 09:51\
\
\
Immutable\
release. Only release title and notes can be modified.\
\
[v2026.9.15](https://github.com/jdx/mise/tree/v2026.9.15)\
\
This tag was signed with the committer’s **verified signature**.\
\
\
[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)\
\
GPG key ID: 8B81C9D17413A06D\
\
Verified\
on Sep 27, 2026, 04:34 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
[`c7c8b86`](https://github.com/jdx/mise/commit/c7c8b86c1b338d5568e077f12bd3fd15e75b94fc)\
\
This commit was created on GitHub.com and signed with GitHub’s **verified signature**.\
\
\
GPG key ID: B5690EEEBB952194\
\
Verified\
on Sep 27, 2026, 04:28 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
`mise oci build` can now package tools installed by vfox plugins, and vfox plugins can repair an existing install when its tool options change. Shell prompts, `cd`, and settings loading are faster. Dotfiles `include`/`exclude` patterns now follow `.gitignore` rules for `*` and a leading `/`, which fixes a case where rollback could delete a live file.\
\
## Added\
\
- **vfox tools in OCI images (experimental).**`mise oci build` used to reject every tool installed by a vfox plugin. It now builds those tools into the image, with one layer per tool plus one layer per plugin at `/mise/plugins/<name>/`, so mise inside the image can resolve the tool without cloning the plugin. The plugin's env hook runs on the build host. Install-dir paths are rewritten to their in-image location, and mise warns when a value points into the host's home directory. Changing a plugin invalidates the reused layers of its tools on `mise oci push`. asdf plugins are still rejected. [#13670](https://github.com/jdx/mise/pull/13670)\
\
- **vfox plugins can repair installs that no longer match tool options.** Plugins can add an optional `hooks/mise_install_satisfied.lua` hook that tells mise an installed version no longer matches its options, for example after a component is added to a gcloud config. `mise install` and auto-install (such as `mise x`) then rerun the plugin's `PostInstall` and the tool's `postinstall` script on the existing install without downloading it again. If the hook itself errors, mise warns and keeps the install. Plugins without the hook work as before. See `docs/tool-plugin-development.md`. [#13668](https://github.com/jdx/mise/pull/13668)\
\
\
\
```\
[tools]\
gcloud = { version = "latest", components = ["gke-gcloud-auth-plugin"] }\
```\
\
- **Git subdirectory installs for `pypi:`.** Git sources now accept a `#subdirectory=` fragment (other fragment keys are passed through as written), and `git+<scheme>://` URLs work without a trailing `.git`. Each subdirectory is its own tool with its own install directory. `latest` still means the repository's newest GitHub release, so pin a branch or commit if those releases predate the subdirectory. [#13607](https://github.com/jdx/mise/pull/13607) ( [@jakedgy](https://github.com/jakedgy))\
\
\
\
```\
[tools]\
"pypi:git+https://github.com/runpantheon/ltui#subdirectory=ltui" = "main"\
"pypi:runpantheon/ltui#subdirectory=jtui" = "main"\
```\
\
- **`max_version` for registry backends.** Registry entries can now set an exclusive `max_version`, alone or together with `min_version`, so older releases can come from a legacy backend and newer ones from another. It requires `version_order = "semver"`. A locked backend is used only for versions it serves. [#13676](https://github.com/jdx/mise/pull/13676)\
\
- **Mac App Store names in `mise bootstrap packages status`.** Installed `mas:` packages now show the app name next to the numeric ID (for example `1056643111 (Clocker)`), and `--json` adds a `name` field. Apps that aren't installed still show only their ID. [#13622](https://github.com/jdx/mise/pull/13622)\
\
- **Registry:** added `sofka` ( [#13612](https://github.com/jdx/mise/pull/13612), [@jylenhof](https://github.com/jylenhof)), `imessage-exporter` ( [#13640](https://github.com/jdx/mise/pull/13640), [@i-api](https://github.com/i-api)), and `spotify-downloader` ( [#13641](https://github.com/jdx/mise/pull/13641), [@i-api](https://github.com/i-api)). `nub` 0.9.5 and later now installs from `github:nubjs/nub`, and the entry lists the `nubr` bin ( [#13643](https://github.com/jdx/mise/pull/13643), [@colinhacks](https://github.com/colinhacks)). `cocogitto` now lists `cog` as its bin ( [#13657](https://github.com/jdx/mise/pull/13657)).\
\
\
## Changed\
\
- **`mise exec` warns when a missing pinned tool falls back to `PATH`.** When auto-install is off (`exec_auto_install = false`, `auto_install = false`, or `auto_install_disable_tools`) and the command belongs to a pinned tool that isn't installed, mise used to run a same-named binary from `PATH` without saying anything. It still runs it, but now prints a warning such as `jq@1.7.1 is not installed and auto-install is disabled, so mise looks for jq on PATH instead`. There's no warning when another configured version of the tool, a command wrapper, or a project `env._.path` entry provides the command. [#13650](https://github.com/jdx/mise/pull/13650), [#13658](https://github.com/jdx/mise/pull/13658)\
- **`mise tasks validate` fails on unparseable usage specs.** A file task's `#USAGE` spec (or a TOML task's `usage`) that doesn't parse is now a `usage-parse-error` error, so validation exits 1, including with `--errors-only`. Before, it was only a warning and validation passed. `mise run` and `mise tasks ls` behave as before. **CI that runs `mise tasks validate` will now fail on these specs.** [#13672](https://github.com/jdx/mise/pull/13672)\
- **Linux GNU release binaries are linked non-PIE.** Every mise command on Linux x64, arm64, and armv7 (GNU) now starts about 3 ms faster. The tradeoff is that ASLR no longer applies to mise's own code and data, though the heap, stack, and shared libraries are still randomized. musl, macOS, source builds, and `cargo install` are unchanged. [#13687](https://github.com/jdx/mise/pull/13687)\
\
## Fixed\
\
### Dotfiles\
\
- **`*` no longer crosses `/` in tracked `include` patterns.** Capture and rollback used to disagree about what `rules/*.md` selected. After you widened the list, `mise dot rollback` to an older checkpoint could delete a nested file such as `rules/deep/two.md`. `include` now follows `.gitignore` rules: `*` stops at `/`, and you need `**` to match nested files. `exclude` lists keep matching what they matched before, but mise now prints a deprecation warning when an exclusion depends on `*` crossing `/`. Use `**` in those patterns instead. [#13618](https://github.com/jdx/mise/pull/13618)\
- **A leading `/` anchors `include`/`exclude` patterns to the entry root.** Before, these patterns matched nothing at all. Now `exclude = ["/cache"]` skips only the top-level `cache` directory, and `include = ["/rules/*.md"]` works. In the global `[history] exclude` list, a leading `/` still means an absolute path. [#13621](https://github.com/jdx/mise/pull/13621)\
\
### Tasks and config\
\
- Tasks in a `conf.d` folder fragment now run in that folder, with `{{config_root}}` and `MISE_CONFIG_ROOT` pointing there. Each folder's `[task_config]` applies only to its own tasks, so a fragment's `includes` no longer hides the default task directories like `~/.config/mise/tasks`. [#13662](https://github.com/jdx/mise/pull/13662)\
- A settings load that was already running could cache a stale snapshot after another thread changed settings, which dropped a just-applied override. This is fixed. [#13646](https://github.com/jdx/mise/pull/13646)\
\
### Plugins and shims\
\
- mise now warns when an installed git plugin's origin URL or checked-out commit doesn't match its `[plugins]` entry. The warning appears in `mise install`, `mise plugins install`, and `mise doctor`. Related fixes [#13663](https://github.com/jdx/mise/pull/13663):\
\
  - `mise plugins install --force <name>` now reinstalls from the `[plugins]` pin.\
  - A failed ref checkout no longer leaves an unpinned clone behind.\
  - Short SHAs fail with a clear error, since a full SHA is required.\
  - Shorthand pins like `owner/repo#v1.2.0` keep their ref.\
- On Windows, `[wrappers.*]` command wrappers (including the `cargo` wrapper that `mr_boxington` generates) now run through `exe`\- and `file`-mode shims and `mise x`. Before, the real tool ran instead. [#13673](https://github.com/jdx/mise/pull/13673)\
\
### Bootstrap\
\
- On apt systems, mise now simulates the install first and runs `apt-get update` once if the simulation fails. This fixes `has no installation candidate` failures on machines whose package lists cover only the install media. [#13659](https://github.com/jdx/mise/pull/13659)\
- On macOS, `mise bootstrap macos defaults` now reads and writes the container plist for sandboxed apps such as Safari, which the app actually uses. Launch the app once first so its container exists. Writing another app's container may require Full Disk Access for your terminal. [#13660](https://github.com/jdx/mise/pull/13660)\
- When `mise bootstrap packages prune` fails on a `brew:` formula it can't resolve, the error now names the config file that declares it. When the name is actually a cask, mise suggests `brew-cask:<name>`. [#13661](https://github.com/jdx/mise/pull/13661)\
\
## Performance\
\
- **Faster shell prompts.** When nothing has changed, `mise hook-env` no longer loads all settings or starts the async runtime (6.6 ms to 4.9 ms on Linux in the PR's measurements), as long as `hook_env.chpwd_only` and `hook_env.cache_ttl` are unset. [#13686](https://github.com/jdx/mise/pull/13686)\
- **Faster `cd` with npm tools installed.** The npm install health check now reads the virtual store's directory listing instead of calling `stat` on every package. [#13685](https://github.com/jdx/mise/pull/13685)\
- **Faster settings loading.** Config discovery skips `conf.d` globs for directories that don't exist, which halves settings load time in deep checkouts. [#13688](https://github.com/jdx/mise/pull/13688)\
- **Faster `brew-cask:` lookups.** Official casks are resolved from Homebrew's bulk `cask.json` index, cached locally and re-checked with a conditional request after 7.5 minutes, instead of one request per cask. In the PR's test, `bootstrap packages status` with 143 casks dropped from about 26s to about 2s. [#13349](https://github.com/jdx/mise/pull/13349) ( [@waynehoover](https://github.com/waynehoover))\
- **Fixed slowdowns from deferred prunes.** When a deferred-prune receipt from `mise upgrade` comes due but the version is still in use, mise now re-checks it once a day instead of on every command. This could make trivial commands about 9x slower. Pruning can now happen up to a day after the last reference is removed. [#13674](https://github.com/jdx/mise/pull/13674)\
- `mise ls`, `mise prune`, and shim rebuilds scan install directories in a single pass. [#13675](https://github.com/jdx/mise/pull/13675)\
\
**Full Changelog**: [https://g](https://g/)...\
\
[Read more](https://github.com/jdx/mise/releases/tag/v2026.9.15)\
\
### Contributors\
\
- [![@waynehoover](https://avatars.githubusercontent.com/u/115143?s=64&v=4)](https://github.com/waynehoover)\
- [![@colinhacks](https://avatars.githubusercontent.com/u/3084745?s=64&v=4)](https://github.com/colinhacks)\
- [![@jylenhof](https://avatars.githubusercontent.com/u/36410287?s=64&v=4)](https://github.com/jylenhof)\
- [![@jakedgy](https://avatars.githubusercontent.com/u/38260964?s=64&v=4)](https://github.com/jakedgy)\
- [![@i-api](https://avatars.githubusercontent.com/u/87678394?s=64&v=4)](https://github.com/i-api)\
\
waynehoover, colinhacks, and 3 other contributors\
\
\
Assets55\
\
Loading\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
👍7kitswas, petr-korobeinikov, aleksa-radojicic, pentiminax, iliqiliev, little-sparkleeesss, and german-quantica reacted with thumbs up emoji🎉1methbkts reacted with hooray emoji\
\
All reactions\
\
- 👍7 reactions\
- 🎉1 reaction\
\
8 people reacted\
\
## v2026.9.14: conf.d folder fragments, Stow-style dotfiles options, and mise-versions for any public GitHub repo\
\
[v2026.9.14: conf.d folder fragments, Stow-style dotfiles options, and mise-versions for any public GitHub repo](https://github.com/jdx/mise/releases/tag/v2026.9.14)\
\
Compare\
\
# Choose a tag to compare\
\
## Sorry, something went wrong.\
\
Filter\
\
Loading\
\
## Sorry, something went wrong.\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
## No results found\
\
[View all tags](https://github.com/jdx/mise/tags)\
\
![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)\
\
released this\
\
last week\
25 Sep 19:20\
\
\
Immutable\
release. Only release title and notes can be modified.\
\
[v2026.9.14](https://github.com/jdx/mise/tree/v2026.9.14)\
\
This tag was signed with the committer’s **verified signature**.\
\
\
[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)\
\
GPG key ID: 8B81C9D17413A06D\
\
Verified\
on Sep 25, 2026, 01:40 PM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
[`ff1bdb8`](https://github.com/jdx/mise/commit/ff1bdb8d29dc969e51877f1817b7c546636e3310)\
\
This commit was created on GitHub.com and signed with GitHub’s **verified signature**.\
\
\
GPG key ID: B5690EEEBB952194\
\
Verified\
on Sep 25, 2026, 01:34 PM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
A folder inside any `conf.d` directory now loads as its own config fragment and serves as the config root for the files in it, which gives `[bootstrap].config_roots` users a direct migration path. `[dotfiles]` gains two GNU Stow-style options: relative symlinks and `dot-<name>` sources. Release metadata for any public github.com repo now comes from mise-versions, and the registry can require GitHub attestations for specific tools.\
\
## Added\
\
- **conf.d folder fragments.** A folder in a global, system, or project `conf.d` directory now loads as a fragment. Relative paths, `{{ config_root }}`, and task working directories resolve inside that folder, so a bundle can keep its files next to its config. Each folder can hold `mise.toml`, `mise.local.toml`, `mise.<env>.toml`, and `mise.<env>.local.toml`. Folders are not searched recursively, and folders whose names start with `.` are skipped. A folder can be a symlink. Folder fragments load after the single-file fragments in the same `conf.d` (in folder-name order) and before `config.toml`. `mise use`/`mise set` never write to them. [#13603](https://github.com/jdx/mise/pull/13603)\
\
\
\
```\
~/.config/mise/conf.d/\
├── git.toml          # single-file fragment, unchanged\
└── git-tools/        # folder fragment\
      ├── mise.toml\
      └── gitconfig\
```\
\
\
\
\
\
\
\
\
\
```\
# ~/.config/mise/conf.d/git-tools/mise.toml\
[dotfiles]\
"~/.gitconfig" = "gitconfig"   # resolves to conf.d/git-tools/gitconfig\
```\
\
\
\
\
\
\
\
**Compatibility:** if a directory inside a `conf.d` that mise reads already contains a `mise.toml`, that file now loads.\
\
- **Relative dotfile symlinks.**`symlink` and `symlink-each` entries can now point at their source by a relative path, so links keep working when a home directory is mounted at a different path or moved. Turn this on for all entries with `dotfiles.relative_symlinks = true` (or `MISE_DOTFILES_RELATIVE_SYMLINKS=1`), or per entry with `relative = true/false`. When you turn it on, existing absolute links are re-pointed on the next apply. Turning it off does not convert relative links back to absolute ones. This option has no effect on Windows. [#13583](https://github.com/jdx/mise/pull/13583)\
\
\
\
```\
[settings]\
dotfiles.relative_symlinks = true\
\
[dotfiles]\
"~/.config/foo" = { source = "~/dotfiles/foo", mode = "symlink" }   # -> ../dotfiles/foo\
"~/.bashrc"     = { source = "~/dotfiles/bashrc", relative = false } # stays absolute\
```\
\
- **`dot_prefix` for dotfiles.** With `dot_prefix = true` on a `symlink-each` or directory `copy` entry, any path component named `dot-<name>` deploys as `.<name>` (for example, `home/dot-config/foo` deploys as `~/.config/foo`). `exclude` and `manifest = "git"` still match source names. If two sources map to the same target, apply fails. `mise dot add` refuses to capture into `dot_prefix` entries, and `mise oci` builds use the same mapping. [#13585](https://github.com/jdx/mise/pull/13585)\
\
\
\
```\
[dotfiles]\
"~" = { source = "home", mode = "symlink-each", dot_prefix = true, exclude = ["README.md"] }\
```\
\
- **mise-versions for any public github.com repo.** For `github:`, `aqua:`, and `packslip:` tools that aren't in the registry, version listing, release lookup, and attestation lookup now go through mise-versions, so they no longer use your GitHub API rate limit in the common case. Private repos still use your own token against api.github.com. [#13584](https://github.com/jdx/mise/pull/13584)\
  - mise treats the mirror as untrusted. Download URLs must match the configured repo, release tag, and asset name, and mirrored attestations must name the requested repo.\
  - In `paranoid` mode, mise checks a "no attestations" answer from the mirror against GitHub before skipping verification.\
  - If `url_replacements` reroutes GitHub API paths, mise skips mise-versions for that metadata.\
  - If mise-versions fails for any reason other than a 404, mise falls back to api.github.com and logs a warning.\
- **Registry-required GitHub attestations.** Registry `github:` backends can declare `attestations_since = "<semver>"`. For versions at or after that boundary:\
\
\
  - `mise lock` records `github-attestations` provenance.\
  - Installs require a verified attestation for every downloaded asset. This requirement overrides weaker provenance recorded in a lockfile.\
  - A missing attestation is a hard error.\
\
42 registry tools now set this boundary, including `aube`, `aqua`, `pixi`, `ty`, `pandoc`, `fnox`, `doppler`, and `syncthing`. Users who have turned off `github_attestations` are not affected. [#13586](https://github.com/jdx/mise/pull/13586)\
\
## Fixed\
\
- **Install lock waits:** when one process is waiting for another to finish installing the same tool version, the message now names the process holding the lock (`waiting for install lock held by pid 61907`). This is usually a shim auto-installing the tool. [#13588](https://github.com/jdx/mise/pull/13588)\
- **Slow downloads:** mise now warns once per download if throughput stays below 16 KiB/s for a full minute, naming the host and suggesting a mirror. The download is not aborted; `http_download_timeout` is still the hard limit. [#13589](https://github.com/jdx/mise/pull/13589)\
- **Interrupted installs:** a half-installed version no longer appears in version listings, can't be picked as the latest installed version, and doesn't keep `latest`/`1`/`1.2` runtime symlinks pointing into it. [#13596](https://github.com/jdx/mise/pull/13596)\
- **`mise prune`:** no longer deletes versions pinned by another project when you run it from a directory whose `.miserc.toml` lists that project in `ignored_config_paths`. The same fix applies to `mise ls --prunable` and the stale-version check in `mise upgrade`. These commands now honor `ignored_config_paths` only from `MISE_IGNORED_CONFIG_PATHS` and global or system `miserc.toml`. [#13602](https://github.com/jdx/mise/pull/13602)\
- **`mise oci build`:** directory `[dotfiles]` entries (`symlink-each` and directory `copy`) now honor `exclude` and `manifest = "git"`, so the image contains the same files `mise dot apply` deploys. [#13591](https://github.com/jdx/mise/pull/13591)\
- **pipx/pypi:**`latest` no longer resolves to PEP 440 developmental releases such as `2026.9.16.232951.dev0`, matching what pip and uv do. Local labels like `1.1+gpu.dev0` are still treated as stable. [#13601](https://github.com/jdx/mise/pull/13601)\
- **pipx/pypi:**`mise use 'pypi:git+ssh://git@github.com/psf/black.git'` now works. Previously, the `@` in `git@` was read as the version separator. [#13610](https://github.com/jdx/mise/pull/13610)\
- **`MISE_USE_VERSIONS_HOST=0`:** now fetches the version list from the source instead of reusing a cached, possibly older list from the versions host. [#13605](https://github.com/jdx/mise/pull/13605)\
- **brew source builds:** checksum-pinned formula source downloads now follow HTTPS-to-HTTP mirror redirects (such as those from `ftpmirror.gnu.org`) and still reject tarballs whose checksum doesn't match. This affects Unix only. Every other download still refuses HTTPS-to-HTTP redirects. [#13611](https://github.com/jdx/mise/pull/13611)\
- **npm backend on Windows:** updating the bundled aube to v2.4.0 fixes lifecycle scripts failing with `EISDIR: illegal operation on a directory, lstat 'C:'` during `npm:` installs. [#13608](https://github.com/jdx/mise/pull/13608)\
\
## Changed\
\
- The `[bootstrap].config_roots` deprecation warning now explains how to move each root into a `conf.d` folder, either by moving it or by symlinking it. The removal date (mise 2027.3.3) is unchanged. [#13598](https://github.com/jdx/mise/pull/13598)\
- Registry: `spin-framework` now installs through aqua by default. The previous backend is still available. [#13594](https://github.com/jdx/mise/pull/13594) by [@scop](https://github.com/scop)\
\
**Full Changelog**: [`vfox-v2026.9.15...v2026.9.14`](https://github.com/jdx/mise/compare/vfox-v2026.9.15...v2026.9.14)\
\
## 💚 Sponsor mise\
\
mise is built and maintained by [@jdx](https://github.com/jdx), an open source developer at [**entire.io**](https://entire.io/), the title sponsor of his open source work.\
\
If mise saves you or your team time, please consider becoming an [individual or company sponsor](https://jdx.dev/sponsors.html). Your support funds ongoing development and helps keep mise fast, free, and independent.\
\
### Contributors\
\
- [![@scop](https://avatars.githubusercontent.com/u/109152?s=64&v=4)](https://github.com/scop)\
\
scop\
\
\
Assets55\
\
Loading\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
👍7petr-korobeinikov, iliqiliev, aleksa-radojicic, gdrc-swastik, little-sparkleeesss, nn0nne, and german-quantica reacted with thumbs up emoji🎉1methbkts reacted with hooray emoji\
\
All reactions\
\
- 👍7 reactions\
- 🎉1 reaction\
\
8 people reacted\
\
## v2026.9.13: OpenTelemetry for tasks, shared daemon providers, \`mise backends switch\`, and declarative dotfile removal\
\
[v2026.9.13: OpenTelemetry for tasks, shared daemon providers, \`mise backends switch\`, and declarative dotfile removal](https://github.com/jdx/mise/releases/tag/v2026.9.13)\
\
Compare\
\
# Choose a tag to compare\
\
## Sorry, something went wrong.\
\
Filter\
\
Loading\
\
## Sorry, something went wrong.\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
## No results found\
\
[View all tags](https://github.com/jdx/mise/tags)\
\
![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)\
\
released this\
\
last week\
24 Sep 19:10\
\
\
Immutable\
release. Only release title and notes can be modified.\
\
[v2026.9.13](https://github.com/jdx/mise/tree/v2026.9.13)\
\
This tag was signed with the committer’s **verified signature**.\
\
\
[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)\
\
GPG key ID: 8B81C9D17413A06D\
\
Verified\
on Sep 24, 2026, 01:24 PM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
[`b09482b`](https://github.com/jdx/mise/commit/b09482b03fb3914da5ad80d3c833fec63a42ab91)\
\
This commit was created on GitHub.com and signed with GitHub’s **verified signature**.\
\
\
GPG key ID: B5690EEEBB952194\
\
Verified\
on Sep 24, 2026, 01:18 PM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
`mise run` can now export OpenTelemetry traces and logs (experimental), and experimental daemon providers let several projects and worktrees share one PostgreSQL, CockroachDB, or NATS server, each with its own database or account. Lockfiles no longer switch backends on their own when the registry moves a tool: the new `mise backends switch` command does it when you ask. `[dotfiles]` and `[bootstrap.files]` can now remove files and manage permissions, and `mise bootstrap unapply` removes what a module set up. The experimental `pkgx:` backend has been removed.\
\
## Highlights\
\
- **Observability and shared services (experimental):** task runs export OTLP traces and, if you opt in, task output as logs. Global `[daemon_providers]` run long-lived servers, and projects attach to them with an isolated database or NATS account per checkout.\
- **Safer lockfiles:** locked tools stay on their locked backend, `mise lock --bump` checks remote versions and fails when it can't, lockfiles no longer record versions that were never confirmed, and tool stubs lock into the project's `mise.lock`.\
- **Declarative cleanup:**`mode = "absent"`, `remove_empty` templates, permissions-only entries, removal of empty directories mise created, and `mise bootstrap unapply` let a config describe what should _not_ be on a machine.\
\
## Added\
\
### Tasks\
\
- **OpenTelemetry export for `mise run` (experimental).** Each run becomes one trace, with a span per task (grouped by monorepo package) that carries its exit code and redacted args. W3C `TRACEPARENT` is read from the environment and passed to each task, so nested `mise run` calls and instrumented tools appear in the same trace. Nothing is exported unless `otel.enabled = true` **and** an OTLP endpoint is set. Offline mode disables export, and each export times out after 3s by default. A separate `otel.logs = true` setting exports task stdout (INFO) and stderr (WARN) as log records linked to their spans, with redactions applied first. With `otel.logs` on, tasks in `interleave`/`quiet` modes no longer get a TTY; use `--raw` for tasks that need one. [#13557](https://github.com/jdx/mise/pull/13557), [#13558](https://github.com/jdx/mise/pull/13558), [#13559](https://github.com/jdx/mise/pull/13559) (built on work by [@MatthiasGrandl](https://github.com/MatthiasGrandl) and [@zeitlinger](https://github.com/zeitlinger))\
\
\
\
```\
[settings]\
otel.enabled = true\
otel.logs = true   # optional; exports task output too\
```\
\
\
\
\
\
\
\
\
\
```\
export OTEL_EXPORTER_OTLP_ENDPOINT=<your OTLP/HTTP collector URL>\
mise run build ::: test\
```\
\
\
### Daemons (experimental)\
\
- **Shared server providers.** Declare long-lived PostgreSQL, CockroachDB, or NATS servers in global config under `[daemon_providers]` and manage them with `mise daemons providers ls|start|stop|restart`. Providers have their own tools, ports, and persistent data. They run in an isolated environment and are not tied to any checkout. [#13534](https://github.com/jdx/mise/pull/13534)\
\
- **Per-checkout databases and accounts on a shared server.** A project daemon with `provider = "..."` gets its own database (PostgreSQL/CockroachDB) or its own NATS account with separate subjects and JetStream data. Each checkout path gets a stable name, so worktrees share the server but not the data. Give several daemons the same `resource` name to share data on purpose. Connection env vars point at the right database, and NATS gets an authenticated `NATS_URL`. [#13536](https://github.com/jdx/mise/pull/13536), [#13537](https://github.com/jdx/mise/pull/13537)\
\
\
\
```\
# ~/.config/mise/config.toml\
[daemon_providers.local-postgres]\
preset = "postgres"\
version = "18"\
port = "auto"\
\
# project mise.toml\
[daemons.db]\
provider = "local-postgres"\
# resource = "shared_app"   # opt in to sharing data\
```\
\
\
### Lockfiles and backends\
\
- **`mise backends switch`.** When the registry moves a tool to a new backend (as happened with hk and communique moving to `packslip:`), a tool locked to the old backend now stays there. `mise install` and `mise lock` print a warning that points to the new command, which moves lock entries to the registry's backend at the same versions, relocks their platforms, and reinstalls. It supports `--dry-run`, `--global`, and `TOOL@VERSION`. If any relock fails, every lockfile it changed is restored. [#13543](https://github.com/jdx/mise/pull/13543)\
\
- **Tool stubs lock into the project's `mise.lock`.**`mise generate tool-stub --lock` now records the stub in the nearest project lockfile (listed under `tool-stubs`), so installs verify the recorded checksums and `--locked`/`MISE_LOCKED=1` accept stubs. Previously the `[lock]` section written into the stub was never used, so checksums were never checked. [#13502](https://github.com/jdx/mise/pull/13502)\
\
- **Install from a local archive.** The `http:` backend accepts `file://` URLs. It copies the archive instead of downloading it, still verifies `checksum`, and works offline. [#13574](https://github.com/jdx/mise/pull/13574)\
\
\
\
```\
[tools]\
"http:my-tool" = { version = "1.0.0", url = "file:///opt/archives/my-tool-v1.0.0-linux-x64.tar.gz", checksum = "sha256:..." }\
```\
\
- **Checksum mismatch hints for re-uploaded GitHub assets.** When a `github:` or `aqua:` install fails a checksum check, mise asks GitHub for the asset's current digest. If that digest matches the download, the error says the maintainer probably re-uploaded the asset. The install still fails. [#13512](https://github.com/jdx/mise/pull/13512)\
\
- **vfox `BackendUninstall` hook.** Backend plugins can define `hooks/backend_uninstall.lua` to clean up outside the install directory. It runs before removal on uninstall, upgrade, and prune. If the hook errors, the install directory is kept. [#13522](https://github.com/jdx/mise/pull/13522)\
\
\
### CLI\
\
- **`mise search` checks package registries.** Add a prefix to search npm, crates.io, RubyGems, or NuGet (`mise search npm:typescript-language`, `cargo:`, `gem:`, `dotnet:`). `--all` searches every source at once. Plain searches and shell completion still make no registry requests, and `MISE_OFFLINE=1` skips them. [#13550](https://github.com/jdx/mise/pull/13550)\
- **`mise ls` by backend.**`-b/--backend` (repeatable) filters by backend and also works with `--json`. `--grouped` prints one section per backend. [#13530](https://github.com/jdx/mise/pull/13530)\
- **Key completion for `mise config get`/`set`.** Tab completes dotted keys, with descriptions, from the schema and from the target file. `--file`, `--global`, and `--system` are respected. [#13551](https://github.com/jdx/mise/pull/13551)\
- **Coloured help and a logo.**`mise --help` is now coloured on terminals (and respects `NO_COLOR`), wraps at the terminal's real width, and shows the mise logo on `mise`/`mise --help` when there's room. [#13449](https://github.com/jdx/mise/pull/13449)\
- **Project URLs in the registry.** Registry entries can set a `url`, which appears in `mise tool` (and `mise tool <name> --url`) and in `mise registry --json`. [#13533](https://github.com/jdx/mise/pull/13533)\
\
### Configuration and hooks\
\
- **`.miserc.local.toml`.** Sets per-checkout early config, such as `env = ["native"]`, without editing the shared `.miserc.toml`. At each directory level it is read before `.miserc.toml`. CLI flags and `MISE_ENV` still take precedence. [#13440](https://github.com/jdx/mise/pull/13440)\
- **`backend` and `install_path` in `MISE_INSTALLED_TOOLS`.** Postinstall hooks can now see where each tool came from and exactly where it was installed. [#13421](https://github.com/jdx/mise/pull/13421) ( [@garysassano](https://github.com/garysassano))\
- **A configured pnpm overrides Node's bundled pnpm**, whichever order the tools are listed in. [#13498](https://github.com/jdx/mise/pull/13498) ( [@EMcCormack](https://github.com/EMcCormack))\
\
### Dotfiles\
\
- **Choose what a tracked directory saves.**`exclude` and `include` lists on `mode = "track"` entries. Exclusions always win. `include = []` selects nothing. Narrowing a list does not delete the files on other machines. [#13418](https://github.com/jdx/mise/pull/13418), [#13432](https://github.com/jdx/mise/pull/13432)\
\
\
\
```\
[dotfiles]\
"~/.codex" = { mode = "track", include = ["config.toml", "rules/**"], exclude = ["*.log"] }\
```\
\
- **Preview before tracking.**`mise dot track --dry-run` and `mise dot paths --preview` show file counts, sizes, exclusions, and skipped nested repositories. Large trees get a warning. [#13417](https://github.com/jdx/mise/pull/13417)\
\
- **`mode = "absent"`** removes a file or symlink at the target, with support for OS `variants`. Directories and special files are refused, even with `--force`. [#13513](https://github.com/jdx/mise/pull/13513)\
\
- **`permissions` key.** Overrides the mode of copy, template, and content entries, or manages only the permissions of an existing file such as `~/.ssh/config`. Status, diff, and apply report and fix drift. The key is ignored on Windows. [#13514](https://github.com/jdx/mise/pull/13514)\
\
- **`remove_empty = true` on templates** removes the target when the template renders empty. A file you have edited since mise last wrote it is kept unless you pass `--force`. [#13515](https://github.com/jdx/mise/pull/13515)\
\
- **Empty parent directories mise created** are removed along with their target on apply and unapply. This only applies inside `$HOME` and never to directories that already existed. [#13518](https://github.com/jdx/mise/pull/13518)\
\
- **Warnings from background captures**, such as credential-named files saved in plaintext, are now shown by the next `mise dot` command or `mise bootstrap`. Previously they only went to the watcher logs. [#13483](https://github.com/jdx/mise/pull/13483)\
\
\
### Bootstrap\
\
- **`mise bootstrap unapply <ENV>...`** removes the files, directories, user services, and dotfile entries a module added after you deselect it. Anything another environment still declares is kept. Supports `--dry-run` and `--force`. [#13441](https://github.com/jdx/mise/pull/13441)\
- **Permissions-only `[bootstrap.files]` entries.** Declare only `mode`/`owner`/\`g...\
\
[Read more](https://github.com/jdx/mise/releases/tag/v2026.9.13)\
\
### Contributors\
\
- [![@elijahr](https://avatars.githubusercontent.com/u/153711?s=64&v=4)](https://github.com/elijahr)\
- [![@takumin](https://avatars.githubusercontent.com/u/391704?s=64&v=4)](https://github.com/takumin)\
- [![@zeitlinger](https://avatars.githubusercontent.com/u/2832627?s=64&v=4)](https://github.com/zeitlinger)\
- [![@garysassano](https://avatars.githubusercontent.com/u/10464497?s=64&v=4)](https://github.com/garysassano)\
- [![@EMcCormack](https://avatars.githubusercontent.com/u/20711436?s=64&v=4)](https://github.com/EMcCormack)\
- [![@nettlesh](https://avatars.githubusercontent.com/u/36604577?s=64&v=4)](https://github.com/nettlesh)\
- [![@MatthiasGrandl](https://avatars.githubusercontent.com/u/50196894?s=64&v=4)](https://github.com/MatthiasGrandl)\
- [![@casparbreloh](https://avatars.githubusercontent.com/u/175426018?s=64&v=4)](https://github.com/casparbreloh)\
\
elijahr, takumin, and 6 other contributors\
\
\
Assets55\
\
Loading\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
👍10petr-korobeinikov, aleksa-radojicic, german-quantica, bwhaley, xiaozhi1218, iliqiliev, philippe-granet, mkolibaba, 777lotto, and little-sparkleeesss reacted with thumbs up emoji🎉1methbkts reacted with hooray emoji\
\
All reactions\
\
- 👍10 reactions\
- 🎉1 reaction\
\
11 people reacted\
\
## v2026.9.12: Tasks that require daemons, worktree-aware ports and URLs, Scoop and zypper packages, and official Docker images\
\
[v2026.9.12: Tasks that require daemons, worktree-aware ports and URLs, Scoop and zypper packages, and official Docker images](https://github.com/jdx/mise/releases/tag/v2026.9.12)\
\
Compare\
\
# Choose a tag to compare\
\
## Sorry, something went wrong.\
\
Filter\
\
Loading\
\
## Sorry, something went wrong.\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
## No results found\
\
[View all tags](https://github.com/jdx/mise/tags)\
\
![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)\
\
released this\
\
2 weeks ago\
20 Sep 11:47\
\
\
Immutable\
release. Only release title and notes can be modified.\
\
[v2026.9.12](https://github.com/jdx/mise/tree/v2026.9.12)\
\
This tag was signed with the committer’s **verified signature**.\
\
\
[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)\
\
GPG key ID: 8B81C9D17413A06D\
\
Verified\
on Sep 20, 2026, 05:59 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
[`1698dd8`](https://github.com/jdx/mise/commit/1698dd8ff8308b6e39fee8ce1537ddf93f246a2a)\
\
This commit was created on GitHub.com and signed with GitHub’s **verified signature**.\
\
\
GPG key ID: B5690EEEBB952194\
\
Verified\
on Sep 20, 2026, 05:50 AM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
The experimental `[daemons]` system grows substantially: tasks can declare the daemons they need, `[daemon_groups]` selects subsets of a project's services, `port = "auto"` and stable `<NAME>_URL` hostnames let several git worktrees run the same stack side by side, and CockroachDB, NATS, and SpiceDB join the PostgreSQL and Redis presets. Outside daemons, `[bootstrap.packages]` gains `scoop:` and `zypper:` managers, `mise install --system` elevates with sudo only for the final publish step, official mise images are published to GHCR and Docker Hub, and a run of `brew-cask` fixes lets many more casks install unattended.\
\
## Highlights\
\
- **Daemons as part of the task graph (experimental):**`[tasks.x] daemons = [...]` starts and waits for services before a task runs, daemons can run a mise task, `[daemon_groups]` and `mise daemons start <group>` select subsets, and `mise daemons register`, `urls`, and `prune` round out the lifecycle. Worktrees get deterministic ports, hostnames, and optionally checkout-local `data_dir` storage without hand-assigned numbers.\
- **More host software from one config:** Scoop on Windows and zypper on openSUSE/SLE join the bootstrap managers; `brew-cask` now picks the right build for the host macOS release, runs installers that need sudo, applies pkg installer choices and `set_ownership` steps, upgrades pkg-only self-updating casks, and survives DMGs with license prompts or unreadable metadata; `brew:` resolves formula aliases such as `openssl`.\
- **Regressions and integrity fixes:**`enter` hooks fire again when a shell starts inside a project, npm tools with only pre-releases resolve `latest` again, generated `pre-push` hooks no longer append git's arguments to the task command, and `mise install` refuses a lockfile entry whose download URL names a different release than its `version`. Lockfile sidecars now verify on Windows checkouts with CRLF line endings.\
\
## Added\
\
- **daemons:** Tasks can require daemons and daemons can run tasks. `daemons = ["postgres", "nats"]` (or `true` for all project daemons) on a task starts them via pitchfork, waits for readiness, and then runs the task; already-running daemons are left alone, and `--skip-deps` / `--dry-run` skip them. A daemon can declare `task = "dev:core"` with `args` instead of `run`, and `init = [...]` runs idempotent setup commands before the long-running process on every start. `mise tasks info` shows a task's daemons. ( [#13340](https://github.com/jdx/mise/pull/13340))\
\
\
\
```\
[daemons]\
postgres = "18"\
\
[daemons.nats]\
run = "exec nats-server"\
ready_port = 4222\
\
[tasks.dev]\
daemons = ["postgres", "nats"]\
run = "npm run dev"\
```\
\
- **daemons:**`[daemon_groups]` names project-scoped subsets of daemons; groups may nest other groups and work wherever a daemon name does, including `--group`. `mise daemons start` with no arguments now starts the `default` group when a project declares one, and every project daemon otherwise. ( [#13347](https://github.com/jdx/mise/pull/13347))\
\
\
\
```\
[daemon_groups]\
default = ["postgres", "core", "node0"]\
two-cluster = ["default", "core2"]\
```\
\
- **daemons:** A daemon entry with `project = "../other-checkout"` and optional `name` runs a daemon defined in another project under that project's tools, environment, and data, and can be used in `depends`. `[daemons_settings] namespace = "services"` gives daemons stable `namespace/name` IDs (with a per-worktree suffix unless `namespace_per_worktree = false`). Requires pitchfork 2.25.0 or later. ( [#13339](https://github.com/jdx/mise/pull/13339))\
\
- **daemons:**`port = "auto"` (or `port = { auto = true, base = 3000, stride = 1 }` for custom daemons) keeps the base port in the primary checkout and derives a deterministic offset in each linked git worktree. Resolved ports are exported before startup as `PGPORT`/`DATABASE_URL` for presets and `<NAME>_PORT` for custom daemons, and `mise daemons ls --json` reports `port` and `port_auto`. mise does not fall back to another port; startup diagnoses conflicts with running mise-managed daemons in other projects, and two daemons in one project claiming the same port now fail at config load. ( [#13342](https://github.com/jdx/mise/pull/13342))\
\
- **daemons:** Every daemon with a `port` gets a stable hostname served by pitchfork's reverse proxy, exported as `<NAME>_URL` (for example `api.shop.localhost` in the primary checkout, or `api.shop-pr-42.shop.localhost` in a linked worktree). Per-daemon `proxy` (a label, `true`, or `false`) and `proxy_tls` (`"terminate"` or `"passthrough"`) control routing; the `postgres` and `redis` presets opt out. `mise daemons urls` lists hostnames, ports, proxy modes, and status. Set `proxy = false` on custom daemons that do not speak HTTP. ( [#13368](https://github.com/jdx/mise/pull/13368))\
\
- **daemons:** CockroachDB (`preset = "cockroachdb"`), NATS (`"nats"`), and SpiceDB (`"spicedb"`) presets install the tool, initialize data, wait for readiness, and export `DATABASE_URL`, `NATS_URL`, `SPICEDB_ENDPOINT`, and `SPICEDB_PRESHARED_KEY`. Presets now support named-port overrides such as `ports.http_port = 8081` and typed `options`. Unix-only; NATS and SpiceDB readiness checks need `curl`. ( [#13346](https://github.com/jdx/mise/pull/13346))\
\
- **daemons:**`mise daemons register` installs missing tools, validates the daemon graph, and registers pitchfork configuration without starting anything, so a fresh checkout can start on its first hostname request. ( [#13399](https://github.com/jdx/mise/pull/13399))\
\
- **daemons:**`mise daemons prune` finds daemon state left behind by deleted projects and worktrees, shows paths and sizes, and removes it after confirmation (`--dry-run` previews, `--yes` confirms ordinary cases). `mise daemons ls --json` adds `root`, `state_dir`, `data_size`, and `data_size_human`. ( [#13338](https://github.com/jdx/mise/pull/13338))\
\
- **daemons:**`data_dir = ".data/postgres"` keeps a preset's persistent data inside the checkout (relative to the project root; absolute and `~/` paths also work), so each worktree gets its own database. Changing the path does not move existing data. ( [#13408](https://github.com/jdx/mise/pull/13408))\
\
- **bootstrap:** A `scoop` manager for Windows. `"scoop:extras/vscode" = "latest"` adds the bucket if missing, pinned versions install via `app@version`, `state = "absent"` uninstalls, and `--update` opts in to `scoop update`. Only user-scope installs are managed; entries are skipped on other platforms. ( [#13324](https://github.com/jdx/mise/pull/13324))\
\
- **bootstrap:** A `zypper` manager for openSUSE and SUSE Linux Enterprise, supporting status, install, `name=version` pins (including downgrades), upgrade, and removal, with retries when zypper asks for a package-manager restart. ( [#13335](https://github.com/jdx/mise/pull/13335), [@m407](https://github.com/m407))\
\
- **bootstrap:**`process_type` on `[bootstrap.macos.launchd.agents.*]` maps to launchd's `ProcessType` (`Background`, `Standard`, `Adaptive`, `Interactive`); misspellings are rejected at config time instead of being silently ignored by launchd. ( [#13402](https://github.com/jdx/mise/pull/13402), [@waynehoover](https://github.com/waynehoover))\
\
- **install:**`mise install --system` on Unix downloads, verifies, and unpacks as the invoking user, then uses sudo only to publish into the system install and shim directories. Supports relocatable tools from `aqua`, `github`, `gitlab`, `forgejo`, `http`, and `s3` without a tool-level `postinstall`; `system_packages.sudo = false` disables elevation. ( [#13384](https://github.com/jdx/mise/pull/13384))\
\
- **docker:** Official release images at `ghcr.io/jdx/mise` and `jdxcode/mise` for `linux/amd64` and `linux/arm64`, built from the minisign-verified release binaries. Tags `2026.9.12`, `2026.9`, and `latest` are a scratch image for `COPY --from=`; `*-debian` and `debian` are a Debian slim base with `curl` and `git`. ( [#13413](https://github.com/jdx/mise/pull/13413))\
\
\
\
```\
FROM debian:13-slim\
COPY --from=ghcr.io/jdx/mise:2026.9.12 /usr/local/bin/mise /usr/local/bin/mise\
```\
\
- **config:**`unix` is accepted as an `os` selector in `[tools]`, `[bootstrap.packages]`, `[doctor.checks]`, and `[dotfiles]` variants, matching every non-Windows platform. A concrete OS variant still wins over a `unix` one. ( [#13395](https://github.com/jdx/mise/pull/13395))\
\
- **go:** With `go` in `idiomatic_version_file_enable_tools`, the `toolchain` line of an active `go.work` selects the Go version and, as with the `go` command, member `go.mod` files are ignored in workspace mode. `GOWORK` (`auto`, `off`, or an absolute path) is honored. ( [#13337](https://github.com/jdx/mise/pull/13337))\
\
- **bazel:**`.bazelversion` is an idiomatic version file for `bazel` when enabled; only concrete releases are read, so `latest`, `last_green`, `8.x`, and commit hashes select nothing rather than failing. ( [#13336](https://github.com/jdx/mise/pull/13336))\
\
- **tasks:** File-task `#USAGE include file="..."` paths may be relative to the task file or use environment variables such as `$MISE_CONFIG_ROOT`, `$MISE_TASK_DIR`, and `$MISE_PROJECT_ROOT`, so shared flagsets no longer need absolute paths. ( [#13372](https://github.com/jdx/mise/pull/13372))\
\
- **dotfiles:**`mise dot apply` now runs matching `[history.reload]` commands for the targets it actually wrote, once each after all writes; `--dry-run` and no-op applies run none. ( [#13414](https://github.com/jdx/mise/pull/13414))\
\
- **registry:** Added `codegraph` (`aqua:colbymchenry/codegraph`). ( [#13355](https://github.com/jdx/mise/pull/13355), [@3w36zj6](https://github.com/3w36zj6))\
\
\
## Fixed\
\
- **activate:** Starting a shell inside a trusted project runs its `enter` hook again; a regression in 2026.9.x had limited it to `cd` into the project. ( [#13383](https://github.com/jdx/mise/pull/13383))\
- **npm:** A tool that publishes only pre-releases (such as `@deepseek-ai/dsh`) is no longer reported missing after `mise use npm:...@latest`; `latest` falls back to the newest installed pre-release when no stable versi...\
\
[Read more](https://github.com/jdx/mise/releases/tag/v2026.9.12)\
\
### Contributors\
\
- [![@waynehoover](https://avatars.githubusercontent.com/u/115143?s=64&v=4)](https://github.com/waynehoover)\
- [![@hisaac](https://avatars.githubusercontent.com/u/923876?s=64&v=4)](https://github.com/hisaac)\
- [![@m407](https://avatars.githubusercontent.com/u/4082825?s=64&v=4)](https://github.com/m407)\
- [![@soodoh](https://avatars.githubusercontent.com/u/18269267?s=64&v=4)](https://github.com/soodoh)\
- [![@nettlesh](https://avatars.githubusercontent.com/u/36604577?s=64&v=4)](https://github.com/nettlesh)\
- [![@3w36zj6](https://avatars.githubusercontent.com/u/52315048?s=64&v=4)](https://github.com/3w36zj6)\
- [![@carldaws](https://avatars.githubusercontent.com/u/83088654?s=64&v=4)](https://github.com/carldaws)\
- [![@casparbreloh](https://avatars.githubusercontent.com/u/175426018?s=64&v=4)](https://github.com/casparbreloh)\
\
waynehoover, hisaac, and 6 other contributors\
\
\
Assets55\
\
Loading\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
👍8aleksa-radojicic, little-sparkleeesss, kitswas, iliqiliev, AlexanderTheGrey, german-quantica, rufatZZ, and petr-korobeinikov reacted with thumbs up emoji\
\
All reactions\
\
- 👍8 reactions\
\
8 people reacted\
\
## v2026.9.11: macos-app bootstrap packages, task template inheritance for flags and file tasks, and Swift on Linux fixes\
\
[v2026.9.11: macos-app bootstrap packages, task template inheritance for flags and file tasks, and Swift on Linux fixes](https://github.com/jdx/mise/releases/tag/v2026.9.11)\
\
Compare\
\
# Choose a tag to compare\
\
## Sorry, something went wrong.\
\
Filter\
\
Loading\
\
## Sorry, something went wrong.\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
## No results found\
\
[View all tags](https://github.com/jdx/mise/tags)\
\
![@mise-en-dev](https://avatars.githubusercontent.com/u/123107610?s=40&v=4)[mise-en-dev](https://github.com/mise-en-dev)\
\
released this\
\
2 weeks ago\
18 Sep 01:05\
\
\
Immutable\
release. Only release title and notes can be modified.\
\
[v2026.9.11](https://github.com/jdx/mise/tree/v2026.9.11)\
\
This tag was signed with the committer’s **verified signature**.\
\
\
[![](https://avatars.githubusercontent.com/u/123107610?s=64&v=4)](https://github.com/mise-en-dev)[mise-en-dev](https://github.com/mise-en-dev)\
\
GPG key ID: 8B81C9D17413A06D\
\
Verified\
on Sep 17, 2026, 04:36 PM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
[`ef1f6b1`](https://github.com/jdx/mise/commit/ef1f6b1dc814d4e830aff70e0a88450f18998e34)\
\
This commit was created on GitHub.com and signed with GitHub’s **verified signature**.\
\
\
GPG key ID: B5690EEEBB952194\
\
Verified\
on Sep 17, 2026, 04:27 PM\
\
[Learn about vigilant mode](https://docs.github.com/github/authenticating-to-github/displaying-verification-statuses-for-all-of-your-commits).\
\
\
A new `macos-app` bootstrap manager installs `.app` bundles from a pinned URL and checksum when no Homebrew cask exists, task templates now compose `usage` flags and can be extended from file tasks, and Swift on Linux picks its distro build from swift.org's release index instead of a hard-coded map that 404'd on arm64 and on newer Fedora, Amazon Linux, and Arch hosts. Install failures also become far more actionable: errors name the `minimum_release_age` cutoff that hid every version, the child's last stderr line, or the shared libraries a Swift toolchain cannot load.\
\
## Highlights\
\
- **Apps without a cask:**`"macos-app:<name>"` entries in `[bootstrap.packages]` download, checksum-verify, and install a `.app` into `/Applications` using mise's existing cask pipeline, with stricter ownership rules for apps already at the target.\
- **Task templates that actually share things:** a task that `extends` a template now inherits the template's `usage` flags alongside its own, file tasks can write `#MISE extends="..."`, and a template's `vars` can read the values the extending task supplies.\
- **Swift on Linux:** arm64 downloads resolve on every distro, the build is chosen from what a release actually publishes (with a warning when a fallback is used), and a fallback that cannot start names the missing libraries instead of exiting 127 after a 1 GB download.\
\
## Added\
\
- **bootstrap:** The `macos-app` package manager installs a macOS `.app` bundle from a vendor or internal download. `version`, `url`, `sha256`, and `artifact` are all required (`"latest"` is rejected because mise cannot discover releases behind a plain URL); `{{version}}` is interpolated into `url`, so a release bump is a two-field edit. Only `.dmg` and `.zip` archives containing an app bundle are supported, state is kept in mise's state directory rather than Homebrew's Caskroom, and an app already at the destination that this entry does not own is refused unless `adopt = true` and the contents match. `mise bootstrap packages upgrade` cannot discover new versions for these entries, and `prune --manager macos-app` is unsupported. Prefer `brew-cask` wherever a cask exists. ( [#13279](https://github.com/jdx/mise/pull/13279))\
\
\
\
```\
[bootstrap.packages."macos-app:example"]\
version = "1.2.3"\
url = "<HTTPS URL of the .dmg or .zip; {{version}} is interpolated>"\
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"\
artifact = "Example.app"\
```\
\
- **tasks:** A task that names a template with `extends` and declares its own `usage` now gets the template's flags too, listed first in `--help`. Previously the task's spec replaced the template's entirely, so shared flags had to be copied into every task. Workspace-root task defaults still only fill in `usage` when the task has none. A flag declared in both places is listed twice; declare each flag in one place, or use usage flagsets for replacements. ( [#13310](https://github.com/jdx/mise/pull/13310))\
\
\
\
```\
[task_templates.deploy]\
usage = 'flag "--env <env>" help="Target environment"'\
\
[tasks.deploy-api]\
extends = "deploy"\
usage = 'flag "--replicas <n>" help="How many to run"'\
run = 'echo "env=$usage_env replicas=$usage_replicas"'\
```\
\
- **tasks:** File tasks (including remote HTTP and `git::` scripts) can use `#MISE extends="<template>"` in their header to inherit tools, env, description, aliases, and other fields from a task template; previously the field was warned about and ignored. A task whose command is a script file no longer picks up a template's `run`. ( [#13307](https://github.com/jdx/mise/pull/13307))\
\
- **swift:** When a Linux install fails its `swift --version` check, mise runs `ldd` over the toolchain and lists every unresolved shared library (for example `libform.so.6, libncurses.so.6, libpanel.so.6` on an Arch-family host running a ubi9 build), with the tool's `install_env` applied so an `LD_LIBRARY_PATH` remedy is not misreported. `docs/lang/swift.md` documents the workaround. ( [#13319](https://github.com/jdx/mise/pull/13319))\
\
\
## Fixed\
\
- **swift:** Installing Swift on arm64 failed with a 404 on every Linux distro except Ubuntu (and on Windows arm64 lock entries) because only Ubuntu used swift.org's `<platform>-<arch>` download directory. ( [#13293](https://github.com/jdx/mise/pull/13293), fixes [#13291](https://github.com/jdx/mise/issues/13291))\
- **swift:** The Linux distro build is now chosen from swift.org's release index rather than a hard-coded map: the host's exact distro version wins, then the newest published build older than the host, then the family's oldest, with `ID_LIKE` consulted (Linux Mint gets an Ubuntu build) and unknown distros such as Arch falling back to `ubi9`. Every compromise is announced with a warning, musl hosts and unsupported architectures fail before downloading, and `swift.platform` still overrides selection without contacting swift.org. The `swift_platform` lockfile option now records the host as detected (e.g. `fedora40` instead of `fedora39`); mismatched entries are re-resolved on the next lock. ( [#13297](https://github.com/jdx/mise/pull/13297), fixes [#13289](https://github.com/jdx/mise/issues/13289))\
- **config:**`install_env` values are now rendered as templates like other tool options, so `LD_LIBRARY_PATH = "{{env.HOME}}/.local/lib/compat"` reaches the install subprocess expanded rather than literally. `{{version}}` is left unchanged. ( [#13314](https://github.com/jdx/mise/pull/13314))\
- **install:** When `minimum_release_age` (default `24h`) hides every candidate, the error names the setting and cutoff, how many releases it hid, the newest one with its release and eligibility dates, and a copy-pasteable exact pin to install it now, instead of `no versions found ... matching date filter`. A query that matched nothing is no longer blamed on the filter. ( [#13308](https://github.com/jdx/mise/pull/13308))\
- **cmd:** A failing command run by mise (installs, tasks, plugin scripts) now appends the child's last non-empty stderr line to the error, e.g. `exit code 127; last stderr: swift: error while loading shared libraries: libncurses.so.6 ...`. This also reaches the final error block under `--quiet`, where stderr was previously never shown. ( [#13315](https://github.com/jdx/mise/pull/13315))\
- **tasks:** A broken `usage` spec now reports the task name and the parser's diagnostic (`invalid usage spec for task 'deploy'` followed by the reason) instead of a bare `Invalid usage config`; file tasks render the same diagnostic rather than a Debug dump, and a missing or unreadable script is reported as such rather than as a bad spec. ( [#13312](https://github.com/jdx/mise/pull/13312))\
- **tasks:** A task template's `vars` can now read the vars the extending task supplies, so `{{ vars.opt | default(value='none') }}` in a template sees the task's `opt` instead of always taking the default. Literal vars within a single task are also bound first, so `vars = { msg = "hi {{ vars.who }}", who = "world" }` works regardless of declaration order. Config-level `[vars]` are unchanged. ( [#13322](https://github.com/jdx/mise/pull/13322))\
- **runtime symlinks:**`latest` and version-prefix links under `installs/<tool>/` that point at an install no longer eligible for a link (for example a directory left with an `incomplete` marker by an interrupted install) are now removed on rebuild instead of surviving indefinitely. Configured aliases, hand-made names, and absolute symlinks are left alone. ( [#13288](https://github.com/jdx/mise/pull/13288))\
- **npm:** Semver pre-releases with numeric suffixes such as `1.3.1-3` no longer claim the `latest`, `1`, and `1.3` runtime symlinks or satisfy `"latest"`/prefix requests over the newest stable install; links an older mise already wrote are cleaned up on the next install. An exact request or the `prerelease` option still selects them. ( [#13272](https://github.com/jdx/mise/pull/13272) by [@pataar](https://github.com/pataar))\
- **lockfile:**`mise lock --global` on a `mise.lock` symlinked into a dotfiles repository now keeps native dependency sidecars beside the target lockfile, so `mise install --locked` works from a fresh checkout. If you used this layout on 2026.9.7 through 2026.9.10 and see missing sidecars, run `mise lock --global` again to repair the pointers. ( [#13268](https://github.com/jdx/mise/pull/13268) by [@nettlesh](https://github.com/nettlesh))\
- **lockfile:** With `lockfile_mode = "generate"`, `mise unuse` now removes the tool's entry from `mise.lock` and its `.mise/locks/...` sidecar immediately rather than leaving them until the next `mise install` or `mise lock`. Merge mode is unchanged. ( [#13304](https://github.com/jdx/mise/pull/13304))\
- **conda:** Commands from `conda:` packages that have nothing to activate (no `activate.d` scripts, no dependency executables, no script entry points) are now plain symlinks instead of shell launchers, so tools like `conda:ripgrep` or `conda:gh` no longer prepend the conda prefix to the `PATH` of every child process and skip the extra shell. Packages that need activation keep their launcher; Windows is unchanged. A relative `MISE_DATA_DIR` is also handled. Existing installs keep their current entries until reinstalled with `mise install --force conda:<pkg>`. ( [#13305](https://github.com/jdx/mise/pull/13305))\
- **backends:** A tool's `postinstall` hook now receives pre-tools `[env]` from the config on every backend (http, aqua, github, cargo, npm, core tools), not only for asdf plugins, and a hook that changes an env input is visible to hooks ordered after it. ( [#13316](https://github.com/jdx/mise/pull/13316))\
- **github:** Tools whose release tags repeat the configured `version_prefix` (tag `a-a-1.2.3` with `version_prefix = "a-"`, listed as `a-1.2.3`) can now be installed; `prefix + version` is tried first so every listed version round-trips to its tag. If a repo publishes both `a-1.2.3` and `a-a-1.2.3`, requesting `a-1.2.3` now resolves to the doubled tag. ( [#13317](https://github.com/jdx/mise/pull/13317))\
\
## Ch...\
\
[Read more](https://github.com/jdx/mise/releases/tag/v2026.9.11)\
\
### Contributors\
\
- [![@waynehoover](https://avatars.githubusercontent.com/u/115143?s=64&v=4)](https://github.com/waynehoover)\
- [![@pataar](https://avatars.githubusercontent.com/u/3403851?s=64&v=4)](https://github.com/pataar)\
- [![@nettlesh](https://avatars.githubusercontent.com/u/36604577?s=64&v=4)](https://github.com/nettlesh)\
\
waynehoover, pataar, and nettlesh\
\
\
Assets55\
\
Loading\
\
### Uh oh!\
\
There was an error while loading. [Please reload this page](https://github.com/jdx/mise/releases).\
\
👍6german-quantica, petr-korobeinikov, little-sparkleeesss, kitswas, iliqiliev, and aleksa-radojicic reacted with thumbs up emoji\
\
All reactions\
\
- 👍6 reactions\
\
6 people reacted\
\
Previous [1](https://github.com/jdx/mise/releases?page=1) [2](https://github.com/jdx/mise/releases?page=2) [3](https://github.com/jdx/mise/releases?page=3) [4](https://github.com/jdx/mise/releases?page=4) [5](https://github.com/jdx/mise/releases?page=5)… [64](https://github.com/jdx/mise/releases?page=64) [65](https://github.com/jdx/mise/releases?page=65) [Next](https://github.com/jdx/mise/releases?page=2)\
\
Previous [Next](https://github.com/jdx/mise/releases?page=2)\
\
You can’t perform that action at this time.