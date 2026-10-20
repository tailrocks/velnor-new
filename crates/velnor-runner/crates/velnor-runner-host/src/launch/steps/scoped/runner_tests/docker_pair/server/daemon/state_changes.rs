use serde_json::json;
use std::sync::{Mutex, OnceLock};

use super::{CapturedRequest, ServerState, route};

#[derive(Clone, Copy, Eq, PartialEq)]
enum VolumeFailure {
    DeleteTransport,
    InspectTransport,
}

static VOLUME_FAILURE: OnceLock<Mutex<Option<(String, VolumeFailure)>>> = OnceLock::new();

fn fail_once(name: &str, failure: VolumeFailure) -> bool {
    let Some(injection) = VOLUME_FAILURE.get() else {
        return false;
    };
    let Ok(mut injection) = injection.lock() else {
        return false;
    };
    if injection
        .as_ref()
        .is_some_and(|(target, kind)| target == name && *kind == failure)
    {
        injection.take();
        true
    } else {
        false
    }
}

impl CapturedRequest {
    /// Inject a transport failure for deletion of one named fixture volume.
    pub(crate) fn inject_volume_delete_transport_failure(name: &str) {
        set_failure(name, VolumeFailure::DeleteTransport);
    }

    /// Inject a transport failure when inspecting one absent fixture volume.
    pub(crate) fn inject_volume_inspect_transport_failure(name: &str) {
        set_failure(name, VolumeFailure::InspectTransport);
    }
}

fn set_failure(name: &str, failure: VolumeFailure) {
    let injection = VOLUME_FAILURE.get_or_init(|| Mutex::new(None));
    if let Ok(mut injection) = injection.lock() {
        *injection = Some((name.to_owned(), failure));
    }
}

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
            ("GET", path) if path.starts_with("/volumes/") => {
                let name = path.trim_start_matches("/volumes/");
                if !self.volumes.contains_key(name)
                    && fail_once(name, VolumeFailure::InspectTransport)
                {
                    return Err("Injected Docker volume inspection transport failure.".to_owned());
                }
                Ok(())
            }
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
        if fail_once(name, VolumeFailure::DeleteTransport) {
            return Err("Injected Docker volume deletion transport failure.".to_owned());
        }
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
