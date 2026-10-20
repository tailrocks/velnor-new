use sha2::{Digest, Sha256};

use super::{CleanupObserverRequest, validate_binding};

/// Validate the shared request identity and its canonical binding digest.
///
/// # Errors
///
/// Returns an error if a request field or digest is incorrect.
pub fn validate_request(request: &CleanupObserverRequest) -> Result<(), &'static str> {
    if request.schema_version != 1
        || !is_lower_hex(&request.operation_id, 32)
        || !is_lower_hex(&request.binding_sha256, 64)
    {
        return Err("cleanup_request_identity");
    }
    validate_binding(&request.binding)?;
    let bytes = serde_json::to_vec(&request.binding).map_err(|_| "cleanup_binding_encode")?;
    if lower_hex(&Sha256::digest(bytes)) != request.binding_sha256 {
        return Err("cleanup_binding_digest");
    }
    Ok(())
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::validate_request;
    use crate::cleanup_observer_wire::{
        CleanupDockerRootIdentity, CleanupObserverBinding, CleanupObserverRequest, CleanupVolume,
        ProcessIdentity,
    };

    fn valid_request() -> CleanupObserverRequest {
        let binding = valid_binding();
        let encoded = serde_json::to_vec(&binding).expect("binding serializes");
        CleanupObserverRequest {
            schema_version: 1,
            operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
            binding_sha256: hex(&Sha256::digest(encoded)),
            binding,
        }
    }

    fn valid_binding() -> CleanupObserverBinding {
        CleanupObserverBinding {
            expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
            docker_root: CleanupDockerRootIdentity {
                target: "/velnor/docker-root".to_owned(),
                mount_id: 10,
                statx_mount_id: 10,
                mount_root: "/".to_owned(),
                mount_device: [8, 1],
                statx_device: [8, 1],
                filesystem_type_magic: 0x0102_1994,
                filesystem_id: 20,
                read_only: true,
            },
            daemon: process(1, 101),
            runner: process(2, 102),
            dind: process(3, 103),
            cgroup_paths: [
                "/sys/fs/cgroup/runner".to_owned(),
                "/sys/fs/cgroup/dind".to_owned(),
            ],
            volume_mountpoints: ["shared-run", "runner-home", "runner-work", "dind-data"]
                .into_iter()
                .enumerate()
                .map(|(index, role)| CleanupVolume {
                    role: role.to_owned(),
                    mountpoint: format!("/var/lib/docker/volumes/volume-{index}/_data"),
                    mount_device: [8, u32::try_from(index).expect("four fixture volumes") + 1],
                    statx_device: [8, u32::try_from(index).expect("four fixture volumes") + 1],
                    inode: index as u64 + 100,
                })
                .collect(),
            owned_tmpfs_devices: [
                [0, 10],
                [0, 11],
                [0, 12],
                [0, 13],
                [0, 14],
                [0, 15],
                [0, 16],
                [0, 17],
            ],
        }
    }

    fn process(pid: u32, namespace_inode: u64) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            start_ticks: u64::from(pid) * 10,
            namespace_device: [0, 6],
            namespace_inode,
        }
    }

    fn hex(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        bytes
            .iter()
            .flat_map(|byte| {
                [
                    char::from(HEX[usize::from(byte >> 4)]),
                    char::from(HEX[usize::from(byte & 0x0f)]),
                ]
            })
            .collect()
    }

    #[test]
    fn validates_canonical_request_and_plain_binding_digest() {
        let request = valid_request();
        let encoded = serde_json::to_vec(&request.binding).expect("binding serializes");
        assert_eq!(request.binding_sha256, hex(&Sha256::digest(&encoded)));
        assert!(!encoded.ends_with(b"\n"));
        assert_eq!(validate_request(&request), Ok(()));
    }

    #[test]
    fn rejects_wrong_schema_or_operation_identity() {
        let mut request = valid_request();
        request.schema_version = 2;
        assert_eq!(validate_request(&request), Err("cleanup_request_identity"));

        let mut request = valid_request();
        request.operation_id.replace_range(..1, "A");
        assert_eq!(validate_request(&request), Err("cleanup_request_identity"));
    }

    #[test]
    fn rejects_wrong_digest_and_changed_binding() {
        let mut request = valid_request();
        request.binding_sha256.replace_range(..1, "f");
        assert_eq!(validate_request(&request), Err("cleanup_binding_digest"));

        let mut request = valid_request();
        request.binding.daemon.start_ticks += 1;
        assert_eq!(validate_request(&request), Err("cleanup_binding_digest"));
    }

    #[test]
    fn rejects_invalid_binding_even_with_matching_digest() {
        let mut request = valid_request();
        request.binding.docker_root.read_only = false;
        let encoded = serde_json::to_vec(&request.binding).expect("binding serializes");
        request.binding_sha256 = hex(&Sha256::digest(encoded));
        assert_eq!(
            validate_request(&request),
            Err("cleanup_docker_root_identity")
        );
    }
}
