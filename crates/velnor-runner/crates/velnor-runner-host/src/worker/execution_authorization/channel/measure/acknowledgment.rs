use std::future::Future;

use crate::error::HostError;

pub(super) async fn validated_then_ack<V, A, F>(
    validate: V,
    acknowledge: A,
) -> Result<(), HostError>
where
    V: FnOnce() -> Result<(), HostError>,
    A: FnOnce() -> F,
    F: Future<Output = Result<(), HostError>>,
{
    validate()?;
    acknowledge().await
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use crate::error::HostError;
    use velnor_resource_probe::execution_channel_wire::{
        ExecutionChannelDirectory, ExecutionChannelObservation,
    };

    use super::validated_then_ack;
    use crate::worker::execution_authorization::channel::measure::validate_observation;

    #[tokio::test]
    async fn rejected_channel_record_does_not_send_success_ack() {
        let acknowledged = Cell::new(false);
        let observation = ExecutionChannelObservation {
            schema_version: 3,
            operation_id: "different-operation".to_owned(),
            guest_boot_id: "expected-boot".to_owned(),
            mount_namespace_id: 1,
            control: directory(),
            inbox: directory(),
            job_facts: directory(),
        };
        let result = validated_then_ack(
            || validate_observation(&observation, "expected-operation", "expected-boot"),
            || async {
                acknowledged.set(true);
                Ok(())
            },
        )
        .await;

        assert!(matches!(result, Err(HostError::Identity)));
        assert!(!acknowledged.get());
    }

    fn directory() -> ExecutionChannelDirectory {
        ExecutionChannelDirectory {
            target: String::new(),
            inode: 1,
            owner_uid: 0,
            owner_gid: 0,
            mode: 0,
            mount_id: 1,
            statx_mount_id: 1,
            mount_device_major: 0,
            mount_device_minor: 0,
            statx_device_major: 0,
            statx_device_minor: 0,
            mount_root: String::new(),
            mount_source: String::new(),
            filesystem_type_magic: 0,
            filesystem_type_name: String::new(),
            filesystem_id: 0,
            read_only: false,
            executable_mount: false,
        }
    }
}
