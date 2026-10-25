#!/usr/bin/env bash
set -euo pipefail

image_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
readonly image_dir
repo_root="$(git -C "$image_dir" rev-parse --show-toplevel)"
readonly repo_root
lock="$repo_root/.velnor/runner.lock"
dockerfile="$image_dir/Dockerfile"
readme="$image_dir/README.md"

fail() {
  printf '%s\n' "$1" >&2
  exit 1
}

validate_lock_syntax() {
  awk '
    /^[[:space:]]*(#.*)?$/ { next }
    {
      line = $0
      sub(/^[[:space:]]+/, "", line)
      sub(/[[:space:]]+$/, "", line)
      if (split(line, pair, " = ") != 2 || pair[1] !~ /^[A-Za-z0-9_]+$/ || seen[pair[1]]++) {
        exit 1
      }
      value = pair[2]
      if (value ~ /^".*"$/ || value ~ /^[0-9]+$/) {
        next
      }
      if (value ~ /^\[.*\]$/) {
        list = substr(value, 2, length(value) - 2)
        if (list == "") {
          next
        }
        count = split(list, items, ",")
        for (item_index = 1; item_index <= count; item_index++) {
          item = items[item_index]
          sub(/^[[:space:]]+/, "", item)
          sub(/[[:space:]]+$/, "", item)
          if (item !~ /^".*"$/) {
            exit 1
          }
        }
        next
      }
      exit 1
    }
  ' "$lock" || fail ".velnor/runner.lock is not valid restricted TOML."
}

lock_value() {
  awk -v key="$1" '
    $1 == key && $2 == "=" {
      count++
      value = substr($0, index($0, "=") + 2)
      if (value !~ /^".*"$/) {
        exit 2
      }
      value = substr(value, 2, length(value) - 2)
    }
    END {
      if (count != 1) {
        exit 1
      }
      print value
    }
  ' "$lock"
}

validate_lock_syntax

locked() {
  local key="$1"
  local expected="$2"
  local actual
  actual="$(lock_value "$key")" || fail ".velnor/runner.lock has no unique string field '$key'."
  [[ "$actual" == "$expected" ]] || fail ".velnor/runner.lock has an unexpected value for '$key'."
}

has() {
  grep -Fq -- "$1" "$2" || fail "$3"
}

base="$(lock_value base_image)" || fail ".velnor/runner.lock has no unique base image."
base_name="$(lock_value base_image_name)" || fail ".velnor/runner.lock has no unique base image name."
base_digest="$(lock_value base_image_digest)" || fail ".velnor/runner.lock has no unique base image digest."
[[ "$base_name" =~ ^ubuntu:26\.04$ ]] || fail ".velnor/runner.lock has an incorrect Ubuntu image identity."
[[ "$base_digest" =~ ^sha256:[0-9a-f]{64}$ ]] || fail ".velnor/runner.lock has an incorrect Ubuntu image digest."
[[ "$base" == "$base_name@$base_digest" ]] || fail "The base image is not pinned to the complete image reference in .velnor/runner.lock."
locked base_image_platform linux/amd64
[[ "$(grep -Fxc "FROM --platform=linux/amd64 $base" "$dockerfile")" == 1 ]] \
  || fail "Dockerfile must use the pinned amd64 Ubuntu image."

docker_version="$(lock_value docker_version)" || fail ".velnor/runner.lock has no unique Docker version."
[[ "$docker_version" == 29.9.1 ]] || fail "The Docker version in .velnor/runner.lock changed. Examine the job image."

artifact_names=("Docker client" Compose Buildx Node.js)
artifact_urls=("$(lock_value docker_archive_url)" "$(lock_value compose_asset_url)" "$(lock_value buildx_asset_url)" "$(lock_value node_archive_url)")
artifact_hashes=("$(lock_value docker_sha256)" "$(lock_value compose_sha256)" "$(lock_value buildx_sha256)" "$(lock_value node_sha256)")
artifact_paths=(/tmp/docker.tgz /usr/local/lib/docker/cli-plugins/docker-compose /usr/local/lib/docker/cli-plugins/docker-buildx /tmp/node.tar.gz)
for index in "${!artifact_names[@]}"; do
  name="${artifact_names[$index]}"
  url="${artifact_urls[$index]}"
  sha256="${artifact_hashes[$index]}"
  destination="${artifact_paths[$index]}"
  [[ "$sha256" =~ ^[0-9a-f]{64}$ ]] || fail "The $name SHA-256 in .velnor/runner.lock has an incorrect format."
  has "$url" "$dockerfile" "Dockerfile does not contain the pinned $name URL."
  has "$sha256" "$dockerfile" "Dockerfile does not contain the pinned $name SHA-256."
  digest_target="${sha256} \\"$'\n        '"${destination}"
  has "$digest_target" "$dockerfile" "The $name checksum has an incorrect file path in Dockerfile."
done
[[ "$(grep -Fc 'sha256sum --check --status' "$dockerfile")" == "${#artifact_names[@]}" ]] \
  || fail "Dockerfile must compare each artifact's SHA-256 with its pinned SHA-256."

locked_checks=(
  "[[ \"\$(docker --version)\" == 'Docker version $docker_version, build '* ]]"
  "[[ \"\$(docker compose version)\" == *'v$(lock_value compose_version)'* ]]"
  "[[ \"\$(docker buildx version)\" == *'v$(lock_value buildx_version)'* ]]"
  "[[ \"\$(node --version)\" == v$(lock_value node_version) ]]"
  "tar -xzf /tmp/docker.tgz --strip-components=1 -C /usr/local/bin docker/docker"
  "tar -xzf /tmp/node.tar.gz --strip-components=1 -C /usr/local"
  "rm -f /tmp/docker.tgz"
  "rm -f /tmp/node.tar.gz"
  "[[ \"\$(tar --version)\" == 'tar (GNU tar)'* ]]"
)
for expected in "${locked_checks[@]}"; do
  has "$expected" "$dockerfile" "Dockerfile does not contain a necessary tool command: $expected"
done

packages=(bash ca-certificates coreutils curl git gzip netcat-openbsd python3 tar unzip xz-utils zstd)
install_section="$(awk '
  /apt-get install/ { in_install = 1; next }
  in_install && /rm -rf \/var\/lib\/apt\/lists/ { exit }
  in_install { print }
' "$dockerfile")"
for package in "${packages[@]}"; do
  grep -Eq "^[[:space:]]+$package([[:space:]]|$)" <<<"$install_section" \
    || fail "Dockerfile does not contain necessary job package: $package"
done

lower="$(tr '[:upper:]' '[:lower:]' < "$dockerfile")"
for forbidden in actions-runner runner.listener runner-home jitconfig /velnor/ resource-probe storage-init dockerd containerd node24; do
  grep -Fq -- "$forbidden" <<<"$lower" && fail "The job Dockerfile has Runner or controller data: $forbidden"
done

grep -Fq 'Runner tools at `/__e`' "$readme" || fail "README must give the official Runner mount path /__e."
runner_version="$(lock_value runner_version)" || fail ".velnor/runner.lock has no unique Runner version."
runner_commit="$(lock_value runner_source_commit)" || fail ".velnor/runner.lock has no unique Runner source commit."
[[ "$runner_version" == 2.338.0 ]] || fail "README must give the pinned official Runner source."
grep -Fq "$runner_commit" "$readme" || fail "README must give the pinned official Runner source."
grep -Fq 'build-test.sh` compares the Dockerfile with `.velnor/runner.lock`' "$readme" \
  || fail "README must give the source test procedure."

printf 'PASS: Job image source agrees with .velnor/runner.lock.\n'
