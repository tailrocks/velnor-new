use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::super::SOURCE_SHA;

pub(crate) fn write_stock_restore_fixtures(
    root: &std::path::Path,
    runner_temp: &std::path::Path,
    shared: &str,
    new: &str,
) {
    fs::write(
        runner_temp.join("event.json"),
        r#"{"inputs":{"mode":"mbx-cache-parallel"}}"#,
    )
    .expect("write workflow-dispatch fixture");
    fs::write(
        root.join("stock-run.json"),
        format!(
            r#"{{"id":123,"run_attempt":2,"head_sha":"{SOURCE_SHA}","repository":{{"full_name":"tailrocks/velnor-new"}},"head_repository":{{"full_name":"tailrocks/velnor-new"}},"event":"workflow_dispatch","head_branch":"main","path":".github/workflows/qualification.yml@refs/heads/main","workflow_id":456}}"#
        ),
    )
    .expect("write workflow run fixture");
    let restore = r#"[{"name":"Restore MBX single bundle","status":"completed","conclusion":"success"}]"#;
    let jobs = [
        stock_job_record("MBX parallel / seed", 11, "completed", "success", restore),
        stock_job_record("MBX parallel / reader-a", 12, "completed", "success", restore),
        stock_job_record("MBX parallel / reader-b", 13, "completed", "success", restore),
        stock_job_record(
            "MBX parallel / new-key-writer",
            14,
            "completed",
            "success",
            restore,
        ),
        stock_job_record(
            "MBX parallel / observer-shared",
            15,
            "in_progress",
            "",
            restore,
        ),
    ]
    .join(",");
    fs::write(
        root.join("stock-jobs.json"),
        format!(r#"{{"total_count":5,"jobs":[{jobs}]}}"#),
    )
    .expect("write stock jobs fixture");
    for (file, key, body) in [
        ("seed.log", shared, "Cache not found for input keys"),
        ("writer.log", new, "Cache not found for input keys"),
        ("reader-a.log", shared, "Cache restored from key"),
        ("reader-b.log", shared, "Cache restored from key"),
        ("observer-shared.log", shared, "Cache restored from key"),
    ] {
        fs::write(
            root.join(file),
            format!("2026-10-04T00:00:00.0000000Z {body}: {key}\n"),
        )
        .expect("write stock restore step log");
    }
    write_fake_curl(&root.join("bin/curl"));
    write_fake_date(&root.join("bin/date"));
    #[cfg(target_os = "macos")]
    write_fake_ln(&root.join("bin/ln"));
    #[cfg(target_os = "macos")]
    write_fake_realpath(&root.join("bin/realpath"));
    #[cfg(target_os = "macos")]
    write_fake_stat(&root.join("bin/stat"));
}

fn stock_job_record(name: &str, id: u64, status: &str, conclusion: &str, steps: &str) -> String {
    if conclusion.is_empty() {
        format!(r#"{{"id":{id},"run_id":123,"run_attempt":2,"head_sha":"{SOURCE_SHA}","name":"{name}","status":"{status}","steps":{steps}}}"#)
    } else {
        format!(
            r#"{{"id":{id},"run_id":123,"run_attempt":2,"head_sha":"{SOURCE_SHA}","name":"{name}","status":"{status}","conclusion":"{conclusion}","steps":{steps}}}"#
        )
    }
}

fn write_fake_curl(path: &std::path::Path) {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
header=''
output=''
url=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --dump-header) header=$2; shift 2 ;;
    --output) output=$2; shift 2 ;;
    --noproxy|--proto|--max-redirs|--connect-timeout|--max-time|--max-filesize|--header|--write-out)
      shift 2 ;;
    --) shift ;;
    https://*) url=$1; shift ;;
    *) shift ;;
  esac
done
case "$url" in
  https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123/attempts/2/jobs\?per_page=100)
    cat "$MBX_STOCK_FIXTURE_JOBS" > "$output"
    printf 200 ;;
  https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123)
    cat "$MBX_STOCK_FIXTURE_RUN" > "$output"
    printf 200 ;;
  */actions/jobs/11/steps/0/logs)
    printf 'HTTP/2 302\r\nLocation: https://signed.example/seed\r\n\r\n' > "$header"
    printf 302 ;;
  */actions/jobs/12/steps/0/logs)
    printf 'HTTP/2 302\r\nLocation: https://signed.example/reader-a\r\n\r\n' > "$header"
    printf 302 ;;
  */actions/jobs/13/steps/0/logs)
    printf 'HTTP/2 302\r\nLocation: https://signed.example/reader-b\r\n\r\n' > "$header"
    printf 302 ;;
  */actions/jobs/14/steps/0/logs)
    printf 'HTTP/2 302\r\nLocation: https://signed.example/writer\r\n\r\n' > "$header"
    printf 302 ;;
  */actions/jobs/15/steps/0/logs)
    printf 'HTTP/2 302\r\nLocation: https://signed.example/observer-shared\r\n\r\n' > "$header"
    printf 302 ;;
  https://signed.example/seed)
    printf 'HTTP/2 200\r\n\r\n' > "$header"
    cat "$MBX_STOCK_FIXTURE_LOG_DIR/seed.log" > "$output"
    printf 200 ;;
  https://signed.example/reader-a)
    printf 'HTTP/2 200\r\n\r\n' > "$header"
    cat "$MBX_STOCK_FIXTURE_LOG_DIR/reader-a.log" > "$output"
    printf 200 ;;
  https://signed.example/reader-b)
    printf 'HTTP/2 200\r\n\r\n' > "$header"
    cat "$MBX_STOCK_FIXTURE_LOG_DIR/reader-b.log" > "$output"
    printf 200 ;;
  https://signed.example/writer)
    printf 'HTTP/2 200\r\n\r\n' > "$header"
    cat "$MBX_STOCK_FIXTURE_LOG_DIR/writer.log" > "$output"
    printf 200 ;;
  https://signed.example/observer-shared)
    printf 'HTTP/2 200\r\n\r\n' > "$header"
    cat "$MBX_STOCK_FIXTURE_LOG_DIR/observer-shared.log" > "$output"
    printf 200 ;;
  *) echo 'unexpected fixture curl request' >&2; exit 97 ;;
esac
"#,
    )
    .expect("write fixture curl command");
    make_executable(path);
}

fn write_fake_date(path: &std::path::Path) {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
case "${3-}" in
  ????-??-??T??:??:??.???????Z) printf '%s' "$3" ;;
  *) printf invalid ;;
esac
"#,
    )
    .expect("write fixture date command");
    make_executable(path);
}


#[cfg(target_os = "macos")]
fn write_fake_ln(path: &std::path::Path) {
    fs::write(
        path,
        "#!/bin/sh\nset -eu\n[ \"${1-}\" != -- ] || shift\nexec /bin/ln \"$@\"\n",
    )
    .expect("write fixture ln command");
    make_executable(path);
}

#[cfg(target_os = "macos")]
fn write_fake_realpath(path: &std::path::Path) {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in -e|-q|--) shift ;; *) target=$1; shift ;; esac
done
[ -n "${target-}" ] && { [ -e "$target" ] || [ -L "$target" ]; }
if [ -d "$target" ]; then
  cd "$target"
  pwd -P
else
  parent=$(dirname "$target")
  base=$(basename "$target")
  cd "$parent"
  printf '%s/%s\n' "$(pwd -P)" "$base"
fi
"#,
    )
    .expect("write fixture realpath command");
    make_executable(path);
}

#[cfg(target_os = "macos")]
fn write_fake_stat(path: &std::path::Path) {
    fs::write(
        path,
        r#"#!/bin/sh
set -eu
test "$1" = -c
format=$2
shift 2
[ "${1-}" = -- ] && shift
target=$1
case "$format" in
  %u) native_format=%u ;;
  %h) native_format=%l ;;
  %s) native_format=%z ;;
  %a) native_format=%Lp ;;
  %a:%u) native_format=%Lp:%u ;;
  *) exit 97 ;;
esac
/usr/bin/stat -f "$native_format" "$target"
"#,
    )
    .expect("write fixture stat command");
    make_executable(path);
}

fn make_executable(path: &std::path::Path) {
    let mut permissions = fs::metadata(path)
        .expect("read fixture command metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make fixture command executable");
}
