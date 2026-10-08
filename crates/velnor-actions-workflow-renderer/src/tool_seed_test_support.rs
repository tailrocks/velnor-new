use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[path = "../../test_support/git_fixture.rs"]
pub(crate) mod git_fixture;

pub(crate) fn mock_trust_commands(script: &str, root: &Path) -> String {
    let bin = root.join("trust-mocks");
    fs::create_dir_all(&bin).expect("mock directory");
    let stat = executable(
        &bin,
        "stat",
        "#!/bin/sh\ncase \"$2\" in %u) printf '%s\\n' \"${SEED_TEST_UID:-0}\" ;; %s|%z) printf '%s\\n' \"${SEED_TEST_FILE_SIZE:-64}\" ;; *) exit 2 ;; esac\n",
    );
    let findmnt = executable(
        &bin,
        "findmnt",
        "#!/bin/sh\n[ \"${SEED_TEST_FINDMNT_FAIL:-0}\" = 1 ] && exit 23\nprintf '%s\\n' \"${SEED_TEST_MOUNTS-}\"\n",
    );
    let find = executable(&bin, "find", FIND_MOCK);
    script
        .replace("/usr/bin/findmnt", findmnt.to_str().expect("findmnt path"))
        .replace("/usr/bin/find", find.to_str().expect("find path"))
        .replace("/usr/bin/stat", stat.to_str().expect("stat path"))
}

pub(crate) fn executable(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).expect("write mock");
    let mut permissions = fs::metadata(&path).expect("mock metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("mock mode");
    path
}

const FIND_MOCK: &str = r#"#!/bin/sh
mode=${SEED_TEST_FIND_MODE-}
case "$mode" in
    tree-error) printf '%s\0' "$1"; exit 23 ;;
    tree-deep-ok|tree-deep-bad)
        path=$1
        printf '%s\0' "$path"
        levels=16
        [ "$mode" = tree-deep-ok ] || levels=17
        i=0
        while [ "$i" -lt "$levels" ]; do path="$path/d"; i=$((i+1)); done
        printf '%s\0' "$path"
        exit 0
        ;;
    tree-too-many)
        i=0
        while [ "$i" -lt 100001 ]; do printf '%s\0' "$1"; i=$((i+1)); done
        exit 0
        ;;
esac
owner_scan=0
for arg do [ "$arg" = -uid ] && owner_scan=1; done
if [ "${SEED_TEST_SKIP_OWNER_SCAN:-0}" = 1 ] && [ "$owner_scan" = 1 ]; then
    exec /usr/bin/find "$1" -xdev \( ! -type d -a ! -type f \) -print0
fi
exec /usr/bin/find "$@"
"#;
