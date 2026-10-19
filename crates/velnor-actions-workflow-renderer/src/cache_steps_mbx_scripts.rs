//! Disk-sample shell fragments for native MBX steps.

const MBX_DISK_SAMPLE_BODY: &[&str] = &[
    "boundary=$1",
    r#"case "${RUNNER_TEMP:-}" in /*) ;; *) printf '%s\n' 'MBX_DISK_SAMPLE_ERROR runner_temp_not_absolute' >&2; return 0 ;; esac"#,
    r#"if [ -d "$RUNNER_TEMP" ] && [ ! -L "$RUNNER_TEMP" ]; then printf 'MBX_DISK_SAMPLE boundary=%s volume=runner_temp blocks_kib\n' "$boundary"; if df -Pk "$RUNNER_TEMP"; then :; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s command=df_blocks\n' "$boundary" >&2; fi; printf 'MBX_DISK_SAMPLE boundary=%s volume=runner_temp inodes\n' "$boundary"; if df -Pi "$RUNNER_TEMP"; then :; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s command=df_inodes\n' "$boundary" >&2; fi; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s runner_temp_not_real_directory\n' "$boundary" >&2; return 0; fi"#,
    r#"store_path="$RUNNER_TEMP/velnor/mbx""#,
    r#"if [ "${MBX_CACHE_DIR:-}" = "$store_path" ] && [ -d "$store_path" ] && [ ! -L "$store_path" ]; then printf 'MBX_DISK_ALLOCATED_KIB boundary=%s root=mbx_store\n' "$boundary"; if du -sk "$store_path"; then :; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s path=mbx_store command=du\n' "$boundary" >&2; fi; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s path=mbx_store_unavailable\n' "$boundary" >&2; fi"#,
    r#"target_path="${target_dir:-target}"; if [ ! -d "$target_path" ]; then for candidate in */target crates/*/target "$RUNNER_TEMP/velnor/target"; do if [ -d "$candidate" ]; then target_path="$candidate"; break; fi; done; fi"#,
    r#"if [ -L "$target_path" ]; then printf 'MBX_DISK_SAMPLE_ERROR boundary=%s path=target_is_symlink\n' "$boundary" >&2; elif [ -d "$target_path" ]; then printf 'MBX_DISK_ALLOCATED_KIB boundary=%s root=target\n' "$boundary"; if du -sk "$target_path"; then :; else printf 'MBX_DISK_SAMPLE_ERROR boundary=%s path=target command=du\n' "$boundary" >&2; fi; else printf 'MBX_DISK_ALLOCATED_KIB boundary=%s root=target state=absent\n' "$boundary"; fi"#,
];

pub(super) fn mbx_disk_sample_commands(boundary: &'static str) -> [String; 2] {
    [
        mbx_disk_sample_function(),
        format!("mbx_disk_sample {boundary}"),
    ]
}

fn mbx_disk_sample_function() -> String {
    format!(
        "mbx_disk_sample() {{ {}; }}",
        MBX_DISK_SAMPLE_BODY.join("; ")
    )
}
