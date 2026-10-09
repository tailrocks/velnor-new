#!/bin/bash
set -euo pipefail
printf '%s\n' "$*" >> "$GH_LOG"
case "$1" in
  attestation)
    if [[ "$2" == download ]]; then
      digest="$(shasum -a 256 "$3" | awk '{print $1}')"
      printf '{"fixture":true}\n' > "sha256:${digest}.jsonl"
    elif [[ "$PUBLISHER_SCENARIO" == final-attestation-failed && "$*" == *consumer-binary-release-download* ]]; then
      echo 'attestation mismatch' >&2
      exit 1
    fi
    exit 0
    ;;
  api)
    endpoint=""
    for arg in "$@"; do [[ "$arg" != repos/* ]] || endpoint="$arg"; done
    case "$endpoint" in
      */immutable-releases)
        [[ "$GH_TOKEN" == "$IMMUTABILITY_READ_TOKEN" ]]
        [[ "$IMMUTABLE_ENABLED" == true ]] && printf '{"enabled":true,"enforced_by_owner":false}\n' || printf '{"enabled":false,"enforced_by_owner":false}\n'
        ;;
      */environments/consumer-binary-release)
        if [[ "$ENVIRONMENT_MISSING" == true ]]; then
          echo 'Not Found (HTTP 404)' >&2
          exit 1
        fi
        if [[ "$PROTECTED_ENVIRONMENT" == true ]]; then
          printf '{"name":"consumer-binary-release","protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"login":"reviewer"}}],"prevent_self_review":true}],"deployment_branch_policy":{"protected_branches":true,"custom_branch_policies":false}}\n'
        else
          printf '{"name":"consumer-binary-release","protection_rules":[],"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":false}}\n'
        fi
        ;;
      */git/ref/tags/*)
        if [[ -f "$GH_LOG.tag-created" || "$TAG_COLLISION" == true ]]; then
          printf '{"ref":"refs/tags/%s","object":{"sha":"%s"}}\n' "$EXPECTED_TAG" "$EXPECTED_SOURCE_SHA"
        elif [[ "$NON_404" == true ]]; then
          echo 'Forbidden (HTTP 403)' >&2
          exit 1
        else
          echo 'Not Found (HTTP 404)' >&2
          exit 1
        fi
        ;;
      */releases/tags/*)
        if [[ "$RELEASE_COLLISION" == true ]]; then
          printf '{"tag_name":"%s"}\n' "$EXPECTED_TAG"
        elif [[ "$NON_404" == true ]]; then
          echo 'Forbidden (HTTP 403)' >&2
          exit 1
        else
          echo 'Not Found (HTTP 404)' >&2
          exit 1
        fi
        ;;
      */git/refs)
        : > "$GH_LOG.tag-created"
        printf '{"ref":"refs/tags/%s","object":{"sha":"%s"}}\n' "$EXPECTED_TAG" "$EXPECTED_SOURCE_SHA"
        ;;
      *) echo "unexpected API endpoint: $endpoint" >&2; exit 1 ;;
    esac
    ;;
  release)
    case "$2" in
      create) ;;
      edit) ;;
      view)
        if [[ "$PUBLISHER_SCENARIO" == extra-release-asset ]]; then
          printf '{"tagName":"%s","isDraft":false,"isImmutable":%s,"assets":[{"name":"%s"},{"name":"SHA256SUMS"},{"name":"release.json"},{"name":"unexpected.txt"}]}\n' \
            "$EXPECTED_TAG" "$FINAL_IMMUTABLE" "$ASSET_NAME"
        else
          printf '{"tagName":"%s","isDraft":false,"isImmutable":%s,"assets":[{"name":"%s"},{"name":"SHA256SUMS"},{"name":"release.json"}]}\n' \
            "$EXPECTED_TAG" "$FINAL_IMMUTABLE" "$ASSET_NAME"
        fi
        ;;
      download)
        destination=""
        while (($#)); do
          if [[ "$1" == --dir ]]; then destination="$2"; shift 2; else shift; fi
        done
        mkdir -p "$destination"
        cp "$ASSET_NAME" SHA256SUMS release.json "$destination/"
        if [[ "$PUBLISHER_SCENARIO" == download-extra-file ]]; then
          printf 'unexpected asset\n' > "$destination/unexpected.txt"
        elif [[ "$PUBLISHER_SCENARIO" == download-corrupt-bytes ]]; then
          printf 'tampered\n' >> "$destination/$ASSET_NAME"
        elif [[ "$PUBLISHER_SCENARIO" == download-extra-checksum ]]; then
          shasum -a 256 release.json >> "$destination/SHA256SUMS"
        fi
        ;;
      *) echo "unexpected release command: $2" >&2; exit 1 ;;
    esac
    ;;
  *) echo "unexpected gh command: $1" >&2; exit 1 ;;
esac
