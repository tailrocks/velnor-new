pub(super) const MISE_STUB: &str = r#"#!/bin/sh
set -eu
[ "$1" = "--no-config" ] && shift
[ "$1" = "--no-env" ] && shift
[ "$1" = "--no-hooks" ] && shift
[ "$1" = "exec" ] && shift
[ "$1" = "gh@2.102.0" ] && shift
[ "$1" = "--" ] && shift
[ "$1" = "gh" ] && shift
exec gh "$@"
"#;

pub(super) const TIMEOUT_STUB: &str = r"#!/bin/sh
# Poison: the gh wrapper is a portable watchdog and must never invoke timeout.
echo 'poison: timeout must never be invoked' >&2
exit 99
";

pub(super) const GIT_STUB: &str = r#"#!/bin/sh
set -eu
if [ "$1" = rev-parse ] && [ "$2" = HEAD ]; then printf '%s\n' "$GITHUB_SHA"; exit 0; fi
exec /usr/bin/git "$@"
"#;

pub(super) const GH_STUB: &str = r#"#!/bin/sh
set -eu
printf 'gh %s\n' "$*" >> "$VELNOR_TEST_CALLS"
if [ "$1" = api ]; then
  shift
  endpoint=
  for argument do endpoint="$argument"; done
  case " $* " in *" -X POST "*)
    [ "$endpoint" = "repos/tailrocks/velnor-new/git/refs" ] || exit 64
    case " $* " in *" ref=refs/tags/$VELNOR_TEST_TAG "*) ;; *) exit 65 ;; esac
    case " $* " in *" sha=$VELNOR_TEST_SOURCE "*) ;; *) exit 66 ;; esac
    : > "$VELNOR_TEST_ROOT/tag-created"
    printf '{"ref":"refs/tags/%s","object":{"type":"commit","sha":"%s"}}\n' "$VELNOR_TEST_TAG" "$VELNOR_TEST_SOURCE"
    exit 0
    ;;
  esac
  case "$endpoint" in
    *"/commits/main")
      cat "$VELNOR_TEST_ROOT/main.json"
      [ "$VELNOR_TEST_FAIL" != main-read ] || exit 83
      ;;
    *"/workflows/ci.yml/runs?"*)
      if [ "$VELNOR_TEST_MODE" = attempt-changed ]; then
        reads=0
        [ -f "$VELNOR_TEST_ROOT/run-reads" ] && reads=$(cat "$VELNOR_TEST_ROOT/run-reads")
        reads=$((reads + 1))
        printf '%s\n' "$reads" > "$VELNOR_TEST_ROOT/run-reads"
        if [ "$reads" -ge 2 ]; then
          cat "$VELNOR_TEST_ROOT/runs-changed.json"
        else
          cat "$VELNOR_TEST_ROOT/runs.json"
        fi
      else
        cat "$VELNOR_TEST_ROOT/runs.json"
      fi
      [ "$VELNOR_TEST_FAIL" != ci-read ] || exit 84
      ;;
    *"/attempts/1/jobs?per_page=100")
      cat "$VELNOR_TEST_ROOT/jobs.json"
      [ "$VELNOR_TEST_FAIL" != required-read ] || exit 85
      ;;
    *"/releases?per_page=100")
      cat "$VELNOR_TEST_ROOT/releases.json"
      [ "$VELNOR_TEST_FAIL" != release-list-read ] || exit 86
      ;;
    *"/matching-refs/tags/"*)
      if [ "$VELNOR_TEST_MODE" = draft-fails ] && [ -f "$VELNOR_TEST_ROOT/tag-created" ]; then
        printf '[[{"ref":"refs/tags/%s"}]]\n' "$VELNOR_TEST_TAG"
      else
        cat "$VELNOR_TEST_ROOT/refs.json"
      fi
      [ "$VELNOR_TEST_FAIL" != matching-refs-read ] || exit 87
      ;;
    *"/git/ref/tags/"*)
      [ -f "$VELNOR_TEST_ROOT/tag-created" ] || exit 67
      cat "$VELNOR_TEST_ROOT/tag.json"
      [ "$VELNOR_TEST_FAIL" != tag-read ] || exit 88
      ;;
    *"/releases/tags/"*)
      [ -f "$VELNOR_TEST_ROOT/published" ] || exit 68
      cat "$VELNOR_TEST_ROOT/published.json"
      ;;
    *) printf 'unexpected api endpoint: %s\n' "$endpoint" >&2; exit 69 ;;
  esac
  exit 0
fi

command="$1"
shift
subcommand="$1"
shift
case "$command/$subcommand" in
  release/download)
    [ "$VELNOR_TEST_FAIL" != download ] || exit 79
    shift
    directory=.
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --repo) shift 2 ;;
        --dir) directory="$2"; shift 2 ;;
        --pattern)
          asset="$2"
          mkdir -p "$directory"
          case "$asset" in
            SHA256SUMS)
              : > "$directory/SHA256SUMS"
              for member in $VELNOR_TEST_ASSETS; do
                [ "$member" = SHA256SUMS ] && continue
                case "$member" in *.sha256) continue ;; esac
                [ -f "$directory/$member" ] || printf 'fixture bytes: %s' "$member" > "$directory/$member"
                if [ "$VELNOR_TEST_FAIL" = checksum ]; then
                  printf '%064d  %s\n' 0 "$member" >> "$directory/SHA256SUMS"
                else
                  (cd "$directory" && shasum -a 256 "$member") >> "$directory/SHA256SUMS"
                fi
              done
              ;;
            *.sha256)
              member="${asset%.sha256}"
              printf 'fixture bytes: %s' "$member" > "$directory/$member"
              if [ "$VELNOR_TEST_FAIL" = checksum ]; then
                printf '%064d  %s\n' 0 "$member" > "$directory/$asset"
              else
                (cd "$directory" && shasum -a 256 "$member") > "$directory/$asset"
              fi
              ;;
            *) printf 'fixture bytes: %s' "$asset" > "$directory/$asset" ;;
          esac
          shift 2
          ;;
        *) shift ;;
      esac
    done
    ;;
  release/verify)
    [ "$VELNOR_TEST_FAIL" != release-verify ] || exit 80
    ;;
  release/verify-asset)
    [ "$VELNOR_TEST_FAIL" != asset-verify ] || exit 81
    ;;
  attestation/verify)
    [ "$VELNOR_TEST_FAIL" != attestation-verify ] || exit 82
    ;;
  release/create)
    [ -f "$VELNOR_TEST_ROOT/tag-created" ] || exit 70
    case " $* " in *" --verify-tag "*) ;; *) exit 71 ;; esac
    case " $* " in *" --draft "*) ;; *) exit 71 ;; esac
    [ "$VELNOR_TEST_MODE" != draft-fails ] || exit 75
    : > "$VELNOR_TEST_ROOT/draft"
    ;;
  release/upload)
    [ -f "$VELNOR_TEST_ROOT/draft" ] || exit 72
    ;;
  release/edit)
    [ -f "$VELNOR_TEST_ROOT/draft" ] || exit 73
    : > "$VELNOR_TEST_ROOT/published"
    ;;
  *) printf 'unexpected gh command: %s/%s\n' "$command" "$subcommand" >&2; exit 74 ;;
esac
"#;
