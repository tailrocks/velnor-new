use super::*;

pub(super) fn assert_verified_preflight_before_mutations(
    requests: &[CapturedRequest],
    images: &crate::launch::VerifiedWorkerImages,
) -> Result<(), String> {
    let mut checks = PreflightChecks::default();
    for request in requests {
        checks.observe(request, images)?;
    }
    Ok(())
}

#[derive(Default)]
struct PreflightChecks {
    engine: bool,
    runner: bool,
    dind: bool,
}

impl PreflightChecks {
    fn observe(
        &mut self,
        request: &CapturedRequest,
        images: &crate::launch::VerifiedWorkerImages,
    ) -> Result<(), String> {
        let path = route(request);
        match (request.method.as_str(), path) {
            ("GET", "/info") => self.observe_engine(request, images),
            ("GET", image_path) if image_path.starts_with("/images/") => {
                self.observe_image(request, images)
            }
            ("POST", "/volumes/create" | "/networks/create") => {
                self.require_engine("resource mutation")?;
                self.reset();
                Ok(())
            }
            ("POST", "/containers/create") => {
                self.require_container_create(request)?;
                self.reset();
                Ok(())
            }
            ("POST", path) if path.ends_with("/start") => {
                self.require_container_start(path)?;
                self.reset();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn observe_engine(
        &mut self,
        request: &CapturedRequest,
        images: &crate::launch::VerifiedWorkerImages,
    ) -> Result<(), String> {
        let info = serde_json::from_slice::<serde_json::Value>(&request.response_body)
            .map_err(|error| format!("Docker info response is invalid: {error}"))?;
        assert_eq!(request.response_status, Some(200));
        assert_eq!(info["ID"], images.engine_id());
        assert_eq!(
            info["DockerRootDir"],
            super::super::super::support::DOCKER_ROOT
        );
        assert_eq!(info["Architecture"], images.daemon_architecture());
        self.engine = true;
        Ok(())
    }

    fn observe_image(
        &mut self,
        request: &CapturedRequest,
        images: &crate::launch::VerifiedWorkerImages,
    ) -> Result<(), String> {
        let normalized = request.target.replace("%3A", ":").replace("%3a", ":");
        let image = serde_json::from_slice::<serde_json::Value>(&request.response_body)
            .map_err(|error| format!("Docker image response is invalid: {error}"))?;
        assert_eq!(request.response_status, Some(200));
        if normalized.contains(images.runner().runtime_id()) {
            assert_image_identity(&image, images, "velnor-runner", images.runner());
            self.runner = true;
        } else if normalized.contains(images.dind().runtime_id()) {
            assert_image_identity(&image, images, "velnor-dind", images.dind());
            self.dind = true;
        } else {
            return Err("Docker inspected an image outside the selected worker bundle.".to_owned());
        }
        Ok(())
    }

    fn require_engine(&self, operation: &str) -> Result<(), String> {
        if self.engine {
            Ok(())
        } else {
            Err(format!(
                "{operation} must follow fresh engine verification."
            ))
        }
    }

    fn require_container_create(&self, request: &CapturedRequest) -> Result<(), String> {
        self.require_engine("Container creation")?;
        let config = serde_json::from_slice::<serde_json::Value>(&request.body)
            .map_err(|error| format!("Docker create request is invalid: {error}"))?;
        if matches!(
            config["Labels"]["io.velnor.role"].as_str(),
            Some("runner" | "dind")
        ) {
            if !self.runner {
                return Err("Worker creation must verify the runner image.".to_owned());
            }
            if !self.dind {
                return Err("Worker creation must verify the DinD image.".to_owned());
            }
        }
        Ok(())
    }

    fn require_container_start(&self, path: &str) -> Result<(), String> {
        self.require_engine("Container start")?;
        if path.contains(DIND_ID) {
            if !self.dind {
                return Err("DinD start must verify the DinD image.".to_owned());
            }
        } else if path.contains(RUNNER_ID) {
            if !self.runner {
                return Err("Runner start must verify the runner image.".to_owned());
            }
        } else if !path.contains(INITIALIZER_ID) {
            return Err(format!(
                "Container start has an unknown worker identity: {path}"
            ));
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.engine = false;
        self.runner = false;
        self.dind = false;
    }
}

fn assert_image_identity(
    image: &serde_json::Value,
    images: &crate::launch::VerifiedWorkerImages,
    name: &str,
    identity: &crate::launch::VerifiedImageIdentity,
) {
    assert_eq!(image["Id"], identity.runtime_id());
    assert_eq!(image["Os"], "linux");
    assert_eq!(image["Architecture"], "amd64");
    assert_eq!(image["RepoTags"].as_array().map(Vec::len), Some(1));
    let expected_tag = format!(
        "{name}:v{}-{}",
        images.source_sha(),
        identity.archive_sha256()
    );
    assert_eq!(image["RepoTags"][0], expected_tag);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_create_requires_image_checks_for_the_selected_role() {
        let mut checks = PreflightChecks {
            engine: true,
            ..PreflightChecks::default()
        };
        let create = |role| CapturedRequest {
            method: "POST".to_owned(),
            target: "/containers/create".to_owned(),
            body: serde_json::json!({"Labels": {"io.velnor.role": role}})
                .to_string()
                .into_bytes(),
            response_status: Some(201),
            response_body: Vec::new(),
        };
        assert!(checks.require_container_create(&create("runner")).is_err());
        checks.runner = true;
        assert!(checks.require_container_create(&create("dind")).is_err());
    }
}
