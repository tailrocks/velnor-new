use crate::RenderError;

pub(crate) const SEED_ROOT: &str = "/opt/velnor/seed";
const SEED_PROVENANCE: &str = "velnor-host-seed-v1";
const MAX_TREE_ENTRIES: &str = "100000";
const MAX_TREE_DEPTH: &str = "16";

pub(crate) fn require_seed_root(root: &str) -> Result<(), RenderError> {
    let bytes_ok = root
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'));
    if root.starts_with('/') && !root.contains("..") && !root.contains("//") && bytes_ok {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_seed_root:{root}")))
    }
}

/// Shell functions which admit an immutable image-provisioned seed tree.
///
/// # Errors
///
/// Returns an error when the fixed seed root cannot be safely embedded.
pub(crate) fn trusted_seed_guard(seed_root: &str) -> Result<String, RenderError> {
    require_seed_root(seed_root)?;
    let script = r#"trusted_seed_owner_root() { if [ "${RUNNER_OS-}" = macOS ]; then /usr/bin/stat -f %u "$1" | /usr/bin/grep -qx 0; else /usr/bin/stat -c %u -- "$1" | /usr/bin/grep -qx 0; fi; }; trusted_seed_file_matches() { local file="$1" expected="$2" actual="" size="" newline=""; [ -f "$file" ] && [ ! -L "$file" ] || return 1; if [ "${RUNNER_OS-}" = macOS ]; then IFS= read -r size < <(/usr/bin/stat -f %z "$file") || return 1; else IFS= read -r size < <(/usr/bin/stat -c %s -- "$file") || return 1; fi; case "$size" in ''|*[!0-9]*) return 1 ;; esac; [ "$size" -le 512 ] || return 1; if IFS= read -r -d '' actual < "$file"; then return 1; fi; printf -v newline '\n'; [ "$actual" = "$expected" ] || [ "$actual" = "$expected$newline" ]; }; trusted_seed_path_is_real() { local current="$1"; while [ "$current" != / ]; do [ ! -L "$current" ] || return 1; current="${current%/*}"; [ -n "$current" ] || current=/; done; }; trusted_seed_mount_is_immutable() { if [ "${RUNNER_OS-}" = macOS ]; then /sbin/mount | /usr/bin/awk -v seed="$1" 'index($0, " on ") { marker=index($0, " on "); source=substr($0, 1, marker-1); rest=substr($0, marker+4); options_at=index(rest, " ("); target=substr(rest, 1, options_at-1); mount_options=substr(rest, options_at+2); targets[NR]=target; sources[NR]=source; options[NR]=mount_options; if (target == seed) { exact++; seed_source=source; if (mount_options ~ /read-only|rdonly/) readonly=1 } else if (index(target, seed "/") == 1) nested=1 } END { for (i=1; i<=NR; i++) if (sources[i] == seed_source && targets[i] != seed && options[i] !~ /read-only|rdonly/) writable_alias=1; exit !(exact == 1 && readonly && !nested && !writable_alias) }'; else /usr/bin/findmnt -arn -o TARGET,FSTYPE,MAJ:MIN,OPTIONS | /usr/bin/awk -v seed="$1" '{ target=$1; targets[NR]=target; devices[NR]=$3; options[NR]=$4; if (target == seed) { exact++; device=$3; if ($2 != "overlay" && $2 != "fuse.overlayfs" && $4 ~ /(^|,)ro(,|$)/) readonly=1 } else if (index(target, seed "/") == 1) nested=1 } END { for (i=1; i<=NR; i++) if (devices[i] == device && targets[i] != seed && options[i] ~ /(^|,)rw(,|$)/) writable_alias=1; exit !(exact == 1 && readonly && !nested && !writable_alias) }'; fi; }; trusted_seed_tree_bounded() { local root="$1" count=0 entry relative depth; if ! /usr/bin/find "$root" -xdev -print0 | while IFS= read -r -d '' entry; do ((count+=1)); [ "$count" -le __MAX_ENTRIES__ ] || exit 1; case "$entry" in "$root") depth=0 ;; "$root"/*) relative="${entry#"$root"/}"; depth=1; while [[ "$relative" == */* ]]; do relative="${relative#*/}"; ((depth+=1)); done ;; *) exit 1 ;; esac; [ "$depth" -le __MAX_DEPTH__ ] || exit 1; done; then return 1; fi; }; trusted_seed_tree_entries_are_safe() { if ! /usr/bin/find "$1" -xdev \( ! -type d -a ! -type f -o ! -uid 0 \) -print0 | while IFS= read -r -d '' entry; do exit 1; done; then return 1; fi; }; trusted_seed_tree_is_safe() { trusted_seed_tree_bounded "$1" && trusted_seed_tree_entries_are_safe "$1"; }; trusted_seed_is_trusted() { [ -d "$1" ] && [ ! -L "$1" ] || return 1; trusted_seed_path_is_real "$1" || return 1; [ -f "$1/PROVENANCE" ] && [ ! -L "$1/PROVENANCE" ] || return 1; trusted_seed_owner_root "$1" || return 1; trusted_seed_owner_root "$1/PROVENANCE" || return 1; trusted_seed_file_matches "$1/PROVENANCE" "__SEED_PROVENANCE__" || return 1; trusted_seed_mount_is_immutable "$1" || return 1; trusted_seed_tree_is_safe "$1"; }"#;
    Ok(script
        .replace("__MAX_ENTRIES__", MAX_TREE_ENTRIES)
        .replace("__MAX_DEPTH__", MAX_TREE_DEPTH)
        .replace("__SEED_PROVENANCE__", SEED_PROVENANCE))
}

#[cfg(test)]
#[path = "tool_seed_admission_tests.rs"]
mod tests;
