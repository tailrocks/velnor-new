set -euo pipefail
: "${VELNOR_UPGRADE_DIR:?}" "${GITHUB_OUTPUT:?}" "${VELNOR_GENERATE_TOOLS:?}"
# Prepare ran repository code and fresh third-party releases, so nothing it
# produced is trusted here. This job regenerates `.github/` itself from the
# default branch plus the release manifest it verified, with the binary it
# verified, and accepts the patch only when prepare's `.github/` is exactly
# that regeneration. No prepared byte is executed or read as configuration.
dir=$VELNOR_UPGRADE_DIR
patch=$dir/upgrade.patch
fail() { printf '::error::%s: %s\n' "$1" "$2"; exit 1; }
velnor_upgrade_check_patch_applies "$patch" "$dir/check.index" || exit 1
prepared=$(GIT_INDEX_FILE=$dir/check.index git write-tree)
# The only Velnor the patch may select is the one verified above.
if git cat-file -e "$prepared:.velnor/release-manifest.json" 2>/dev/null; then
  git cat-file blob "$prepared:.velnor/release-manifest.json" > "$dir/prepared-manifest.json"
  cmp -s "$dir/prepared-manifest.json" "$dir/velnor/release-manifest.json" \
    || fail upgrade_velnor_manifest_unverified "the patch selects a Velnor release this job did not verify"
else
  fail upgrade_velnor_manifest_unverified ".velnor/release-manifest.json is missing"
fi
# Regenerate from trusted inputs only: the default branch checkout, which
# hard decision 4 keeps version-free in `.github/`, plus the verified manifest.
cp "$dir/velnor/release-manifest.json" .velnor/release-manifest.json
velnor_generate_prerequisites
"$dir/velnor/bin/velnor-actions" generate > "$dir/generate.log" 2>&1 \
  || { cat "$dir/generate.log"; fail upgrade_regenerate_failed "velnor-actions generate failed on the default branch"; }
git add -A -- .github .velnor/release-manifest.json
expected=$(git write-tree)
subtree() { git rev-parse --verify --quiet "$1:$2" || echo absent; }
if [ "$(subtree "$expected" .github)" != "$(subtree "$prepared" .github)" ]; then
  git diff --no-renames --stat "$expected" "$prepared" -- .github
  fail upgrade_patch_github_mismatch "prepare's .github/ differs from this job's regeneration; nothing is published"
fi
# `workflows: write` only when the push really changes a workflow file.
workflows=
if [ "$(subtree HEAD .github/workflows)" != "$(subtree "$prepared" .github/workflows)" ]; then workflows='write'; fi
sha=$(if command -v sha256sum >/dev/null 2>&1; then sha256sum "$patch"; else shasum -a 256 "$patch"; fi | cut -d' ' -f1)
{
  echo "tree=$prepared"
  echo "patch_sha256=$sha"
  echo "workflows=$workflows"
} >> "$GITHUB_OUTPUT"
echo "velnor: the patch's .github/ is exactly the verified regeneration (tree $prepared)"
