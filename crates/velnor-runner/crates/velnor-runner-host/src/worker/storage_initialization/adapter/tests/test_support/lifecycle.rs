use super::*;

impl InitializationDockerApi {
    pub(crate) async fn wait_for_create(&mut self) -> Result<(), HostError> {
        self.create_seen
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(crate) fn release_create(&mut self) {
        if let Some(release) = self.release_create.take() {
            let _sent = release.send(());
        }
    }

    pub(crate) async fn wait_for_create_completion(&mut self) -> Result<(), HostError> {
        self.create_completed
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(crate) async fn wait_for_create_response(&mut self) -> Result<(), HostError> {
        self.response_seen
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(crate) fn release_create_response(&mut self) {
        if let Some(release) = self.release_response.take() {
            let _sent = release.send(());
        }
    }

    pub(crate) async fn wait_for_delete(&mut self) -> Result<(), HostError> {
        self.delete_seen
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(crate) fn release_delete_response(&mut self) {
        if let Some(release) = self.release_delete_response.take() {
            let _sent = release.send(());
        }
    }

    pub(crate) async fn wait_for_inspect(&mut self) -> Result<(), HostError> {
        self.inspect_seen
            .take()
            .ok_or(HostError::Docker)?
            .await
            .map_err(|_| HostError::Docker)
    }

    pub(crate) fn release_inspect_response(&mut self) {
        if let Some(release) = self.release_inspect_response.take() {
            let _sent = release.send(());
        }
    }

    pub(crate) async fn finish(mut self) -> Result<ApiCapture, HostError> {
        self.release_create();
        self.release_create_response();
        self.release_delete_response();
        self.release_inspect_response();
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        let task = self.task.take().ok_or(HostError::Docker)?;
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .map_err(|_| HostError::DockerTimeout)?
            .map_err(|_| HostError::Docker)?
            .map_err(|_| HostError::Docker)?;
        std::fs::remove_file(&self.path).map_err(|_| HostError::Path)?;
        let capture = self.capture.lock().await;
        Ok(ApiCapture {
            requests: capture.requests.clone(),
            stdin: capture.stdin.clone(),
            metadata_inodes: capture.metadata_inodes,
            create_name: capture.create_name.clone(),
        })
    }
}

impl Drop for InitializationDockerApi {
    fn drop(&mut self) {
        self.release_create();
        self.release_create_response();
        self.release_delete_response();
        self.release_inspect_response();
        if let Some(stop) = self.stop.take() {
            let _sent = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let _removed = std::fs::remove_file(&self.path);
    }
}
