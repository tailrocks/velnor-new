set -euo pipefail
: "${VELNOR_UPGRADE_DIR:?}" "${VELNOR_VERIFIED_TREE:?}" "${VELNOR_VERIFIED_PATCH_SHA256:?}"
# Runs before the App token exists: the patch must be byte-for-byte the one
# the verify job regenerated against, pass the filter again, and apply to
# exactly the tree that job accepted.
dir=$VELNOR_UPGRADE_DIR
patch=$dir/upgrade.patch
fail() { printf '::error::%s: %s\n' "$1" "$2"; exit 1; }
test -s "$patch" || fail upgrade_patch_unverified "the upgrade patch is missing"
sha=$(if command -v sha256sum >/dev/null 2>&1; then sha256sum "$patch"; else shasum -a 256 "$patch"; fi | cut -d' ' -f1)
[ "$sha" = "$VELNOR_VERIFIED_PATCH_SHA256" ] || fail upgrade_patch_unverified "the downloaded patch is not the patch the verify job checked"
velnor_upgrade_check_patch_applies "$patch" "$dir/check.index" || exit 1
git -c core.hooksPath=/dev/null apply --index "$patch"
velnor_upgrade_check_index strict || exit 1
[ "$(git write-tree)" = "$VELNOR_VERIFIED_TREE" ] || fail upgrade_patch_unverified "the applied tree is not the tree the verify job accepted"
echo "velnor: the patch is the verified upgrade (tree $VELNOR_VERIFIED_TREE)"
