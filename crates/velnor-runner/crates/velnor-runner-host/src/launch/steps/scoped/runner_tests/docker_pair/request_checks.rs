use serde_json::Value;

use super::server::{CapturedRequest, DIND_ID, RUNNER_ID, route};

pub(super) fn verify_worker_image_ids(
    requests: &[CapturedRequest],
    images: &crate::launch::VerifiedWorkerImages,
) -> Result<(), String> {
    let creates = requests
        .iter()
        .enumerate()
        .filter(|(_, request)| request.method == "POST" && route(request) == "/containers/create")
        .collect::<Vec<_>>();
    let (_, dind, _) = created_role(&creates, "dind")?;
    let (_, runner, _) = created_role(&creates, "runner")?;
    if dind["Image"] != images.dind().runtime_id()
        || runner["Image"] != images.runner().runtime_id()
    {
        return Err("The pair create requests use incorrect verified image IDs.".to_owned());
    }
    Ok(())
}

pub(super) fn valid_runner_attach(request: &CapturedRequest) -> bool {
    if request.method != "POST" || route(request) != format!("/containers/{RUNNER_ID}/attach") {
        return false;
    }
    let Some((_, query)) = request.target.split_once('?') else {
        return false;
    };
    let mut options = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .collect::<Vec<_>>();
    options.sort_unstable();
    options
        == [
            ("logs", "false"),
            ("stderr", "false"),
            ("stdin", "true"),
            ("stdout", "false"),
            ("stream", "true"),
        ]
}

pub(super) fn verify_pair_requests(
    requests: &[CapturedRequest],
    network_name: &str,
    budget: crate::worker::ResourceBudget,
    initializer_released: bool,
) -> Result<(), String> {
    let creates = requests
        .iter()
        .enumerate()
        .filter(|(_, request)| request.method == "POST" && route(request) == "/containers/create")
        .collect::<Vec<_>>();
    let (dind_index, dind, dind_id) = created_role(&creates, "dind")?;
    let (runner_index, runner, runner_id) = created_role(&creates, "runner")?;
    let initializer_id = initializer_create_id(&creates)?;
    if dind_id != DIND_ID || runner_id != RUNNER_ID {
        return Err("The pair create replies contain incorrect role identities.".to_owned());
    }
    assert_container_limits(&dind, budget.dind(), 2_048, true);
    assert_container_limits(&runner, budget.runner(), 1_024, false);
    assert_eq!(dind["HostConfig"]["NetworkMode"], network_name);
    assert_eq!(
        runner["HostConfig"]["NetworkMode"],
        format!("container:{DIND_ID}")
    );
    assert!(has_mounts(&dind, &["/var/lib/docker", "/run"]));
    assert_dind_metadata_mounts(&dind);
    assert!(has_mounts(
        &runner,
        &[
            "/home/runner",
            "/home/runner/_work",
            "/run",
            "/opt/velnor/job-facts"
        ]
    ));
    assert!(runner["OpenStdin"].as_bool().unwrap_or(false));
    assert!(runner["StdinOnce"].as_bool().unwrap_or(false));
    assert_pair_event_order(
        requests,
        dind_index,
        runner_index,
        &initializer_id,
        initializer_released,
    )?;
    Ok(())
}

fn created_role(
    creates: &[(usize, &CapturedRequest)],
    role: &str,
) -> Result<(usize, Value, String), String> {
    let found = creates
        .iter()
        .filter_map(|(index, request)| {
            let config = serde_json::from_slice::<Value>(&request.body).ok()?;
            (config["Labels"]["io.velnor.role"].as_str() == Some(role))
                .then_some((*index, request, config))
        })
        .collect::<Vec<_>>();
    if found.len() != 1 {
        return Err(format!("There must be one {role} bootstrap create."));
    }
    let (index, request, config) = &found[0];
    let id = serde_json::from_slice::<Value>(&request.response_body)
        .ok()
        .and_then(|response| response["Id"].as_str().map(str::to_owned))
        .ok_or_else(|| format!("The {role} bootstrap create has no ID response."))?;
    Ok((*index, config.clone(), id))
}

fn initializer_create_id(creates: &[(usize, &CapturedRequest)]) -> Result<String, String> {
    let matches = creates
        .iter()
        .filter(|(_, request)| {
            serde_json::from_slice::<Value>(&request.body)
                .ok()
                .is_some_and(|config| {
                    config["Labels"]["velnor.role"].as_str() == Some("storage-initializer")
                })
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err("There must be one storage-initializer bootstrap create.".to_owned());
    }
    serde_json::from_slice::<Value>(&matches[0].1.response_body)
        .ok()
        .and_then(|response| response["Id"].as_str().map(str::to_owned))
        .ok_or_else(|| "The storage initializer create has no ID response.".to_owned())
}

fn assert_pair_event_order(
    requests: &[CapturedRequest],
    dind_create: usize,
    runner_create: usize,
    initializer_id: &str,
    initializer_released: bool,
) -> Result<(), String> {
    let attaches = requests
        .iter()
        .enumerate()
        .filter(|(_, request)| route(request).ends_with("/attach"))
        .collect::<Vec<_>>();
    let runner_attaches = attaches
        .iter()
        .filter(|(_, request)| valid_runner_attach(request))
        .count();
    let initializer_attaches = attaches
        .iter()
        .filter(|(_, request)| {
            request.method == "POST"
                && route(request) == format!("/containers/{initializer_id}/attach")
        })
        .count();
    if runner_attaches != 1 || initializer_attaches != 1 {
        return Err("The Runner or initializer attach target is incorrect.".to_owned());
    }
    let runner_attach = attaches
        .iter()
        .find(|(_, request)| valid_runner_attach(request))
        .map(|(index, _)| *index)
        .ok_or_else(|| "The Runner attach event is absent.".to_owned())?;
    let initializer_attach = attaches
        .iter()
        .find(|(_, request)| {
            request.method == "POST"
                && route(request) == format!("/containers/{initializer_id}/attach")
        })
        .map(|(index, _)| *index)
        .ok_or_else(|| "The initializer attach event is absent.".to_owned())?;
    let dind_start = event_index(requests, &format!("/containers/{DIND_ID}/start"))?;
    let runner_start = event_index(requests, &format!("/containers/{RUNNER_ID}/start"))?;
    let initializer_start = event_index(requests, &format!("/containers/{initializer_id}/start"))?;
    let initializer_waits = requests
        .iter()
        .enumerate()
        .filter(|(_, request)| {
            request.method == "POST"
                && route(request) == format!("/containers/{initializer_id}/wait")
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if initializer_waits.len() != usize::from(initializer_released) {
        return Err(
            "The initializer release state does not match the requested handoff.".to_owned(),
        );
    }
    if !(initializer_start < dind_create
        && initializer_attach < dind_create
        && dind_create < runner_create
        && runner_create < runner_attach
        && runner_attach < dind_start
        && dind_start < runner_start
        && initializer_waits
            .first()
            .is_none_or(|wait| runner_start < *wait))
    {
        return Err("The pair create, attach, and start order is invalid.".to_owned());
    }
    Ok(())
}

fn assert_dind_metadata_mounts(config: &Value) {
    let mounts = config["HostConfig"]["Mounts"]
        .as_array()
        .expect("The DinD mounts must be present.");
    let expected = [
        ("/.velnor-runtime-identity", "runtime-identity"),
        ("/opt/velnor/job-facts", "job-facts"),
    ];
    let mut metadata_volume = None;
    for (target, subpath) in expected {
        let matching = mounts
            .iter()
            .filter(|mount| mount["Target"].as_str() == Some(target))
            .collect::<Vec<_>>();
        assert_eq!(
            matching.len(),
            1,
            "There must be one DinD mount at {target}."
        );
        let mount = matching[0];
        assert_eq!(mount["Type"], "volume");
        assert_eq!(mount["ReadOnly"], true);
        let source = mount["Source"]
            .as_str()
            .expect("The volume source must be present.");
        assert!(!source.is_empty());
        if let Some(expected_source) = metadata_volume {
            assert_eq!(source, expected_source);
        } else {
            metadata_volume = Some(source);
        }
        assert_eq!(mount["VolumeOptions"]["NoCopy"], true);
        assert_eq!(mount["VolumeOptions"]["Subpath"], subpath);
    }
}

fn event_index(requests: &[CapturedRequest], path: &str) -> Result<usize, String> {
    let matches = requests
        .iter()
        .enumerate()
        .filter(|(_, request)| request.method == "POST" && route(request) == path)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Ok(matches[0])
    } else {
        Err(format!(
            "There must be one Docker event for {path}; observed {}.",
            matches.len()
        ))
    }
}

fn assert_container_limits(
    config: &Value,
    limits: crate::worker::DockerResourceLimits,
    pids: i64,
    privileged: bool,
) {
    assert_eq!(config["HostConfig"]["NanoCpus"], limits.nano_cpus);
    assert_eq!(config["HostConfig"]["Memory"], limits.memory_bytes);
    assert_eq!(config["HostConfig"]["MemorySwap"], limits.memory_bytes);
    assert_eq!(config["HostConfig"]["PidsLimit"], pids);
    assert_eq!(config["HostConfig"]["Privileged"], privileged);
    assert_eq!(config["HostConfig"]["ReadonlyRootfs"], true);
    assert!(config["HostConfig"]["Tmpfs"].get("/tmp").is_some());
    assert!(config["HostConfig"]["Tmpfs"].get("/dev/shm").is_some());
}

fn has_mounts(config: &Value, targets: &[&str]) -> bool {
    let Some(mounts) = config["HostConfig"]["Mounts"].as_array() else {
        return false;
    };
    targets.iter().all(|target| {
        mounts
            .iter()
            .any(|mount| mount["Target"].as_str() == Some(target))
    })
}
