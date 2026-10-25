//! Fixed shell and jq text for the publication workflow.
//!
//! Lives with the renderer because it is generated-workflow content, not
//! orchestrator behavior: nothing here runs at generation time.
//!
//! Every script is a constant template filled only with values the
//! contract already validated (artifact ids, repo-relative paths, a branch
//! name, a repository slug), so no config text can introduce shell
//! syntax. GitHub API reads go through the pinned `gh` function; the jq
//! programs end in `error(...)`, which makes `gh api --jq` exit non-zero.

use velnor_actions_contract::config::VersionSource;

/// Maven bundle publication scripts (hard decision 8).
#[path = "publish_maven_scripts.rs"]
pub mod maven;
/// npm publication scripts (hard decision 8).
#[path = "publish_npm_scripts.rs"]
pub mod npm;
/// Declared Sigstore-bundle provenance scripts (`[publish] provenance`).
#[path = "publish_provenance_scripts.rs"]
pub mod provenance;

/// Pattern a strict `X.Y.Z` version must match (POSIX ERE, whole line).
const SEMVER_ERE: &str = r"(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*)){2}";
/// Pattern an artifact id must match (POSIX ERE, whole line).
const ID_ERE: &str = "[a-z][a-z0-9]*(-[a-z0-9]+)*";

/// Define the `gh` shell function through the pinned Mise tool.
///
/// `prefix` is the Mise argv up to and including `gh`.
#[must_use]
pub fn gh_function(prefix: &[String]) -> String {
    format!("gh() {{ {} \"$@\"; }}", prefix.join(" "))
}

/// Parse the pushed tag, require an exact declared artifact, write outputs.
#[must_use]
pub fn parse_tag_script(repository: &str, artifacts: &[&str]) -> String {
    let declared = artifacts.join("|");
    format!(
        "set -eu; [ \"$GITHUB_EVENT_NAME\" = push ]; [ \"$GITHUB_REF_TYPE\" = tag ]; \
         [ \"$GITHUB_REPOSITORY\" = '{repository}' ]; ver=${{GITHUB_REF_NAME##*-v}}; \
         id=${{GITHUB_REF_NAME%-v*}}; printf '%s' \"$ver\" | grep -Eqx '{SEMVER_ERE}'; \
         printf '%s' \"$id\" | grep -Eqx '{ID_ERE}'; \
         case \"$id\" in {declared}) ;; *) printf 'undeclared artifact: %s\\n' \"$id\" >&2; exit 1 ;; esac; \
         printf 'artifact=%s\\nversion=%s\\n' \"$id\" \"$ver\" >> \"$GITHUB_OUTPUT\""
    )
}

/// Require an annotated tag whose object points at the pushed commit.
#[must_use]
pub fn annotated_tag_script(gh: &str) -> String {
    format!(
        "set -eu; {gh}; \
         gh api \"repos/$GITHUB_REPOSITORY/git/ref/tags/$GITHUB_REF_NAME\" \
         --jq '.object | if .type == \"tag\" then .sha else error(\"lightweight_tag\") end' \
         | {{ read -r tag_sha; gh api \"repos/$GITHUB_REPOSITORY/git/tags/$tag_sha\" \
         --jq 'if .object.type == \"commit\" and .object.sha == env.GITHUB_SHA then empty else error(\"tag_target_mismatch\") end'; }}"
    )
}

/// Require the tagged commit to be reachable from the default branch.
#[must_use]
pub fn ancestry_script(gh: &str, branch: &str) -> String {
    format!(
        "set -eu; {gh}; \
         gh api \"repos/$GITHUB_REPOSITORY/compare/{branch}...$GITHUB_SHA\" \
         --jq 'if .status == \"behind\" or .status == \"identical\" then empty else error(\"not_on_default_branch\") end'"
    )
}

/// Require a successful `Required` check run, from GitHub Actions, at the commit.
#[must_use]
pub fn required_script(gh: &str) -> String {
    format!(
        "set -eu; {gh}; \
         gh api \"repos/$GITHUB_REPOSITORY/commits/$GITHUB_SHA/check-runs?check_name=Required\" \
         --jq 'if ([.check_runs[] | select(.name == \"Required\" and .conclusion == \"success\" and .app.slug == \"github-actions\")] | length) > 0 then empty else error(\"required_not_success\") end'"
    )
}

/// Require the manifest at the tagged commit to carry the tag's version.
#[must_use]
pub fn manifest_script(gh: &str, source: VersionSource, path: &str) -> String {
    let program = manifest_program(source);
    format!(
        "set -eu; {gh}; export VELNOR_TAG_VERSION=\"${{GITHUB_REF_NAME##*-v}}\"; \
         gh api \"repos/$GITHUB_REPOSITORY/contents/{path}?ref=$GITHUB_SHA\" --jq '{program}'"
    )
}

/// Require the tag version to equal an inline config literal.
#[must_use]
pub fn inline_version_script(version: &str) -> String {
    format!("set -eu; [ \"${{GITHUB_REF_NAME##*-v}}\" = '{version}' ]")
}

/// jq program reading the version from a contents-API document.
///
/// Mirrors `VersionSource::read` line for line, then compares the result
/// with `env.VELNOR_TAG_VERSION`; every miss ends in `error(...)`.
#[must_use]
pub fn manifest_program(source: VersionSource) -> String {
    let compare = r#"if . == env.VELNOR_TAG_VERSION then empty else error("version_mismatch") end"#;
    let found = match source {
        VersionSource::CargoToml => {
            r#"[.content | @base64d | split("\n") | foreach .[] as $l ({s: "", v: null}; if ($l | test("^\\[")) then .s = ($l | sub(" +$"; "")) | .v = null elif ((.s == "[package]" or .s == "[workspace.package]") and ($l | test("^version *= *\"[0-9]+\\.[0-9]+\\.[0-9]+\" *$"))) then .v = ($l | capture("\"(?<x>[^\"]+)\"").x) else .v = null end; .v) | select(. != null)] | first // error("version_not_found")"#
        }
        VersionSource::GradleProperties => {
            r#".content | @base64d | split("\n") | map(select(test("^version=[0-9]+\\.[0-9]+\\.[0-9]+$"))) | first // error("version_not_found") | ltrimstr("version=")"#
        }
        VersionSource::PackageJson => {
            r#".content | @base64d | fromjson | .version // error("version_not_found")"#
        }
    };
    format!("{found} | {compare}")
}

/// Build a deterministic gzip tarball and its SHA-256 sidecar.
///
/// Sorted entries, zero mtime, root ownership, normalized modes, no gzip
/// name or timestamp: identical inputs give identical bytes. Symlinks and
/// missing paths fail the build.
#[must_use]
pub fn pack_script(id: &str, paths: &[String]) -> String {
    let list = paths.join(" ");
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; out=\"$RUNNER_TEMP/bundle\"; mkdir -p \"$out\"; \
         for p in {list}; do test -e \"$p\"; done; \
         find -- {list} -type l | {{ if read -r link; then printf 'symlink in bundle: %s\\n' \"$link\" >&2; exit 1; fi; }}; \
         tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=ustar --mode='u=rwX,go=rX' -cf - -- {list} \
         | gzip -n -9 > \"$out/{id}-$ver.tar.gz\"; \
         cd \"$out\"; sha256sum \"{id}-$ver.tar.gz\" > \"{id}-$ver.tar.gz.sha256\""
    )
}

/// Re-verify the checksum sidecar against the downloaded tarball.
#[must_use]
pub fn verify_checksum_script(id: &str) -> String {
    verify_sidecar_script(&format!("{id}-$ver.tar.gz"))
}

/// Re-verify `file`'s `.sha256` sidecar; the directory holds exactly the two files.
///
/// `file` may use the shell variable `$ver` (the tag's version).
#[must_use]
pub fn verify_sidecar_script(file: &str) -> String {
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; cd \"$RUNNER_TEMP/bundle\"; \
         ls | wc -l | grep -Eqx ' *2'; sha256sum -c -- \"{file}.sha256\""
    )
}

/// Upload the bundle as immutable assets of the tag's GitHub release.
#[must_use]
pub fn upload_script(gh: &str, id: &str) -> String {
    upload_assets_script(gh, &format!("{id}-$ver.tar.gz"))
}

/// Upload `file` and its `.sha256` from the bundle directory as immutable
/// assets of the tag's GitHub release.
#[must_use]
pub fn upload_assets_script(gh: &str, file: &str) -> String {
    let names = [file.to_owned(), format!("{file}.sha256")];
    upload_files_script(gh, "$RUNNER_TEMP/bundle", &names, "Release asset bundle")
}

/// Upload `names` from `dir` as immutable assets of the tag's GitHub release.
///
/// A release that already holds an asset is never overwritten: all existing
/// assets are downloaded and compared by sha256 before any missing asset is
/// uploaded. Identical assets are skipped; a different one fails the job.
/// A missing release is created with every file at once (`--verify-tag`), so
/// an immutable release is complete when it appears. `dir` is a fixed
/// runner-temp directory, `names` are validated file names that may use
/// `$ver`, and `notes` is fixed text.
#[must_use]
pub fn upload_files_script(gh: &str, dir: &str, names: &[String], notes: &str) -> String {
    let quoted: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
    let list = quoted.join(" ");
    let files: Vec<String> = names
        .iter()
        .map(|name| format!("\"{dir}/{name}\""))
        .collect();
    let files = files.join(" ");
    format!(
        "set -eu; {gh}; ver=${{GITHUB_REF_NAME##*-v}}; b=\"{dir}\"; \
         t=\"$RUNNER_TEMP/published\"; \
         if gh release view \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" > /dev/null 2>&1; then \
         rm -rf -- \"$t\"; mkdir -p -- \"$t\"; \
         gh release view \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" \
         --json assets --jq '.assets[].name' > \"$t/names\"; \
         for f in {list}; do \
         if grep -Fxq -- \"$f\" \"$t/names\"; then \
         gh release download \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" --pattern \"$f\" --dir \"$t\"; \
         sha256sum < \"$t/$f\" > \"$t/old.sum\"; sha256sum < \"$b/$f\" > \"$t/new.sum\"; \
         if cmp -s \"$t/old.sum\" \"$t/new.sum\"; then \
         printf 'velnor: %s already published with the same sha256, skipping\\n' \"$f\"; \
         else printf 'velnor: %s already published with a different sha256, refusing to overwrite\\n' \"$f\" >&2; \
         exit 1; fi; fi; done; \
         for f in {list}; do \
         if ! grep -Fxq -- \"$f\" \"$t/names\"; then \
         gh release upload \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" \"$b/$f\"; fi; done; \
         else gh release create \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" --verify-tag \
         --title \"$GITHUB_REF_NAME\" --notes '{notes}' {files}; fi"
    )
}

/// Require a push of the default branch in the generation-time repository.
#[must_use]
pub fn branch_push_script(repository: &str, branch: &str) -> String {
    format!(
        "set -eu; [ \"$GITHUB_EVENT_NAME\" = push ]; [ \"$GITHUB_REF_TYPE\" = branch ]; \
         [ \"$GITHUB_REPOSITORY\" = '{repository}' ]; [ \"$GITHUB_REF\" = 'refs/heads/{branch}' ]"
    )
}

/// Poll the newest `Required` check run at the commit until it settles.
///
/// The default-branch push starts CI and this workflow together, so the
/// verdict may not exist yet: pending (absent or incomplete) sleeps and
/// retries, a settled non-success fails at once, and exhausting the budget
/// fails closed. `attempts` and `pause_seconds` are literals chosen by the
/// caller; the job timeout must exceed their product.
#[must_use]
pub fn required_wait_script(gh: &str, attempts: u16, pause_seconds: u16) -> String {
    let rounds: Vec<String> = (1..=attempts).map(|n| n.to_string()).collect();
    let rounds = rounds.join(" ");
    let program = r#"[.check_runs[] | select(.name == "Required" and .app.slug == "github-actions")] | sort_by(.id) | last | if . == null or .status != "completed" then "pending" elif .conclusion == "success" then "success" else "failure" end"#;
    format!(
        "set -eu; {gh}; state_file=\"$RUNNER_TEMP/required.state\"; \
         for round in {rounds}; do \
         gh api \"repos/$GITHUB_REPOSITORY/commits/$GITHUB_SHA/check-runs?check_name=Required\" \
         --jq '{program}' > \"$state_file\"; read -r state < \"$state_file\"; \
         case \"$state\" in success) exit 0 ;; pending) sleep {pause_seconds} ;; \
         *) printf 'Required concluded %s\\n' \"$state\" >&2; exit 1 ;; esac; done; \
         printf 'Required did not settle\\n' >&2; exit 1"
    )
}
