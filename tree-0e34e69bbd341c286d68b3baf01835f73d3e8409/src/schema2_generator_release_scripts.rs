//! Fixed shell for release admission and catalog-pinned MBX builds.

#[path = "schema2_generator_release_publish.rs"]
mod publish;

pub(super) fn prepare_manifest(version: &str, assets: &super::AssetNames) -> String {
    publish::prepare_manifest(version, assets)
}

pub(super) fn publish(version: &str, assets: &super::AssetNames) -> String {
    publish::publish(version, assets)
}

const CATALOG_VERSION: &str = r#"
catalog_version() {
  awk -F '"' -v name="$1" '
    $1 == "pub const " name ": &str = " && $3 == ";" {
      value = $2
      count += 1
    }
    END {
      if (count != 1 || value !~ /^[0-9]+\.[0-9]+\.[0-9]+$/) exit 1
      print value
    }
  ' crates/velnor-actions-mise/src/catalog.rs
}
"#;

const GH_SETUP: &str = r#"
GH_VERSION="$(catalog_version GH_VERSION)"
mise --no-config --no-env --no-hooks install "gh@$GH_VERSION"
gh() {
  mise --no-config --no-env --no-hooks exec "gh@$GH_VERSION" -- gh "$@"
}
"#;

const CI_CHECK: &str = r#"
verify_same_sha_ci() {
  runs="$(gh api "repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml/runs?head_sha=$GITHUB_SHA&event=push&branch=main&per_page=100")" || return 1
  printf '%s\n' "$runs" | jq -e --arg sha "$GITHUB_SHA" --arg repo "$GITHUB_REPOSITORY" '
      [
        .workflow_runs[]? |
        select(
          .head_sha == $sha and
          .head_branch == "main" and
          .event == "push" and
          .path == ".github/workflows/ci.yml" and
          .repository.full_name == $repo and
          .head_repository.full_name == $repo
        )
      ] |
      sort_by(.run_started_at) | last |
      select(. != null and .status == "completed" and .conclusion == "success") != null
    ' >/dev/null || return 1
}
"#;

const ENVIRONMENT_CHECK: &str = r#"
verify_release_environment() {
  environment="$(gh api "repos/$GITHUB_REPOSITORY/environments/generator-release")" || return 1
  printf '%s\n' "$environment" | jq -e '
      any(.protection_rules[]?;
        .type == "required_reviewers" and
        .prevent_self_review == true and
        (.reviewers | length) > 0
      ) and
      .deployment_branch_policy.protected_branches == true and
      .deployment_branch_policy.custom_branch_policies == false
    ' >/dev/null || return 1
}
"#;

const RELEASE_CONTEXT_CHECK: &str = r#"
test "$GITHUB_REPOSITORY" = "tailrocks/velnor-new"
test "$GITHUB_REF" = "refs/heads/main"
test "$GITHUB_REF_PROTECTED" = "true"
test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
repository="$(gh api "repos/$GITHUB_REPOSITORY")" || exit 1
printf '%s\n' "$repository" | jq -e '.default_branch == "main"' >/dev/null || exit 1
"#;

const FRESHNESS_CHECK: &str = "scripts/check-freshness.sh";

/// Read-only gate: protected main, same-SHA CI, freshness, and environment.
pub(super) fn release_gate() -> String {
    format!(
        "set -eu\n{CATALOG_VERSION}\n{GH_SETUP}\n{CI_CHECK}\n{ENVIRONMENT_CHECK}\n{RELEASE_CONTEXT_CHECK}\nverify_same_sha_ci\n{FRESHNESS_CHECK}\nverify_release_environment"
    )
}

/// Install catalog-pinned tools and reject MBX for the wrong host architecture.
pub(super) fn install_tools(_os: &str) -> String {
    format!("set -eu\n{CATALOG_VERSION}\n{INSTALL_TOOLS_BODY}")
}

const INSTALL_TOOLS_BODY: &str = r#"
test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
RUST_VERSION="$(catalog_version RUST_VERSION)"
MBX_VERSION="$(catalog_version MR_BOXINGTON_VERSION)"
mise --no-config --no-env --no-hooks install "rust@$RUST_VERSION" "mr-boxington@$MBX_VERSION"
mbx_path="$(mise --no-config --no-env --no-hooks exec "rust@$RUST_VERSION" "mr-boxington@$MBX_VERSION" -- which mbx)"
mbx_desc="$(file -b "$mbx_path")"
case "$RUNNER_OS:$(uname -m):$mbx_desc" in
  Linux:x86_64:*ELF*x86-64*) ;;
  macOS:arm64:*Mach-O*arm64*) ;;
  *) echo "MBX binary does not match official runner architecture: $mbx_desc" >&2; exit 1 ;;
esac
mbx_version="$(mise --no-config --no-env --no-hooks exec "rust@$RUST_VERSION" "mr-boxington@$MBX_VERSION" -- mbx --version)"
case "$mbx_version" in
  *"$MBX_VERSION"*) ;;
  *) echo "unexpected MBX version: $mbx_version" >&2; exit 1 ;;
esac
"#;

/// Install a cross target into the exact catalog Rust toolchain.
pub(super) fn install_rust_target(target: &str) -> String {
    format!(
        "set -eu\n{CATALOG_VERSION}\nRUST_VERSION=\"$(catalog_version RUST_VERSION)\"\nmise --no-config --no-env --no-hooks exec \"rust@$RUST_VERSION\" -- rustup target add {target}\nmise --no-config --no-env --no-hooks exec \"rust@$RUST_VERSION\" -- rustup target list --installed | grep -Fx {target}"
    )
}

/// Install the catalog Rust toolchain used by the candidate CLI at runtime.
/// This provides `cargo metadata` to `plan`/`generate` and never builds the
/// downloaded candidate.
pub(super) fn install_qualification_toolchain() -> String {
    format!(
        "set -eu\n{CATALOG_VERSION}\nRUST_VERSION=\"$(catalog_version RUST_VERSION)\"\nACTIONLINT_VERSION=\"$(catalog_version ACTIONLINT_VERSION)\"\nSHELLCHECK_VERSION=\"$(catalog_version SHELLCHECK_VERSION)\"\nZIZMOR_VERSION=\"$(catalog_version ZIZMOR_VERSION)\"\nmise --no-config --no-env --no-hooks install \"rust@$RUST_VERSION\" \"actionlint@$ACTIONLINT_VERSION\" \"shellcheck@$SHELLCHECK_VERSION\" \"zizmor@$ZIZMOR_VERSION\""
    )
}

/// Build with exact Rust and MBX versions, verify MBX recorded this build,
/// verify the output architecture, and write its checksum sidecar.
pub(super) fn build(
    target: &str,
    cross_compile: bool,
    os: &str,
    asset: &str,
    sidecar: &str,
    sum_command: &str,
) -> String {
    let (build_prefix, target_path, host) = match (os, cross_compile) {
        ("linux", false) => ("", "target/release", "Linux:x86_64"),
        ("macos", false) => ("", "target/release", "macOS:arm64"),
        ("macos", true) => (
            "CARGO_BUILD_TARGET=x86_64-apple-darwin ",
            "target/x86_64-apple-darwin/release",
            "macOS:arm64",
        ),
        _ => ("", "target/release", "invalid"),
    };
    let verify = match target {
        "x86_64-unknown-linux-gnu" => {
            r#"desc="$(file -b "$asset")"
case "$desc" in *ELF*x86-64*) ;; *) echo "wrong Linux target: $desc" >&2; exit 1 ;; esac"#
        }
        "aarch64-apple-darwin" => {
            r#"desc="$(file -b "$asset")"
case "$desc" in *Mach-O*arm64*) ;; *) echo "wrong macOS arm64 target: $desc" >&2; exit 1 ;; esac"#
        }
        "x86_64-apple-darwin" => {
            r#"desc="$(file -b "$asset")"
case "$desc" in *Mach-O*x86_64*) ;; *) echo "wrong macOS x86_64 target: $desc" >&2; exit 1 ;; esac"#
        }
        _ => "echo unsupported release target >&2; exit 1",
    };
    BUILD_BODY
        .replace("@CATALOG_VERSION@", CATALOG_VERSION)
        .replace("@TARGET@", target)
        .replace("@HOST@", host)
        .replace("@ASSET@", asset)
        .replace("@SIDECAR@", sidecar)
        .replace("@SUM_COMMAND@", sum_command)
        .replace("@BUILD_PREFIX@", build_prefix)
        .replace("@TARGET_PATH@", target_path)
        .replace("@VERIFY@", verify)
}

const BUILD_BODY: &str = r#"
set -eu
@CATALOG_VERSION@
test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
test "$RUNNER_OS:$(uname -m)" = "@HOST@"
RUST_VERSION="$(catalog_version RUST_VERSION)"
MBX_VERSION="$(catalog_version MR_BOXINGTON_VERSION)"
mbx_stats() {
  mise --no-config --no-env --no-hooks exec "rust@$RUST_VERSION" "mr-boxington@$MBX_VERSION" -- mbx stats --json
}
before="$(mbx_stats | jq -er '.savings.builds | select(type == "number")')"
@BUILD_PREFIX@mise --no-config --no-env --no-hooks exec "rust@$RUST_VERSION" "mr-boxington@$MBX_VERSION" -- mbx build --release --locked --package velnor-actions-cli --bin velnor-actions
after="$(mbx_stats | jq -er '.savings.builds | select(type == "number")')"
test "$after" -gt "$before" || { echo "MBX did not record this build" >&2; exit 1; }
asset="@ASSET@"
cp "@TARGET_PATH@/velnor-actions" "$asset"
test -s "$asset"
@VERIFY@
@SUM_COMMAND@ "$asset" > "@SIDECAR@"
test -s "@SIDECAR@"
"#;
