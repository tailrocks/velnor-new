pub(super) const PREFLIGHT_ENV: [(&str, &str); 17] = [
    (
        "VELNOR_MBX_JOB_ROOT",
        "${{ steps.mbx_cache_preflight.outputs.job_root }}",
    ),
    (
        "VELNOR_MBX_JOB_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.job_identity }}",
    ),
    (
        "VELNOR_MBX_JOB_ROOT_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.job_root_identity }}",
    ),
    (
        "VELNOR_MBX_WORKSPACE_ROOT",
        "${{ steps.mbx_cache_preflight.outputs.workspace_root }}",
    ),
    (
        "VELNOR_MBX_WORKSPACE_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.workspace_identity }}",
    ),
    (
        "VELNOR_MBX_TRANSPORT_PARENT",
        "${{ steps.mbx_cache_preflight.outputs.transport_parent }}",
    ),
    (
        "VELNOR_MBX_TRANSPORT_PARENT_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.transport_parent_identity }}",
    ),
    (
        "VELNOR_MBX_TRANSPORT_LEASE",
        "${{ steps.mbx_cache_preflight.outputs.transport_lease }}",
    ),
    (
        "VELNOR_MBX_TRANSPORT_LEASE_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.transport_lease_identity }}",
    ),
    (
        "VELNOR_MBX_PRIMARY_CACHE_DIR",
        "${{ steps.mbx_cache_preflight.outputs.cache_dir }}",
    ),
    (
        "VELNOR_MBX_PRIMARY_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.cache_dir_identity }}",
    ),
    (
        "VELNOR_MBX_COLD_CACHE_DIR",
        "${{ steps.mbx_cache_preflight.outputs.cold_cache_dir }}",
    ),
    (
        "VELNOR_MBX_COLD_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.cold_cache_dir_identity }}",
    ),
    (
        "VELNOR_MBX_BUNDLE_PATH",
        "${{ steps.mbx_cache_preflight.outputs.bundle_path }}",
    ),
    (
        "VELNOR_MBX_BUNDLE_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.bundle_path_identity }}",
    ),
    (
        "VELNOR_MBX_IMPORT_CWD",
        "${{ steps.mbx_cache_preflight.outputs.import_cwd }}",
    ),
    (
        "VELNOR_MBX_IMPORT_IDENTITY",
        "${{ steps.mbx_cache_preflight.outputs.import_cwd_identity }}",
    ),
];
