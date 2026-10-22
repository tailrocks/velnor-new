set -euo pipefail
: "${VELNOR_UPGRADE_DIR:?}" "${GITHUB_OUTPUT:?}"
dir=$VELNOR_UPGRADE_DIR
rm -rf "$dir/out"
mkdir -p "$dir/out"
printf '%s\n' "$VELNOR_CATALOG_BUMPER_SOURCE" > "$dir/bump-gradle-catalog.py"
summary=$dir/out/summary.md
failures=$dir/failures.md
: > "$dir/changes.md"
: > "$failures"
changed() { printf -- '- %s\n' "$1" >> "$dir/changes.md"; printf 'velnor: %s\n' "$1"; }
failed() { printf -- '- %s\n' "$1" >> "$failures"; printf '::warning::%s\n' "$1"; }
newer() { [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1)" = "$2" ]; }

# 1. Velnor itself: the newest stable release, verified (digest and build
# provenance) by the previous step before any repository code ran. The patch
# may select only that release, and verify regenerates with it.
manifest=.velnor/release-manifest.json
verified=$dir/velnor/release-manifest.json
current_velnor=$(jq -r .version "$manifest")
latest_velnor=$(jq -r .version "$verified")
if newer "$latest_velnor" "$current_velnor"; then
  echo "::error::upgrade_velnor_pinned_ahead: $manifest pins velnor-actions $current_velnor, newer than the latest stable release $latest_velnor"
  exit 1
fi
if ! cmp -s "$verified" "$manifest"; then
  cp "$verified" "$manifest"
  changed "velnor-actions $current_velnor -> $latest_velnor"
fi
version=$latest_velnor

# 2. Mise tools other than Rust, then Rust (kept separate so a Rust release
# the new Velnor catalog does not know yet can be rolled back alone).
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
    if mise upgrade --bump rust && lock_mise && mirror_rust; then :; else failed "mise upgrade --bump rust failed"; for file in $rust_files; do cp "$dir/pre-rust/$file" "$file"; done; fi
  fi
  after=$(mise ls --local --json)
  printf '%s\n%s\n' "$before" "$after" | jq -rs '
    (.[0] | map_values(.[0].requested_version // .[0].version)) as $old
    | (.[1] | map_values(.[0].requested_version // .[0].version)) as $new
    | $new | to_entries[] | select($old[.key] != .value)
    | "\(.key) \($old[.key] // "new") -> \(.value)"' | while IFS= read -r line; do changed "$line"; done
fi

# 3. Cargo: every tracked lockfile, incompatible and pinned requirements too.
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
      failed "Cargo upgrade in $root failed (private git sources need the fetch credential; resolve by hand)"
      git diff --name-only -- "$root" | grep -E '(^|/)Cargo[.](toml|lock)$' | xargs -r git checkout --
    fi
  done
fi

# 4. npm through Bun: every tracked bun.lock, lifecycle scripts off.
for root in $(git ls-files -- bun.lock '*/bun.lock' | xargs -r -n1 dirname | sort -u); do
  if (cd "$root" && mise exec -- bun update --latest --ignore-scripts); then
    if ! git diff --quiet -- "$root/package.json" "$root/bun.lock"; then changed "npm dependencies in $root"; fi
  else
    failed "bun update --latest in $root failed (private npm scopes need the registry credential; resolve by hand)"
    git checkout -- "$root/package.json" "$root/bun.lock"
  fi
done

# 5. Gradle version catalogs: [versions] entries referenced by libraries and
# plugins move to the newest stable release on Maven Central or the Gradle
# Plugin Portal; then the build refreshes its locks and verification data.
for catalog in $(git ls-files -- 'gradle/libs.versions.toml' '*/gradle/libs.versions.toml'); do
  if python3 -I "$dir/bump-gradle-catalog.py" "$catalog" >> "$dir/changes.md"; then :; else
    failed "Gradle catalog $catalog could not be bumped"; git checkout -- "$catalog"; continue
  fi
  root=$(dirname "$(dirname "$catalog")")
  if git diff --quiet -- "$catalog"; then continue; fi
  if [ -x "$root/gradlew" ]; then
    (cd "$root" && ./gradlew --no-daemon --quiet --write-locks --write-verification-metadata sha256 dependencies) \
      || failed "Gradle lock and verification refresh in $root failed; run it by hand"
  fi
done

# 6. Regenerate .github/ (and every generated file) with the selected Velnor.
generate() { velnor_generate_prerequisites > "$dir/generate.log" 2>&1 && "$dir/velnor/bin/velnor-actions" generate >> "$dir/generate.log" 2>&1; }
if ! generate; then
  if grep -q 'rust_channel_manifest_not_catalogued' "$dir/generate.log" && [ -d "$dir/pre-rust" ]; then
    for file in $rust_files; do cp "$dir/pre-rust/$file" "$file"; done
    failed "the newest Rust is not in velnor-actions $version's channel-manifest catalog yet; Rust stays pinned until a Velnor release adds it"
    generate || { cat "$dir/generate.log"; failed "velnor-actions generate failed; .github/ is not regenerated"; }
  else
    cat "$dir/generate.log"
    failed "velnor-actions generate failed; .github/ is not regenerated"
  fi
fi

# 7. Keep only what the patch filter admits (upgrade-owned regular files,
# no rename, mode change, symlink, foreign deletion or binary); anything
# else is unstaged and reported, not shipped.
git add -A
velnor_upgrade_check_index drop > "$dir/rejected"
while read -r code path; do failed "unexpected change left out of the upgrade ($code): $path"; done < "$dir/rejected"
{
  echo "Velnor upgrades every pin to the latest stable release (no cooldown)."
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
git diff --cached --no-renames --full-index --no-ext-diff --no-textconv > "$dir/out/upgrade.patch"
# Self-check: publish runs exactly this filter again before it trusts a byte.
velnor_upgrade_check_patch_applies "$dir/out/upgrade.patch" "$dir/check.index" || exit 1
echo "changed=true" >> "$GITHUB_OUTPUT"
cat "$summary"
