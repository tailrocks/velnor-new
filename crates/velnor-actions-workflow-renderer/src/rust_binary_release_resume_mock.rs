//! Stateful mock GitHub CLI used by the existing-release safety regression.

pub(super) const MOCK_GH: &str = r#"#!/bin/sh
set -eu
if [ "$1" = api ]; then
  shift
  method=GET; endpoint=; jq_expr=; form=; paginate=false; slurp=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --paginate) paginate=true; shift ;;
      --slurp) slurp=true; shift ;;
      --jq) jq_expr="$2"; shift 2 ;;
      --method) method="$2"; shift 2 ;;
      -F) form="$2"; shift 2 ;;
      *) if [ -z "$endpoint" ]; then endpoint="$1"; fi; shift ;;
    esac
  done
  case "$endpoint" in
    "repos/$MOCK_REPOSITORY") printf '{"default_branch":"main"}\n' ;;
    "repos/$MOCK_REPOSITORY/commits/main")
      if [ "$jq_expr" = .sha ]; then printf '%s\n' "$MOCK_DEFAULT_SHA"; else printf '{"sha":"%s"}\n' "$MOCK_DEFAULT_SHA"; fi ;;
    "repos/$MOCK_REPOSITORY/git/ref/tags/$MOCK_TAG")
      printf '{"ref":"refs/tags/%s","object":{"type":"commit","sha":"%s"}}\n' "$MOCK_TAG" "$MOCK_SOURCE_SHA" ;;
    "repos/$MOCK_REPOSITORY/compare/$MOCK_SOURCE_SHA...$MOCK_DEFAULT_SHA")
      printf '{"status":"ahead"}\n' ;;
    "repos/$MOCK_REPOSITORY/releases?per_page=100")
      if [ "${GH_TOKEN-}" = read-only-test-token ]; then
        if [ "$paginate" != true ] || [ "$jq_expr" != '.[].tag_name' ]; then exit 82; fi
        if [ -f "$MOCK_RELEASE_STATE" ] && jq -e '.draft == false' "$MOCK_RELEASE_STATE" >/dev/null; then
          jq -r '.tag_name' "$MOCK_RELEASE_STATE"
          printf 'release-list-read:published\n' >> "$MOCK_LOG"
        else
          printf 'release-list-read:hidden-draft\n' >> "$MOCK_LOG"
        fi
      elif [ "${GH_TOKEN-}" = write-test-token ]; then
        if [ "$paginate" != true ] || [ "$slurp" != true ] || [ -n "$jq_expr" ]; then exit 82; fi
        if [ -f "$MOCK_RELEASE_STATE" ]; then
          if [ "${MOCK_DUPLICATE_DRAFT_PAGES-false}" = true ]; then
            jq -s '[[], ., .]' "$MOCK_RELEASE_STATE"
            printf 'release-list-write:duplicate-draft-later-pages\n' >> "$MOCK_LOG"
          else
            jq -s '[[], .]' "$MOCK_RELEASE_STATE"
            printf 'release-list-write:draft-later-page\n' >> "$MOCK_LOG"
          fi
        else
          printf '[[]]\n'
          printf 'release-list-write:empty\n' >> "$MOCK_LOG"
        fi
      else
        exit 89
      fi ;;
    "repos/$MOCK_REPOSITORY/releases/$MOCK_RELEASE_ID")
      if [ "$method" = PATCH ]; then
        [ "$form" = draft=false ] || exit 83
        jq '.draft = false' "$MOCK_RELEASE_STATE" > "$MOCK_RELEASE_STATE.tmp"
        mv "$MOCK_RELEASE_STATE.tmp" "$MOCK_RELEASE_STATE"
        printf 'patch\n' >> "$MOCK_LOG"
      else
        printf 'draft-detail:%s\n' "$MOCK_RELEASE_ID" >> "$MOCK_LOG"
      fi
      cat "$MOCK_RELEASE_STATE" ;;
    *) printf 'unexpected API endpoint: %s\n' "$endpoint" >&2; exit 84 ;;
  esac
  exit 0
fi
[ "$1" = release ] || exit 85
shift
case "$1" in
  create)
    shift
    tag="$1"
    linux_asset=
    for arg in "$@"; do
      case "$arg" in *-x86_64-unknown-linux-gnu.tar.gz) linux_asset="$arg" ;; esac
    done
    [ -n "$linux_asset" ] || exit 86
    digest="$(sha256sum "$linux_asset" | cut -d ' ' -f 1)"
    size="$(wc -c < "$linux_asset" | tr -d ' ')"
    jq -cn --arg tag "$tag" --arg source "$MOCK_SOURCE_SHA" \
      --arg body "Automated binary release for $tag." --arg asset "$linux_asset" \
      --arg digest "sha256:$digest" --argjson size "$size" \
      '{id:987,tag_name:$tag,name:$tag,target_commitish:$source,draft:true,prerelease:false,body:$body,assets:[{name:$asset,digest:$digest,size:$size}]}' \
      > "$MOCK_RELEASE_STATE"
    printf 'create-left-partial-draft\n' >> "$MOCK_LOG"
    exit 1 ;;
  upload)
    shift
    tag="$1"; asset="$2"
    [ "$tag" = "$MOCK_TAG" ] || exit 87
    digest="$(sha256sum "$asset" | cut -d ' ' -f 1)"
    size="$(wc -c < "$asset" | tr -d ' ')"
    jq --arg name "$asset" --arg digest "sha256:$digest" --argjson size "$size" \
      '.assets += [{name:$name,digest:$digest,size:$size}]' "$MOCK_RELEASE_STATE" > "$MOCK_RELEASE_STATE.tmp"
    mv "$MOCK_RELEASE_STATE.tmp" "$MOCK_RELEASE_STATE"
    printf 'upload:%s\n' "$asset" >> "$MOCK_LOG" ;;
  *) exit 88 ;;
esac
"#;
