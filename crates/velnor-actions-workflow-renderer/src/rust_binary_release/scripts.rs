//! Fixed shell bodies for generic Rust binary-release jobs.

use velnor_actions_contract::RustBinaryReleaseConfig;

use super::RenderedCommands;

mod assets;
pub(super) use assets::{prepare_assets, publish};

const SEMVER_TAG_SORTER: &str = r#"python3 - "$tag_file" "$ordered_tag_file" "@@PACKAGE@@-v" <<'PY' || fail 'cannot order release tags by SemVer precedence'
import functools
import re
import sys

semver = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?",
    re.ASCII,
)

def parse(tag):
    if not tag.startswith(sys.argv[3]):
        return None
    version = tag[len(sys.argv[3]):]
    match = semver.fullmatch(version)
    if match is None:
        return None
    prerelease = match.group(4)
    identifiers = prerelease.split(".") if prerelease is not None else None
    if identifiers is not None and any(
        item.isdigit() and len(item) > 1 and item.startswith("0")
        for item in identifiers
    ):
        return None
    return tag, match.group(1, 2, 3), identifiers

def compare_decimal(left, right):
    if len(left) != len(right):
        return (len(left) > len(right)) - (len(left) < len(right))
    return (left > right) - (left < right)

def compare_versions(left, right):
    for a, b in zip(left[1], right[1]):
        result = compare_decimal(a, b)
        if result:
            return result
    a, b = left[2], right[2]
    if a is None or b is None:
        if a is not b:
            return 1 if a is None else -1
        return 0
    for x, y in zip(a, b):
        if x == y:
            continue
        x_numeric, y_numeric = x.isdigit(), y.isdigit()
        if x_numeric != y_numeric:
            return -1 if x_numeric else 1
        return compare_decimal(x, y) if x_numeric else ((x > y) - (x < y))
    return (len(a) > len(b)) - (len(a) < len(b))

def compare_tags(left, right):
    result = compare_versions(left, right)
    if result:
        return -result
    return (left[0] > right[0]) - (left[0] < right[0])

with open(sys.argv[1], encoding="utf-8", errors="surrogateescape") as source:
    tags = [parsed for line in source if (parsed := parse(line.rstrip("\n"))) is not None]
tags.sort(key=functools.cmp_to_key(compare_tags))
with open(sys.argv[2], "w", encoding="utf-8", errors="surrogateescape") as target:
    target.writelines(tag[0] + "\n" for tag in tags)
PY"#;

const VERIFY_SOURCE_TEMPLATE: &str = r#"set -euo pipefail

fail() { printf 'binary release verification: %s\n' "$1" >&2; exit 1; }
api_token="${GH_TOKEN-}"
[[ -n "$api_token" ]] || fail 'read-only GitHub token is missing'
unset GH_TOKEN
gh_api() { GH_TOKEN="$api_token" @@GH_PREFIX@@ api "$@"; }
[[ "${GITHUB_EVENT_NAME-}" == schedule ]] || fail 'event is not a trusted schedule'
trusted_sha="${GITHUB_SHA-}"
[[ "$trusted_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'scheduled source SHA is malformed'
checkout_sha="$(git rev-parse HEAD)" || fail 'cannot read scheduled checkout SHA'
[[ "$checkout_sha" == "$trusted_sha" ]] || fail 'checkout differs from scheduled source SHA'

repository="${GITHUB_REPOSITORY-}"
[[ "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail 'repository identity is malformed'
repository_json="$(gh_api "repos/$repository")" || fail 'cannot read repository metadata'
default_branch="$(jq -er '.default_branch | select(type == "string" and length > 0)' <<<"$repository_json")" || fail 'default branch is missing'
[[ "${GITHUB_REF-}" == "refs/heads/$default_branch" && "${GITHUB_REF_NAME-}" == "$default_branch" ]] || fail 'schedule is not running on the current default branch'
default_branch_ref="$(jq -rn --arg branch "$default_branch" '$branch | @uri')" || fail 'cannot encode default branch ref'
default_sha="$(gh_api "repos/$repository/commits/$default_branch_ref" --jq '.sha')" || fail 'cannot read current default branch SHA'
[[ "$default_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'current default branch SHA is malformed'

if [[ "$default_sha" != "$trusted_sha" ]]; then
  printf 'should_release=false\ndefault_sha=%s\nresume_release_id=\n' "$trusted_sha" >> "$GITHUB_OUTPUT"
  exit 0
fi

existing_releases="$(gh_api --paginate "repos/$repository/releases?per_page=100" --jq '.[].tag_name')" || fail 'cannot inventory existing releases'
existing_drafts="$(gh_api --paginate "repos/$repository/releases?per_page=100" --jq '.[] | select(.draft == true) | [.tag_name, (.id|tostring)] | @tsv')" || fail 'cannot inventory existing draft releases'
unset api_token
release_file="$(mktemp "$RUNNER_TEMP/binary-release-existing.XXXXXX")" || fail 'cannot create release inventory'; draft_file="$(mktemp "$RUNNER_TEMP/binary-release-drafts.XXXXXX")" || fail 'cannot create draft inventory'; tag_file="$(mktemp "$RUNNER_TEMP/binary-release-tags.XXXXXX")" || fail 'cannot create tag inventory'; ordered_tag_file="$(mktemp "$RUNNER_TEMP/binary-release-ordered-tags.XXXXXX")" || fail 'cannot create ordered tag inventory'; draft_tag_file="$(mktemp "$RUNNER_TEMP/binary-release-draft-tags.XXXXXX")" || fail 'cannot create draft tag inventory'; ordered_draft_tag_file="$(mktemp "$RUNNER_TEMP/binary-release-ordered-draft-tags.XXXXXX")" || fail 'cannot create ordered draft tag inventory'; candidate_tag_file="$(mktemp "$RUNNER_TEMP/binary-release-candidate-tags.XXXXXX")" || fail 'cannot create candidate tag inventory'
printf '%s\n' "$existing_releases" > "$release_file"
printf '%s\n' "$existing_drafts" > "$draft_file"
git for-each-ref --format='%(refname:strip=2)' "refs/tags/@@PACKAGE@@-v*" > "$tag_file" || fail 'cannot inventory release tags'
@@SEMVER_TAG_SORTER@@
awk -F '\t' -v prefix='@@PACKAGE@@-v' 'index($1, prefix) == 1 { print $1 }' "$draft_file" > "$draft_tag_file" || fail 'cannot prepare draft tag inventory'
tag_count="$(wc -l < "$draft_tag_file" | tr -d ' ')" || fail 'cannot count draft tags'
local_tag_file="$tag_file"; ordered_local_tag_file="$ordered_tag_file"
tag_file="$draft_tag_file"; ordered_tag_file="$ordered_draft_tag_file"
@@SEMVER_TAG_SORTER@@
ordered_draft_count="$(wc -l < "$ordered_draft_tag_file" | tr -d ' ')" || fail 'cannot count ordered draft tags'
[[ "$tag_count" == "$ordered_draft_count" ]] || fail 'an existing package draft has an invalid release tag'
awk '!seen[$0]++' "$ordered_draft_tag_file" "$ordered_local_tag_file" > "$candidate_tag_file" || fail 'cannot prioritize pending drafts'
selected_tag=''; selected_version=''; selected_sha=''; selected_resume_id=''
candidate_resume_id=''
reject_draft_candidate() { if [[ -n "$candidate_resume_id" ]]; then fail "existing draft cannot be resumed: $1"; fi; }

while IFS= read -r candidate_tag; do
  [[ -n "$candidate_tag" ]] || continue
  version="${candidate_tag#@@PACKAGE@@-v}"
  [[ "$candidate_tag" == "@@PACKAGE@@-v$version" ]] || continue
  candidate_resume_id=''
  if grep -Fxq -- "$candidate_tag" "$release_file"; then
    draft_ids="$(awk -F '\t' -v tag="$candidate_tag" '$1 == tag { print $2 }' "$draft_file")" || fail 'cannot inspect matching draft release'
    [[ -n "$draft_ids" ]] || continue
    [[ "$(wc -l <<<"$draft_ids" | tr -d ' ')" == 1 ]] || fail 'multiple drafts exist for the same release tag'
    candidate_resume_id="${draft_ids%%$'\n'*}"
    [[ "$candidate_resume_id" =~ ^[0-9]+$ ]] || fail 'existing draft release ID is malformed'
  fi

  tag_ref="refs/tags/$candidate_tag"
  tag_type="$(git cat-file -t "$tag_ref" 2>/dev/null)" || { reject_draft_candidate 'tag is missing locally'; continue; }
  case "$tag_type" in
    commit) candidate_sha="$(git rev-parse "$tag_ref")" || { reject_draft_candidate 'tag target is unreadable'; continue; } ;;
    tag)
      tag_header="$(git cat-file tag "$tag_ref")" || { reject_draft_candidate 'annotated tag is malformed'; continue; }
      target_type="$(sed -n 's/^type //p' <<<"$tag_header" | head -n 1)"
      candidate_sha="$(sed -n 's/^object //p' <<<"$tag_header" | head -n 1)"
      [[ "$target_type" == commit && "$candidate_sha" =~ ^[0-9a-f]{40}$ ]] || { reject_draft_candidate 'annotated tag does not point to a commit'; continue; }
      [[ "$(git cat-file -t "$candidate_sha" 2>/dev/null)" == commit ]] || { reject_draft_candidate 'tag target commit is missing'; continue; }
      ;;
    *) reject_draft_candidate 'tag does not point to a commit'; continue ;;
  esac
  [[ "$candidate_sha" =~ ^[0-9a-f]{40}$ ]] || { reject_draft_candidate 'tag target SHA is malformed'; continue; }
  git merge-base --is-ancestor "$candidate_sha" "$trusted_sha" || { reject_draft_candidate 'tag target is not reachable from the trusted branch'; continue; }

  candidate_root="$(mktemp -d "$RUNNER_TEMP/binary-release-source.XXXXXX")" || fail 'cannot create candidate source directory'
  if ! git archive --format=tar "$candidate_sha" | tar -xf - -C "$candidate_root"; then
    rm -rf -- "$candidate_root"
    reject_draft_candidate 'tagged source cannot be extracted'
    continue
  fi
  # Cargo metadata reads source manifests but does not run build scripts. The
  # read token is unset before parsing any tag-controlled repository content.
  if ! metadata="$(cd "$candidate_root" && env -u GH_TOKEN @@CARGO_METADATA@@)"; then
    rm -rf -- "$candidate_root"
    reject_draft_candidate 'tagged source metadata cannot be read'
    continue
  fi
  candidate_version="$(jq -er --arg package '@@PACKAGE@@' --arg binary '@@BINARY@@' '[.packages[] | select(.name == $package)] as $packages | if ($packages | length) != 1 then error("selected Cargo package must be unique") elif ([$packages[0].targets[] | select(.name == $binary and (.kind | index("bin")))] | length) != 1 then error("selected binary target must be unique") else $packages[0].version end' <<<"$metadata")" || candidate_version=''
  rm -rf -- "$candidate_root"; [[ "$candidate_version" == "$version" ]] || { reject_draft_candidate 'tag does not match its Cargo package version'; continue; }

  selected_tag="$candidate_tag"
  selected_version="$candidate_version"
  selected_sha="$candidate_sha"
  selected_resume_id="$candidate_resume_id"
  break
done < "$candidate_tag_file"; rm -f -- "$release_file" "$draft_file" "$local_tag_file" "$ordered_local_tag_file" "$draft_tag_file" "$ordered_draft_tag_file" "$candidate_tag_file"

if [[ -z "$selected_tag" ]]; then
  printf 'should_release=false\ndefault_sha=%s\nresume_release_id=\n' "$trusted_sha" >> "$GITHUB_OUTPUT"
  exit 0
fi
printf 'should_release=true\nsource_sha=%s\ndefault_sha=%s\nversion=%s\ntag=%s\nresume_release_id=%s\n' \
  "$selected_sha" "$trusted_sha" "$selected_version" "$selected_tag" "$selected_resume_id" >> "$GITHUB_OUTPUT""#;

pub(super) fn verify_source(
    config: &RustBinaryReleaseConfig,
    binary: &str,
    commands: &RenderedCommands,
) -> String {
    let template = VERIFY_SOURCE_TEMPLATE;
    template
        .replace("@@SEMVER_TAG_SORTER@@", SEMVER_TAG_SORTER)
        .replace("@@CARGO_METADATA@@", &commands.metadata)
        .replace("@@GH_PREFIX@@", &commands.gh_prefix)
        .replace("@@PACKAGE@@", &config.package)
        .replace("@@BINARY@@", binary)
}

pub(super) fn verify_build_source(
    config: &RustBinaryReleaseConfig,
    binary: &str,
    commands: &RenderedCommands,
) -> String {
    let template = r#"set -euo pipefail

fail() { printf 'binary release source checkout: %s\n' "$1" >&2; exit 1; }
source_sha="${SOURCE_SHA-}"
tag="${RELEASE_TAG-}"
version="${RELEASE_VERSION-}"
[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'source SHA is malformed'
[[ "$tag" == "@@PACKAGE@@-v$version" ]] || fail 'selected tag and version disagree'
[[ "$(git rev-parse HEAD)" == "$source_sha" ]] || fail 'checkout differs from selected source SHA'
tag_ref="refs/tags/$tag"
tag_type="$(git cat-file -t "$tag_ref")" || fail 'selected tag is missing from checkout'
case "$tag_type" in
  commit) tag_sha="$(git rev-parse "$tag_ref")" ;;
  tag)
    tag_object="$(git cat-file tag "$tag_ref")" || fail 'annotated tag is malformed'
    target_type="$(sed -n 's/^type //p' <<<"$tag_object" | head -n 1)"
    tag_sha="$(sed -n 's/^object //p' <<<"$tag_object" | head -n 1)"
    [[ "$target_type" == commit ]] || fail 'nested or non-commit tags are unsupported'
    ;;
  *) fail 'tag does not point to a commit' ;;
esac
[[ "$tag_sha" == "$source_sha" ]] || fail 'tag target differs from selected source SHA'
metadata="$(@@CARGO_METADATA@@)" || fail 'Cargo metadata failed'
source_version="$(jq -er --arg package '@@PACKAGE@@' --arg binary '@@BINARY@@' '[.packages[] | select(.name == $package)] as $packages | if ($packages | length) != 1 then error("selected Cargo package must be unique") elif ([$packages[0].targets[] | select(.name == $binary and (.kind | index("bin")))] | length) != 1 then error("selected binary target must be unique") else $packages[0].version end' <<<"$metadata")" || fail 'selected Cargo package or binary was not found'
[[ "$source_version" == "$version" ]] || fail 'tag version differs from Cargo package version'"#;
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
