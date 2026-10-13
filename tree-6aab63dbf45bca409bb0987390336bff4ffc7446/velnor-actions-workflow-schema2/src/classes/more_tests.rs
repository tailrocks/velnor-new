use super::{REDIS, TESTCONTAINERS_RYUK, testcontainers_jobs};
use crate::{RunnerSpec, classes::topology};
use velnor_actions_workflow_tree::yaml::Yaml;

#[test]
fn testcontainers_pair_pins_images_and_provider_endpoints() {
    let hosted = RunnerSpec::hosted("ubuntu-26.04").expect("hosted catalog entry");
    let scale = RunnerSpec::scale_set(Yaml::Flow(vec![
        "self-hosted".into(),
        "velnor-linux".into(),
    ]));
    let jobs = testcontainers_jobs(&hosted, &scale);
    assert_eq!(jobs.len(), 2);
    for (id, job) in jobs {
        let runner = if id.ends_with("hosted") {
            &hosted
        } else {
            &scale
        };
        let Yaml::Map(fields) = job else {
            panic!("{id} is not a job map");
        };
        let env = fields
            .iter()
            .find(|(key, _)| key == "env")
            .expect("job env");
        assert_eq!(env.1, expected_env(runner));
        let Yaml::Seq(steps) = fields
            .iter()
            .find(|(key, _)| key == "steps")
            .expect("steps entry")
            .1
            .clone()
        else {
            panic!("{id} steps");
        };
        let text = format!("{steps:?}");
        assert!(text.contains("stock Docker") || text.contains("private DinD socket"));
        assert!(text.contains("npm ci --engine-strict --ignore-scripts"));
        assert!(text.contains("npm test --prefix qualification/testcontainers"));
        assert!(text.contains("reap.mjs"));
    }
}

fn expected_env(runner: &RunnerSpec) -> Yaml {
    Yaml::Map(vec![
        pair("DOCKER_HOST", topology::docker_endpoint(runner)),
        pair("DOCKER_CONTEXT", ""),
        pair(
            "TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE",
            topology::docker_socket_path(runner),
        ),
        pair("TESTCONTAINERS_HOST_OVERRIDE", "localhost"),
        pair("TESTCONTAINERS_RYUK_DISABLED", "false"),
        pair("TESTCONTAINERS_RYUK_TEST_LABEL", "true"),
        pair("RYUK_CONTAINER_IMAGE", TESTCONTAINERS_RYUK),
        pair("VELNOR_TESTCONTAINERS_REDIS_IMAGE", REDIS),
    ])
}

fn pair(key: &str, value: impl Into<String>) -> (String, Yaml) {
    (key.to_owned(), Yaml::str(value))
}
