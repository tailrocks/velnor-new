use serde::Serialize;

#[derive(Serialize)]
pub(super) struct BootstrapPhaseEvidence {
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) platform: String,
    pub(super) daemon_architecture: String,
    pub(super) containers: Vec<ContainerObservation>,
}

#[derive(Serialize)]
pub(super) struct ContainerObservation {
    role: String,
    name: String,
    id: Option<String>,
    outcome: &'static str,
    spec_sha256: String,
    image_runtime_id: String,
    labels: Option<std::collections::HashMap<String, String>>,
}

impl ContainerObservation {
    pub(super) fn new(
        role: &str,
        name: &str,
        id: Option<&str>,
        outcome: &'static str,
        spec_sha256: &str,
        image_runtime_id: &str,
        labels: Option<std::collections::HashMap<String, String>>,
    ) -> Self {
        Self {
            role: role.to_owned(),
            name: name.to_owned(),
            id: id.map(str::to_owned),
            outcome,
            spec_sha256: spec_sha256.to_owned(),
            image_runtime_id: image_runtime_id.to_owned(),
            labels,
        }
    }
}
