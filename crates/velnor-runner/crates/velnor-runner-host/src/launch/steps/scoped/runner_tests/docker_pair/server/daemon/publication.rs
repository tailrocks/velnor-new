use std::sync::Arc;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

use super::super::{TIMEOUT, publication_fixture, route};
use super::{CapturedRequest, ServerState, exec_fixture};

impl ServerState {
    pub(super) fn is_execution_publication_attach(&self, request: &CapturedRequest) -> bool {
        let Some(id) = route(request)
            .strip_prefix("/containers/")
            .and_then(|path| path.strip_suffix("/attach"))
        else {
            return false;
        };
        self.containers.get(id).is_some_and(|container| {
            container["Config"]["Labels"]["velnor.role"] == "execution-publication"
        })
    }

    pub(super) async fn start_execution_publication_attach(
        &mut self,
        mut stream: UnixStream,
        request: &CapturedRequest,
    ) -> Result<(), String> {
        let id = route(request)
            .strip_prefix("/containers/")
            .and_then(|path| path.strip_suffix("/attach"))
            .ok_or_else(|| "The publisher attach target has no container ID.".to_owned())?
            .to_owned();
        let logs = Arc::clone(&self.publication_logs);
        stream
            .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
            .await
            .map_err(|error| error.to_string())?;
        self.attach_tasks.push(tokio::spawn(async move {
            let line = exec_fixture::read_line(&mut stream, 256 * 1024).await?;
            let output = super::publication_fixture::publication_output(&line)?;
            let length = u32::try_from(output.len())
                .map_err(|_| "The publisher output is too large.".to_owned())?;
            let mut frame = Vec::with_capacity(output.len() + 8);
            frame.extend_from_slice(&[1, 0, 0, 0]);
            frame.extend_from_slice(&length.to_be_bytes());
            frame.extend_from_slice(&output);
            let mut tail = [0_u8; 1];
            if stream
                .read(&mut tail)
                .await
                .map_err(|error| error.to_string())?
                != 0
            {
                return Err("The publisher attach received data after request shutdown.".to_owned());
            }
            logs.lock().await.insert(id, frame);
            Ok(())
        }));
        Ok(())
    }

    pub(super) fn is_execution_publication_logs(&self, request: &CapturedRequest) -> bool {
        if request.method != "GET" {
            return false;
        }
        let Some(id) = route(request)
            .strip_prefix("/containers/")
            .and_then(|path| path.strip_suffix("/logs"))
        else {
            return false;
        };
        self.containers.get(id).is_some_and(|container| {
            container["Config"]["Labels"]["velnor.role"] == "execution-publication"
        })
    }

    pub(super) async fn execution_publication_logs(
        &self,
        request: &CapturedRequest,
    ) -> Result<(u16, Vec<u8>), String> {
        let id = route(request)
            .strip_prefix("/containers/")
            .and_then(|path| path.strip_suffix("/logs"))
            .ok_or_else(|| "The publisher logs target has no container ID.".to_owned())?;
        tokio::time::timeout(TIMEOUT, async {
            loop {
                if let Some(output) = self.publication_logs.lock().await.get(id).cloned() {
                    return Ok((200, output));
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .map_err(|_| "The publisher output did not arrive before the Docker deadline.".to_owned())?
    }
}
