# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
case "$1 $2" in
  'api --paginate') cat "$FIXTURE_RELEASES" ;;
  'api repos/tailrocks/velnor-new/git/ref/tags/'*)
    printf '{"object":{"type":"commit","sha":"%s"}}\n' "$GITHUB_SHA"
    ;;
  'release verify'|'release verify-asset') exit 0 ;;
  'release download')
    shift 3
    directory=''
    patterns=()
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --repo) shift 2 ;;
        --dir) directory="$2"; shift 2 ;;
        --pattern) patterns+=("$2"); shift 2 ;;
        *) exit 82 ;;
      esac
    done
    for name in "${patterns[@]}"; do
      source_path="$(find "$FIXTURE_ASSETS" -type f -name "$name")"
      [[ -f "$source_path" ]] || exit 83
      cp "$source_path" "$directory/$name"
    done
    ;;
  'attestation verify')
    case "$3" in
      linux-assets/velnor-actions-*-x86_64-unknown-linux-gnu)
        printf 'external attestation rejection: passive subject is not authenticated\n' >&2
        exit 17
        ;;
      *) exit 0 ;;
    esac
    ;;
  *) printf 'unsupported external GH fixture request\n' >&2; exit 84 ;;
esac
