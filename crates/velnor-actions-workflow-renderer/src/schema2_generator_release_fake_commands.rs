use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const MOCK_GH: &str = r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$GH_CALLS"
if [ "$1" = attestation ] && [ "$2" = verify ]; then exit 0; fi
if [ "$1" = api ]; then
  shift
  if [ "$1" = --paginate ] && [ "$2" = --slurp ]; then
    case "$3" in
      *actions/workflows/ci.yml/runs*)
        printf '[{"workflow_runs":[{"id":91,"run_number":9,"run_attempt":1,"path":".github/workflows/ci.yml","head_sha":"%s","head_branch":"main","head_repository":{"full_name":"tailrocks/velnor-new"},"event":"push","status":"completed","conclusion":"success"}]}]\n' "$GITHUB_SHA" ;;
      *actions/runs/91/attempts/1/jobs*)
        printf '[{"jobs":[{"name":"Required","head_sha":"%s","head_branch":"main","status":"completed","conclusion":"success"}]}]\n' "$GITHUB_SHA" ;;
      *) exit 43 ;;
    esac
    exit 0
  fi
  if [ "$1" = --include ]; then
    case "$GH_PREFLIGHT" in
      404) printf 'HTTP/2 404\r\n\r\n{"message":"Not Found","status":"404"}\n'; exit 1 ;;
      401) printf 'HTTP/2 401\r\n\r\n{"message":"Bad credentials","status":"401"}\n'; exit 1 ;;
      403) printf 'HTTP/2 403\r\n\r\n{"message":"Forbidden","status":"403"}\n'; exit 1 ;;
      transient) exit 22 ;;
    esac
  fi
  case "$1" in
    repos/tailrocks/velnor-new/commits/main)
      if [ "$2" = --jq ] && [ "$3" = .sha ]; then printf '%s\n' "$GITHUB_SHA"; else printf '{"sha":"%s"}\n' "$GITHUB_SHA"; fi ;;
    repos/tailrocks/velnor-new/git/refs)
      test "$2 $3" = '--method POST'
      printf '{"ref":"refs/tags/v0.1.1","object":{"type":"commit","sha":"%s"}}\n' "$GITHUB_SHA" ;;
    repos/tailrocks/velnor-new/git/ref/tags/v0.1.1)
      printf '{"object":{"type":"commit","sha":"%s"}}\n' "$GITHUB_SHA" ;;
    repos/tailrocks/velnor-new/releases/123)
      if [ "${2:-} ${3:-}" = '--method PATCH' ]; then
        test "$4 $5" = '-F draft=false'
        printf '%s\n' "$*" > "$GH_PATCH_LOG"
        printf 'published\n' > "$GH_STATE"
      elif [ "$(cat "$GH_STATE")" = draft ]; then
        cat "$GH_DRAFT_JSON"
      else
        cat "$GH_RELEASE_JSON"
      fi ;;
    *) exit 44 ;;
  esac
  exit 0
fi
if [ "$1 $2" = 'release create' ]; then
  test "$3" = v0.1.1
  printf '%s\n' "$3" > "$GH_CREATE_TAG"
  printf 'draft\n' > "$GH_STATE"
  shift 3
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --repo) test "$2" = tailrocks/velnor-new; shift 2 ;;
      --verify-tag|--latest=false|--draft) shift ;;
      --title) test "$2" = 'velnor-actions v0.1.1'; shift 2 ;;
      --notes) test "$2" = "velnor-actions 0.1.1 built from $GITHUB_SHA."; shift 2 ;;
      *) exit 45 ;;
    esac
  done
  exit 0
fi
if [ "$1 $2" = 'release view' ]; then
  test "$3" = v0.1.1
  test "$4 $5" = '--repo tailrocks/velnor-new'
  test "$6 $7" = '--json databaseId,tagName,isDraft'
  printf '{"databaseId":123,"tagName":"v0.1.1","isDraft":true}\n'
  exit 0
fi
if [ "$1 $2" = 'release upload' ]; then
  test "$3" = v0.1.1
  test "$4 $5" = '--repo tailrocks/velnor-new'
  shift 5
  for path in "$@"; do test -s "$path"; done
  printf '%s\n' "$@" > "$GH_ASSET_ARGS"
  exit 0
fi
exit 46
"#;

pub(super) fn install_mock_gh(root: &Path) -> Result<(), Box<dyn Error>> {
    let bin = root.join("mock-bin");
    fs::create_dir_all(&bin)?;
    let mock = bin.join("gh");
    fs::write(&mock, MOCK_GH)?;
    let mut permissions = fs::metadata(&mock)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(mock, permissions)?;
    let git = bin.join("git");
    fs::write(
        &git,
        "#!/bin/sh\nset -eu\nif [ \"$1\" = rev-parse ] && [ \"$2\" = HEAD ]; then printf '%s\\n' \"$GITHUB_SHA\"; exit 0; fi\nexec /usr/bin/git \"$@\"\n",
    )?;
    let mut permissions = fs::metadata(&git)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(git, permissions)?;
    super::cli_tests::install_mock_mise(root)?;
    Ok(())
}
