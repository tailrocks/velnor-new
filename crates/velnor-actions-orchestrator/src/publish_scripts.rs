//! Fixed shell and jq text for the publication workflow.
//!
//! Every script is a constant template filled only with values the
//! contract already validated (artifact ids, repo-relative paths, a branch
//! name, a repository slug), so no config text can introduce shell
//! syntax. GitHub API reads go through the pinned `gh` function; the jq
//! programs end in `error(...)`, which makes `gh api --jq` exit non-zero.

use velnor_actions_contract::config::VersionSource;

/// Pattern a strict `X.Y.Z` version must match (POSIX ERE, whole line).
const SEMVER_ERE: &str = r"(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*)){2}";
/// Pattern an artifact id must match (POSIX ERE, whole line).
const ID_ERE: &str = "[a-z][a-z0-9]*(-[a-z0-9]+)*";

/// Define the `gh` shell function through the pinned Mise tool.
///
/// `prefix` is the Mise argv up to and including `gh`.
pub(crate) fn gh_function(prefix: &[String]) -> String {
    format!("gh() {{ {} \"$@\"; }}", prefix.join(" "))
}

/// Parse the pushed tag, require an exact declared artifact, write outputs.
pub(crate) fn parse_tag_script(repository: &str, artifacts: &[&str]) -> String {
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
pub(crate) fn annotated_tag_script(gh: &str) -> String {
    format!(
        "set -eu; {gh}; \
         gh api \"repos/$GITHUB_REPOSITORY/git/ref/tags/$GITHUB_REF_NAME\" \
         --jq '.object | if .type == \"tag\" then .sha else error(\"lightweight_tag\") end' \
         | {{ read -r tag_sha; gh api \"repos/$GITHUB_REPOSITORY/git/tags/$tag_sha\" \
         --jq 'if .object.type == \"commit\" and .object.sha == env.GITHUB_SHA then empty else error(\"tag_target_mismatch\") end'; }}"
    )
}

/// Require the tagged commit to be reachable from the default branch.
pub(crate) fn ancestry_script(gh: &str, branch: &str) -> String {
    format!(
        "set -eu; {gh}; \
         gh api \"repos/$GITHUB_REPOSITORY/compare/{branch}...$GITHUB_SHA\" \
         --jq 'if .status == \"behind\" or .status == \"identical\" then empty else error(\"not_on_default_branch\") end'"
    )
}

/// Require a successful `Required` check run, from GitHub Actions, at the commit.
pub(crate) fn required_script(gh: &str) -> String {
    format!(
        "set -eu; {gh}; \
         gh api \"repos/$GITHUB_REPOSITORY/commits/$GITHUB_SHA/check-runs?check_name=Required\" \
         --jq 'if ([.check_runs[] | select(.name == \"Required\" and .conclusion == \"success\" and .app.slug == \"github-actions\")] | length) > 0 then empty else error(\"required_not_success\") end'"
    )
}

/// Require the manifest at the tagged commit to carry the tag's version.
pub(crate) fn manifest_script(gh: &str, source: VersionSource, path: &str) -> String {
    let program = manifest_program(source);
    format!(
        "set -eu; {gh}; export VELNOR_TAG_VERSION=\"${{GITHUB_REF_NAME##*-v}}\"; \
         gh api \"repos/$GITHUB_REPOSITORY/contents/{path}?ref=$GITHUB_SHA\" --jq '{program}'"
    )
}

/// Require the tag version to equal an inline config literal.
pub(crate) fn inline_version_script(version: &str) -> String {
    format!("set -eu; [ \"${{GITHUB_REF_NAME##*-v}}\" = '{version}' ]")
}

/// jq program reading the version from a contents-API document.
///
/// Mirrors `VersionSource::read` line for line, then compares the result
/// with `env.VELNOR_TAG_VERSION`; every miss ends in `error(...)`.
fn manifest_program(source: VersionSource) -> String {
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
pub(crate) fn pack_script(id: &str, paths: &[String]) -> String {
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
pub(crate) fn verify_checksum_script(id: &str) -> String {
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; cd \"$RUNNER_TEMP/bundle\"; \
         ls | wc -l | grep -Eqx ' *2'; sha256sum -c -- \"{id}-$ver.tar.gz.sha256\""
    )
}

/// Upload the bundle as immutable assets of the tag's GitHub release.
pub(crate) fn upload_script(gh: &str, id: &str) -> String {
    let files = format!(
        "\"$RUNNER_TEMP/bundle/{id}-$ver.tar.gz\" \"$RUNNER_TEMP/bundle/{id}-$ver.tar.gz.sha256\""
    );
    format!(
        "set -eu; {gh}; ver=${{GITHUB_REF_NAME##*-v}}; \
         if gh release view \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" > /dev/null 2>&1; \
         then gh release upload \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" {files}; \
         else gh release create \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" --verify-tag \
         --title \"$GITHUB_REF_NAME\" --notes 'Release asset bundle' {files}; fi"
    )
}

#[cfg(test)]
#[path = "publish_scripts_tests.rs"]
mod tests;
