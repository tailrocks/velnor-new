//! Shell script fragments and content-addressed target-cache keys for native MBX steps.

use crate::action_mbx_shell::ACTION_MBX_BIN;

const MBX_DISK_SAMPLE_BODY: &[&str] = &[
    "boundary=$1",
    r#"case "${RUNNER_TEMP:-}" in /*) ;; *) printf '%s\n' 'MBX_DISK_SAMPLE_ERROR runner_temp_not_absolute' >&2; return 0 ;; esac"#,
    r#"if [ -d "$RUNNER_TEMP" ] && [ ! -L "$RUNNER_TEMP" ]; then printf 'MBX_DISK_SAMPLE boundary=%s volume=runner_temp blocks_kib\n' "$boundary"; if df -Pk "$RUNNER_TEMP"; then :; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s command=df_blocks\n' "$boundary" >&2; fi; printf 'MBX_DISK_SAMPLE boundary=%s volume=runner_temp inodes\n' "$boundary"; if df -Pi "$RUNNER_TEMP"; then :; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s command=df_inodes\n' "$boundary" >&2; fi; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s runner_temp_not_real_directory\n' "$boundary" >&2; return 0; fi"#,
    r#"store_path="$RUNNER_TEMP/velnor/mbx""#,
    r#"if [ "${MBX_CACHE_DIR:-}" = "$store_path" ] && [ -d "$store_path" ] && [ ! -L "$store_path" ]; then printf 'MBX_DISK_ALLOCATED_KIB boundary=%s root=mbx_store\n' "$boundary"; if du -sk "$store_path"; then :; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s path=mbx_store command=du\n' "$boundary" >&2; fi; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s path=mbx_store_unavailable\n' "$boundary" >&2; fi"#,
    r#"target_path="${target_dir:-target}"; if [ ! -d "$target_path" ]; then for candidate in */target crates/*/target "$RUNNER_TEMP/velnor/target"; do if [ -d "$candidate" ]; then target_path="$candidate"; break; fi; done; fi"#,
    r#"if [ -L "$target_path" ]; then printf 'MBX_DISK_SAMPLE_ERROR boundary=%s path=target_is_symlink\n' "$boundary" >&2; elif [ -d "$target_path" ]; then printf 'MBX_DISK_ALLOCATED_KIB boundary=%s root=target\n' "$boundary"; if du -sk "$target_path"; then :; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s path=target command=du\n' "$boundary" >&2; fi; else printf 'MBX_DISK_ALLOCATED_KIB boundary=%s root=target state=absent\n' "$boundary"; fi"#,
];

const MBX_PREPARE_TARGET_CACHE_CMD: &str = r#"python3 -c 'exec("import os, pathlib, shutil, subprocess, sys\ntd = pathlib.Path(sys.argv[1])\ndbg = td / \"debug\"\nif dbg.is_dir() and not dbg.is_symlink():\n    shims = dbg / \".mbx-build-script-shims\"\n    os.makedirs(shims, exist_ok=True)\n    for base in [td] + sorted(c for c in td.iterdir() if c.name.startswith(\"velnor-\") and c.is_dir() and not c.is_symlink()):\n        bdbg = base / \"debug\"\n        if not bdbg.is_dir() or bdbg.is_symlink():\n            continue\n        bshims = bdbg / \".mbx-build-script-shims\"\n        os.makedirs(bshims, exist_ok=True)\n        vdoc = bshims / \".velnor-doc\"\n        shutil.rmtree(vdoc, ignore_errors=True)\n        doc = base / \"doc\"\n        if doc.is_dir() and not doc.is_symlink():\n            shutil.move(str(doc), str(vdoc))\n    ws = os.environ.get(\"GITHUB_WORKSPACE\", \"\")\n    if ws and os.path.exists(os.path.join(ws, \".git\")):\n        out = subprocess.check_output([\"git\", \"-C\", ws, \"ls-files\", \"-s\", \"-z\"])\n        lines = []\n        for entry in out.split(b\"\\0\"):\n            if not entry:\n                continue\n            meta, rel_b = entry.split(b\"\\t\", 1)\n            oid = meta.split()[1].decode()\n            rel = rel_b.decode()\n            full = os.path.join(ws, rel)\n            if os.path.isfile(full) and not os.path.islink(full):\n                lines.append(f\"{oid}\\t{os.stat(full).st_mtime_ns}\\t{rel}\\n\")\n        (shims / \".velnor-source-oids\").write_text(\"\".join(lines))\n")' "$target_dir""#;

/// Restore cached `target/doc` (and each `target/velnor-*` lane's `doc`) and unchanged tracked
/// source `mtime`s after MBX target-cache restore.
pub(super) const MBX_RESTORE_SOURCE_MTIMES_CMD: &str = r#"python3 -c 'exec("import os, pathlib, shutil, subprocess\nws = os.environ.get(\"GITHUB_WORKSPACE\", \"\")\nif ws and os.path.exists(os.path.join(ws, \".git\")):\n    dirty = {p.decode() for p in subprocess.check_output([\"git\", \"-C\", ws, \"diff\", \"--name-only\", \"-z\", \"HEAD\"]).split(b\"\\0\") if p}\n    cur = {}\n    for entry in subprocess.check_output([\"git\", \"-C\", ws, \"ls-files\", \"-s\", \"-z\"]).split(b\"\\0\"):\n        if entry:\n            meta, rel_b = entry.split(b\"\\t\", 1)\n            cur[rel_b.decode()] = meta.split()[1].decode()\n    for pattern in (\"**/target/debug/.mbx-build-script-shims/.velnor-doc\", \"**/target/velnor-*/debug/.mbx-build-script-shims/.velnor-doc\"):\n        for vdoc in sorted(pathlib.Path(ws).glob(pattern)):\n            doc = vdoc.parent.parent.parent / \"doc\"\n            if vdoc.is_dir() and not vdoc.is_symlink():\n                shutil.rmtree(doc, ignore_errors=True)\n                shutil.move(str(vdoc), str(doc))\n    for oids_file in pathlib.Path(ws).glob(\"**/target/debug/.mbx-build-script-shims/.velnor-source-oids\"):\n        for line in oids_file.read_text().splitlines():\n            if not line:\n                continue\n            oid, ns_s, rel = line.split(\"\\t\", 2)\n            if rel not in dirty and cur.get(rel) == oid:\n                full = os.path.join(ws, rel)\n                if os.path.isfile(full) and not os.path.islink(full):\n                    ns = int(ns_s)\n                    os.utime(full, ns=(ns, ns))\n")'"#;

pub(super) fn mbx_disk_sample_commands(boundary: &'static str) -> [String; 2] {
    [
        mbx_disk_sample_function(),
        format!("mbx_disk_sample {boundary}"),
    ]
}

pub(super) fn mbx_disk_sample_function() -> String {
    format!(
        "mbx_disk_sample() {{ {}; }}",
        MBX_DISK_SAMPLE_BODY.join("; ")
    )
}

pub(super) fn mbx_target_clean_command(rust_toolchain: &str) -> String {
    format!(
        "clean_status=0; if mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- \"{ACTION_MBX_BIN}\" cache dir >/dev/null 2>&1; then clean_status=0; else clean_status=$?; fi; if [ -d \"$target_dir\" ] && [ ! -L \"$target_dir\" ]; then rm -rf \"$target_dir/velnor-cli-nested-cargo\" \"$target_dir/tmp\" \"$target_dir/debug/examples\" \"$target_dir/debug/incremental\"; {MBX_PREPARE_TARGET_CACHE_CMD}; fi"
    )
}

pub(super) fn target_cache_keys(
    generation: &str,
    rust_toolchain: &str,
    working_directory: Option<&str>,
) -> (String, String) {
    let working_dir = working_directory.filter(|dir| !dir.is_empty() && *dir != ".");
    let (lock_hash, source_hash) = if let Some(dir) = working_dir {
        (
            format!(
                "${{{{ hashFiles('{dir}/Cargo.lock','{dir}/Cargo.toml','{dir}/**/Cargo.toml','rust-toolchain.toml','rust-toolchain','{dir}/.config/nextest.toml') }}}}"
            ),
            format!(
                "${{{{ hashFiles('{dir}/**/*.rs','{dir}/**/Cargo.toml','{dir}/Cargo.lock') }}}}"
            ),
        )
    } else {
        (
            "${{ hashFiles('Cargo.lock','Cargo.toml','crates/*/Cargo.toml','rust-toolchain.toml','rust-toolchain','.config/nextest.toml') }}".to_owned(),
            "${{ hashFiles('Cargo.lock','Cargo.toml','crates/*/Cargo.toml','src/**/*.rs','crates/*/src/**/*.rs','crates/*/tests/**/*.rs','crates/*/benches/**/*.rs','crates/*/examples/**/*.rs','crates/*/build.rs','tests/**/*.rs','benches/**/*.rs','examples/**/*.rs','build.rs') }}".to_owned(),
        )
    };
    let cache_key = format!("{generation}-target-rust-{rust_toolchain}-{lock_hash}-{source_hash}");
    let restore_keys = format!(
        "{generation}-target-rust-{rust_toolchain}-{lock_hash}-\n{generation}-target-rust-{rust_toolchain}-"
    );
    (cache_key, restore_keys)
}
