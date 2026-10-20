use std::sync::Arc;

use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

use super::{CapturedRequest, ExecRecord, ServerState, exec_fixture, route};

impl ServerState {
    pub(super) fn is_owned_cgroup_probe_attach(&self, request: &CapturedRequest) -> bool {
        let Some(id) = route(request)
            .strip_prefix("/containers/")
            .and_then(|path| path.strip_suffix("/attach"))
        else {
            return false;
        };
        self.containers.get(id).is_some_and(|container| {
            container["Config"]["Labels"]["velnor.probe.purpose"] == "owned-cgroups"
        })
    }

    pub(super) async fn start_owned_cgroup_attach(
        &mut self,
        mut stream: UnixStream,
    ) -> Result<(), String> {
        stream
            .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
            .await
            .map_err(|error| error.to_string())?;
        self.attach_tasks.push(tokio::spawn(async move {
            let line = exec_fixture::read_line(&mut stream, 8192).await?;
            let request: velnor_resource_probe::worker_binding::OwnedCgroupRequest =
                serde_json::from_slice(&line).map_err(|error| error.to_string())?;
            request.validate().map_err(|error| error.to_string())?;
            if request.runner_container_id != super::super::RUNNER_ID
                || request.dind_container_id != super::super::DIND_ID
                || request.docker_root_path != "/var/lib/docker"
            {
                return Err("Owned-cgroup request changed the acquired worker identity".to_owned());
            }
            let record = exec_fixture::owned_cgroup_record(&request)?;
            exec_fixture::write_stdout_frame(&mut stream, &record).await
        }));
        Ok(())
    }

    pub(super) fn create_exec(
        &mut self,
        request: &CapturedRequest,
    ) -> Result<(u16, Vec<u8>), String> {
        let container_id = route(request)
            .strip_prefix("/containers/")
            .and_then(|path| path.strip_suffix("/exec"))
            .ok_or_else(|| "The Docker exec target has no container ID.".to_owned())?;
        let container = self
            .containers
            .get(container_id)
            .ok_or_else(|| "Docker exec target is not an owned worker".to_owned())?;
        if container["State"]["Running"] != true {
            return Err("Docker exec target is not running".to_owned());
        }
        let suffix = if container_id == super::super::DIND_ID {
            "7"
        } else {
            "6"
        };
        let id = format!("{}{}", suffix.repeat(63), suffix);
        let pid = if container_id == super::super::DIND_ID {
            1_302
        } else {
            1_301
        };
        self.execs
            .try_lock()
            .map_err(|_| "The Docker exec state is busy.".to_owned())?
            .insert(
                id.clone(),
                ExecRecord {
                    container_id: container_id.to_owned(),
                    pid,
                    running: true,
                },
            );
        Ok((
            201,
            serde_json::json!({ "Id": id }).to_string().into_bytes(),
        ))
    }

    pub(super) fn inspect_exec(&self, request: &CapturedRequest) -> Result<(u16, Vec<u8>), String> {
        let id = route(request)
            .strip_prefix("/exec/")
            .and_then(|path| path.strip_suffix("/json"))
            .ok_or_else(|| "The Docker exec inspect target has no ID.".to_owned())?;
        let state = self
            .execs
            .try_lock()
            .map_err(|_| "The Docker exec state is busy.".to_owned())?;
        let record = state
            .get(id)
            .ok_or_else(|| "Docker exec ID is not owned".to_owned())?;
        let body = serde_json::json!({
            "ID": id,
            "ContainerID": record.container_id,
            "Running": record.running,
            "ExitCode": if record.running { Value::Null } else { serde_json::json!(0) },
            "Pid": record.pid
        });
        Ok((200, body.to_string().into_bytes()))
    }

    pub(super) async fn start_exec(
        &mut self,
        mut stream: UnixStream,
        request: &CapturedRequest,
        request_index: usize,
    ) -> Result<(), String> {
        let id = route(request)
            .strip_prefix("/exec/")
            .and_then(|path| path.strip_suffix("/start"))
            .ok_or_else(|| "The Docker exec start target has no ID.".to_owned())?
            .to_owned();
        let record = self
            .execs
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| "Docker exec start ID is not owned".to_owned())?;
        {
            let mut requests = self
                .requests
                .lock()
                .map_err(|_| "Docker request list is poisoned".to_owned())?;
            let captured = requests
                .get_mut(request_index)
                .ok_or_else(|| "Docker exec request record is absent".to_owned())?;
            captured.response_status = Some(101);
        }
        stream
            .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
            .await
            .map_err(|error| error.to_string())?;
        let execs = Arc::clone(&self.execs);
        let source_revision = self.images.source_sha().to_owned();
        let initializer = Arc::clone(&self.initializer()?.1);
        let tamper_channel_response = Arc::clone(&self.tamper_channel_response);
        self.attach_tasks.push(tokio::spawn(async move {
            let line = exec_fixture::read_line(&mut stream, 8192)
                .await
                .map_err(|error| format!("Exec {id} request: {error}"))?;
            let metadata_inodes = initializer.lock().await.metadata_inodes().await?;
            let is_channel_request = serde_json::from_slice::<Value>(&line)
                .ok()
                .is_some_and(|request| request["schema_version"] == 3);
            let (response, operation_id) = exec_fixture::exec_record(
                &line,
                &record.container_id,
                &source_revision,
                metadata_inodes,
                if is_channel_request {
                    tamper_channel_response.swap(0, std::sync::atomic::Ordering::SeqCst)
                } else {
                    0
                },
            )?;
            exec_fixture::write_stdout_frame(&mut stream, &response).await?;
            let ack = exec_fixture::read_line(&mut stream, 128)
                .await
                .map_err(|error| format!("Exec {id} acknowledgment: {error}"))?;
            velnor_resource_probe::worker_binding::validate_acknowledgment(&ack, &operation_id)
                .map_err(|error| error.to_string())?;
            let mut tail = [0_u8; 1];
            if stream
                .read(&mut tail)
                .await
                .map_err(|error| error.to_string())?
                != 0
            {
                return Err(
                    "The Docker exec server received data after the acknowledgment.".to_owned(),
                );
            }
            if let Some(state) = execs.lock().await.get_mut(&id) {
                state.running = false;
            }
            Ok(())
        }));
        Ok(())
    }
}
