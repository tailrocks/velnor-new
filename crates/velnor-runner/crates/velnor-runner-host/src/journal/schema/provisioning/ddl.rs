include!("cleanup_guards.rs");

pub(super) const BOUND_STORAGE_ATTACHMENTS: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_bound_storage_attachments (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), role TEXT NOT NULL CHECK (role IN ('runner-root', 'runner-home', 'runner-work', 'runner-tmp', 'runner-shm', 'shared-run', 'dind-root', 'dind-data', 'dind-tmp', 'dind-shm', 'engine-metadata')), observer TEXT NOT NULL CHECK (observer IN ('runner', 'dind', 'probe')), container_id TEXT, mount_target TEXT NOT NULL CHECK (length(mount_target) BETWEEN 1 AND 512), source_target TEXT, read_only INTEGER NOT NULL CHECK (read_only IN (0, 1)), mount_id INTEGER NOT NULL CHECK (mount_id > 0), device_major INTEGER NOT NULL CHECK (device_major >= 0), device_minor INTEGER NOT NULL CHECK (device_minor >= 0), statx_device_major INTEGER NOT NULL CHECK (statx_device_major >= 0), statx_device_minor INTEGER NOT NULL CHECK (statx_device_minor >= 0), fs_type_magic INTEGER NOT NULL CHECK (fs_type_magic >= 0), v16_filesystem_id_0 INTEGER, v16_filesystem_id_1 INTEGER, filesystem_id TEXT CHECK (filesystem_id IS NULL OR (length(filesystem_id) = 16 AND filesystem_id NOT GLOB '*[^0-9a-f]*')), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), CHECK ((v16_filesystem_id_0 IS NOT NULL AND v16_filesystem_id_1 IS NOT NULL AND filesystem_id IS NULL) OR (v16_filesystem_id_0 IS NULL AND v16_filesystem_id_1 IS NULL AND filesystem_id IS NOT NULL)), CHECK ((observer = 'probe' AND container_id IS NULL AND role = 'engine-metadata') OR (observer IN ('runner', 'dind') AND container_id IS NOT NULL)), CHECK ((role = 'engine-metadata' AND ((observer = 'probe' AND mount_target = '/velnor/docker-root' AND source_target = '/var/lib/docker' AND read_only = 1) OR (observer = 'runner' AND mount_target IN ('/.velnor-runtime-identity', '/opt/velnor/job-facts', '/opt/velnor/source-proof/control', '/opt/velnor/source-proof/inbox') AND source_target IS NULL AND read_only = 1))) OR (role != 'engine-metadata' AND source_target IS NULL)), PRIMARY KEY (reservation_id, role, observer, mount_target))";
pub(super) const LEGACY_BOUND_STORAGE_ATTACHMENTS: &str = "CREATE TABLE IF NOT EXISTS capacity_reservation_bound_storage_attachments (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), role TEXT NOT NULL CHECK (role IN ('runner-root', 'runner-home', 'runner-work', 'runner-tmp', 'runner-shm', 'shared-run', 'dind-root', 'dind-data', 'dind-tmp', 'dind-shm', 'engine-metadata')), observer TEXT NOT NULL CHECK (observer IN ('runner', 'dind', 'probe')), container_id TEXT, mount_target TEXT NOT NULL CHECK (length(mount_target) BETWEEN 1 AND 512), source_target TEXT, read_only INTEGER NOT NULL CHECK (read_only IN (0, 1)), mount_id INTEGER NOT NULL CHECK (mount_id > 0), device_major INTEGER NOT NULL CHECK (device_major >= 0), device_minor INTEGER NOT NULL CHECK (device_minor >= 0), statx_device_major INTEGER NOT NULL CHECK (statx_device_major >= 0), statx_device_minor INTEGER NOT NULL CHECK (statx_device_minor >= 0), fs_type_magic INTEGER NOT NULL CHECK (fs_type_magic >= 0), v16_filesystem_id_0 INTEGER, v16_filesystem_id_1 INTEGER, filesystem_id TEXT CHECK (filesystem_id IS NULL OR (length(filesystem_id) = 16 AND filesystem_id NOT GLOB '*[^0-9a-f]*')), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), CHECK ((v16_filesystem_id_0 IS NOT NULL AND v16_filesystem_id_1 IS NOT NULL AND filesystem_id IS NULL) OR (v16_filesystem_id_0 IS NULL AND v16_filesystem_id_1 IS NULL AND filesystem_id IS NOT NULL)), CHECK ((observer = 'probe' AND container_id IS NULL AND role = 'engine-metadata') OR (observer IN ('runner', 'dind') AND container_id IS NOT NULL)), CHECK ((role = 'engine-metadata' AND ((observer = 'probe' AND mount_target = '/velnor/docker-root' AND source_target = '/var/lib/docker' AND read_only = 1) OR (observer = 'runner' AND mount_target IN ('/.velnor-runtime-identity', '/opt/velnor/job-facts') AND source_target IS NULL AND read_only = 1))) OR (role != 'engine-metadata' AND source_target IS NULL)), PRIMARY KEY (reservation_id, role, observer, mount_target))";

const TABLES: [(&str, &str); 17] = [
    (
        "capacity_reservation_provisioning",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservations(id), operation_id INTEGER NOT NULL UNIQUE REFERENCES connection_intents(id), profile_generation_id TEXT NOT NULL REFERENCES host_storage_profiles(generation_id), profile_digest TEXT NOT NULL CHECK (length(profile_digest) = 64 AND profile_digest NOT GLOB '*[^0-9a-f]*'), sample_generation_id TEXT NOT NULL CHECK (length(sample_generation_id) = 32 AND sample_generation_id NOT GLOB '*[^0-9a-f]*'), observed_at_unix_ms INTEGER NOT NULL CHECK (observed_at_unix_ms > 0), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), retained_memory_bytes INTEGER NOT NULL CHECK (retained_memory_bytes >= 0))",
    ),
    (
        "capacity_reservation_provisioning_topology",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_topology (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_provisioning(reservation_id), topology_version INTEGER NOT NULL CHECK (topology_version IN (1, 2)))",
    ),
    (
        "capacity_reservation_provisioning_pools",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_pools (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), pool_id TEXT NOT NULL CHECK (length(pool_id) = 64 AND pool_id NOT GLOB '*[^0-9a-f]*'), scope TEXT NOT NULL CHECK (scope IN ('shared-disk', 'exclusive-tmpfs')), medium TEXT NOT NULL CHECK (medium IN ('disk', 'tmpfs')), filesystem_kind TEXT NOT NULL CHECK (filesystem_kind IN ('btrfs', 'tmpfs', 'other')), inode_policy TEXT NOT NULL CHECK (inode_policy IN ('finite', 'dynamic-btrfs')), max_bytes INTEGER NOT NULL CHECK (max_bytes > 0), max_inodes INTEGER NOT NULL CHECK (max_inodes >= 0), reserve_bytes INTEGER NOT NULL CHECK (reserve_bytes >= 0), reserve_inodes INTEGER NOT NULL CHECK (reserve_inodes >= 0), filesystem_instance_digest TEXT CHECK (filesystem_instance_digest IS NULL OR (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*')), total_bytes INTEGER, available_bytes INTEGER, total_inodes INTEGER, available_inodes INTEGER, CHECK ((scope = 'shared-disk' AND medium = 'disk' AND filesystem_instance_digest IS NOT NULL AND total_bytes > 0 AND available_bytes >= 0 AND available_bytes <= total_bytes AND total_inodes >= 0 AND available_inodes >= 0 AND available_inodes <= total_inodes) OR (scope = 'exclusive-tmpfs' AND medium = 'tmpfs' AND inode_policy = 'finite' AND filesystem_kind = 'tmpfs' AND filesystem_instance_digest IS NULL AND total_bytes IS NULL AND available_bytes IS NULL AND total_inodes IS NULL AND available_inodes IS NULL)), CHECK ((inode_policy = 'finite' AND max_inodes > 0 AND (scope != 'shared-disk' OR total_inodes > 0)) OR (inode_policy = 'dynamic-btrfs' AND scope = 'shared-disk' AND filesystem_kind = 'btrfs' AND max_inodes = 0 AND reserve_inodes = 0 AND total_inodes = 0 AND available_inodes = 0)), PRIMARY KEY (reservation_id, pool_id))",
    ),
    (
        "capacity_reservation_provisioning_memory",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_memory (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_provisioning(reservation_id), guest_total_bytes INTEGER NOT NULL CHECK (guest_total_bytes > 0), guest_available_bytes INTEGER NOT NULL CHECK (guest_available_bytes > 0 AND guest_available_bytes <= guest_total_bytes))",
    ),
    (
        "capacity_reservation_provisioning_roles",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_roles (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), role TEXT NOT NULL CHECK (role IN ('runner-root', 'runner-home', 'runner-work', 'runner-tmp', 'runner-shm', 'shared-run', 'dind-root', 'dind-data', 'dind-tmp', 'dind-shm', 'engine-metadata')), pool_id TEXT NOT NULL CHECK (length(pool_id) = 64 AND pool_id NOT GLOB '*[^0-9a-f]*'), medium TEXT NOT NULL CHECK (medium IN ('disk', 'tmpfs')), filesystem_kind TEXT NOT NULL CHECK (filesystem_kind IN ('btrfs', 'tmpfs', 'other')), inode_policy TEXT NOT NULL CHECK (inode_policy IN ('finite', 'dynamic-btrfs')), planned_filesystem_instance_digest TEXT CHECK (planned_filesystem_instance_digest IS NULL OR (length(planned_filesystem_instance_digest) = 64 AND planned_filesystem_instance_digest NOT GLOB '*[^0-9a-f]*')), charge_bytes INTEGER NOT NULL CHECK (charge_bytes > 0), charge_inodes INTEGER NOT NULL CHECK (charge_inodes > 0), retained_memory_bytes INTEGER NOT NULL CHECK (retained_memory_bytes >= 0), CHECK ((medium = 'disk' AND filesystem_kind IN ('btrfs', 'other') AND planned_filesystem_instance_digest IS NOT NULL AND retained_memory_bytes = 0) OR (medium = 'tmpfs' AND filesystem_kind = 'tmpfs' AND inode_policy = 'finite' AND planned_filesystem_instance_digest IS NULL AND retained_memory_bytes = charge_bytes)), CHECK (inode_policy != 'dynamic-btrfs' OR (medium = 'disk' AND filesystem_kind = 'btrfs')), PRIMARY KEY (reservation_id, role))",
    ),
    (
        "capacity_reservation_provisioning_objects",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_objects (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), role TEXT NOT NULL CHECK (role IN ('shared-run', 'runner-home', 'runner-work', 'dind-data', 'engine-metadata')), docker_name TEXT NOT NULL CHECK (length(docker_name) BETWEEN 1 AND 128), docker_id TEXT CHECK (docker_id IS NULL OR docker_id = docker_name), publication_policy_digest TEXT CHECK (publication_policy_digest IS NULL OR (length(publication_policy_digest) = 64 AND publication_policy_digest NOT GLOB '*[^0-9a-f]*')), create_state TEXT NOT NULL CHECK (create_state IN ('unclaimed', 'requested', 'created', 'uncertain')), CHECK ((create_state IN ('unclaimed', 'requested', 'uncertain') AND docker_id IS NULL) OR (create_state = 'created' AND docker_id = docker_name)), CHECK ((role = 'engine-metadata' AND publication_policy_digest IS NOT NULL) OR (role != 'engine-metadata' AND publication_policy_digest IS NULL)), PRIMARY KEY (reservation_id, role), UNIQUE (docker_name))",
    ),
    (
        "capacity_reservation_bootstrap_containers",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_bootstrap_containers (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), role TEXT NOT NULL CHECK (role IN ('runner', 'dind')), container_name TEXT NOT NULL CHECK (length(container_name) BETWEEN 1 AND 128), container_id TEXT CHECK (container_id IS NULL OR length(container_id) BETWEEN 1 AND 256), create_state TEXT NOT NULL CHECK (create_state IN ('unclaimed', 'requested', 'created', 'uncertain')), start_state TEXT NOT NULL CHECK (start_state IN ('not_requested', 'requested', 'started', 'uncertain')), CHECK ((create_state IN ('unclaimed', 'requested', 'uncertain') AND container_id IS NULL AND start_state = 'not_requested') OR (create_state = 'created' AND container_id IS NOT NULL)), CHECK (start_state = 'not_requested' OR create_state = 'created'), PRIMARY KEY (reservation_id, role), UNIQUE (container_name))",
    ),
    (
        "capacity_reservation_provisioning_state",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_state (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_provisioning(reservation_id), bootstrap_start_state TEXT NOT NULL CHECK (bootstrap_start_state IN ('not_requested', 'requested', 'started', 'uncertain')), runner_container_id TEXT, dind_container_id TEXT, resources_bound INTEGER NOT NULL DEFAULT 0 CHECK (resources_bound IN (0, 1)), instance_binding_digest TEXT CHECK (instance_binding_digest IS NULL OR (length(instance_binding_digest) = 64 AND instance_binding_digest NOT GLOB '*[^0-9a-f]*')), guest_boot_id TEXT, jit_state TEXT NOT NULL DEFAULT 'not_requested' CHECK (jit_state IN ('not_requested', 'requested', 'bound', 'uncertain', 'cleaned')), runner_id INTEGER, runner_name TEXT, runner_scale_set_id INTEGER, publication_state TEXT NOT NULL DEFAULT 'not_requested' CHECK (publication_state IN ('not_requested', 'requested', 'published', 'uncertain')), projection_sha256 TEXT CHECK (projection_sha256 IS NULL OR (length(projection_sha256) = 64 AND projection_sha256 NOT GLOB '*[^0-9a-f]*')), publication_directory_device INTEGER, publication_directory_inode INTEGER, publication_mount_id INTEGER, delivery_state TEXT NOT NULL DEFAULT 'not_requested' CHECK (delivery_state IN ('not_requested', 'requested', 'delivered', 'uncertain')), storage_cleanup_proven INTEGER NOT NULL DEFAULT 0 CHECK (storage_cleanup_proven IN (0, 1)), CHECK ((bootstrap_start_state IN ('not_requested', 'requested', 'uncertain') AND runner_container_id IS NULL AND dind_container_id IS NULL) OR (bootstrap_start_state = 'started' AND runner_container_id IS NOT NULL AND dind_container_id IS NOT NULL)), CHECK ((resources_bound = 0 AND instance_binding_digest IS NULL AND guest_boot_id IS NULL) OR (resources_bound = 1 AND instance_binding_digest IS NOT NULL AND guest_boot_id IS NOT NULL)), CHECK ((jit_state IN ('not_requested', 'requested', 'uncertain') AND runner_id IS NULL AND runner_name IS NULL AND runner_scale_set_id IS NULL) OR (jit_state IN ('bound', 'cleaned') AND runner_id > 0 AND runner_name IS NOT NULL AND runner_scale_set_id > 0)), CHECK ((publication_state IN ('not_requested', 'requested', 'uncertain') AND projection_sha256 IS NULL AND publication_directory_device IS NULL AND publication_directory_inode IS NULL AND publication_mount_id IS NULL) OR (publication_state = 'published' AND projection_sha256 IS NOT NULL AND publication_directory_device IS NOT NULL AND publication_directory_inode IS NOT NULL AND publication_mount_id > 0)), CHECK ((delivery_state = 'delivered' AND publication_state = 'published' AND jit_state IN ('bound', 'cleaned')) OR delivery_state != 'delivered'), CHECK (storage_cleanup_proven = 0 OR resources_bound = 1))",
    ),
    (
        "capacity_reservation_bound_storage_roles",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_bound_storage_roles (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), role TEXT NOT NULL CHECK (role IN ('runner-root', 'runner-home', 'runner-work', 'runner-tmp', 'runner-shm', 'shared-run', 'dind-root', 'dind-data', 'dind-tmp', 'dind-shm', 'engine-metadata')), pool_id TEXT NOT NULL CHECK (length(pool_id) = 64 AND pool_id NOT GLOB '*[^0-9a-f]*'), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), medium TEXT NOT NULL CHECK (medium IN ('disk', 'tmpfs')), filesystem_kind TEXT NOT NULL CHECK (filesystem_kind IN ('btrfs', 'tmpfs', 'other')), total_bytes INTEGER NOT NULL CHECK (total_bytes > 0), free_bytes INTEGER NOT NULL CHECK (free_bytes >= 0 AND free_bytes <= total_bytes), available_bytes INTEGER NOT NULL CHECK (available_bytes >= 0 AND available_bytes <= free_bytes), inode_total INTEGER, inode_free INTEGER, inode_available INTEGER, inode_policy TEXT NOT NULL CHECK (inode_policy IN ('finite', 'dynamic-btrfs')), owned_usage_bytes INTEGER NOT NULL CHECK (owned_usage_bytes >= 0), owned_usage_inodes INTEGER NOT NULL CHECK (owned_usage_inodes >= 0), charge_bytes INTEGER NOT NULL CHECK (charge_bytes > 0), charge_inodes INTEGER NOT NULL CHECK (charge_inodes > 0), retained_memory_bytes INTEGER NOT NULL CHECK (retained_memory_bytes >= 0), CHECK ((inode_policy = 'finite' AND inode_total > 0 AND inode_free >= 0 AND inode_free <= inode_total AND inode_available >= 0 AND inode_available <= inode_free) OR (inode_policy = 'dynamic-btrfs' AND medium = 'disk' AND filesystem_kind = 'btrfs' AND inode_total = 0 AND inode_free = 0 AND inode_available = 0)), CHECK ((medium = 'disk' AND filesystem_kind IN ('btrfs', 'other') AND retained_memory_bytes = 0) OR (medium = 'tmpfs' AND filesystem_kind = 'tmpfs' AND retained_memory_bytes = charge_bytes)), PRIMARY KEY (reservation_id, role))",
    ),
    ("capacity_reservation_bound_storage_attachments", BOUND_STORAGE_ATTACHMENTS),
    (
        "capacity_reservation_bound_cgroups",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_bound_cgroups (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), role TEXT NOT NULL CHECK (role IN ('runner', 'dind')), container_id TEXT NOT NULL CHECK (length(container_id) BETWEEN 1 AND 256), path TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 512), memory_max_bytes INTEGER NOT NULL CHECK (memory_max_bytes > 0), memory_current_bytes INTEGER NOT NULL CHECK (memory_current_bytes >= 0 AND memory_current_bytes <= memory_max_bytes), cpu_quota_micros INTEGER NOT NULL CHECK (cpu_quota_micros > 0), cpu_period_micros INTEGER NOT NULL CHECK (cpu_period_micros > 0), pids_max INTEGER NOT NULL CHECK (pids_max > 0), PRIMARY KEY (reservation_id, role))",
    ),
    (
        "capacity_reservation_provisioning_publications",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_publications (reservation_id INTEGER NOT NULL REFERENCES capacity_reservation_provisioning(reservation_id), runner_id INTEGER NOT NULL CHECK (runner_id > 0), runner_name TEXT NOT NULL CHECK (length(runner_name) BETWEEN 1 AND 128), runner_scale_set_id INTEGER NOT NULL CHECK (runner_scale_set_id > 0), target TEXT NOT NULL CHECK (target = '/opt/velnor/job-facts/storage-cache-admission.json'), projection_sha256 TEXT NOT NULL CHECK (length(projection_sha256) = 64 AND projection_sha256 NOT GLOB '*[^0-9a-f]*'), directory_device_major INTEGER NOT NULL CHECK (directory_device_major >= 0), directory_device_minor INTEGER NOT NULL CHECK (directory_device_minor >= 0), directory_inode INTEGER NOT NULL CHECK (directory_inode > 0), mount_id INTEGER NOT NULL CHECK (mount_id > 0), v16_filesystem_id_0 INTEGER, v16_filesystem_id_1 INTEGER, filesystem_id TEXT CHECK (filesystem_id IS NULL OR (length(filesystem_id) = 16 AND filesystem_id NOT GLOB '*[^0-9a-f]*')), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), read_only INTEGER NOT NULL CHECK (read_only = 1), CHECK ((v16_filesystem_id_0 IS NOT NULL AND v16_filesystem_id_1 IS NOT NULL AND filesystem_id IS NULL) OR (v16_filesystem_id_0 IS NULL AND v16_filesystem_id_1 IS NULL AND filesystem_id IS NOT NULL)), PRIMARY KEY (reservation_id, target))",
    ),
    (
        "capacity_reservation_provisioning_cleanups",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_provisioning_cleanups (reservation_id INTEGER NOT NULL PRIMARY KEY REFERENCES capacity_reservation_provisioning(reservation_id), runner_container_id TEXT NOT NULL CHECK (length(runner_container_id) BETWEEN 1 AND 256), dind_container_id TEXT NOT NULL CHECK (length(dind_container_id) BETWEEN 1 AND 256), shared_run_volume TEXT NOT NULL CHECK (length(shared_run_volume) BETWEEN 1 AND 128), runner_home_volume TEXT NOT NULL CHECK (length(runner_home_volume) BETWEEN 1 AND 128), runner_work_volume TEXT NOT NULL CHECK (length(runner_work_volume) BETWEEN 1 AND 128), dind_data_volume TEXT NOT NULL CHECK (length(dind_data_volume) BETWEEN 1 AND 128), metadata_volume TEXT CHECK (metadata_volume IS NULL OR length(metadata_volume) BETWEEN 1 AND 128), instance_binding_digest TEXT NOT NULL CHECK (length(instance_binding_digest) = 64 AND instance_binding_digest NOT GLOB '*[^0-9a-f]*'), role_mask INTEGER NOT NULL CHECK (role_mask = 2047), containers_absent INTEGER NOT NULL CHECK (containers_absent = 1), volumes_absent INTEGER NOT NULL CHECK (volumes_absent = 1), mounts_detached INTEGER NOT NULL CHECK (mounts_detached = 1), cgroups_absent INTEGER NOT NULL CHECK (cgroups_absent = 1))",
    ),
    (
        "capacity_reservation_metadata_settlement_claims",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_metadata_settlement_claims (reservation_id INTEGER NOT NULL REFERENCES capacity_reservations(id), generation INTEGER NOT NULL CHECK (generation BETWEEN 1 AND 1024), operation_id INTEGER NOT NULL REFERENCES connection_intents(id), connection_id TEXT NOT NULL REFERENCES connection_scopes(connection_id), scale_set_id INTEGER NOT NULL CHECK (scale_set_id > 0), instance_id TEXT NOT NULL CHECK (length(instance_id) = 32 AND instance_id NOT GLOB '*[^0-9a-f]*'), probe_operation_id TEXT NOT NULL UNIQUE CHECK (length(probe_operation_id) = 32 AND probe_operation_id NOT GLOB '*[^0-9a-f]*'), profile_generation_id TEXT NOT NULL REFERENCES host_storage_profiles(generation_id), profile_digest TEXT NOT NULL CHECK (length(profile_digest) = 64 AND profile_digest NOT GLOB '*[^0-9a-f]*'), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), instance_binding_digest TEXT NOT NULL CHECK (length(instance_binding_digest) = 64 AND instance_binding_digest NOT GLOB '*[^0-9a-f]*'), PRIMARY KEY (reservation_id, generation), UNIQUE (reservation_id, generation, probe_operation_id))",
    ),
    (
        "capacity_reservation_metadata_probe_cleanups",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_metadata_probe_cleanups (reservation_id INTEGER NOT NULL, generation INTEGER NOT NULL, probe_operation_id TEXT NOT NULL, engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), operation_name TEXT NOT NULL CHECK (length(operation_name) BETWEEN 1 AND 128), container_id TEXT NOT NULL CHECK (length(container_id) BETWEEN 1 AND 256), phase TEXT NOT NULL CHECK (phase = 'removed'), lease_released INTEGER NOT NULL CHECK (lease_released = 1), PRIMARY KEY (reservation_id, generation), UNIQUE (probe_operation_id), FOREIGN KEY (reservation_id, generation, probe_operation_id) REFERENCES capacity_reservation_metadata_settlement_claims(reservation_id, generation, probe_operation_id))",
    ),
    (
        "capacity_reservation_metadata_settlements",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_metadata_settlements (reservation_id INTEGER NOT NULL, generation INTEGER NOT NULL, probe_operation_id TEXT NOT NULL, sample_generation_id TEXT NOT NULL CHECK (sample_generation_id = probe_operation_id), profile_generation_id TEXT NOT NULL REFERENCES host_storage_profiles(generation_id), profile_digest TEXT NOT NULL CHECK (length(profile_digest) = 64 AND profile_digest NOT GLOB '*[^0-9a-f]*'), engine_id TEXT NOT NULL CHECK (length(engine_id) BETWEEN 1 AND 128), docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), observed_at_unix_ms INTEGER NOT NULL CHECK (observed_at_unix_ms > 0), outcome TEXT NOT NULL CHECK (outcome IN ('settled', 'held_at_capacity')), PRIMARY KEY (reservation_id, generation), UNIQUE (probe_operation_id), FOREIGN KEY (reservation_id, generation) REFERENCES capacity_reservation_metadata_probe_cleanups(reservation_id, generation))",
    ),
    (
        "capacity_reservation_metadata_settlement_pools",
        "CREATE TABLE IF NOT EXISTS capacity_reservation_metadata_settlement_pools (reservation_id INTEGER NOT NULL, generation INTEGER NOT NULL, pool_id TEXT NOT NULL CHECK (length(pool_id) = 64 AND pool_id NOT GLOB '*[^0-9a-f]*'), filesystem_instance_digest TEXT NOT NULL CHECK (length(filesystem_instance_digest) = 64 AND filesystem_instance_digest NOT GLOB '*[^0-9a-f]*'), medium TEXT NOT NULL CHECK (medium = 'disk'), filesystem_kind TEXT NOT NULL CHECK (filesystem_kind IN ('btrfs', 'other')), inode_policy TEXT NOT NULL CHECK (inode_policy IN ('finite', 'dynamic-btrfs')), total_bytes INTEGER NOT NULL CHECK (total_bytes > 0), available_bytes INTEGER NOT NULL CHECK (available_bytes >= 0 AND available_bytes <= total_bytes), total_inodes INTEGER NOT NULL CHECK (total_inodes >= 0), available_inodes INTEGER NOT NULL CHECK (available_inodes >= 0 AND available_inodes <= total_inodes), CHECK ((inode_policy = 'finite' AND total_inodes > 0) OR (inode_policy = 'dynamic-btrfs' AND filesystem_kind = 'btrfs' AND total_inodes = 0 AND available_inodes = 0)), PRIMARY KEY (reservation_id, generation, pool_id), FOREIGN KEY (reservation_id, generation) REFERENCES capacity_reservation_metadata_settlements(reservation_id, generation))",
    ),
];

const INDEXES: [(&str, &str); 3] = [
    (
        "capacity_provisioning_pool_sample",
        "CREATE INDEX IF NOT EXISTS capacity_provisioning_pool_sample ON capacity_reservation_provisioning_pools(pool_id, filesystem_instance_digest)",
    ),
    (
        "capacity_bound_storage_pool",
        "CREATE INDEX IF NOT EXISTS capacity_bound_storage_pool ON capacity_reservation_bound_storage_roles(pool_id, filesystem_instance_digest)",
    ),
    (
        "capacity_bound_attachment_mount",
        "CREATE INDEX IF NOT EXISTS capacity_bound_attachment_mount ON capacity_reservation_bound_storage_attachments(observer, mount_id)",
    ),
];

const TRIGGERS: [(&str, &str); 56] = [
    (
        "provisioning_hold_no_update",
        "CREATE TRIGGER IF NOT EXISTS provisioning_hold_no_update BEFORE UPDATE ON capacity_reservation_provisioning BEGIN SELECT RAISE(ABORT, 'immutable provisioning hold'); END",
    ),
    (
        "provisioning_hold_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_hold_no_delete BEFORE DELETE ON capacity_reservation_provisioning BEGIN SELECT RAISE(ABORT, 'immutable provisioning hold'); END",
    ),
    (
        "provisioning_topology_no_update",
        "CREATE TRIGGER IF NOT EXISTS provisioning_topology_no_update BEFORE UPDATE ON capacity_reservation_provisioning_topology BEGIN SELECT RAISE(ABORT, 'immutable provisioning topology'); END",
    ),
    (
        "provisioning_topology_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_topology_no_delete BEFORE DELETE ON capacity_reservation_provisioning_topology BEGIN SELECT RAISE(ABORT, 'immutable provisioning topology'); END",
    ),
    (
        "provisioning_pool_no_update",
        "CREATE TRIGGER IF NOT EXISTS provisioning_pool_no_update BEFORE UPDATE ON capacity_reservation_provisioning_pools BEGIN SELECT RAISE(ABORT, 'immutable provisioning pool'); END",
    ),
    (
        "provisioning_pool_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_pool_no_delete BEFORE DELETE ON capacity_reservation_provisioning_pools BEGIN SELECT RAISE(ABORT, 'immutable provisioning pool'); END",
    ),
    (
        "provisioning_memory_no_update",
        "CREATE TRIGGER IF NOT EXISTS provisioning_memory_no_update BEFORE UPDATE ON capacity_reservation_provisioning_memory BEGIN SELECT RAISE(ABORT, 'immutable provisioning memory sample'); END",
    ),
    (
        "provisioning_memory_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_memory_no_delete BEFORE DELETE ON capacity_reservation_provisioning_memory BEGIN SELECT RAISE(ABORT, 'immutable provisioning memory sample'); END",
    ),
    (
        "provisioning_role_no_update",
        "CREATE TRIGGER IF NOT EXISTS provisioning_role_no_update BEFORE UPDATE ON capacity_reservation_provisioning_roles BEGIN SELECT RAISE(ABORT, 'immutable provisioning role'); END",
    ),
    (
        "provisioning_role_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_role_no_delete BEFORE DELETE ON capacity_reservation_provisioning_roles BEGIN SELECT RAISE(ABORT, 'immutable provisioning role'); END",
    ),
    (
        "provisioning_object_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_object_no_delete BEFORE DELETE ON capacity_reservation_provisioning_objects BEGIN SELECT RAISE(ABORT, 'immutable provisioning object'); END",
    ),
    (
        "provisioning_object_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_object_transition BEFORE UPDATE ON capacity_reservation_provisioning_objects WHEN OLD.role != NEW.role OR OLD.reservation_id != NEW.reservation_id OR OLD.docker_name != NEW.docker_name OR OLD.publication_policy_digest IS NOT NEW.publication_policy_digest OR NOT ((OLD.create_state = 'unclaimed' AND NEW.create_state = 'requested' AND NEW.docker_id IS NULL) OR (OLD.create_state = 'requested' AND NEW.create_state = 'created' AND NEW.docker_id = NEW.docker_name) OR (OLD.create_state = 'requested' AND NEW.create_state = 'uncertain' AND NEW.docker_id IS NULL)) BEGIN SELECT RAISE(ABORT, 'invalid volume transition'); END",
    ),
    (
        "provisioning_container_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_container_no_delete BEFORE DELETE ON capacity_reservation_bootstrap_containers BEGIN SELECT RAISE(ABORT, 'immutable bootstrap container'); END",
    ),
    (
        "provisioning_container_role_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_container_role_guard BEFORE UPDATE ON capacity_reservation_bootstrap_containers WHEN OLD.role != NEW.role BEGIN SELECT RAISE(ABORT, 'invalid bootstrap transition'); END",
    ),
    (
        "provisioning_container_reservation_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_container_reservation_guard BEFORE UPDATE ON capacity_reservation_bootstrap_containers WHEN OLD.reservation_id != NEW.reservation_id BEGIN SELECT RAISE(ABORT, 'invalid bootstrap transition'); END",
    ),
    (
        "provisioning_container_name_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_container_name_guard BEFORE UPDATE ON capacity_reservation_bootstrap_containers WHEN OLD.container_name != NEW.container_name BEGIN SELECT RAISE(ABORT, 'invalid bootstrap transition'); END",
    ),
    (
        "provisioning_container_unclaimed_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_container_unclaimed_transition BEFORE UPDATE ON capacity_reservation_bootstrap_containers WHEN OLD.create_state = 'unclaimed' AND NOT ((OLD.create_state = NEW.create_state AND OLD.container_id IS NEW.container_id AND OLD.start_state = NEW.start_state) OR (OLD.create_state = 'unclaimed' AND NEW.create_state = 'requested' AND NEW.container_id IS NULL AND NEW.start_state = 'not_requested')) BEGIN SELECT RAISE(ABORT, 'invalid bootstrap transition'); END",
    ),
    (
        "provisioning_container_requested_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_container_requested_transition BEFORE UPDATE ON capacity_reservation_bootstrap_containers WHEN OLD.create_state = 'requested' AND NOT ((OLD.create_state = NEW.create_state AND OLD.container_id IS NEW.container_id AND OLD.start_state = NEW.start_state) OR (OLD.create_state = 'requested' AND NEW.create_state = 'created' AND NEW.container_id IS NOT NULL AND NEW.start_state = 'not_requested') OR (OLD.create_state = 'requested' AND NEW.create_state = 'uncertain' AND NEW.container_id IS NULL AND NEW.start_state = 'not_requested')) BEGIN SELECT RAISE(ABORT, 'invalid bootstrap transition'); END",
    ),
    (
        "provisioning_container_created_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_container_created_transition BEFORE UPDATE ON capacity_reservation_bootstrap_containers WHEN OLD.create_state = 'created' AND NOT ((OLD.create_state = NEW.create_state AND OLD.container_id IS NEW.container_id AND OLD.start_state = NEW.start_state) OR (OLD.create_state = 'created' AND OLD.container_id = NEW.container_id AND ((OLD.start_state = 'not_requested' AND NEW.start_state = 'requested') OR (OLD.start_state = 'requested' AND NEW.start_state IN ('started', 'uncertain'))))) BEGIN SELECT RAISE(ABORT, 'invalid bootstrap transition'); END",
    ),
    (
        "provisioning_container_uncertain_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_container_uncertain_transition BEFORE UPDATE ON capacity_reservation_bootstrap_containers WHEN OLD.create_state = 'uncertain' AND NOT (OLD.create_state = NEW.create_state AND OLD.container_id IS NEW.container_id AND OLD.start_state = NEW.start_state) BEGIN SELECT RAISE(ABORT, 'invalid bootstrap transition'); END",
    ),
    (
        "provisioning_state_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_state_no_delete BEFORE DELETE ON capacity_reservation_provisioning_state BEGIN SELECT RAISE(ABORT, 'immutable provisioning state'); END",
    ),
    (
        "provisioning_state_reservation_guard",
        "CREATE TRIGGER IF NOT EXISTS provisioning_state_reservation_guard BEFORE UPDATE ON capacity_reservation_provisioning_state WHEN OLD.reservation_id != NEW.reservation_id BEGIN SELECT RAISE(ABORT, 'invalid provisioning state transition'); END",
    ),
    (
        "provisioning_state_bootstrap_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_state_bootstrap_transition BEFORE UPDATE ON capacity_reservation_provisioning_state WHEN NOT ((OLD.bootstrap_start_state = NEW.bootstrap_start_state AND OLD.runner_container_id IS NEW.runner_container_id AND OLD.dind_container_id IS NEW.dind_container_id) OR (OLD.bootstrap_start_state = 'not_requested' AND NEW.bootstrap_start_state = 'requested' AND NEW.runner_container_id IS NULL AND NEW.dind_container_id IS NULL) OR (OLD.bootstrap_start_state = 'requested' AND NEW.bootstrap_start_state = 'started' AND NEW.runner_container_id IS NOT NULL AND NEW.dind_container_id IS NOT NULL) OR (OLD.bootstrap_start_state = 'requested' AND NEW.bootstrap_start_state = 'uncertain' AND NEW.runner_container_id IS NULL AND NEW.dind_container_id IS NULL)) BEGIN SELECT RAISE(ABORT, 'invalid provisioning state transition'); END",
    ),
    (
        "provisioning_state_resources_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_state_resources_transition BEFORE UPDATE ON capacity_reservation_provisioning_state WHEN NOT ((OLD.resources_bound = NEW.resources_bound AND OLD.instance_binding_digest IS NEW.instance_binding_digest AND OLD.guest_boot_id IS NEW.guest_boot_id) OR (OLD.resources_bound = 0 AND NEW.resources_bound = 1 AND NEW.instance_binding_digest IS NOT NULL AND NEW.guest_boot_id IS NOT NULL)) BEGIN SELECT RAISE(ABORT, 'invalid provisioning state transition'); END",
    ),
    (
        "provisioning_state_jit_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_state_jit_transition BEFORE UPDATE ON capacity_reservation_provisioning_state WHEN NOT ((OLD.jit_state = NEW.jit_state AND OLD.runner_id IS NEW.runner_id AND OLD.runner_name IS NEW.runner_name AND OLD.runner_scale_set_id IS NEW.runner_scale_set_id) OR (OLD.jit_state = 'not_requested' AND NEW.jit_state = 'requested' AND NEW.runner_id IS NULL AND NEW.runner_name IS NULL AND NEW.runner_scale_set_id IS NULL) OR (OLD.jit_state = 'requested' AND NEW.jit_state = 'bound' AND NEW.runner_id > 0 AND NEW.runner_name IS NOT NULL AND NEW.runner_scale_set_id > 0) OR (OLD.jit_state = 'requested' AND NEW.jit_state = 'uncertain' AND NEW.runner_id IS NULL AND NEW.runner_name IS NULL AND NEW.runner_scale_set_id IS NULL) OR (OLD.jit_state = 'bound' AND NEW.jit_state = 'cleaned' AND NEW.runner_id = OLD.runner_id AND NEW.runner_name = OLD.runner_name AND NEW.runner_scale_set_id = OLD.runner_scale_set_id)) BEGIN SELECT RAISE(ABORT, 'invalid provisioning state transition'); END",
    ),
    (
        "provisioning_state_publication_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_state_publication_transition BEFORE UPDATE ON capacity_reservation_provisioning_state WHEN NOT ((OLD.publication_state = NEW.publication_state AND OLD.projection_sha256 IS NEW.projection_sha256 AND OLD.publication_directory_device IS NEW.publication_directory_device AND OLD.publication_directory_inode IS NEW.publication_directory_inode AND OLD.publication_mount_id IS NEW.publication_mount_id) OR (OLD.publication_state = 'not_requested' AND NEW.publication_state = 'requested' AND NEW.projection_sha256 IS NULL AND NEW.publication_directory_device IS NULL AND NEW.publication_directory_inode IS NULL AND NEW.publication_mount_id IS NULL) OR (OLD.publication_state = 'requested' AND NEW.publication_state = 'published' AND NEW.projection_sha256 IS NOT NULL AND NEW.publication_directory_device IS NOT NULL AND NEW.publication_directory_inode IS NOT NULL AND NEW.publication_mount_id > 0) OR (OLD.publication_state = 'requested' AND NEW.publication_state = 'uncertain' AND NEW.projection_sha256 IS NULL AND NEW.publication_directory_device IS NULL AND NEW.publication_directory_inode IS NULL AND NEW.publication_mount_id IS NULL)) BEGIN SELECT RAISE(ABORT, 'invalid provisioning state transition'); END",
    ),
    (
        "provisioning_state_delivery_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_state_delivery_transition BEFORE UPDATE ON capacity_reservation_provisioning_state WHEN NOT ((OLD.delivery_state = NEW.delivery_state) OR (OLD.delivery_state = 'not_requested' AND NEW.delivery_state = 'requested' AND NEW.publication_state = 'published' AND NEW.jit_state = 'bound') OR (OLD.delivery_state = 'requested' AND NEW.delivery_state IN ('delivered', 'uncertain'))) BEGIN SELECT RAISE(ABORT, 'invalid provisioning state transition'); END",
    ),
    (
        "provisioning_state_cleanup_transition",
        "CREATE TRIGGER IF NOT EXISTS provisioning_state_cleanup_transition BEFORE UPDATE ON capacity_reservation_provisioning_state WHEN NOT ((OLD.storage_cleanup_proven = NEW.storage_cleanup_proven) OR (OLD.storage_cleanup_proven = 0 AND NEW.storage_cleanup_proven = 1 AND NEW.resources_bound = 1)) BEGIN SELECT RAISE(ABORT, 'invalid provisioning state transition'); END",
    ),
    CLEANUP_GUARDS[0],
    CLEANUP_GUARDS[1],
    CLEANUP_GUARDS[2],
    CLEANUP_GUARDS[3],
    CLEANUP_GUARDS[4],
    CLEANUP_GUARDS[5],
    CLEANUP_GUARDS[6],
    (
        "bound_storage_no_update",
        "CREATE TRIGGER IF NOT EXISTS bound_storage_no_update BEFORE UPDATE ON capacity_reservation_bound_storage_roles BEGIN SELECT RAISE(ABORT, 'immutable storage binding'); END",
    ),
    (
        "bound_storage_no_delete",
        "CREATE TRIGGER IF NOT EXISTS bound_storage_no_delete BEFORE DELETE ON capacity_reservation_bound_storage_roles BEGIN SELECT RAISE(ABORT, 'immutable storage binding'); END",
    ),
    (
        "bound_attachment_no_update",
        "CREATE TRIGGER IF NOT EXISTS bound_attachment_no_update BEFORE UPDATE ON capacity_reservation_bound_storage_attachments BEGIN SELECT RAISE(ABORT, 'immutable storage attachment'); END",
    ),
    (
        "bound_attachment_no_delete",
        "CREATE TRIGGER IF NOT EXISTS bound_attachment_no_delete BEFORE DELETE ON capacity_reservation_bound_storage_attachments BEGIN SELECT RAISE(ABORT, 'immutable storage attachment'); END",
    ),
    (
        "provisioning_publication_no_update",
        "CREATE TRIGGER IF NOT EXISTS provisioning_publication_no_update BEFORE UPDATE ON capacity_reservation_provisioning_publications BEGIN SELECT RAISE(ABORT, 'immutable job-facts publication'); END",
    ),
    (
        "provisioning_publication_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_publication_no_delete BEFORE DELETE ON capacity_reservation_provisioning_publications BEGIN SELECT RAISE(ABORT, 'immutable job-facts publication'); END",
    ),
    (
        "provisioning_cleanup_no_update",
        "CREATE TRIGGER IF NOT EXISTS provisioning_cleanup_no_update BEFORE UPDATE ON capacity_reservation_provisioning_cleanups BEGIN SELECT RAISE(ABORT, 'immutable cleanup proof'); END",
    ),
    (
        "provisioning_cleanup_no_delete",
        "CREATE TRIGGER IF NOT EXISTS provisioning_cleanup_no_delete BEFORE DELETE ON capacity_reservation_provisioning_cleanups BEGIN SELECT RAISE(ABORT, 'immutable cleanup proof'); END",
    ),
    (
        "metadata_settlement_claim_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS metadata_settlement_claim_insert_guard BEFORE INSERT ON capacity_reservation_metadata_settlement_claims WHEN NEW.generation != COALESCE((SELECT MAX(generation) + 1 FROM capacity_reservation_metadata_settlement_claims WHERE reservation_id = NEW.reservation_id), 1) OR EXISTS (SELECT 1 FROM capacity_reservation_metadata_settlement_claims AS prior_claim JOIN resource_probe_operations AS probe ON probe.operation_id = prior_claim.probe_operation_id WHERE prior_claim.reservation_id = NEW.reservation_id AND probe.phase NOT IN ('removed', 'aborted')) OR EXISTS (SELECT 1 FROM capacity_reservation_metadata_settlement_claims AS prior_claim JOIN resource_probe_operations AS probe ON probe.operation_id = prior_claim.probe_operation_id LEFT JOIN capacity_reservation_metadata_probe_cleanups AS cleanup ON cleanup.reservation_id = prior_claim.reservation_id AND cleanup.generation = prior_claim.generation WHERE prior_claim.reservation_id = NEW.reservation_id AND probe.phase = 'removed' AND cleanup.reservation_id IS NULL) OR NOT EXISTS (SELECT 1 FROM capacity_reservations AS r JOIN connection_intents AS i ON i.id = r.intent_id AND i.connection_id = r.connection_id JOIN capacity_reservation_provisioning AS p ON p.reservation_id = r.id JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = r.id JOIN capacity_reservation_provisioning_cleanups AS c ON c.reservation_id = r.id JOIN host_storage_profile_current AS current_profile ON current_profile.singleton = 1 AND current_profile.generation_id = p.profile_generation_id JOIN resource_probe_operations AS probe ON probe.operation_id = NEW.probe_operation_id JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = probe.operation_id WHERE r.id = NEW.reservation_id AND r.state != 'released' AND r.remote_cleanup_proven = 1 AND r.local_cleanup_proven = 1 AND i.id = NEW.operation_id AND i.connection_id = NEW.connection_id AND i.scale_set_id = NEW.scale_set_id AND i.kind = 'launch' AND p.engine_id = NEW.engine_id AND p.docker_root_digest = NEW.docker_root_digest AND p.profile_generation_id = NEW.profile_generation_id AND p.profile_digest = NEW.profile_digest AND s.resources_bound = 1 AND s.storage_cleanup_proven = 1 AND s.instance_binding_digest = NEW.instance_binding_digest AND s.jit_state IN ('not_requested', 'cleaned') AND c.instance_binding_digest = NEW.instance_binding_digest AND probe.instance_id = NEW.instance_id AND probe.engine_id = NEW.engine_id AND probe.docker_root_digest = NEW.docker_root_digest AND probe.phase = 'prepared' AND lease.state = 'reserved' AND lease.engine_id = NEW.engine_id AND lease.docker_root_digest = NEW.docker_root_digest AND current_profile.generation_id = NEW.profile_generation_id) BEGIN SELECT RAISE(ABORT, 'invalid metadata settlement claim'); END",
    ),
    (
        "metadata_probe_cleanup_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS metadata_probe_cleanup_insert_guard BEFORE INSERT ON capacity_reservation_metadata_probe_cleanups WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_metadata_settlement_claims AS claim JOIN resource_probe_operations AS probe ON probe.operation_id = claim.probe_operation_id JOIN host_operation_leases AS lease ON lease.singleton = 1 AND lease.operation_id = probe.operation_id WHERE claim.reservation_id = NEW.reservation_id AND claim.generation = NEW.generation AND claim.probe_operation_id = NEW.probe_operation_id AND claim.engine_id = NEW.engine_id AND claim.docker_root_digest = NEW.docker_root_digest AND probe.phase = 'removed' AND probe.engine_id = NEW.engine_id AND probe.docker_root_digest = NEW.docker_root_digest AND probe.operation_name = NEW.operation_name AND probe.container_id = NEW.container_id AND NEW.phase = 'removed' AND lease.state = 'released' AND lease.engine_id = NEW.engine_id AND lease.docker_root_digest = NEW.docker_root_digest AND lease.operation_name = NEW.operation_name AND lease.container_id = NEW.container_id AND NEW.lease_released = 1) BEGIN SELECT RAISE(ABORT, 'invalid linked probe cleanup'); END",
    ),
    (
        "metadata_settlement_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS metadata_settlement_insert_guard BEFORE INSERT ON capacity_reservation_metadata_settlements WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_metadata_settlement_claims AS claim JOIN capacity_reservation_metadata_probe_cleanups AS cleanup ON cleanup.reservation_id = claim.reservation_id AND cleanup.generation = claim.generation JOIN host_storage_profile_current AS current_profile ON current_profile.singleton = 1 AND current_profile.generation_id = claim.profile_generation_id WHERE claim.reservation_id = NEW.reservation_id AND claim.generation = NEW.generation AND claim.probe_operation_id = NEW.probe_operation_id AND NEW.sample_generation_id = claim.probe_operation_id AND NEW.profile_generation_id = claim.profile_generation_id AND NEW.profile_digest = claim.profile_digest AND NEW.engine_id = claim.engine_id AND NEW.docker_root_digest = claim.docker_root_digest AND cleanup.probe_operation_id = NEW.probe_operation_id AND current_profile.generation_id = NEW.profile_generation_id) BEGIN SELECT RAISE(ABORT, 'invalid metadata settlement'); END",
    ),
    (
        "metadata_pool_insert_guard",
        "CREATE TRIGGER IF NOT EXISTS metadata_pool_insert_guard BEFORE INSERT ON capacity_reservation_metadata_settlement_pools WHEN NOT EXISTS (SELECT 1 FROM capacity_reservation_metadata_settlements AS settlement JOIN capacity_reservation_metadata_settlement_claims AS claim ON claim.reservation_id = settlement.reservation_id AND claim.generation = settlement.generation JOIN host_storage_profile_pools AS policy ON policy.generation_id = claim.profile_generation_id AND policy.pool_id = NEW.pool_id JOIN capacity_reservation_provisioning_pools AS held ON held.reservation_id = NEW.reservation_id AND held.pool_id = NEW.pool_id AND held.scope = 'shared-disk' WHERE settlement.reservation_id = NEW.reservation_id AND settlement.generation = NEW.generation AND policy.medium = 'disk' AND policy.inode_policy = NEW.inode_policy AND held.filesystem_instance_digest = NEW.filesystem_instance_digest AND NEW.total_bytes > 0 AND (policy.inode_policy != 'finite' OR NEW.total_inodes > 0) AND (policy.inode_policy != 'dynamic-btrfs' OR (NEW.filesystem_kind = 'btrfs' AND NEW.total_inodes = 0 AND NEW.available_inodes = 0))) BEGIN SELECT RAISE(ABORT, 'invalid sampled storage pool'); END",
    ),
    (
        "metadata_claim_no_update",
        "CREATE TRIGGER IF NOT EXISTS metadata_claim_no_update BEFORE UPDATE ON capacity_reservation_metadata_settlement_claims BEGIN SELECT RAISE(ABORT, 'immutable settlement claim'); END",
    ),
    (
        "metadata_claim_no_delete",
        "CREATE TRIGGER IF NOT EXISTS metadata_claim_no_delete BEFORE DELETE ON capacity_reservation_metadata_settlement_claims BEGIN SELECT RAISE(ABORT, 'immutable settlement claim'); END",
    ),
    (
        "metadata_probe_cleanup_no_update",
        "CREATE TRIGGER IF NOT EXISTS metadata_probe_cleanup_no_update BEFORE UPDATE ON capacity_reservation_metadata_probe_cleanups BEGIN SELECT RAISE(ABORT, 'immutable linked probe cleanup'); END",
    ),
    (
        "metadata_probe_cleanup_no_delete",
        "CREATE TRIGGER IF NOT EXISTS metadata_probe_cleanup_no_delete BEFORE DELETE ON capacity_reservation_metadata_probe_cleanups BEGIN SELECT RAISE(ABORT, 'immutable linked probe cleanup'); END",
    ),
    (
        "metadata_settlement_no_update",
        "CREATE TRIGGER IF NOT EXISTS metadata_settlement_no_update BEFORE UPDATE ON capacity_reservation_metadata_settlements BEGIN SELECT RAISE(ABORT, 'immutable metadata settlement'); END",
    ),
    (
        "metadata_settlement_no_delete",
        "CREATE TRIGGER IF NOT EXISTS metadata_settlement_no_delete BEFORE DELETE ON capacity_reservation_metadata_settlements BEGIN SELECT RAISE(ABORT, 'immutable metadata settlement'); END",
    ),
    (
        "metadata_pool_no_update",
        "CREATE TRIGGER IF NOT EXISTS metadata_pool_no_update BEFORE UPDATE ON capacity_reservation_metadata_settlement_pools BEGIN SELECT RAISE(ABORT, 'immutable metadata sample'); END",
    ),
    (
        "metadata_pool_no_delete",
        "CREATE TRIGGER IF NOT EXISTS metadata_pool_no_delete BEFORE DELETE ON capacity_reservation_metadata_settlement_pools BEGIN SELECT RAISE(ABORT, 'immutable metadata sample'); END",
    ),
    (
        "capacity_reservation_storage_release_guard",
        "CREATE TRIGGER IF NOT EXISTS capacity_reservation_storage_release_guard BEFORE UPDATE OF state ON capacity_reservations WHEN NEW.state = 'released' AND EXISTS (SELECT 1 FROM capacity_reservation_provisioning_state AS state WHERE state.reservation_id = NEW.id AND state.resources_bound = 1 AND state.storage_cleanup_proven = 1) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_metadata_settlement_claims AS claim JOIN capacity_reservation_metadata_settlements AS sample ON sample.reservation_id = claim.reservation_id AND sample.generation = claim.generation WHERE claim.reservation_id = NEW.id AND claim.generation = (SELECT MAX(latest.generation) FROM capacity_reservation_metadata_settlement_claims AS latest WHERE latest.reservation_id = NEW.id) AND sample.outcome = 'settled') BEGIN SELECT RAISE(ABORT, 'metadata capacity not settled'); END",
    ),
];

pub(super) async fn restore_bound_storage_attachment_guards(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    for (name, statement) in INDEXES
        .iter()
        .filter(|(name, _)| *name == "capacity_bound_attachment_mount")
        .chain(
            TRIGGERS
                .iter()
                .filter(|(name, _)| name.starts_with("bound_attachment_")),
        )
    {
        let _ = name;
        connection
            .execute(*statement, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}
