//! Fixed shell bodies for generic Rust binary-release jobs.

use velnor_actions_contract::RustBinaryReleaseConfig;

use super::RenderedCommands;

pub(super) fn verify_source(
    config: &RustBinaryReleaseConfig,
    binary: &str,
    commands: &RenderedCommands,
) -> String {
    let template = r#"set -euo pipefail

fail() { printf 'binary release verification: %s\n' "$1" >&2; exit 1; }
[[ "${GITHUB_EVENT_NAME-}" == push ]] || fail 'event is not a tag push'
[[ "${GITHUB_REF_TYPE-}" == tag ]] || fail 'ref is not a tag'
[[ "${GITHUB_REF-}" == "refs/tags/$GITHUB_REF_NAME" ]] || fail 'tag ref is malformed'
source_sha="${GITHUB_SHA-}"
[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'source SHA is malformed'
checkout_sha="$(git rev-parse HEAD)" || fail 'cannot read checkout SHA'
[[ "$checkout_sha" == "$source_sha" ]] || fail 'checkout differs from event SHA'
tag_ref="refs/tags/$GITHUB_REF_NAME"
tag_type="$(git cat-file -t "$tag_ref")" || fail 'tag is missing from checkout'
case "$tag_type" in
  commit) tag_sha="$(git rev-parse "$tag_ref")" ;;
  tag)
    tag_object="$(git cat-file tag "$tag_ref")" || fail 'annotated tag is malformed'
    tag_sha="$(sed -n 's/^object //p' <<<"$tag_object")"
    [[ "$(git cat-file -t "$tag_sha")" == commit ]] || fail 'nested or non-commit tags are unsupported'
    ;;
  *) fail 'tag does not point to a commit' ;;
esac
[[ "$tag_sha" == "$source_sha" ]] || fail 'tag does not target event SHA'
metadata="$(@@CARGO_METADATA@@)" || fail 'Cargo metadata failed'
version="$(jq -er --arg package '@@PACKAGE@@' --arg binary '@@BINARY@@' '[.packages[] | select(.name == $package)] as $packages | if ($packages | length) != 1 then error("selected Cargo package must be unique") elif ([$packages[0].targets[] | select(.name == $binary and (.kind | index("bin")))] | length) != 1 then error("selected binary target must be unique") else $packages[0].version end' <<<"$metadata")" || fail 'selected Cargo package or binary was not found'
expected_tag="@@PACKAGE@@-v$version"
[[ "$GITHUB_REF_NAME" == "$expected_tag" ]] || fail 'tag does not match Cargo package version'
printf 'source_sha=%s\nversion=%s\ntag=%s\n' "$source_sha" "$version" "$expected_tag" >> "$GITHUB_OUTPUT""#;
    template
        .replace("@@CARGO_METADATA@@", &commands.metadata)
        .replace("@@PACKAGE@@", &config.package)
        .replace("@@BINARY@@", binary)
}

pub(super) fn build(binary: &str, target: &str, build: &str, rustc: &str) -> String {
    let target_check = if target == "x86_64-unknown-linux-gnu" {
        "case \"$description\" in *ELF*'x86-64'*) ;; *) fail 'built file is not Linux x86_64' ;; esac"
    } else {
        "case \"$description\" in *Mach-O*arm64*) ;; *) fail 'built file is not macOS ARM64' ;; esac"
    };
    let template = r#"set -euo pipefail

fail() { printf 'binary build: %s\n' "$1" >&2; exit 1; }
source_sha="$(git rev-parse HEAD)" || fail 'cannot read checkout SHA'
[[ "$source_sha" == "${SOURCE_SHA-}" ]] || fail 'build checkout differs from verified source'
host="$(@@RUSTC_VERSION@@ | sed -n 's/^host: //p')" || fail 'rustc host lookup failed'
[[ "$host" == '@@TARGET@@' ]] || fail 'runner Rust host does not match requested target'
mkdir -p dist
CARGO_BUILD > "$RUNNER_TEMP/cargo-build.json" || fail 'Cargo build failed'
executable="$(jq -s -er --arg binary '@@BINARY@@' '[.[] | select(.reason == "compiler-artifact" and .target.name == $binary and (.target.kind | index("bin")) and .executable != null) | .executable] | if length == 1 then .[0] else error("expected exactly one selected executable") end' "$RUNNER_TEMP/cargo-build.json")" || fail 'Cargo did not report exactly one selected executable'
[[ -f "$executable" && ! -L "$executable" ]] || fail 'built executable is missing or not a regular file'
[[ -x "$executable" ]] || fail 'built binary is not executable'
description="$(file -b "$executable")" || fail 'cannot inspect executable format'
TARGET_CHECK
asset="@@BINARY@@-${RELEASE_VERSION}-@@TARGET@@.tar.gz"
archive_dir="$(mktemp -d "$RUNNER_TEMP/binary-release.XXXXXX")" || fail 'cannot create archive directory'
cp "$executable" "$archive_dir/@@BINARY@@"
chmod 755 "$archive_dir/@@BINARY@@"
tar -czf "dist/$asset" -C "$archive_dir" "@@BINARY@@" || fail 'cannot create binary archive'
rm -rf "$archive_dir"
test -s "dist/$asset" || fail 'release archive is empty'
[[ "$(tar -tzf "dist/$asset")" == '@@BINARY@@' ]] || fail 'release archive has an unexpected file set'"#;
    template
        .replace("@@RUSTC_VERSION@@", rustc)
        .replace("@@TARGET@@", target)
        .replace("CARGO_BUILD", build)
        .replace("@@BINARY@@", binary)
        .replace("TARGET_CHECK", target_check)
}

pub(super) fn prepare_assets(package: &str, binary: &str) -> String {
    let template = r#"set -euo pipefail

fail() { printf 'binary release asset validation: %s\n' "$1" >&2; exit 1; }
source_sha="${SOURCE_SHA-}"
tag="${RELEASE_TAG-}"
[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'source SHA is malformed'
[[ "$tag" == '@@PACKAGE@@-v${RELEASE_VERSION-}' ]] || fail 'tag and version disagree'
[[ "${GITHUB_EVENT_NAME-}" == push && "${GITHUB_REF_TYPE-}" == tag ]] || fail 'event is not a tag push'
[[ "${GITHUB_REF_NAME-}" == "$tag" && "${GITHUB_REF-}" == "refs/tags/$tag" ]] || fail 'event tag differs from selected package version'

[[ -d assets/incoming-linux && -d assets/incoming-macos ]] || fail 'one or more build artifacts are missing'
linux_name="@@BINARY@@-${RELEASE_VERSION}-x86_64-unknown-linux-gnu.tar.gz"
macos_name="@@BINARY@@-${RELEASE_VERSION}-aarch64-apple-darwin.tar.gz"
linux_source="assets/incoming-linux/$linux_name"
macos_source="assets/incoming-macos/$macos_name"
[[ -f "$linux_source" && ! -L "$linux_source" && -s "$linux_source" ]] || fail 'Linux artifact is missing, empty, or not a regular file'
[[ -f "$macos_source" && ! -L "$macos_source" && -s "$macos_source" ]] || fail 'macOS artifact is missing, empty, or not a regular file'
[[ "$(find assets/incoming-linux -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')" == 1 ]] || fail 'Linux artifact contains an unexpected file set'
[[ "$(find assets/incoming-macos -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')" == 1 ]] || fail 'macOS artifact contains an unexpected file set'
[[ -z "$(find assets/incoming-linux assets/incoming-macos ! -type d ! -type f -print)" ]] || fail 'artifact contains a non-regular file'
tar -tzf "$linux_source" | awk -v name='@@BINARY@@' 'NR == 1 && $0 == name { valid = 1 } END { if (NR != 1 || !valid) exit 1 }' || fail 'Linux archive has an unexpected file set'
tar -tzf "$macos_source" | awk -v name='@@BINARY@@' 'NR == 1 && $0 == name { valid = 1 } END { if (NR != 1 || !valid) exit 1 }' || fail 'macOS archive has an unexpected file set'
tar -tvzf "$linux_source" | awk -v name='@@BINARY@@' 'NR == 1 && substr($0, 1, 1) == "-" && substr($0, 2, 9) == "rwxr-xr-x" && $NF == name { valid = 1 } END { if (NR != 1 || !valid) exit 1 }' || fail 'Linux archive binary mode or type is invalid'
tar -tvzf "$macos_source" | awk -v name='@@BINARY@@' 'NR == 1 && substr($0, 1, 1) == "-" && substr($0, 2, 9) == "rwxr-xr-x" && $NF == name { valid = 1 } END { if (NR != 1 || !valid) exit 1 }' || fail 'macOS archive binary mode or type is invalid'
mkdir -p assets
cp -- "$linux_source" "assets/$linux_name"
cp -- "$macos_source" "assets/$macos_name"
cd assets
sha256sum "$linux_name" "$macos_name" > SHA256SUMS
sha256sum --check SHA256SUMS || fail 'checksum verification failed'
"#;
    template
        .replace("@@PACKAGE@@", package)
        .replace("@@BINARY@@", binary)
}

pub(super) fn publish(binary: &str, gh_prefix: &str) -> String {
    let template = r#"set -euo pipefail

fail() { printf 'binary release publish: %s\n' "$1" >&2; exit 1; }
gh_api() { @@GH_PREFIX@@ api "$@"; }
source_sha="${SOURCE_SHA-}"
tag="${RELEASE_TAG-}"
linux_name="@@BINARY@@-${RELEASE_VERSION}-x86_64-unknown-linux-gnu.tar.gz"
macos_name="@@BINARY@@-${RELEASE_VERSION}-aarch64-apple-darwin.tar.gz"
[[ -f assets/SHA256SUMS && ! -L assets/SHA256SUMS ]] || fail 'prepared checksum file is missing or not regular'
cd assets
prepared_sums="$(cat SHA256SUMS)" || fail 'cannot read prepared checksums'
actual_sums="$(sha256sum "$linux_name" "$macos_name")" || fail 'cannot calculate asset checksums'
[[ "$prepared_sums" == "$actual_sums" ]] || fail 'prepared checksums do not match the release assets'

remote_ref="$(gh_api "repos/$GITHUB_REPOSITORY/git/ref/tags/$tag")" || fail 'remote tag is missing or unreadable'
[[ "$(jq -er '.ref' <<<"$remote_ref")" == "refs/tags/$tag" ]] || fail 'GitHub returned a different tag ref'
object_type="$(jq -er '.object.type' <<<"$remote_ref")" || fail 'remote tag object type is malformed'
object_sha="$(jq -er '.object.sha' <<<"$remote_ref")" || fail 'remote tag object SHA is malformed'
[[ "$object_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'remote tag object SHA is malformed'
case "$object_type" in
  commit) remote_source_sha="$object_sha" ;;
  tag)
    tag_object="$(gh_api "repos/$GITHUB_REPOSITORY/git/tags/$object_sha")" || fail 'annotated tag object is unreadable'
    [[ "$(jq -er '.object.type' <<<"$tag_object")" == commit ]] || fail 'nested or non-commit tags are unsupported'
    remote_source_sha="$(jq -er '.object.sha' <<<"$tag_object")" || fail 'annotated tag target is malformed'
    [[ "$remote_source_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'annotated tag target SHA is malformed'
    ;;
  *) fail 'remote tag does not point to a commit or annotated tag' ;;
esac
[[ "$remote_source_sha" == "$source_sha" ]] || fail 'remote tag moved after verification'

@@GH_PREFIX@@ release create "$tag" --verify-tag --target "$source_sha" --title "$tag" --notes "Automated binary release for $tag." --latest=false "$linux_name" "$macos_name" SHA256SUMS"#;
    template
        .replace("@@BINARY@@", binary)
        .replace("@@GH_PREFIX@@", gh_prefix)
}
