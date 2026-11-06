# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

: "${base:?}" "${repo:?}"

download_artifact() {
  download_id=$1
  destination=$2
  shift 2
  archive_file="$base/download-$download_id.zip"
  (ulimit -f 12582912; gh api "repos/$repo/actions/artifacts/$download_id/zip" > "$archive_file")
  python3 - "$archive_file" "$destination" "$@" <<'PY'
__VELNOR_ARTIFACT_EXTRACTOR__
PY
}
