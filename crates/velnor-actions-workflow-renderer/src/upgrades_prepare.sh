set -euo pipefail
: "${VELNOR_UPGRADE_DIR:?}" "${GITHUB_OUTPUT:?}"
dir=$VELNOR_UPGRADE_DIR
rm -rf "$dir"
mkdir -p "$dir/out" "$dir/bin"
printf '%s\n' "$VELNOR_CATALOG_BUMPER_SOURCE" > "$dir/bump-gradle-catalog.py"
summary=$dir/out/summary.md
failures=$dir/failures.md
: > "$dir/changes.md"
: > "$failures"
changed() { printf -- '- %s\n' "$1" >> "$dir/changes.md"; printf 'velnor: %s\n' "$1"; }
failed() { printf -- '- %s\n' "$1" >> "$failures"; printf '::warning::%s\n' "$1"; }
fetch() { curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error --location --retry 3 --output "$1" "$2"; }
sha256_check() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum -c "$1" >/dev/null; else shasum -a 256 -c "$1" >/dev/null; fi
}
newer() { [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1)" = "$2" ]; }

# 1. Download the latest published Velnor release manifest during this upgrade job.
# Do not use the latest-release endpoint during workflow generation.
manifest=.velnor/release-manifest.json
fetch "$dir/release-manifest.json" https://github.com/tailrocks/velnor-new/releases/latest/download/release-manifest.json
jq -e '.schema == 1 and .repository == "tailrocks/velnor-new" and (.version | test("^[0-9]+[.][0-9]+[.][0-9]+$")) and (.commit | test("^[0-9a-f]{40}$"))' "$dir/release-manifest.json" >/dev/null \
  || { echo "::error::upgrade_velnor_manifest_invalid: The latest tailrocks/velnor-new release manifest failed validation."; exit 1; }
current_velnor=$(jq -r .version "$manifest")
latest_velnor=$(jq -r .version "$dir/release-manifest.json")
if newer "$current_velnor" "$latest_velnor"; then
  cp "$dir/release-manifest.json" "$manifest"
  changed "velnor-actions $current_velnor -> $latest_velnor"
fi
version=$(jq -r .version "$manifest")
asset=$(jq -r '.targets[] | select(.target == "x86_64-unknown-linux-gnu") | .artifact' "$manifest")
digest=$(jq -r '.targets[] | select(.target == "x86_64-unknown-linux-gnu") | .sha256' "$manifest")
case "$asset" in
  "https://github.com/tailrocks/velnor-new/releases/download/v$version/velnor-actions-$version-x86_64-unknown-linux-gnu") ;;
  *) echo "::error::upgrade_velnor_asset_untrusted: $asset"; exit 1 ;;
esac
fetch "$dir/bin/velnor-actions" "$asset"
printf '%s  %s\n' "$digest" "$dir/bin/velnor-actions" > "$dir/velnor.sha256"
sha256_check "$dir/velnor.sha256" || { echo "::error::upgrade_velnor_digest_mismatch: $asset"; exit 1; }
chmod 755 "$dir/bin/velnor-actions"

# 2. Update other Mise tools before Rust.
# Keep the Rust update separate so it can be reversed if the catalog has no digest.
mise_files=$(git ls-files -- mise.toml .mise.toml mise.lock .mise.lock)
# rust-toolchain files mirror the Mise Rust version (owner layout); they move
# with it and roll back with it.
toolchain_files=$(git ls-files -- rust-toolchain.toml '*/rust-toolchain.toml' rust-toolchain '*/rust-toolchain')
rust_files="$mise_files $toolchain_files"
mirror_rust() {
  new_rust=$(mise ls --local --json | jq -r '(.rust // .["core:rust"] // [])[0].requested_version // empty')
  [ -n "$new_rust" ] || return 0
  for file in $toolchain_files; do
    case "$file" in
      *.toml) sed -i -E "s/^([[:space:]]*channel[[:space:]]*=[[:space:]]*\")[0-9]+[.][0-9]+[.][0-9]+(\")/\1$new_rust\2/" "$file" ;;
      *)
        if grep -q '^[[:space:]]*\[' "$file"; then
          sed -i -E "s/^([[:space:]]*channel[[:space:]]*=[[:space:]]*\")[0-9]+[.][0-9]+[.][0-9]+(\")/\1$new_rust\2/" "$file"
        else
          printf '%s\n' "$new_rust" > "$file"
        fi
        ;;
    esac
  done
}
lock_mise() {
  if git ls-files --error-unmatch mise.lock >/dev/null 2>&1; then
    mise lock --platform linux-x64,linux-arm64,macos-arm64
  fi
}
if [ -n "$mise_files" ]; then
  mise trust --yes . >/dev/null
  before=$(mise ls --local --json)
  tools=$(printf '%s' "$before" | jq -r 'keys[] | select(. != "rust" and . != "core:rust")')
  if [ -n "$tools" ]; then
    # shellcheck disable=SC2086 # one tool name per word
    if mise upgrade --bump $tools && lock_mise; then :; else failed "mise upgrade --bump failed for: $(printf '%s' "$tools" | tr '\n' ' ')"; git checkout -- $mise_files; fi
  fi
  mkdir -p "$dir/pre-rust"
  for file in $rust_files; do mkdir -p "$dir/pre-rust/$(dirname "$file")"; cp "$file" "$dir/pre-rust/$file"; done
  if printf '%s' "$before" | jq -e 'has("rust") or has("core:rust")' >/dev/null; then
    if mise upgrade --bump rust && lock_mise && mirror_rust; then :; else failed "The command mise upgrade --bump rust failed."; for file in $rust_files; do cp "$dir/pre-rust/$file" "$file"; done; fi
  fi
  after=$(mise ls --local --json)
  printf '%s\n%s\n' "$before" "$after" | jq -rs '
    (.[0] | map_values(.[0].requested_version // .[0].version)) as $old
    | (.[1] | map_values(.[0].requested_version // .[0].version)) as $new
    | $new | to_entries[] | select($old[.key] != .value)
    | "\(.key) \($old[.key] // "new") -> \(.value)"' | while IFS= read -r line; do changed "$line"; done
fi

# 3. Update each tracked Cargo lock file, including incompatible and pinned requirements.
cargo_roots=$(git ls-files -- Cargo.lock '*/Cargo.lock' | xargs -r -n1 dirname | sort -u)
if [ -n "$cargo_roots" ]; then
  mise install "cargo:cargo-edit@$VELNOR_CARGO_EDIT_VERSION" >/dev/null
  for root in $cargo_roots; do
    manifest_path="$root/Cargo.toml"
    cp "$root/Cargo.lock" "$dir/Cargo.lock.before"
    if mise exec "cargo:cargo-edit@$VELNOR_CARGO_EDIT_VERSION" -- cargo upgrade --incompatible --pinned --manifest-path "$manifest_path" \
      && mise exec -- cargo update --manifest-path "$manifest_path" \
      && mise exec -- cargo metadata --locked --format-version 1 --manifest-path "$manifest_path" >/dev/null; then
      if ! cmp -s "$dir/Cargo.lock.before" "$root/Cargo.lock"; then changed "Cargo dependencies in $root"; fi
    else
      failed "The Cargo upgrade in $root failed. Private Git sources require a fetch credential. Resolve the failure manually."
      git diff --name-only -- "$root" | grep -E '(^|/)Cargo[.](toml|lock)$' | xargs -r git checkout --
    fi
  done
fi

# 4. Update each tracked bun.lock with lifecycle scripts disabled.
for root in $(git ls-files -- bun.lock '*/bun.lock' | xargs -r -n1 dirname | sort -u); do
  if (cd "$root" && mise exec -- bun update --latest --ignore-scripts); then
    if ! git diff --quiet -- "$root/package.json" "$root/bun.lock"; then changed "npm dependencies in $root"; fi
  else
    failed "The Bun update in $root failed. Private npm scopes require a registry credential. Resolve the failure manually."
    git checkout -- "$root/package.json" "$root/bun.lock"
  fi
done

# 5. Update referenced Gradle catalog versions from Maven Central or the Gradle Plugin Portal.
# Then refresh the build lock files and verification data.
for catalog in $(git ls-files -- 'gradle/libs.versions.toml' '*/gradle/libs.versions.toml'); do
  if python3 -I "$dir/bump-gradle-catalog.py" "$catalog" >> "$dir/changes.md"; then :; else
    failed "The Gradle catalog $catalog could not be updated."; git checkout -- "$catalog"; continue
  fi
  root=$(dirname "$(dirname "$catalog")")
  if git diff --quiet -- "$catalog"; then continue; fi
  if [ -x "$root/gradlew" ]; then
    (cd "$root" && ./gradlew --no-daemon --quiet --write-locks --write-verification-metadata sha256 dependencies) \
      || failed "The Gradle lock file and verification refresh in $root failed. Run it manually."
  fi
done

# 6. Regenerate .github/ (and every generated file) with the selected Velnor.
generate() { "$dir/bin/velnor-actions" generate > "$dir/generate.log" 2>&1; }
if ! generate; then
  if grep -q 'rust_channel_manifest_not_catalogued' "$dir/generate.log" && [ -d "$dir/pre-rust" ]; then
    for file in $rust_files; do cp "$dir/pre-rust/$file" "$file"; done
    failed "The latest Rust version has no channel manifest digest in velnor-actions $version. Keep the previous Rust pin until a Velnor release adds the digest."
    generate || { cat "$dir/generate.log"; failed "The command velnor-actions generate failed. The .github/ files have not been regenerated."; }
  else
    cat "$dir/generate.log"
    failed "velnor-actions generate failed; .github/ is not regenerated"
  fi
fi

# 7. Keep only paths permitted for upgrades. Report and exclude other paths.
git add -A
git diff --cached --name-only > "$dir/paths"
: > "$dir/rejected"
while IFS= read -r path; do
  case "$path" in
    mise.toml|.mise.toml|mise.lock|.mise.lock|.velnor/release-manifest.json|.github/*) ;;
    rust-toolchain.toml|*/rust-toolchain.toml|rust-toolchain|*/rust-toolchain) ;;
    Cargo.toml|Cargo.lock|*/Cargo.toml|*/Cargo.lock) ;;
    package.json|bun.lock|*/package.json|*/bun.lock) ;;
    gradle/libs.versions.toml|*/gradle/libs.versions.toml|gradle.lockfile|*/gradle.lockfile|gradle/verification-metadata.xml|*/gradle/verification-metadata.xml) ;;
    *) printf '%s\n' "$path" >> "$dir/rejected" ;;
  esac
done < "$dir/paths"
if [ -s "$dir/rejected" ]; then
  while IFS= read -r path; do git reset --quiet -- "$path"; failed "The upgrade excludes a change outside the permitted paths: $path."; done < "$dir/rejected"
fi
{
  echo "Velnor updates supported pins without a cooldown. The sections below list changes and failures."
  echo
  echo "## Changes"
  if [ -s "$dir/changes.md" ]; then cat "$dir/changes.md"; else echo "- regenerated files only"; fi
  if [ -s "$failures" ]; then echo; echo "## Needs migration"; cat "$failures"; fi
} > "$summary"
if git diff --cached --quiet; then
  echo "changed=false" >> "$GITHUB_OUTPUT"
  cat "$summary"
  exit 0
fi
git diff --cached --binary > "$dir/out/upgrade.patch"
echo "changed=true" >> "$GITHUB_OUTPUT"
cat "$summary"
