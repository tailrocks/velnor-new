//! Pure shell source for the cached Mise bootstrap guard.
//!
//! The guard runs after the explicit tools-cache restore and immediately
//! before `jdx/mise-action`. It validates the runner-temp root and every
//! owned cache ancestor, rejects anything other than an executable regular
//! file, and verifies a surviving payload with a trusted absolute checksum
//! utility. A rejected payload is unlinked only after its ancestors pass the
//! same checks; the action then sees the ordinary cache-miss path.

/// Build the POSIX shell guard for one already validated catalog SHA-256.
///
/// The caller validates the digest as a lowercase 64-character hexadecimal
/// value before constructing the setup step. Invalid input still produces a
/// fail-closed script, rather than allowing shell text into the generated
/// workflow. The script selects `/usr/bin/sha256sum` on GNU runners and
/// `/usr/bin/shasum -a 256` on macOS; no checksum command is resolved through
/// `PATH`.
#[must_use]
pub(crate) fn mise_bootstrap_guard_script(sha256: &str) -> String {
    if !velnor_actions_contract::ids::is_lower_hex_len(sha256, 64) {
        return "set -eu\nprintf '%s\\n' 'invalid Mise bootstrap SHA-256' >&2\nexit 1\n".to_owned();
    }

    let mut script = String::from(
        r#"set -eu
fail() {
    printf '%s\n' "$1" >&2
    exit 1
}

runner_temp="${RUNNER_TEMP-}"
case "$runner_temp" in
    /*) ;;
    *) fail 'unsafe RUNNER_TEMP: must be absolute' ;;
esac
case "$runner_temp" in
    ""|"/"|*/|*//*|*/.|*/./*|*/..|*/../*|*[![:print:]]*)
        fail 'unsafe RUNNER_TEMP: invalid lexical path'
        ;;
esac

# Split the root lexically. Filesystem checks below cover the root itself and
# every directory owned by this workflow; system prefixes such as macOS's
# `/var` may legitimately be symlinks.
remaining=${runner_temp#/}
while [ -n "$remaining" ]; do
    case "$remaining" in
        */*) component=${remaining%%/*}; remaining=${remaining#*/} ;;
        *) component=$remaining; remaining= ;;
    esac
    case "$component" in
        ""|.|..) fail 'unsafe RUNNER_TEMP: empty or dot path component' ;;
    esac
done

if [ -L "$runner_temp" ] || [ ! -d "$runner_temp" ]; then
    fail 'unsafe RUNNER_TEMP: root is not a real directory'
fi

# Missing owned directories are a normal cache miss; existing objects must
# never be symlinks, files, or special nodes where a directory is expected.
owned_dir() {
    if [ -L "$1" ]; then
        return 1
    fi
    if [ -e "$1" ] && [ ! -d "$1" ]; then
        return 1
    fi
    return 0
}

velnor_dir="$runner_temp/velnor"
bootstrap_dir="$velnor_dir/mise-bootstrap"
bin_dir="$bootstrap_dir/bin"
if ! owned_dir "$velnor_dir" || ! owned_dir "$bootstrap_dir" || ! owned_dir "$bin_dir"; then
    fail 'unsafe Mise bootstrap cache ancestor'
fi

mise_bin="$bin_dir/mise"
if [ -e "$mise_bin" ] || [ -L "$mise_bin" ]; then
    if [ ! -f "$mise_bin" ] || [ -L "$mise_bin" ] || [ ! -x "$mise_bin" ]; then
        if ! /bin/rm -f -- "$mise_bin"; then
            fail 'could not remove untrusted Mise bootstrap'
        fi
        if [ -e "$mise_bin" ] || [ -L "$mise_bin" ]; then
            fail 'untrusted Mise bootstrap remains'
        fi
        printf '%s\n' 'Mise bootstrap cache miss'
        exit 0
    fi

    if [ -x /usr/bin/sha256sum ]; then
        if printf '%s  %s\n' '"#,
    );
    script.push_str(sha256);
    script.push_str(
        r#"' "$mise_bin" | /usr/bin/sha256sum -c - >/dev/null 2>&1; then
            printf '%s\n' 'verified cached Mise bootstrap'
            exit 0
        fi
    elif [ -x /usr/bin/shasum ]; then
        if printf '%s  %s\n' '"#,
    );
    script.push_str(sha256);
    script.push_str(
        r#"' "$mise_bin" | /usr/bin/shasum -a 256 -c - >/dev/null 2>&1; then
            printf '%s\n' 'verified cached Mise bootstrap'
            exit 0
        fi
    else
        fail 'no trusted SHA-256 utility'
    fi

    if ! /bin/rm -f -- "$mise_bin"; then
        fail 'could not remove unverified Mise bootstrap'
    fi
    if [ -e "$mise_bin" ] || [ -L "$mise_bin" ]; then
        fail 'unverified Mise bootstrap remains'
    fi
    printf '%s\n' 'removed unverified Mise bootstrap'
fi

printf '%s\n' 'Mise bootstrap cache miss'
"#,
    );
    script
}

#[cfg(test)]
#[path = "mise_bootstrap_guard_tests.rs"]
mod tests;
