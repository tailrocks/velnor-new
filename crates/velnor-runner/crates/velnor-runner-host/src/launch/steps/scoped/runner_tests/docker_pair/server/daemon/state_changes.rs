use serde_json::json;

use super::{CapturedRequest, ServerState, route};

impl ServerState {
    pub(super) fn record_docker_change(
        &mut self,
        request: &CapturedRequest,
        response: &(u16, Vec<u8>),
    ) -> Result<(), String> {
        match (request.method.as_str(), route(request)) {
            ("POST", "/volumes/create") => self.record_volume(response),
            ("POST", "/networks/create") => self.record_network(request),
            ("POST", "/containers/create") => self.record_container(request, response),
            ("POST", path) if path.ends_with("/start") => {
                super::super::responses::mark_container_started(&mut self.containers, request)
            }
            ("POST", path) if path.ends_with("/stop") => self.mark_container_stopped(request),
            ("POST", path) if path.starts_with("/containers/") && path.ends_with("/wait") => {
                self.mark_container_exited(request)
            }
            ("DELETE", path) if path.starts_with("/volumes/") => self.remove_volume(request),
            ("DELETE", path) if path.starts_with("/networks/") => self.remove_network(request),
            ("DELETE", path) if path.starts_with("/containers/") => self.remove_container(request),
            _ => Ok(()),
        }
    }

    pub(super) fn mark_container_exited(
        &mut self,
        request: &CapturedRequest,
    ) -> Result<(), String> {
        let id = route(request)
            .strip_prefix("/containers/")
            .and_then(|path| path.strip_suffix("/wait"))
            .ok_or_else(|| "The Docker wait path has no container ID.".to_owned())?;
        let container = self
            .containers
            .get_mut(id)
            .ok_or_else(|| "The Docker wait target is not owned.".to_owned())?;
        let pid = container["State"]["Pid"].clone();
        container["State"] = json!({
            "Status": "exited",
            "Running": false,
            "Pid": pid,
            "ExitCode": 0,
            "OOMKilled": false
        });
        Ok(())
    }

    fn mark_container_stopped(&mut self, request: &CapturedRequest) -> Result<(), String> {
        let id = route(request)
            .strip_prefix("/containers/")
            .and_then(|path| path.strip_suffix("/stop"))
            .ok_or_else(|| "The Docker stop path has no container ID.".to_owned())?;
        let container = self
            .containers
            .get_mut(id)
            .ok_or_else(|| "The Docker stop target is not owned.".to_owned())?;
        container["State"] =
            json!({"Status": "exited", "Running": false, "ExitCode": 0, "OOMKilled": false});
        Ok(())
    }

    pub(super) fn remove_container(&mut self, request: &CapturedRequest) -> Result<(), String> {
        let id = route(request)
            .strip_prefix("/containers/")
            .ok_or_else(|| "The Docker remove path has no container ID.".to_owned())?;
        let id = id.to_owned();
        self.containers.retain(|_, container| container["Id"] != id);
        Ok(())
    }

    fn remove_volume(&mut self, request: &CapturedRequest) -> Result<(), String> {
        let name = route(request)
            .strip_prefix("/volumes/")
            .ok_or_else(|| "The Docker volume delete path is invalid.".to_owned())?;
        self.volumes.remove(name);
        Ok(())
    }

    fn remove_network(&mut self, request: &CapturedRequest) -> Result<(), String> {
        let id = route(request)
            .strip_prefix("/networks/")
            .ok_or_else(|| "The Docker network delete path is invalid.".to_owned())?;
        self.networks
            .retain(|name, network| name != id && network["Id"] != id);
        Ok(())
    }
}
