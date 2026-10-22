set -euo pipefail
: "${GH_TOKEN:?}" "${VELNOR_UPGRADE_DIR:?}" "${VELNOR_GH_VERSION:?}"
# Resolve the newest stable velnor-actions release and verify it before any
# job runs it: the release manifest and the Linux binary must each carry a
# build-provenance attestation from Velnor's generator release workflow on
# `main` at the manifest's commit, and the binary must match the manifest's
# SHA-256. Runs before any repository code; the read-only `github.token` it
# holds never reaches a later step.
velnor_gh() { mise --no-config --no-env --no-hooks exec "gh@$VELNOR_GH_VERSION" -- gh "$@"; }
repository=tailrocks/velnor-new
signer="$repository/.github/workflows/product-release-generator.yml"
out=$VELNOR_UPGRADE_DIR/velnor
rm -rf "$out"
mkdir -p "$out/bin"
fail() { printf '::error::%s: %s\n' "$1" "$2"; exit 1; }
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}
attested() {
  velnor_gh attestation verify "$1" --bundle "$1.intoto.jsonl" --repo "$repository" \
    --signer-workflow "$signer" --source-ref refs/heads/main \
    --source-digest "$2" --signer-digest "$2" >/dev/null \
    || fail upgrade_velnor_provenance_unverified "$(basename "$1") has no valid $signer attestation at $2"
}
tag=$(velnor_gh api "repos/$repository/releases?per_page=100" --jq '
  [ .[] | select(.draft == false and .prerelease == false
      and (.tag_name | test("^v[0-9]+[.][0-9]+[.][0-9]+$"))) ]
  | sort_by(.tag_name | ltrimstr("v") | split(".") | map(tonumber))
  | last | .tag_name // empty')
[ -n "$tag" ] || fail upgrade_velnor_release_missing "no stable velnor-actions release in $repository"
version=${tag#v}
velnor_gh release download "$tag" --repo "$repository" --dir "$out" \
  --pattern release-manifest.json --pattern release-manifest.json.intoto.jsonl
manifest=$out/release-manifest.json
jq -e --arg version "$version" '.schema == 1 and .repository == "tailrocks/velnor-new" and .version == $version and (.commit | test("^[0-9a-f]{40}$"))' "$manifest" >/dev/null \
  || fail upgrade_velnor_manifest_invalid "$tag release manifest failed validation"
commit=$(jq -r .commit "$manifest")
attested "$manifest" "$commit"
name=velnor-actions-$version-x86_64-unknown-linux-gnu
asset=$(jq -r '[.targets[] | select(.target == "x86_64-unknown-linux-gnu")] | if length == 1 then .[0].artifact else empty end' "$manifest")
digest=$(jq -r '[.targets[] | select(.target == "x86_64-unknown-linux-gnu")] | if length == 1 then .[0].sha256 else empty end' "$manifest")
[ "$asset" = "https://github.com/$repository/releases/download/$tag/$name" ] || fail upgrade_velnor_asset_untrusted "$asset"
case "$digest" in *[!0-9a-f]*|'') fail upgrade_velnor_manifest_invalid "$tag Linux digest" ;; esac
[ "${#digest}" = 64 ] || fail upgrade_velnor_manifest_invalid "$tag Linux digest"
velnor_gh release download "$tag" --repo "$repository" --dir "$out" --pattern "$name" --pattern "$name.intoto.jsonl"
[ "$(sha256_of "$out/$name")" = "$digest" ] || fail upgrade_velnor_digest_mismatch "$name"
attested "$out/$name" "$commit"
mv "$out/$name" "$out/bin/velnor-actions"
chmod 755 "$out/bin/velnor-actions"
echo "velnor: verified velnor-actions $version ($commit)"
