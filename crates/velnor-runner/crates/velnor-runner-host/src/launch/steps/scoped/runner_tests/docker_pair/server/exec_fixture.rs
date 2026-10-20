use serde::Serialize;
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use velnor_resource_probe::execution_channel_wire::ExecutionChannelContext;
use velnor_resource_probe::worker_binding::SelfStorageRequest;

#[path = "exec_fixture/channel.rs"]
mod channel;
#[path = "exec_fixture/owned_cgroups.rs"]
mod owned_cgroups;
#[path = "exec_fixture/self_storage.rs"]
mod self_storage;

pub(super) use channel::execution_channel_record;
pub(super) use owned_cgroups::owned_cgroup_record;
pub(super) use self_storage::self_storage_record;

pub(super) fn exec_record(
    line: &[u8],
    container_id: &str,
    source_revision: &str,
    metadata_inodes: [u64; 3],
) -> Result<(Vec<u8>, String), String> {
    let value: Value = serde_json::from_slice(line).map_err(|error| error.to_string())?;
    if value["schema_version"] == 1 {
        let request: SelfStorageRequest =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        request.validate().map_err(|error| error.to_string())?;
        if request.container_id != container_id {
            return Err("The Docker exec request changed the container ID.".to_owned());
        }
        return Ok((
            self_storage_record(&request, source_revision, metadata_inodes)?,
            request.operation_id,
        ));
    }
    let context: ExecutionChannelContext =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    if context.schema_version != 3 || context.operation_id.len() != 32 {
        return Err("The execution-channel request changed its wire identity.".to_owned());
    }
    Ok((
        execution_channel_record(&context, metadata_inodes)?,
        context.operation_id,
    ))
}

pub(super) async fn read_line(stream: &mut UnixStream, limit: usize) -> Result<Vec<u8>, String> {
    let mut line = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        let count = stream
            .read(&mut byte)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err(format!(
                "The Docker exec input ended after {} bytes without a complete line.",
                line.len()
            ));
        }
        line.push(byte[0]);
        if line.len() > limit {
            return Err("The Docker exec input exceeded its line limit.".to_owned());
        }
        if byte[0] == b'\n' {
            return Ok(line);
        }
    }
}

pub(super) async fn write_stdout_frame(
    stream: &mut UnixStream,
    record: &[u8],
) -> Result<(), String> {
    let length = u32::try_from(record.len())
        .map_err(|_| "The Docker exec output is too large.".to_owned())?;
    stream
        .write_all(&[1, 0, 0, 0])
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(&length.to_be_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(record)
        .await
        .map_err(|error| error.to_string())
}

fn canonical_line(record: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
