//! MBX rustc-wrapper identity probe for the version check.

use super::ACTION_MBX_BIN;

/// Require `mbx-rustc <rustc> -vV` to print exactly `rustc -vV`.
///
/// MBX dispatches on argv0 `mbx-rustc` exactly as Cargo's `RUSTC_WRAPPER`
/// shim does, and Cargo hashes the wrapper's `-vV` output into every unit
/// fingerprint: identical output keeps fingerprints identical with and
/// without MBX.
pub(super) fn mbx_rustc_identity_commands(rust_toolchain: &str) -> [String; 9] {
    [
        format!("ln -s \"{ACTION_MBX_BIN}\" \"$probe_dir/mbx-rustc\""),
        format!("mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- rustup which --toolchain '{rust_toolchain}' rustc > \"$probe_dir/rustc-path\""),
        "exec 3< \"$probe_dir/rustc-path\"".to_owned(),
        "IFS= read -r rustc_path <&3 || { printf '%s\\n' 'rustup returned no rustc path' >&2; exit 1; }".to_owned(),
        "exec 3<&-".to_owned(),
        "case \"$rustc_path\" in /*) ;; *) printf '%s\\n' 'rustc path must be absolute' >&2; exit 1 ;; esac".to_owned(),
        "\"$rustc_path\" -vV > \"$probe_dir/rustc-vv\"".to_owned(),
        format!("mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- \"$probe_dir/mbx-rustc\" \"$rustc_path\" -vV > \"$probe_dir/mbx-rustc-vv\""),
        "cmp -s \"$probe_dir/rustc-vv\" \"$probe_dir/mbx-rustc-vv\" || { printf '%s\\n' 'mbx-rustc -vV differs from rustc -vV' >&2; exit 1; }".to_owned(),
    ]
}
