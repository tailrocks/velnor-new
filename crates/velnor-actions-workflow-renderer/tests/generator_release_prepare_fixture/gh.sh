#!/bin/bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
case "$1" in
  api)
    endpoint="${@: -1}"
    case "$endpoint" in
      */releases\?per_page=100) cat "$FAKE_GH_RELEASES" ;;
      */git/ref/tags/*) cat "$FAKE_GH_TAG_REF" ;;
      *) printf 'unexpected fixture GitHub API request: %s\n' "$endpoint" >&2; exit 2 ;;
    esac
    ;;
  release)
    case "$2" in
      verify) exit 0 ;;
      download)
        shift 2
        destination=
        patterns=()
        while (($#)); do
          case "$1" in
            --dir) destination="$2"; shift 2 ;;
            --pattern) patterns+=("$2"); shift 2 ;;
            *) shift ;;
          esac
        done
        mkdir -p "$destination"
        source_dir="${destination##*/}"
        for pattern in "${patterns[@]}"; do
          cp "$FAKE_RELEASE_ASSETS/$source_dir/$pattern" "$destination/$pattern"
        done
        ;;
      verify-asset)
        test -s "$4"
        ;;
      *) printf 'unexpected fixture release request: %s\n' "$2" >&2; exit 2 ;;
    esac
    ;;
  attestation)
    [[ "$2" == verify ]] || exit 2
    subject="$3"
    if [[ "${FAKE_GH_REFUSE_ATTESTATION:-}" == 1 ]]; then exit 1; fi
    if [[ -n "${FAKE_GH_REFUSE_ATTESTATION_SUBJECT:-}" && "$subject" == "$FAKE_GH_REFUSE_ATTESTATION_SUBJECT" ]]; then exit 1; fi
    source_digest=
    source_ref=
    signer_workflow=
    signer_digest=
    bundle=
    shift 3
    while (($#)); do
      case "$1" in
        --source-digest) source_digest="$2"; shift 2 ;;
        --source-ref) source_ref="$2"; shift 2 ;;
        --signer-workflow) signer_workflow="$2"; shift 2 ;;
        --signer-digest) signer_digest="$2"; shift 2 ;;
        --bundle) bundle="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    [[ "$source_digest" == "$GITHUB_SHA" ]]
    [[ "$source_ref" == refs/heads/main ]]
    [[ "$signer_workflow" == "$GITHUB_REPOSITORY/.github/workflows/product-release-generator.yml" ]]
    [[ "$signer_digest" == "$GITHUB_SHA" ]]
    test -s "$subject"
    test -s "$bundle"
    ;;
  *) printf 'unexpected fixture GitHub command: %s\n' "$1" >&2; exit 2 ;;
esac
