use super::*;

pub(crate) async fn serve_attach_stream(
    mut stream: UnixStream,
    expected_operation: String,
    capture: Arc<Mutex<ApiCapture>>,
) -> Result<(), String> {
    let Some(payload) = read_bounded_line(&mut stream, 256).await? else {
        return Ok(());
    };
    if payload.is_empty() {
        return Ok(());
    }
    let input: Value = serde_json::from_slice(&payload).map_err(|error| error.to_string())?;
    let operation_key = input["operation_id"]
        .as_str()
        .ok_or_else(|| "The initializer request has no operation ID.".to_owned())?;
    if operation_key != expected_operation {
        return Err(
            "The initializer request operation does not match the create operation.".to_owned(),
        );
    }
    let mut result = format!(
            "{{\"schema\":2,\"operation_id\":\"{operation_key}\",\"home\":{{\"target\":\"/velnor/init/home\",\"mount_id\":31,\"parent_mount_id\":1,\"device_major\":0,\"device_minor\":48,\"inode\":61,\"uid\":1000,\"gid\":1000,\"mode\":493}},\"work\":{{\"target\":\"/velnor/init/work\",\"mount_id\":32,\"parent_mount_id\":1,\"device_major\":0,\"device_minor\":48,\"inode\":62,\"uid\":1000,\"gid\":1000,\"mode\":493}},\"metadata\":{{\"target\":\"/velnor/init/metadata\",\"mount_id\":33,\"parent_mount_id\":1,\"device_major\":0,\"device_minor\":48,\"inode\":63,\"uid\":0,\"gid\":0,\"mode\":365}},\"runtime_identity\":{{\"target\":\"/velnor/init/metadata/runtime-identity\",\"mount_id\":33,\"parent_mount_id\":33,\"device_major\":0,\"device_minor\":48,\"inode\":64,\"uid\":0,\"gid\":0,\"mode\":365}},\"job_facts\":{{\"target\":\"/velnor/init/metadata/job-facts\",\"mount_id\":33,\"parent_mount_id\":33,\"device_major\":0,\"device_minor\":48,\"inode\":65,\"uid\":0,\"gid\":0,\"mode\":365}},\"source_proof\":{{\"target\":\"/velnor/init/metadata/source-proof\",\"mount_id\":33,\"parent_mount_id\":33,\"device_major\":0,\"device_minor\":48,\"inode\":66,\"uid\":0,\"gid\":0,\"mode\":493}},\"control\":{{\"target\":\"/velnor/init/metadata/source-proof/control\",\"mount_id\":33,\"parent_mount_id\":33,\"device_major\":0,\"device_minor\":48,\"inode\":67,\"uid\":0,\"gid\":0,\"mode\":365}},\"inbox\":{{\"target\":\"/velnor/init/home/source-proof/inbox\",\"mount_id\":31,\"parent_mount_id\":31,\"device_major\":0,\"device_minor\":48,\"inode\":68,\"uid\":1000,\"gid\":1000,\"mode\":448}}}}"
        )
        .into_bytes();
    let response: Value = serde_json::from_slice(&result).map_err(|error| error.to_string())?;
    let inode = |field: &str| {
        response[field]["inode"]
            .as_u64()
            .ok_or_else(|| format!("The initializer response has no {field} inode."))
    };
    let metadata_inodes = [
        inode("runtime_identity")?,
        inode("job_facts")?,
        inode("control")?,
    ];
    result.push(b'\n');
    let mut frame = Vec::with_capacity(8 + result.len());
    frame.extend_from_slice(&[1, 0, 0, 0]);
    let result_length = u32::try_from(result.len()).map_err(|error| error.to_string())?;
    frame.extend_from_slice(&result_length.to_be_bytes());
    frame.extend_from_slice(&result);
    stream
        .write_all(&frame)
        .await
        .map_err(|error| error.to_string())?;
    let mut capture = capture.lock().await;
    capture.stdin = payload;
    capture.metadata_inodes = Some(metadata_inodes);
    drop(capture);
    let Some(acknowledgment) = read_bounded_line(&mut stream, 256).await? else {
        return Ok(());
    };
    let ack: Value = serde_json::from_slice(&acknowledgment).map_err(|error| error.to_string())?;
    if ack != json!({"operation_id": operation_key}) {
        return Err("The initializer acknowledgment does not match the request.".to_owned());
    }
    let mut trailing = [0_u8; 1];
    if stream
        .read(&mut trailing)
        .await
        .map_err(|error| error.to_string())?
        != 0
    {
        return Err("The initializer acknowledgment had extra bytes.".to_owned());
    }
    Ok(())
}

async fn read_bounded_line(
    stream: &mut UnixStream,
    limit: usize,
) -> Result<Option<Vec<u8>>, String> {
    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let count = stream
            .read(&mut byte)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err("The initializer protocol line was incomplete.".to_owned())
            };
        }
        line.push(byte[0]);
        if line.len() > limit {
            return Err("The initializer protocol line was too long.".to_owned());
        }
        if byte[0] == b'\n' {
            return Ok(Some(line));
        }
    }
}
