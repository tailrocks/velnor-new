# Fixed registry and forge publisher protocol

Primary-source audit, 2026-10-03. The credentialed helper consumes verified
anonymous package bytes. It executes no Cargo, Git, release-plz, repository
configuration, templates, build scripts, or artifact-provided programs.

## Immutable source anchors

- Rust 1.98.1 tag resolves to
  [`48a229ceaefd4985c50990b14116b6d856af0985`](https://github.com/rust-lang/rust/tree/48a229ceaefd4985c50990b14116b6d856af0985).
  Its `src/tools/cargo` submodule is Cargo
  [`797e8a9bca276c1c9f9f738d2a20f484fa4eea9d`](https://github.com/rust-lang/cargo/tree/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d),
  also the peeled `0.99.0` Cargo tag. Cargo package version and Rust toolchain
  version are different version namespaces.
- Crates.io server/API source:
  [`693b912ffc88759dbdc51ffb652a471dc54e28b9`](https://github.com/rust-lang/crates.io/tree/693b912ffc88759dbdc51ffb652a471dc54e28b9).
- Official trusted publishing action:
  [`9c067fc5d5c254b33978fed918cb50c032549d0e`](https://github.com/rust-lang/crates-io-auth-action/tree/9c067fc5d5c254b33978fed918cb50c032549d0e).
- Release-plz 0.3.169:
  [`786894b6ce1abad0d0e9bea0ace4958e099af8ca`](https://github.com/release-plz/release-plz/tree/786894b6ce1abad0d0e9bea0ace4958e099af8ca).

## Upload and metadata

Send `PUT https://crates.io/api/v1/crates/new` with `Accept: application/json`,
`Content-Type: application/octet-stream`, and `Authorization: <token>`.
Cargo uses the raw token, without a Bearer prefix. The server accepts either
form, but the fixed uploader follows Cargo's form.

The body is `u32LE(json_utf8_length) || json_utf8 || u32LE(crate_length) || crate`.
Both lengths count bytes. Preserve the compressed `.crate` bytes exactly; reject
lengths exceeding unsigned 32-bit range. No multipart body, rebuild, repack, or
replacement archive is allowed.
[Cargo framing](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/crates/crates-io/lib.rs#L277-L320).

The JSON schema is Cargo `NewCrate`: `name`, `vers`, `deps`, `features`, `authors`,
`description`, `documentation`, `homepage`, `readme`, `readme_file`, `keywords`,
`categories`, `license`, `license_file`, `repository`, `badges`, `links`,
`rust_version`. Nullable fields remain nullable; collections default empty.
`features` comes from the normalized published manifest, without adding implicit
optional-dependency features. `readme` is content; `readme_file` is the normalized
packaged relative path. Verify content against the anonymous package inventory.
[Schema](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/crates/crates-io/lib.rs#L56-L95),
[normalization](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/registry/publish.rs#L527-L645).

Each upload dependency has `name` (actual package name), `version_req` (Cargo's
normalized requirement), `features`, `optional`, `default_features`, `target`
(string or null), and `kind` (`normal`, `build`, `dev`). Renamed dependencies add
`explicit_name_in_toml` (manifest alias). `registry` is omitted for the same
registry; Cargo rejects alternate registry dependencies when publishing to
crates.io. Artifact dependencies additionally have `artifact`, `bindep_target`,
and sometimes `lib`; unsupported artifact dependencies must fail explicitly.
[Dependency validation](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/registry/publish.rs#L485-L568).

Anonymous `cargo metadata --no-deps` reads workspace members without resolving
the dependency graph; its `resolve` is null. This permits normalized archive
metadata checks without requiring unpublished dependencies in the index.
[Cargo implementation](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/cargo_output_metadata.rs#L27-L43).

Cargo normalizes `readme = false` to no README metadata; `true` means
`README.md`, and absent configuration searches `README.md`, `README.txt`, then
`README`. The packaged manifest may retain the false boolean. Upload fields
`readme` and `readme_file` are null in that case.
[README normalization](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/util/toml/mod.rs#L836-L859).

Dependency requirement formatting follows locked `semver 1.0.28`, upstream
source `7625c7aa3f0e8ba21e099d1765bcebcb72aa8816`. Cargo formats the parsed
`VersionReq`, not the manifest's raw string. Bare requirements receive caret,
comparators join with comma-space, build metadata is discarded, and wildcard
syntax is normalized. Valid range syntax must not be restricted to exact pins.
Targets likewise use Cargo platform `Display`; whitespace normalization must
follow its parser and formatter rather than raw TOML text equality.
[Semver parser/formatter](https://github.com/dtolnay/semver/tree/7625c7aa3f0e8ba21e099d1765bcebcb72aa8816/src),
[Cargo formatting](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/registry/publish.rs#L551-L557).

The upload response may contain `warnings.invalid_categories`,
`warnings.invalid_badges`, and `warnings.other`; successful Cargo handling also
accepts an empty response. A response is not final publication proof. Index
propagation can lag successful upload. Cargo specially classifies a crates.io
503 after at least 29 seconds as a possible upload timeout.
[Response handling](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/crates/crates-io/lib.rs#L320-L359),
[registry API](https://doc.rust-lang.org/cargo/reference/registry-web-api.html#publish).

## Authentication and first publication

Request a GitHub OIDC JWT with audience exactly `crates.io`. Exchange it using
`POST https://crates.io/api/v1/trusted_publishing/tokens`, JSON `{"jwt": "..."}`,
`Content-Type: application/json`, and no Authorization header. A successful 200
response contains `{"token": "..."}` and no expiry field. The server lifetime
is 30 minutes. The token covers all configured matching crates. JWT replay is
rejected; another exchange needs a fresh JWT.
[Audience](https://github.com/rust-lang/crates-io-auth-action/blob/9c067fc5d5c254b33978fed918cb50c032549d0e/src/registry_url.ts#L3-L24),
[exchange](https://github.com/rust-lang/crates-io-auth-action/blob/9c067fc5d5c254b33978fed918cb50c032549d0e/src/main.ts#L60-L98),
[lifetime/replay](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/trustpub/tokens/exchange/mod.rs#L24-L94),
[scope](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/trustpub/tokens/exchange/mod.rs#L222-L245).

Revoke a temporary token with `DELETE` to the exchange endpoint and
`Authorization: Bearer <temporary token>`. Revoke on success and failure; preserve
publication receipts if revocation itself fails.
[Official revocation](https://github.com/rust-lang/crates-io-auth-action/blob/9c067fc5d5c254b33978fed918cb50c032549d0e/src/post.ts#L29-L40).

Trusted publishing cannot create a new crate. Bootstrap needs an ordinary token
with `publish-new` authority; updating a crate requires `publish-update`.
Static token creation automatically adds the authenticated user as owner. Exact
approved owner IDs must account for that identity. The publisher must not add
owners or teams automatically to make a policy pass.
[Scope checks](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/krate/publish.rs#L187-L228),
[crate/owner creation](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/krate/publish.rs#L466-L507).

There is no supported API-token identity read in the audited routes. `/api/v1/me`
accepts cookies only. `/api/v1/tokens/current` only revokes. Reading a known
`/api/v1/me/tokens/{id}` token accepts API authentication, but serializing its
database model explicitly omits `user_id`. Bootstrap needs an explicit trusted
credential-provisioning identity assertion or an externally established owner
proof; do not guess a token identity endpoint.
[Cookie restriction](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/user/me.rs#L35-L41),
[token read](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/token.rs#L249-L284),
[hidden user ID](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/crates/crates_io_database/src/models/token.rs#L41-L50).

## Fresh remote proof and recovery

Verify the exact version API, sparse index, downloaded archive, and owners:

1. `GET /api/v1/crates/{name}/{version}`: exact `version.crate`, `num`,
   `yanked == false`, lowercase SHA256 `checksum`, expected metadata.
2. `GET https://index.crates.io/{index_path}`: exactly one version entry;
   exact `name`, `vers`, `yanked == false`, and `cksum` equal local archive digest.
   Merge `features2` with `features`; verify features, dependencies, links, MSRV.
3. `GET https://static.crates.io/crates/{name}/{name}-{version}.crate`: exact
   compressed digest plus independent source/file/normalized manifest proof.
   Download redirects alone prove nothing; the download controller does not
   consult database existence when constructing a redirect.
4. `GET /api/v1/crates/{name}/owners`: `users` includes both kinds;
   `kind` is string `user` or `team`, not a numeric discriminator. Compare exact
   approved sorted `kind:id` identities; login strings are descriptive.

[Version API](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/version/metadata.rs#L16-L46),
[version schema/checksum](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/crates/crates_io_api_types/src/lib.rs#L922-L1002),
[index schema](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/crates/crates_io_index/src/data.rs#L6-L72),
[download redirect](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/version/downloads.rs#L47-L66),
[owners](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/krate/owners.rs#L32-L67),
[owner schema](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/crates/crates_io_api_types/src/lib.rs#L547-L605).

Index dependencies encode aliases in `name` and actual renamed package in
`package`; unrenamed entries omit `package`. Normalize absent/null `kind` to
`normal` for old index records. The version dependencies API exposes only
`crate_id`, `req`, `optional`, `default_features`, `features`, `target`, `kind`
plus IDs/download counts; it cannot independently prove rename or registry.
[API dependency schema](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/crates/crates_io_api_types/src/lib.rs#L148-L215).

Recovery policy is a local contract, not a crates.io idempotency endpoint. Before
upload, reconcile any existing version fully. After timeout, transport failure,
5xx, or duplicate response, poll fresh immutable proof before retrying. Equal
proof means success; differing bytes/metadata/owners means collision. Absence
after a finite deadline means incomplete, never successful. Bound reads, polls,
and write attempts; preserve per-package partial receipts and do not upload
dependents until dependency publication is verified. Crates.io enforces duplicate
version uniqueness and stores the hash of compressed bytes.
[Uniqueness](https://github.com/rust-lang/crates.io/blob/693b912ffc88759dbdc51ffb652a471dc54e28b9/src/controllers/krate/publish.rs#L560-L575).

After an upload attempt, success requires remote checksum equal to the submitted
compressed archive checksum. Source-equivalent archives with different gzip or
tar metadata cannot prove that attempted upload succeeded. Any explicitly
permitted existing-version recovery based on normalized source equivalence must
be recorded separately as existing remote bytes; never call it submitted-byte
publication proof.

## Forge tag and release obligations

Release-plz creates an annotated tag with message
`chore: Release package {name} version {version}`. `POST /repos/{repository}/git/tags`
uses `{tag, message, object: approved_source_sha, type: "commit"}`; then
`POST /repos/{repository}/git/refs` uses
`{ref: "refs/tags/{tag}", sha: tag_object_sha}`. Creating an object does not create
its ref. Freeze names, notes, and attributes anonymously; require existing refs
to peel to the approved source before writes and after ambiguous responses.
[Release-plz tag logic](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/git/forge.rs#L1040-L1091),
[GitHub tags API](https://docs.github.com/en/rest/git/tags?apiVersion=2022-11-28#create-a-tag-object).

`POST /repos/{repository}/releases` uses `tag_name`, `name`, `body`, `draft`,
`prerelease`, and configured optional `make_latest`/`generate_release_notes`.
Default draft is false; default prerelease follows SemVer prerelease. Default
release name independently uses full project cardinality: `v{version}` for
single releasable package, `{package}-v{version}` for multiple. A custom tag
pattern does not change that default name. Default body is the last changelog
entry. Fixed publication must require explicit verified notes/status rather
than inventing an empty body after parser failure. Tag write precedes release
write; record a tag-only receipt if release publication fails. Never silently
overwrite conflicting tag/release metadata.
[Release-plz creation](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/release.rs#L1044-L1100),
[name defaults](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/project.rs#L145-L178),
[body defaults](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/tera.rs#L22-L44),
[GitHub releases API](https://docs.github.com/en/rest/releases/releases?apiVersion=2022-11-28#create-a-release).

Cardinality specifically counts workspace packages whose resolved effective
configuration has `release = true`, before the single-package filter; even a
`publish = false` package counts. Use the generated effective selection config.
Release-plz's last-entry parser delegates to locked `parse-changelog 0.6.17`,
crate SHA256 `c0878368d958d9eac74fcbbe62d3fa9fc0e6f1c2ea7c16f9cdbd7232722d58d9`.
It returns first release notes, skipping the first entry if its parsed version
contains `unreleased` case-insensitively. Notes preserve source Markdown and
reference definitions, with trailing Unicode whitespace trimmed. A replacement
must faithfully port its heading/fence/comment behavior or explicitly reject
unsupported syntax, never approximate successful notes with an empty string.
[Cardinality source](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/project.rs),
[last-entry parser](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/changelog_parser.rs#L75-L145),
[locked parser source archive](https://static.crates.io/crates/parse-changelog/parse-changelog-0.6.17.crate).

## Anonymous preparation seam

`release-plz update --config <fixed-config> --manifest-path <approved-manifest>
--repo-url https://github.com/{repository} --forge github` performs anonymous
version/manifests/changelog/lock preparation. Remove all token environment.
Update has human summary output, no `-o json`; derive typed preparation proof
from inspected outputs. The existing `release-pr` implementation calls the same
core update before its authenticated Git phase, proving the split seam exists.
Fixed preparation artifact carries only validated allowlisted changed file bytes,
base blob IDs, exact source SHA, workflow SHA, run ID, attempt, and generated
PR metadata. Dependent workspace manifests can legitimately change version
requirements. The credentialed Forge coordinator creates blobs/tree/commit/ref
and PR through GitHub API only, after independent artifact and source checks.
[CLI seam](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz/src/main.rs#L31-L46),
[core update](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/update/mod.rs#L41-L63),
[release-pr split](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/release_pr/mod.rs#L129-L160).

Upstream release PR defaults: branch `release-plz-<UTC timestamp>` with colons
replaced by hyphens; legacy discovery also accepts `release-plz/`. Draft false,
labels empty. One updated package in a project with multiple public packages
uses `chore({package}): release v{version}`; different next versions use
`chore: release`; otherwise `chore: release v{version}`. Body contains version
transitions, compatibility/breaking-change results, and changelogs.
[PR metadata](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/pr.rs#L7-L145).

Upstream discovery checks open head-prefix only, keeps the first matching PR,
and closes extras. It lacks author/base/head-repository enforcement. Later human
contributors cause replacement; otherwise it rebuilds and force-updates the
branch. Fixed publication must strengthen ownership and exact base/head checks,
not transplant prefix-only destructive selection. Require authenticated writer
identity, same repository head/base, allowed branch, frozen expected head, and
one matching owned PR before updates. Never close unrelated prefix matches.
Use independently reconciled immutable commit/tree proof after ambiguous writes.
[Discovery](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/git/forge.rs#L432-L457),
[existing update semantics](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/release_pr/mod.rs#L200-L388).

Semver-check configuration alone is not proof: upstream warns and skips library
checks if `cargo-semver-checks` is absent. Its checker cleans newly created
package-local `target`/`Cargo.lock`, but retains changes to existing targets.
Anonymous preparation must require the pinned checker when applicable and use
an external fixed Cargo target directory; never include build output in PR files.
[Skip condition](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/update/updater.rs#L940-L960),
[cleanup](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/semver_check.rs#L47-L99).

The anonymous update's stdout contains `PackagesUpdate::summary`: exact package
rows with previous/next versions, optional compatible/breaking marker, then full
incompatibility reports. Preserve reports and validate parsed row identities
against manifest proof. If previous and next versions are equal, the row omits
its semver marker even if a check ran; do not infer compatibility merely from
tool presence or version equality. Mark unavailable status explicitly or use
the native core result when exact status is required.
[Summary source](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/update/packages_update.rs#L46-L85).

Local dependency rewrites use `cargo_utils::upgrade_requirement`: exact, tilde,
and caret comparators retain their operator and component precision, replace
present numeric components and prerelease with the new version; wildcard
comparators replace present numeric components. A leading implicit caret is
removed when the original string did not start with caret. Empty comparator
requirements remain unchanged; ordered comparator rewrites fail upstream.
Validate this transformation exactly, rather than merely checking that a new
version falls inside an arbitrary changed requirement.
[Rewrite source](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/cargo_utils/src/version.rs#L1-L73).

Anonymous update uses a local copied Git repository; its update path does not
fetch or pull Git remotes. A full-history private repository checkout therefore
supports the fixed default changelog without inherited Git credentials. Remote
API data is required only when changelog templates request `remote.username` or
`remote.pr_number`; those templates fail without a Git client. Explicitly freeze
the changelog configuration too: update otherwise searches ambient git-cliff
configuration even when release-plz's own `--config` was supplied.
[Local repository](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/project.rs#L131-L143),
[conditional remote data](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/changelog_filler.rs#L44-L90),
[ambient config lookup](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz/src/args/update.rs#L242-L269).

Independent workspace reconstruction must follow Cargo's selected manifest
root/pointer discovery, members glob expansion, transitive path-dependency
membership, and final root/name/membership validation. Excludes are literal
path prefixes, not glob patterns; original literal member prefixes override
exclusions. Explicit members still permit transitive path dependencies. Outside
root dependencies join only when their discovered workspace matches. Directory
containment alone does not establish membership.
[Root discovery](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/core/workspace.rs#L832-L900),
[dependency traversal](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/core/workspace.rs#L939-L992),
[validation](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/core/workspace.rs#L1025-L1134),
[literal exclude](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/core/workspace.rs#L1982-L1999).

## Package preparation trust boundary

`cargo package --no-verify --locked` skips only the compile verification phase.
It still resolves dependencies and creates normalized lockfiles, reads package
files/README/license/path dependencies, and performs Git status. `--list` also
prepares archive inventory and is not a universal no-execution guarantee.
[Package phases](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/cargo_package/mod.rs#L253-L351),
[lock resolution](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/cargo_package/mod.rs#L742-L799).

Cargo configuration discovery starts from process cwd ancestors plus Cargo
home. An outside-source cwd avoids source `.cargo/config`, but all searched
ancestor configs and environment overrides must also be absent or fixed.
Use trusted pinned Cargo/rustc executables, empty wrappers, an allowlisted
environment, clean Cargo home, registry-only dependency sources, fixed sparse
crates.io authority, and no external credential provider. Default Cargo
credential providers are built-in `cargo:token`; configured external providers
can execute programs. Resolution can query `rustc -vV`, so fixing only the
eventual build command is insufficient.
[Config discovery](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/util/context/mod.rs#L1349-L1362),
[cwd/home search](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/util/context/mod.rs#L1692-L1719),
[credential providers](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/util/auth/mod.rs#L83-L148),
[compiler query](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/util/rustc.rs#L47-L68).

Git status is an execution boundary too. Cargo's locked gitoxide components
(`gix 0.85.0`, `gix-status 0.32.0`, `gix-filter 0.32.0`) can apply configured
clean/process filters during status; matching `.gitattributes` plus repository
`filter.*.clean` or `filter.*.process` configuration leads to shell-capable
process spawning before Cargo's verification phase. Fresh Cargo home and an
outside cwd do not sanitize `.git/config`. Initial package production cannot be
qualified as no repository execution until local/worktree/submodule/global/
system/environment Git configuration is removed or proven fixed, or Git
discovery is eliminated with independently preserved source/VCS proof.
[Cargo status](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/cargo_package/vcs.rs#L285-L300),
[filter config](https://github.com/GitoxideLabs/gitoxide/blob/6428edc82fc8a16d5ef34ca2d49aa6fdff3645fe/gix/src/filter.rs#L291-L325),
[driver spawn](https://github.com/GitoxideLabs/gitoxide/blob/6428edc82fc8a16d5ef34ca2d49aa6fdff3645fe/gix-filter/src/driver/init.rs#L89-L101).

Sanitizing only the parent Git config is insufficient for initialized
submodules: Cargo recursively opens and statuses their repositories, applying
each repository's configuration. Reject or independently sterilize initialized
`.git/modules` and nested repositories before package status. Gitoxide's default
open permissions disable querying config through the Git binary; the audited
status/dirwalk paths have no implemented fsmonitor/hook execution path. This
does not eliminate the proven clean-filter execution path.
[Recursive submodule status](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/cargo_package/vcs.rs#L336-L348),
[Git binary permissions](https://github.com/GitoxideLabs/gitoxide/blob/6428edc82fc8a16d5ef34ca2d49aa6fdff3645fe/gix/src/open/permissions.rs#L39-L51).

Cargo's normal verifier consumes the generated compressed archive, unpacks it,
creates an ephemeral workspace, compiles it, and compares extracted source
fingerprints before/after execution. The public CLI cannot call this verifier
directly on an arbitrary supplied archive; normal `cargo package` first packages
and then verifies. A separate mandatory normal package verification consumer
must preserve the original pre-execution archive and prove its newly packaged
verification input has equivalent normalized source inventory. Publish the
original immutable bytes, not verifier-produced replacement bytes. Compressed
byte equality is a stricter relation than normalized source equality; record
which relation each proof establishes.
[Verifier](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/cargo_package/verify.rs#L35-L132).

## Native qualified semver extension

Stock release-plz 0.3.169 exposes no API for injecting external semver results
before version selection. `Diff` is internal; its checker runs inside private
`get_packages_diffs`. Separate locked file-mode checks followed by stock update
with semver disabled do not preserve behavior: incompatible diffs determine
breaking increments before version groups, workspace versions, dependency
propagation, manifests, changelogs, and PR metadata are calculated.
[Injection site](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/update/updater.rs#L287-L311),
[selection order](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/update/updater.rs#L57-L110),
[breaking increment](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/version.rs#L12-L21).

The minimum owned source extension adds a typed qualified comparison provider to
`UpdateRequest`, and consumes it at that injection site before any executable
installation probe. Preserve eligibility: baseline exists, both packages expose
libraries, config enables checking, diff requires version update. Missing
eligible proof fails; the owned route never probes or invokes the stock checker
and never falls back. Bind comparisons to exact package, baseline/candidate
source proofs, checker version/hash, toolchain, qualified mode and producer.
Preserve upstream minor-release comparison: status 0 is compatible; status 100
is incompatible with the full trimmed UTF-8 stdout report; all others are errors.
[Eligibility](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/update/updater.rs#L943-L959),
[output mapping](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/semver_check.rs#L102-L124).

Reuse native `update`, `PackagesUpdate::releases()` serialization, and `Pr::new`
through a thin anonymous preparation wrapper. PR title cardinality counts
publishable workspace packages; Git release name cardinality separately counts
effective release-enabled packages. Preserve both native rules. The helper needs
its own qualified upstream-commit/patch-hash/binary provenance; the unmodified
stock release-plz binary must not be claimed to implement this extension.
Checker injection alone does not qualify native metadata/lock subprocesses:
stock update calls `cargo update [--workspace]` without `--locked`.
[Native PR construction](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/release_pr/mod.rs#L221-L236),
[native output](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/update/packages_update.rs#L90-L140),
[lock subprocess](https://github.com/release-plz/release-plz/blob/786894b6ce1abad0d0e9bea0ace4958e099af8ca/crates/release_plz_core/src/command/update/mod.rs#L139-L162).

Registry resolution uses index summaries; archive manifest parsing does not
itself enqueue newly declared Git sources. Stock Cargo has no proven dependency
equality check between index and archive. Workspace inheritance can read outside
the archive; promoting an extracted baseline into a new root resolution can
activate Git/alternate-registry declarations. Require admission before manifest
normalization and promotion, and at every actual source load. Checksums prove
bytes, not index/manifest semantic equality.
[Index summaries](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/sources/registry/index/mod.rs#L194-L223),
[resolved download graph](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/core/package.rs#L613-L709),
[manifest source IDs](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/util/toml/mod.rs#L2376-L2431),
[workspace inheritance](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/util/toml/mod.rs#L975-L1018).
