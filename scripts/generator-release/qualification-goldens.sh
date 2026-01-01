#!/usr/bin/env bash
# Candidate release manifest verification and qualification parity helpers.

file_sha256() {
  local path="$1" output
  if command -v sha256sum >/dev/null 2>&1; then
    output="$(sha256sum "$path")" || return 1
  elif command -v shasum >/dev/null 2>&1; then
    output="$(shasum -a 256 "$path")" || return 1
  else
    echo "FATAL: no SHA-256 utility is available" >&2
    return 1
  fi
  printf '%s\n' "$output" | awk 'NR == 1 && length($1) == 64 && $1 !~ /[^0-9a-f]/ { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
}

host_target() {
  case "$(uname -s):$(uname -m)" in
    Linux:x86_64|Linux:amd64) echo "x86_64-unknown-linux-gnu" ;;
    Darwin:arm64|Darwin:aarch64) echo "aarch64-apple-darwin" ;;
    *) return 1 ;;
  esac
}

validate_candidate_manifest() {
  local target version source_sha checkout_sha candidate_sha manifest_sha manifest_file_sha
  if [ ! -f "$CANDIDATE_MANIFEST" ] || [ -L "$CANDIDATE_MANIFEST" ]; then
    echo "FATAL: candidate manifest must be a regular non-symlink file: $CANDIDATE_MANIFEST" >&2
    exit 2
  fi
  if [[ ! "$CANDIDATE_MANIFEST_SHA256" =~ ^[0-9a-f]{64}$ ]]; then
    echo "FATAL: expected candidate manifest SHA-256 must be 64 lowercase hexadecimal characters" >&2
    exit 2
  fi
  if ! manifest_file_sha="$(file_sha256 "$CANDIDATE_MANIFEST")"; then
    echo "FATAL: could not compute candidate manifest SHA-256" >&2
    exit 2
  fi
  if [ "$manifest_file_sha" != "$CANDIDATE_MANIFEST_SHA256" ]; then
    echo "FATAL: candidate manifest bytes do not match expected SHA-256" >&2
    exit 2
  fi
  if ! jq -e . "$CANDIDATE_MANIFEST" >/dev/null 2>&1; then
    echo "FATAL: candidate manifest is malformed JSON" >&2
    exit 2
  fi
  if [ "${GITHUB_REPOSITORY-unset}" != "tailrocks/velnor-new" ]; then
    echo "FATAL: GITHUB_REPOSITORY must be tailrocks/velnor-new" >&2
    exit 2
  fi
  source_sha="${GITHUB_SHA:-}"
  if [[ ! "$source_sha" =~ ^[0-9a-f]{40}$ ]]; then
    echo "FATAL: GITHUB_SHA must be a lowercase 40-character source SHA" >&2
    exit 2
  fi
  if ! checkout_sha="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null)" || [ "$checkout_sha" != "$source_sha" ]; then
    echo "FATAL: checked-out source does not match GITHUB_SHA" >&2
    exit 2
  fi
  if ! manifest_sha="$(jq -er '.commit | strings' "$CANDIDATE_MANIFEST" 2>/dev/null)" \
    || [ "$manifest_sha" != "$source_sha" ]; then
    echo "FATAL: candidate manifest source does not match GITHUB_SHA" >&2
    exit 2
  fi
  if ! version="$("$BIN" --version | awk 'NR == 1 && NF == 2 && $1 == "velnor-actions" { print $2; next } { exit 1 } END { if (NR != 1) exit 1 }')"; then
    echo "FATAL: could not read candidate CLI version" >&2
    exit 2
  fi
  if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "FATAL: candidate CLI version is malformed: $version" >&2
    exit 2
  fi
  if ! jq -e --arg version "$version" --arg repository "$GITHUB_REPOSITORY" '
    .schema == 1 and .version == $version and .repository == $repository and
    (.targets | length) == 2 and
    [.targets[].target] == ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] and
    all(.targets[];
      (.sha256 | type == "string" and test("^[0-9a-f]{64}$")) and
      .artifact == ("https://github.com/" + $repository + "/releases/download/v" +
        $version + "/velnor-actions-" + $version + "-" + .target))
  ' "$CANDIDATE_MANIFEST" >/dev/null 2>&1; then
    echo "FATAL: candidate manifest does not match the release schema or CLI version" >&2
    exit 2
  fi
  if ! target="$(host_target)"; then
    echo "FATAL: candidate qualification does not support host $(uname -s)/$(uname -m)" >&2
    exit 2
  fi
  if ! candidate_sha="$(file_sha256 "$BIN")"; then
    echo "FATAL: could not compute candidate CLI SHA-256" >&2
    exit 2
  fi
  manifest_sha="$(jq -er --arg target "$target" '[.targets[] | select(.target == $target) | .sha256] | if length == 1 then .[0] else error("missing or duplicate host target") end' "$CANDIDATE_MANIFEST" 2>/dev/null)" \
    || { echo "FATAL: candidate manifest has no unique digest for $target" >&2; exit 2; }
  if [ "$manifest_sha" != "$candidate_sha" ]; then
    echo "FATAL: candidate manifest digest does not match candidate CLI for $target" >&2
    exit 2
  fi
}

stage_candidate_manifest() {
  local repo="$1" context="$2" staged_sha
  if [ ! -f "$CANDIDATE_MANIFEST" ] || [ -L "$CANDIDATE_MANIFEST" ]; then
    echo "FATAL: candidate manifest must be a regular non-symlink file: $CANDIDATE_MANIFEST" >&2
    exit 2
  fi
  if ! mkdir -p "$repo/.velnor" \
    || ! cp "$CANDIDATE_MANIFEST" "$repo/.velnor/release-manifest.json"; then
    echo "FATAL: could not stage exact candidate manifest bytes for $context" >&2
    exit 2
  fi
  if ! staged_sha="$(file_sha256 "$repo/.velnor/release-manifest.json")" \
    || [ "$staged_sha" != "$CANDIDATE_MANIFEST_SHA256" ] \
    || ! cmp -s "$CANDIDATE_MANIFEST" "$repo/.velnor/release-manifest.json"; then
    echo "FATAL: could not stage exact candidate manifest bytes for $context" >&2
    exit 2
  fi
}

capture_release_case() {
  local case="$1" repo="$2" out="$3" preview="$3/preview"
  mkdir -p "$out" "$preview"
  (cd "$repo" && "$BIN" generate --output-dir "$preview" >"$out/stdout.txt" 2>"$out/stderr.txt") \
    || { echo "FATAL: release candidate generate failed for $case"; exit 2; }
  if [ ! -d "$preview/.github" ]; then
    echo "FATAL: release candidate emitted no workflows for $case"
    exit 2
  fi
  if ! normalize_candidate_digests "$preview"; then
    echo "FATAL: could not normalize candidate asset digests for $case"
    exit 2
  fi
  hash_tree "$preview" "$out/tree.sha256"
}

normalize_candidate_digests() {
  local preview="$1" digest file normalized placeholder
  placeholder="$(printf '%064d' 0 | tr '0' 'a')"
  while IFS= read -r digest; do
    if [[ ! "$digest" =~ ^[0-9a-f]{64}$ ]]; then
      echo "FATAL: candidate manifest contains a malformed asset digest" >&2
      return 1
    fi
    while IFS= read -r file; do
      normalized="$file.normalized"
      if ! sed -e "s/$digest/$placeholder/g" "$file" >"$normalized" \
        || ! mv "$normalized" "$file"; then
        rm -f "$normalized"
        echo "FATAL: could not normalize candidate manifest digest in $file" >&2
        return 1
      fi
    done < <(find "$preview/.github" -type f -print)
  done < <(jq -r '.targets[].sha256' "$CANDIDATE_MANIFEST")
}

capture_release_dogfood() {
  local out="$1" repo="$WORK/dogfood-repo" preview="$1/preview" diff_status
  mkdir -p "$repo" "$out" "$preview"
  if ! git -C "$ROOT" archive --format=tar HEAD | tar -xf - -C "$repo"; then
    echo "FATAL: could not stage committed dogfood source"
    exit 2
  fi
  stage_candidate_manifest "$repo" dogfood
  (
    cd "$repo" \
      && git -c init.defaultBranch=main init -q \
      && git remote add origin https://github.com/tailrocks/velnor-new.git \
      && git add -A \
      && GIT_AUTHOR_NAME=v GIT_AUTHOR_EMAIL=v@v GIT_COMMITTER_NAME=v GIT_COMMITTER_EMAIL=v@v \
         GIT_AUTHOR_DATE='2026-01-01T00:00:00Z' GIT_COMMITTER_DATE='2026-01-01T00:00:00Z' \
         git -c commit.gpgsign=false commit -qm "golden"
  ) >/dev/null 2>&1 || { echo "FATAL: dogfood git setup failed"; exit 2; }
  if ! (cd "$repo" && "$BIN" generate --output-dir "$preview" >"$out/stdout.txt" 2>"$out/stderr.txt"); then
    echo "FATAL: release candidate dogfood generate failed"
    exit 2
  fi
  if [ ! -d "$preview/.github" ]; then
    echo "FATAL: release candidate dogfood emitted no workflows"
    exit 2
  fi
  if (cd "$repo" && diff -r .github "$preview/.github") >"$out/dogfood.diff" 2>&1; then
    echo "identical" >"$out/dogfood.verdict"
  else
    diff_status=$?
    if [ "$diff_status" -eq 1 ]; then
      cat "$out/dogfood.diff" >&2
      echo "FATAL: release candidate dogfood differs from committed .github"
      exit 2
    fi
    echo "FATAL: dogfood tree comparison failed (diff status $diff_status)"
    exit 2
  fi
  if ! link_identity "$repo/.github" >"$out/committed.links" \
    || ! link_identity "$preview/.github" >"$out/generated.links" \
    || ! diff "$out/committed.links" "$out/generated.links" >/dev/null; then
    echo "FATAL: release candidate dogfood symlink tree differs from committed .github"
    exit 2
  fi
  hash_tree "$preview" "$out/tree.sha256"
  note "release dogfood parity: all committed .github files match"
}
