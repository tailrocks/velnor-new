for digest in "$VELNOR_MANIFEST_DIGEST" "$VELNOR_CHECKSUMS_DIGEST" "$VELNOR_LICENSE_DIGEST" "$VELNOR_NOTICE_DIGEST"; do
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]] || family_fail 'verified asset digest is malformed'
done
draft="$(family_gh api "repos/$release_repository/releases/tags/$release_tag")"
jq -e --arg tag "$release_tag" \
  --arg manifest "$VELNOR_MANIFEST_DIGEST" --arg checksums "$VELNOR_CHECKSUMS_DIGEST" \
  --arg license "$VELNOR_LICENSE_DIGEST" --arg notice "$VELNOR_NOTICE_DIGEST" \
  '.tag_name == $tag and .draft == true and
   ([.assets[] | {name, digest}] | sort_by(.name)) ==
   ([{name:"RUNNER_IMAGE_MANIFEST.json",digest:$manifest}, {name:"SHA256SUMS",digest:$checksums},
     {name:"LICENSE",digest:$license}, {name:"NOTICE",digest:$notice}] | sort_by(.name))' \
  <<<"$draft" >/dev/null || family_fail 'draft release assets differ from qualified digests'
