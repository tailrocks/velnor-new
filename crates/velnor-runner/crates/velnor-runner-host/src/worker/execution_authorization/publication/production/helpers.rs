use std::time::Instant;

use crate::error::HostError;

pub(super) fn append_mount_root(root: &str, subpath: &str) -> String {
    if root == "/" {
        subpath.to_owned()
    } else {
        format!("{}{subpath}", root.trim_end_matches('/'))
    }
}

pub(super) fn valid_mount_root(value: &str) -> bool {
    value.starts_with('/')
        && !value.chars().any(char::is_control)
        && (value == "/"
            || (!value.ends_with('/')
                && !value.contains("//")
                && value
                    .split('/')
                    .skip(1)
                    .all(|part| !part.is_empty() && part != "." && part != "..")))
}

pub(super) fn valid_operation_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) async fn within<T>(
    deadline: Instant,
    future: impl std::future::Future<Output = T>,
) -> Result<T, HostError> {
    tokio::time::timeout_at(deadline.into(), future)
        .await
        .map_err(|_| HostError::DockerTimeout)
}
