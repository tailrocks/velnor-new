// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::config::VersionSource;

#[path = "publish_maven_scripts.rs"]
pub mod maven;
#[path = "publish_npm_scripts.rs"]
pub mod npm;
#[path = "publish_provenance_scripts.rs"]
pub mod provenance;

const SEMVER_ERE: &str = r"(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*)){2}";
const ID_ERE: &str = "[a-z][a-z0-9]*(-[a-z0-9]+)*";

#[must_use]
pub fn gh_function(prefix: &[String]) -> String {
    format!("gh() {{ {} \"$@\"; }}", prefix.join(" "))
}

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

#[must_use]
pub fn ancestry_script(gh: &str, branch: &str) -> String {
    format!(
        "set -eu; {gh}; \
         gh api \"repos/$GITHUB_REPOSITORY/compare/{branch}...$GITHUB_SHA\" \
         --jq 'if .status == \"behind\" or .status == \"identical\" then empty else error(\"not_on_default_branch\") end'"
    )
}

#[must_use]
pub fn required_script(gh: &str) -> String {
    format!(
        "set -eu; {gh}; \
         gh api \"repos/$GITHUB_REPOSITORY/commits/$GITHUB_SHA/check-runs?check_name=Required\" \
         --jq 'if ([.check_runs[] | select(.name == \"Required\" and .conclusion == \"success\" and .app.slug == \"github-actions\")] | length) > 0 then empty else error(\"required_not_success\") end'"
    )
}

#[must_use]
pub fn manifest_script(gh: &str, source: VersionSource, path: &str) -> String {
    let program = manifest_program(source);
    format!(
        "set -eu; {gh}; export VELNOR_TAG_VERSION=\"${{GITHUB_REF_NAME##*-v}}\"; \
         gh api \"repos/$GITHUB_REPOSITORY/contents/{path}?ref=$GITHUB_SHA\" --jq '{program}'"
    )
}

#[must_use]
pub fn inline_version_script(version: &str) -> String {
    format!("set -eu; [ \"${{GITHUB_REF_NAME##*-v}}\" = '{version}' ]")
}

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

#[must_use]
pub fn verify_checksum_script(id: &str) -> String {
    verify_sidecar_script(&format!("{id}-$ver.tar.gz"))
}

#[must_use]
pub fn verify_sidecar_script(file: &str) -> String {
    format!(
        "set -eu; ver=${{GITHUB_REF_NAME##*-v}}; cd \"$RUNNER_TEMP/bundle\"; \
         ls | wc -l | grep -Eqx ' *2'; sha256sum -c -- \"{file}.sha256\""
    )
}

#[must_use]
pub fn upload_script(gh: &str, id: &str) -> String {
    upload_assets_script(gh, &format!("{id}-$ver.tar.gz"))
}

#[must_use]
pub fn upload_assets_script(gh: &str, file: &str) -> String {
    let names = [file.to_owned(), format!("{file}.sha256")];
    upload_files_script(gh, "$RUNNER_TEMP/bundle", &names, "Release asset bundle")
}

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
         exit 1; fi; \
         else gh release upload \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" \"$b/$f\"; fi; done; \
         else gh release create \"$GITHUB_REF_NAME\" --repo \"$GITHUB_REPOSITORY\" --verify-tag \
         --title \"$GITHUB_REF_NAME\" --notes '{notes}' {files}; fi"
    )
}

#[must_use]
pub fn branch_push_script(repository: &str, branch: &str) -> String {
    format!(
        "set -eu; [ \"$GITHUB_EVENT_NAME\" = push ]; [ \"$GITHUB_REF_TYPE\" = branch ]; \
         [ \"$GITHUB_REPOSITORY\" = '{repository}' ]; [ \"$GITHUB_REF\" = 'refs/heads/{branch}' ]"
    )
}

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
