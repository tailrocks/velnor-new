#!/usr/bin/env bash

readonly VELNOR_RELEASE_REPOSITORY='tailrocks/velnor-new'
readonly VELNOR_RELEASE_GH_VERSION='2.102.0'

fail() {
  printf 'generator release: %s\n' "$1" >&2
  exit 1
}

pinned_gh() {
  timeout --foreground 600s mise --no-config --no-env --no-hooks \
    exec "gh@$VELNOR_RELEASE_GH_VERSION" -- gh "$@"
}

api_get() {
  local path="$1" body="$2"
  [[ -n "${GH_TOKEN-}" ]] || fail 'GitHub API token is missing'
  if ! API_STATUS="$(curl --connect-timeout 5 --max-time 20 --max-filesize 4194304 \
    --silent --show-error --output "$body" --write-out '%{http_code}' \
    --header 'Accept: application/vnd.github+json' \
    --header "Authorization: Bearer ${GH_TOKEN}" \
    --header 'X-GitHub-Api-Version: 2022-11-28' \
    "https://api.github.com/repos/${VELNOR_RELEASE_REPOSITORY}/${path}")"; then
    fail "GitHub API request failed: $path"
  fi
}

require_api_object() {
  local path="$1" body="$2"
  api_get "$path" "$body"
  [[ "$API_STATUS" == '200' ]] || fail "GitHub API returned HTTP $API_STATUS: $path"
  jq -e 'type == "object"' "$body" >/dev/null \
    || fail "malformed GitHub API response: $path"
}

require_absent() {
  local path="$1" body="$2"
  api_get "$path" "$body"
  if [[ "$API_STATUS" == '404' ]]; then
    jq -e 'type == "object" and .message == "Not Found" and (.status == "404" or .status == 404)' \
      "$body" >/dev/null || fail "unconfirmed 404 response: $path"
    return
  fi
  [[ "$API_STATUS" == '200' ]] && fail "release identity already exists: $path"
  fail "GitHub API returned HTTP $API_STATUS: $path"
}

is_sha() {
  [[ "${#1}" -eq 40 && "$1" != *[!0123456789abcdef]* ]]
}

is_hex_digest() {
  [[ "${#1}" -eq 64 && "$1" != *[!0123456789abcdef]* ]]
}

verify_sidecar() {
  local directory="$1" sidecar="$2" binary="$3" digest name extra
  [[ -s "$directory/$binary" && -s "$directory/$sidecar" ]] \
    || fail "missing release asset: $binary"
  (cd "$directory" && sha256sum --check --status "$sidecar") \
    || fail "checksum mismatch: $binary"
  [[ "$(wc -l < "$directory/$sidecar" | tr -d ' ')" == '1' ]] \
    || fail "malformed checksum sidecar: $sidecar"
  read -r digest name extra < "$directory/$sidecar" \
    || fail "malformed checksum sidecar: $sidecar"
  [[ -z "${extra:-}" && "$name" == "$binary" ]] \
    || fail "unexpected checksum filename: $sidecar"
  is_hex_digest "$digest" || fail "malformed checksum digest: $sidecar"
  printf '%s' "$digest"
}

file_digest() {
  sha256sum -- "$1" | awk '{print $1}'
}

file_size() {
  wc -c < "$1" | tr -d ' '
}

verify_bundle() {
  local path="$1"
  [[ -s "$path" ]] || fail "missing provenance bundle: $path"
  jq -e '(.mediaType | type == "string" and startswith("application/vnd.dev.sigstore.bundle+json"))
    and (.verificationMaterial | type == "object")
    and (.dsseEnvelope | type == "object")' "$path" >/dev/null \
    || fail "malformed provenance bundle: $path"
}
