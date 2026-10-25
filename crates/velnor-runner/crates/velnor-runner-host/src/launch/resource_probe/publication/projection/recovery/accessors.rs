use super::PublicationRecoveryProjection;

impl PublicationRecoveryProjection {
    pub(in crate::launch::resource_probe::publication) fn projection_digest(&self) -> &str {
        &self.projection_digest
    }

    pub(in crate::launch::resource_probe::publication) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }

    pub(in crate::launch::resource_probe::publication) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }

    pub(in crate::launch::resource_probe::publication) fn metadata_volume_mountpoint(
        &self,
    ) -> &str {
        &self.metadata_volume_mountpoint
    }

    pub(in crate::launch::resource_probe::publication) fn matches_record(
        &self,
        record: &crate::journal::ExecutionPublicationRecoveryRecord,
    ) -> bool {
        self.operation_id == record.operation_id
            && self.instance_id == record.instance_id
            && self.engine_id == record.engine_id
            && self.docker_root_digest == record.docker_root_digest
            && self.source_revision == record.source_revision
            && self.runtime_image_id == record.runtime_image_id
            && self.image_binding_digest == record.image_binding_digest
            && self.projection_digest == record.projection_digest
            && self.parent_kind == record.parent_kind
            && self.parent_operation_id == record.parent_operation_id
            && self.reservation_id == record.reservation_id
            && self.connection_id == record.connection_id
            && self.runner_id == record.runner_id
            && self.runner_container_id == record.runner_container_id
            && self.dind_container_id == record.dind_container_id
            && self.metadata_volume_name == record.metadata_volume_name
            && self.metadata_volume_id == record.metadata_volume_id
            && self.metadata_layout_version == record.metadata_layout_version
            && self.metadata_layout_digest == record.metadata_layout_digest
            && self.guest_boot_id == record.guest_boot_id
            && self.expected_payload_sha256 == record.payload_sha256
    }

    pub(in crate::launch::resource_probe::publication) fn operation_name(&self) -> &str {
        &self.operation_name
    }

    pub(in crate::launch::resource_probe::publication) fn metadata_operation_id(&self) -> String {
        self.parent_operation_id.to_string()
    }
}
